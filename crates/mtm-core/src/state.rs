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
    /// 劣化／回復を確定させるまでの保持状態。
    hold: HoldState,
}

/// 劣化・回復の確定待ち（ヒステリシス）。
///
/// 1 周期だけ黄色になったような瞬時の変化でイベントを出すと、
/// 「劣化」と「回復」が交互に並ぶノイズになる。`hold_secs` だけ状態が
/// 続いてから確定し、記録時刻は**状態が最初に変わった時点**まで遡る。
/// これにより、一瞬の劣化も「いつ起きたか」を失わずに 1 件だけ残る。
#[derive(Debug, Clone, Default, PartialEq)]
struct HoldState {
    /// 劣化が続いている先頭の時刻（`None` は劣化していない）。
    degraded_since: Option<DateTime<FixedOffset>>,
    /// 劣化先頭時点の深刻度。
    degraded_severity: Option<SrtSeverity>,
    /// 回復が続いている先頭の時刻。
    recovered_since: Option<DateTime<FixedOffset>>,
    /// 劣化を確定済みか（回復イベントを出す条件）。
    degraded_announced: bool,
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
            match self.known_srt.get(&c.id).cloned() {
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
                            // 初観測時点の状態を「確定済み」の起点にする。
                            hold: HoldState {
                                degraded_announced: c.severity != SrtSeverity::Ok,
                                degraded_since: (c.severity != SrtSeverity::Ok).then_some(obs.at),
                                degraded_severity: (c.severity != SrtSeverity::Ok)
                                    .then_some(c.severity),
                                recovered_since: None,
                            },
                        },
                    );
                }
                Some(prev) => {
                    let prev_state = prev.state.clone();
                    self.observe_srt_quality(t, obs, &prev, c);
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

    /// SRT 品質の劣化・回復を、保持時間を経てから確定してイベント化する。
    ///
    /// 1 周期だけ黄色になったような瞬時の変化でイベントを出すと、
    /// 「劣化」と「回復」が交互に並ぶ。`hold_secs` だけ状態が続いてから確定し、
    /// イベントの時刻は**状態が最初に変わった時点**に遡らせる。
    /// これで一瞬の劣化も 1 件だけ、発生時刻付きで履歴に残る。
    fn observe_srt_quality(
        &mut self,
        t: &mut Transition,
        obs: &Observation,
        prev: &KnownSrt,
        c: &ObservedSrt,
    ) {
        let hold = chrono::Duration::seconds(self.thresholds.hold_secs.min(i64::MAX as u64) as i64);
        let now = obs.at;
        let degraded_now = c.severity != SrtSeverity::Ok;
        let mut next = prev.hold.clone();

        // ---- 劣化の追跡 ----
        if degraded_now {
            next.recovered_since = None;
            if next.degraded_since.is_none() {
                // 劣化が始まった瞬間。まだ記録せず、保持時間の経過を待つ。
                next.degraded_since = Some(now);
                next.degraded_severity = Some(c.severity);
            } else if c.severity == SrtSeverity::Critical
                && next.degraded_severity != Some(SrtSeverity::Critical)
            {
                // 続いている劣化が深刻になったら、その時点へ更新する。
                next.degraded_since = Some(now);
                next.degraded_severity = Some(SrtSeverity::Critical);
            }

            if !next.degraded_announced {
                let started = next.degraded_since.unwrap_or(now);
                if now - started >= hold {
                    // 保持時間を超えて続いたので確定する。
                    let severity = next.degraded_severity.unwrap_or(c.severity);
                    if self.allow_cooldown(&c.id, &c.path, &now) {
                        let detail = describe_quality(c);
                        t.events.push(event_at(
                            started,
                            obs,
                            StatusEventKind::SrtDegraded,
                            &c.id,
                            &c.path,
                            format!(
                                "SRT 品質が{}（path=`{}`）: {detail}",
                                if severity == SrtSeverity::Critical {
                                    "劣化"
                                } else {
                                    "注意"
                                },
                                c.path
                            ),
                            if severity == SrtSeverity::Critical {
                                EventSeverity::Critical
                            } else {
                                EventSeverity::Warning
                            },
                        ));
                    }
                    next.degraded_announced = true;
                }
            } else if c.severity == SrtSeverity::Critical && prev.severity == SrtSeverity::Warning {
                // 記録済みの劣化がさらに深刻になった場合も、深刻化を記録する。
                if self.allow_cooldown(&c.id, &format!("{}/crit", c.path), &now) {
                    let detail = describe_quality(c);
                    t.events.push(event_at(
                        now,
                        obs,
                        StatusEventKind::SrtDegraded,
                        &c.id,
                        &c.path,
                        format!("SRT 品質が劣化（path=`{}`）: {detail}", c.path),
                        EventSeverity::Critical,
                    ));
                }
            }
        } else {
            // ---- 回復の追跡 ----
            next.degraded_since = None;
            next.degraded_severity = None;
            if !next.degraded_announced {
                // 保持中に劣化が消えた ＝ 瞬時の劣化。ノイズとして記録しない。
                next.recovered_since = None;
            } else if next.recovered_since.is_none() {
                next.recovered_since = Some(now);
            } else {
                let since = next.recovered_since.unwrap_or(now);
                if now - since >= hold {
                    t.events.push(event_at(
                        since,
                        obs,
                        StatusEventKind::SrtRecovered,
                        &c.id,
                        &c.path,
                        format!("SRT 品質が回復しました（path=`{}`）。", c.path),
                        EventSeverity::Info,
                    ));
                    next.degraded_announced = false;
                    next.recovered_since = None;
                }
            }
        }

        if let Some(known) = self.known_srt.get_mut(&c.id) {
            known.hold = next;
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
    event_at(obs.at, obs, kind, target, label, message, severity)
}

/// 記録時刻を指定してイベントを作る。
///
/// 保持時間を経て確定した劣化は、確定した時刻ではなく
/// **状態が最初に変わった時刻**で残す（見逃した一瞬の発生時刻を失わないため）。
#[allow(clippy::too_many_arguments)]
fn event_at(
    at: DateTime<FixedOffset>,
    obs: &Observation,
    kind: StatusEventKind,
    target: &str,
    label: &str,
    message: impl Into<String>,
    severity: EventSeverity,
) -> StatusEvent {
    StatusEvent {
        seq: obs.seq,
        at,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::srt::{SrtCounters, SrtDerived};

    fn at(secs: i64) -> DateTime<FixedOffset> {
        use chrono::TimeZone;
        chrono::FixedOffset::east_opt(9 * 3600)
            .unwrap()
            .timestamp_opt(1_760_000_000 + secs, 0)
            .unwrap()
    }

    fn conn(severity: SrtSeverity) -> ObservedSrt {
        ObservedSrt {
            id: "conn-1".into(),
            path: "cam1".into(),
            remote_addr: "127.0.0.1:1234".into(),
            state: "read".into(),
            created: None,
            counters: SrtCounters::default(),
            derived: SrtDerived::default(),
            severity,
            from_metrics: true,
        }
    }

    /// 1 周期ぶん観測して、発生したイベントを返す。
    fn tick(machine: &mut StateMachine, secs: i64, severity: SrtSeverity) -> Vec<StatusEvent> {
        let obs = Observation {
            at: at(secs),
            seq: secs as u64 + 1,
            paths: Some(vec![]),
            srt: Some(vec![conn(severity)]),
        };
        machine.observe(&obs).events
    }

    fn kinds(events: &[StatusEvent]) -> Vec<StatusEventKind> {
        events.iter().map(|e| e.kind).collect()
    }

    fn machine(hold_secs: u64) -> StateMachine {
        let mut m = StateMachine::new(SrtThresholds {
            hold_secs,
            cooldown_secs: 0,
            ..SrtThresholds::default()
        });
        // 最初の 1 周期は「接続の出現」なので、正常状態で確立させる。
        tick(&mut m, 0, SrtSeverity::Ok);
        m
    }

    #[test]
    fn instant_degradation_is_not_recorded_as_noise() {
        // 1 周期だけ黄色になり、すぐ戻る（保持 3 秒）
        let mut m = machine(3);
        let e1 = tick(&mut m, 1, SrtSeverity::Warning);
        let e2 = tick(&mut m, 2, SrtSeverity::Ok);
        let e3 = tick(&mut m, 3, SrtSeverity::Ok);
        assert!(!kinds(&e1).contains(&StatusEventKind::SrtDegraded));
        assert!(!kinds(&e2).contains(&StatusEventKind::SrtDegraded));
        assert!(!kinds(&e2).contains(&StatusEventKind::SrtRecovered));
        assert!(!kinds(&e3).contains(&StatusEventKind::SrtRecovered));
    }

    #[test]
    fn sustained_degradation_is_recorded_at_its_start_time() {
        // 黄色が 3 秒続いたら確定し、時刻は「発生した 1 秒」に遡る
        let mut m = machine(3);
        assert!(tick(&mut m, 1, SrtSeverity::Warning).is_empty());
        assert!(tick(&mut m, 2, SrtSeverity::Warning).is_empty());
        let e3 = tick(&mut m, 3, SrtSeverity::Warning);
        // 1 秒始まりで 3 秒経過した 4 秒時点で確定する
        assert!(!kinds(&e3).contains(&StatusEventKind::SrtDegraded));
        let e4 = tick(&mut m, 4, SrtSeverity::Warning);
        let degraded: Vec<_> = e4
            .iter()
            .filter(|e| e.kind == StatusEventKind::SrtDegraded)
            .collect();
        assert_eq!(degraded.len(), 1);
        assert_eq!(degraded[0].severity, EventSeverity::Warning);
        // 確定は 4 秒時点だが、記録時刻は劣化が始まった 1 秒時点
        assert_eq!(degraded[0].at, at(1));
    }

    #[test]
    fn instant_blip_while_already_degraded_is_still_recorded_once() {
        // 保持でならした後に一瞬だけ黄→正常→黄でも、劣化は 1 回だけ記録される
        let mut m = machine(3);
        tick(&mut m, 1, SrtSeverity::Warning);
        tick(&mut m, 2, SrtSeverity::Warning);
        tick(&mut m, 3, SrtSeverity::Warning);
        let e4 = tick(&mut m, 4, SrtSeverity::Warning);
        assert_eq!(
            e4.iter()
                .filter(|e| e.kind == StatusEventKind::SrtDegraded)
                .count(),
            1
        );
        // 保持時間内の回復（瞬時）はノイズにしない
        let e5 = tick(&mut m, 5, SrtSeverity::Ok);
        assert!(!kinds(&e5).contains(&StatusEventKind::SrtRecovered));
        // その後さらに続けば回復として記録される
        tick(&mut m, 6, SrtSeverity::Ok);
        tick(&mut m, 7, SrtSeverity::Ok);
        let e8 = tick(&mut m, 8, SrtSeverity::Ok);
        assert!(kinds(&e8).contains(&StatusEventKind::SrtRecovered));
    }

    #[test]
    fn recovery_is_recorded_after_holding() {
        let mut m = machine(3);
        // 1 秒に劣化開始 → 4 秒で確定
        let mut degraded_at = None;
        for s in 1..=4 {
            let e = tick(&mut m, s, SrtSeverity::Warning);
            if let Some(ev) = e.iter().find(|e| e.kind == StatusEventKind::SrtDegraded) {
                degraded_at = Some(ev.at);
            }
        }
        assert_eq!(degraded_at, Some(at(1)), "劣化は発生時刻で記録される");

        // 5 秒から回復。保持 3 秒なので 8 秒で確定する。
        let e5 = tick(&mut m, 5, SrtSeverity::Ok);
        let e6 = tick(&mut m, 6, SrtSeverity::Ok);
        assert!(!kinds(&e5).contains(&StatusEventKind::SrtRecovered));
        assert!(!kinds(&e6).contains(&StatusEventKind::SrtRecovered));
        let e7 = tick(&mut m, 7, SrtSeverity::Ok);
        assert!(!kinds(&e7).contains(&StatusEventKind::SrtRecovered));
        let e8 = tick(&mut m, 8, SrtSeverity::Ok);
        let rec: Vec<_> = e8
            .iter()
            .filter(|e| e.kind == StatusEventKind::SrtRecovered)
            .collect();
        assert_eq!(rec.len(), 1, "回復イベントが 1 件だけ出るはず");
        // 回復が始まった 5 秒時点に遡って記録される
        assert_eq!(rec[0].at, at(5));
    }

    #[test]
    fn worsening_to_critical_is_recorded_immediately() {
        let mut m = machine(3);
        for s in 1..=4 {
            tick(&mut m, s, SrtSeverity::Warning);
        }
        // 確定後に赤へ悪化したら、その場で critical として記録する
        let e5 = tick(&mut m, 5, SrtSeverity::Critical);
        let crit: Vec<_> = e5
            .iter()
            .filter(|e| e.kind == StatusEventKind::SrtDegraded)
            .collect();
        assert_eq!(crit.len(), 1);
        assert_eq!(crit[0].severity, EventSeverity::Critical);
    }

    #[test]
    fn hold_zero_keeps_previous_immediate_behavior() {
        // hold_secs = 0 なら、従来どおりその場で記録する
        let mut m = machine(0);
        let e1 = tick(&mut m, 1, SrtSeverity::Warning);
        assert!(kinds(&e1).contains(&StatusEventKind::SrtDegraded));
        assert_eq!(e1.last().unwrap().at, at(1));
    }
}
