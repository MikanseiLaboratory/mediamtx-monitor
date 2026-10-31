//! 遷移イベントと差分を生成する。UI・時刻・スレッドに依存しないので
//! テストで任意の系列を突っ込める。
//!
//! - 初回観測は「既知の集合」として登録するだけでイベントを出さない。
//! - `down` 判定はパスの `online`（ソースが接続されているか）で行う。
//!   `available` は「パス定義が利用可能」で、配信が来ていない待機状態と
//!   区別する。UI では `PathSeverity::{Healthy, Degraded, Down}` に落とす。
//! - API 全体の到達不可は「パス個別の DOWN」として数えない。

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, FixedOffset};

use crate::config::SrtThresholds;
use crate::events::{EventSeverity, PathSeverity, StatusEvent, StatusEventKind};
use crate::srt::{SrtCounters, SrtDerived, SrtSeverity};

/// 1 周期分の観測スナップショット（状態機械の入力）。
#[derive(Debug, Clone, Default)]
pub struct Observation {
    pub at: DateTime<FixedOffset>,
    pub seq: u64,
    /// API 全体の結果。`None` は「取得できなかった（API 到達不可）」。
    pub paths: Option<Vec<ObservedPath>>,
    pub srt: Option<Vec<ObservedSrt>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObservedPath {
    pub name: String,
    pub conf_name: String,
    pub available: bool,
    pub online: bool,
    pub source_type: String,
    pub reader_count: usize,
    pub codecs: Vec<String>,
    pub inbound_bytes: u64,
    pub outbound_bytes: u64,
    pub inbound_frames_in_error: u64,
    pub online_since: Option<DateTime<FixedOffset>>,
    pub available_since: Option<DateTime<FixedOffset>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObservedSrt {
    pub id: String,
    pub path: String,
    pub remote_addr: String,
    pub state: String,
    pub created: Option<DateTime<FixedOffset>>,
    pub counters: SrtCounters,
    pub derived: SrtDerived,
    pub severity: SrtSeverity,
    pub from_metrics: bool,
}

/// 状態機械の出力。
#[derive(Debug, Clone, Default)]
pub struct Transition {
    pub events: Vec<StatusEvent>,
    pub removed_paths: Vec<String>,
    pub removed_srt: Vec<String>,
}

#[derive(Clone)]
pub struct StateMachine {
    known_paths: BTreeMap<String, KnownPath>,
    known_srt: BTreeMap<String, KnownSrt>,
    api_unreachable: bool,
    api_failures: u32,
    thresholds: SrtThresholds,
    cooldown: std::collections::HashMap<String, DateTime<FixedOffset>>,
    started: bool,
}

#[derive(Debug, Clone)]
struct KnownPath {
    online: bool,
    source_type: String,
}

#[derive(Debug, Clone)]
struct KnownSrt {
    path: String,
    severity: SrtSeverity,
    state: String,
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new(SrtThresholds::default())
    }
}

impl StateMachine {
    pub fn new(thresholds: SrtThresholds) -> Self {
        Self {
            known_paths: BTreeMap::new(),
            known_srt: BTreeMap::new(),
            api_unreachable: false,
            api_failures: 0,
            thresholds,
            cooldown: std::collections::HashMap::new(),
            started: false,
        }
    }

    pub fn set_thresholds(&mut self, thresholds: SrtThresholds) {
        self.thresholds = thresholds;
    }

    /// 観測を 1 つ適用し、遷移を返す。`api_ok=false` のときはパス/SRT を更新しない。
    pub fn observe(&mut self, obs: &Observation) -> Transition {
        let mut t = Transition::default();
        self.apply_api_health(obs, &mut t);

        if let Some(paths) = &obs.paths {
            self.apply_paths(obs, paths, &mut t);
        }
        if let Some(srt) = &obs.srt {
            self.apply_srt(obs, srt, &mut t);
        }
        self.started = true;
        t
    }

    fn apply_api_health(&mut self, obs: &Observation, t: &mut Transition) {
        if obs.paths.is_some() {
            self.api_failures = 0;
            if self.api_unreachable {
                self.api_unreachable = false;
                t.events.push(event(
                    obs,
                    StatusEventKind::ApiRestored,
                    "api",
                    "MediaMTX",
                    "MediaMTX API への接続が回復しました。",
                    EventSeverity::Info,
                ));
            }
        } else {
            self.api_failures += 1;
            // 1 回だけですぐ警告にすると瞬断でノイズになるので、連続 2 回で判定する。
            if !self.api_unreachable && self.api_failures >= 2 && self.started {
                self.api_unreachable = true;
                t.events.push(event(
                    obs,
                    StatusEventKind::ApiUnreachable,
                    "api",
                    "MediaMTX",
                    "MediaMTX API に接続できません。サーバーが停止しているか、`api: true` が未設定の可能性があります。",
                    EventSeverity::Critical,
                ));
            }
        }
    }

    fn apply_paths(&mut self, obs: &Observation, paths: &[ObservedPath], t: &mut Transition) {
        let mut seen = HashSet::with_capacity(paths.len());
        for p in paths {
            seen.insert(p.name.clone());
            match self.known_paths.get(&p.name) {
                None => {
                    if self.started {
                        if !p.online {
                            t.events.push(event(
                                obs,
                                StatusEventKind::PathDown,
                                &p.name,
                                &p.name,
                                format!("パス `{}` がオフラインで検出されました。", p.name),
                                EventSeverity::Warning,
                            ));
                        }
                        t.events.push(event(
                            obs,
                            StatusEventKind::PathOnlineStarted,
                            &p.name,
                            &p.name,
                            format!("パス `{}` を監視対象に追加しました。", p.name),
                            EventSeverity::Info,
                        ));
                    }
                    self.known_paths.insert(
                        p.name.clone(),
                        KnownPath {
                            online: p.online,
                            source_type: p.source_type.clone(),
                        },
                    );
                }
                Some(prev) => {
                    if prev.online && !p.online {
                        t.events.push(event(
                            obs,
                            StatusEventKind::PathOnlineStopped,
                            &p.name,
                            &p.name,
                            format!("パス `{}` の配信が停止しました（オフライン）。", p.name),
                            EventSeverity::Critical,
                        ));
                    } else if !prev.online && p.online {
                        t.events.push(event(
                            obs,
                            StatusEventKind::PathOnlineStarted,
                            &p.name,
                            &p.name,
                            format!("パス `{}` の配信が復帰しました。", p.name),
                            EventSeverity::Info,
                        ));
                    }
                    if prev.source_type != p.source_type {
                        t.events.push(event(
                            obs,
                            StatusEventKind::SourceChanged,
                            &p.name,
                            &p.name,
                            format!(
                                "パス `{}` のソースが `{}` から `{}` に変わりました。",
                                p.name,
                                if prev.source_type.is_empty() {
                                    "(なし)"
                                } else {
                                    &prev.source_type
                                },
                                if p.source_type.is_empty() {
                                    "(なし)"
                                } else {
                                    &p.source_type
                                }
                            ),
                            EventSeverity::Info,
                        ));
                    }
                    if let Some(known) = self.known_paths.get_mut(&p.name) {
                        known.online = p.online;
                        known.source_type = p.source_type.clone();
                    }
                }
            }
        }

        // 消えたパス
        let vanished: Vec<String> = self
            .known_paths
            .keys()
            .filter(|k| !seen.contains(*k))
            .cloned()
            .collect();
        for name in vanished {
            self.known_paths.remove(&name);
            t.removed_paths.push(name.clone());
            t.events.push(event(
                obs,
                StatusEventKind::PathDown,
                &name,
                &name,
                format!("パス `{name}` が MediaMTX 上から消えました。"),
                EventSeverity::Critical,
            ));
        }
    }

    fn apply_srt(&mut self, obs: &Observation, srt: &[ObservedSrt], t: &mut Transition) {
        let mut seen = HashSet::with_capacity(srt.len());
        for c in srt {
            seen.insert(c.id.clone());
            match self.known_srt.get(&c.id) {
                None => {
                    if self.started {
                        t.events.push(event(
                            obs,
                            StatusEventKind::SrtConnAppeared,
                            &c.id,
                            &c.path,
                            format!(
                                "SRT 接続が確立しました（path=`{}`, state={}, {}）。",
                                c.path,
                                c.state,
                                if c.remote_addr.is_empty() {
                                    "remote=不明"
                                } else {
                                    &c.remote_addr
                                }
                            ),
                            EventSeverity::Info,
                        ));
                    }
                    self.known_srt.insert(
                        c.id.clone(),
                        KnownSrt {
                            path: c.path.clone(),
                            severity: c.severity,
                            state: c.state.clone(),
                        },
                    );
                }
                Some(prev) => {
                    let prev_sev = prev.severity;
                    let prev_state = prev.state.clone();
                    let recovered = prev_sev != SrtSeverity::Ok && c.severity == SrtSeverity::Ok;
                    let degraded = c.severity != SrtSeverity::Ok
                        && (prev_sev == SrtSeverity::Ok
                            || (c.severity == SrtSeverity::Critical
                                && prev_sev == SrtSeverity::Warning));

                    if degraded && self.allow_cooldown(&c.id, &c.path, &obs.at) {
                        let detail = describe_quality(c);
                        t.events.push(event(
                            obs,
                            StatusEventKind::SrtDegraded,
                            &c.id,
                            &c.path,
                            format!(
                                "SRT 品質が{}（path=`{}`）: {detail}",
                                if c.severity == SrtSeverity::Critical {
                                    "劣化"
                                } else {
                                    "注意"
                                },
                                c.path
                            ),
                            if c.severity == SrtSeverity::Critical {
                                EventSeverity::Critical
                            } else {
                                EventSeverity::Warning
                            },
                        ));
                    }
                    if recovered {
                        t.events.push(event(
                            obs,
                            StatusEventKind::SrtRecovered,
                            &c.id,
                            &c.path,
                            format!("SRT 品質が回復しました（path=`{}`）。", c.path),
                            EventSeverity::Info,
                        ));
                    }
                    if prev_state != c.state {
                        t.events.push(event(
                            obs,
                            StatusEventKind::SourceChanged,
                            &c.id,
                            &c.path,
                            format!(
                                "SRT 接続の状態が `{prev_state}` から `{}` に変わりました（path=`{}`）。",
                                c.state, c.path
                            ),
                            EventSeverity::Info,
                        ));
                    }
                    if let Some(known) = self.known_srt.get_mut(&c.id) {
                        known.path = c.path.clone();
                        known.severity = c.severity;
                        known.state = c.state.clone();
                    }
                }
            }
        }

        let vanished: Vec<(String, String)> = self
            .known_srt
            .iter()
            .filter(|(id, _)| !seen.contains(*id))
            .map(|(id, k)| (id.clone(), k.path.clone()))
            .collect();
        for (id, path) in vanished {
            self.known_srt.remove(&id);
            t.removed_srt.push(id.clone());
            if self.started {
                t.events.push(event(
                    obs,
                    StatusEventKind::SrtConnDropped,
                    &id,
                    &path,
                    format!("SRT 接続が切断されました（path=`{path}`）。"),
                    EventSeverity::Critical,
                ));
            }
        }
    }

    /// 同じ接続・パスの劣化通知をクールダウン秒数以内は抑える。
    fn allow_cooldown(&mut self, id: &str, path: &str, now: &DateTime<FixedOffset>) -> bool {
        if self.thresholds.cooldown_secs == 0 {
            return true;
        }
        let key = format!("{id}|{path}");
        let window =
            chrono::Duration::seconds(self.thresholds.cooldown_secs.min(i64::MAX as u64) as i64);
        match self.cooldown.get(&key) {
            Some(last) if *now - *last < window => false,
            _ => {
                self.cooldown.insert(key, *now);
                // 古いキーを掃除して無制限に増えないようにする。
                if self.cooldown.len() > 1024 {
                    let cutoff = *now - window;
                    self.cooldown.retain(|_, v| *v >= cutoff);
                }
                true
            }
        }
    }

    /// 現在のオンライン状態からパス重大度を決める。
    pub fn path_severity(available: bool, online: bool) -> PathSeverity {
        if online {
            PathSeverity::Healthy
        } else if available {
            PathSeverity::Degraded
        } else {
            PathSeverity::Down
        }
    }

    /// これまでの登録数を返す（テスト・デバッグ用）。
    pub fn known_counts(&self) -> (usize, usize) {
        (self.known_paths.len(), self.known_srt.len())
    }

    /// 監視停止時に内部状態をリセットする。
    pub fn reset(&mut self) {
        self.known_paths.clear();
        self.known_srt.clear();
        self.api_unreachable = false;
        self.api_failures = 0;
        self.cooldown.clear();
        self.started = false;
    }
}

fn event(
    obs: &Observation,
    kind: StatusEventKind,
    target: &str,
    label: &str,
    message: impl Into<String>,
    severity: EventSeverity,
) -> StatusEvent {
    StatusEvent {
        seq: obs.seq,
        at: obs.at,
        kind,
        target: target.to_string(),
        label: label.to_string(),
        message: message.into(),
        severity,
    }
}

fn describe_quality(c: &ObservedSrt) -> String {
    let mut parts = Vec::new();
    if let Some(v) = c.derived.drop_rate {
        parts.push(format!("ドロップ率 {v:.2}%"));
    }
    if let Some(v) = c.derived.recovery_rate {
        parts.push(format!("回復率 {v:.1}%"));
    }
    if c.derived.rtt_ms > 0.0 {
        parts.push(format!("RTT {:.0}ms", c.derived.rtt_ms));
    }
    if c.derived.recv_mbps > 0.0 {
        parts.push(format!("受信 {:.2}Mbps", c.derived.recv_mbps));
    }
    if c.counters.outbound_frames_discarded.is_some_and(|v| v > 0) {
        parts.push(format!(
            "破棄フレーム {}",
            c.counters.outbound_frames_discarded.unwrap_or(0)
        ));
    }
    if parts.is_empty() {
        "計測値を取得できません".to_string()
    } else {
        parts.join(" / ")
    }
}
