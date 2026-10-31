//! 1 周期分の「フレーム」を組み立てる層。
//!
//! `Observation`（API/メトリクスから得た生データ）を状態機械に通し、差分付きの
//! 差分付きの `MonitorUpdate` を組み立てる。時系列リング・イベント履歴も
//! ここで管理するので、アダプタ側は「これを emit するだけ」で済む。

use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::config::AppConfig;
use crate::events::{
    ApiHealth, EventSeverity, MonitorUpdate, PathPatch, PathSample, PathSeverity, ServerTotals,
    SrtConnPatch, SrtConnSample, StatusEvent,
};
use crate::ring::SeriesSet;
use crate::srt::{SrtCounters, SrtDerived, SrtSeverity};
use crate::state::{Observation, ObservedPath, ObservedSrt, StateMachine};

/// 時系列に載せるメトリクス名（フロントと共有するキー）。
pub mod metric_key {
    pub const API_RTT_MS: &str = "api_rtt_ms";
    pub const PATHS_ONLINE: &str = "paths_online";
    pub const PATHS_TOTAL: &str = "paths_total";
    pub const READERS: &str = "readers";
    pub const INBOUND_MBPS: &str = "inbound_mbps";
    pub const OUTBOUND_MBPS: &str = "outbound_mbps";
    pub const SRT_CONNS: &str = "srt_conns";
    pub const SRT_DROP_RATE: &str = "srt_drop_rate";
    pub const SRT_RECOVERY_RATE: &str = "srt_recovery_rate";
    pub const SRT_RECV_MBPS: &str = "srt_recv_mbps";
    pub const SRT_SEND_MBPS: &str = "srt_send_mbps";
    pub const SRT_RTT_MS: &str = "srt_rtt_ms";
    pub const SRT_LINK_CAPACITY: &str = "srt_link_capacity";
    pub const SRT_UNRECOVERED: &str = "srt_unrecovered_packets";
    pub const SRT_NAK_RATE: &str = "srt_nak_rate";
    pub const PATH_INBOUND_MBPS: &str = "inbound_mbps";
    pub const PATH_OUTBOUND_MBPS: &str = "outbound_mbps";
    pub const PATH_READERS: &str = "readers";
    pub const PATH_FRAMES_ERR: &str = "frames_in_error_per_sec";
    pub const PATH_ONLINE: &str = "online";

    /// 系列のスコープ。フロントの `get_series` の `scope` と一致させる。
    pub fn srt_scope(conn_id: &str) -> String {
        format!("srt:{conn_id}")
    }
    pub fn path_scope(name: &str) -> String {
        format!("path:{name}")
    }
    pub const SERVER_SCOPE: &str = "server";
}

/// パスごと・SRT 接続ごとの差分計算用の前回値。
#[derive(Debug, Clone, Default)]
struct PrevPath {
    inbound_bytes: u64,
    outbound_bytes: u64,
    frames_err: u64,
}

#[derive(Clone)]
pub struct FrameBuilder {
    config: AppConfig,
    machine: std::sync::Arc<StateMachine>,
    server_series: SeriesSet,
    path_series: BTreeMap<String, SeriesSet>,
    srt_series: BTreeMap<String, SeriesSet>,
    prev_paths: HashMap<String, PrevPath>,
    prev_srt: HashMap<String, SrtCounters>,
    /// 直近の SRT 接続スナップショット（スナップショット生成用）。
    last_srt: Vec<SrtConnSample>,
    prev_tick: Option<chrono::DateTime<chrono::FixedOffset>>,
    events: VecDeque<StatusEvent>,
    max_events: usize,
    seq: u64,
    api: ApiHealth,
    server_version: Option<String>,
}

impl FrameBuilder {
    pub fn new(config: &AppConfig) -> Self {
        let cap = config.max_samples();
        Self {
            config: config.clone(),
            machine: std::sync::Arc::new(StateMachine::new(config.srt.clone())),
            server_series: SeriesSet::new(cap),
            path_series: BTreeMap::new(),
            srt_series: BTreeMap::new(),
            prev_paths: HashMap::new(),
            prev_srt: HashMap::new(),
            last_srt: Vec::new(),
            prev_tick: None,
            events: VecDeque::new(),
            max_events: 2_000,
            seq: 0,
            api: ApiHealth::default(),
            server_version: None,
        }
    }

    /// 設定変更を反映する（リング容量としきい値のみ。履歴は保持する）。
    pub fn reconfigure(&mut self, config: &AppConfig) {
        self.config = config.clone();
        let cap = config.max_samples();
        self.server_series.set_capacity(cap);
        for s in self.path_series.values_mut() {
            s.set_capacity(cap);
        }
        for s in self.srt_series.values_mut() {
            s.set_capacity(cap);
        }
        std::sync::Arc::make_mut(&mut self.machine).set_thresholds(config.srt.clone());
    }

    /// 差分で埋まる内容だけを `other` から取り込む。
    ///
    /// 監視スレッド → 読み取り用バッファの同期に使う。毎周期のリング全複製を
    /// 避けるため、対象ごとに置き換える（リングの履歴はコピーしない）。
    pub fn merge_from(&mut self, other: &FrameBuilder) {
        self.config = other.config.clone();
        self.machine = std::sync::Arc::new((*other.machine).clone());
        self.path_series = other.path_series.clone();
        self.srt_series = other.srt_series.clone();
        self.events = other.events.clone();
        self.last_srt = other.last_srt.clone();
        self.seq = other.seq;
        self.api = other.api.clone();
        self.server_version = other.server_version.clone();
        self.prev_tick = other.prev_tick;
        // サーバー系列も履歴ごと引き継ぐ。
        //
        // 以前は最新 1 点だけを push していたため、起動直後に空のバッファへ
        // 同期した場合、読み取り用バッファには 1 点しか無かった。その状態では
        // グラフの x 範囲が 1 点に潰れ、右端に縦線が出るだけになる
        // （時間窓を広げても過去が出ない）。クローンは参照カウント式の
        // `VecDeque` の複製で、点数ぶんの配列を作るより軽い。
        self.server_series = other.server_series.clone();
    }
    /// 内容を `slot` へコピーする（コマンド側の読み取り用バッファへ写す）。
    pub fn clone_into(&self, slot: &mut Option<FrameBuilder>) {
        *slot = Some(self.clone());
    }

    /// 観測済みのパス名（ソート済み）。
    pub fn path_names(&self) -> Vec<&str> {
        self.path_series.keys().map(String::as_str).collect()
    }

    /// 現在観測中の SRT 接続数。
    pub fn srt_conn_count(&self) -> usize {
        self.srt_series.len()
    }

    pub fn reset(&mut self) {
        std::sync::Arc::make_mut(&mut self.machine).reset();
        self.prev_paths.clear();
        self.prev_srt.clear();
        self.prev_tick = None;
    }

    pub fn server_series(&self) -> &SeriesSet {
        &self.server_series
    }

    pub fn path_series(&self, name: &str) -> Option<&SeriesSet> {
        self.path_series.get(name)
    }

    pub fn srt_series(&self, id: &str) -> Option<&SeriesSet> {
        self.srt_series.get(id)
    }

    pub fn events(&self, limit: usize) -> Vec<StatusEvent> {
        self.events
            .iter()
            .rev()
            .take(limit)
            .rev()
            .cloned()
            .collect()
    }

    pub fn api(&self) -> &ApiHealth {
        &self.api
    }

    pub fn server_version(&self) -> Option<&str> {
        self.server_version.as_deref()
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 1 周期分の観測を取り込み、emit 用の更新を作る。
    pub fn ingest(
        &mut self,
        at: chrono::DateTime<chrono::FixedOffset>,
        api: ApiHealth,
        server_version: Option<String>,
        paths: Option<Vec<ObservedPath>>,
        srt: Option<Vec<ObservedSrt>>,
        full: bool,
    ) -> MonitorUpdate {
        self.seq += 1;
        let seq = self.seq;
        let at_ms = at.timestamp_millis();
        let interval_ms = self
            .prev_tick
            .map(|p| (at - p).num_milliseconds())
            .unwrap_or(0);
        self.prev_tick = Some(at);
        self.api = api.clone();
        self.server_version = server_version.clone();

        let obs = Observation {
            at,
            seq,
            paths: paths.clone(),
            srt: srt.clone(),
        };
        let transition = std::sync::Arc::make_mut(&mut self.machine).observe(&obs);

        // ---- パス ----
        let mut path_patches = Vec::new();
        let mut totals = ServerTotals::default();
        if let Some(paths) = &paths {
            let mut seen = std::collections::HashSet::new();
            for p in paths {
                seen.insert(p.name.clone());
                let prev = self.prev_paths.get(&p.name).cloned().unwrap_or_default();
                let first = !self.prev_paths.contains_key(&p.name);
                let (in_mbps, out_mbps, err_rate) = if first || interval_ms <= 0 {
                    (None, None, None)
                } else {
                    (
                        Some(bytes_per_interval(
                            p.inbound_bytes,
                            prev.inbound_bytes,
                            interval_ms,
                        )),
                        Some(bytes_per_interval(
                            p.outbound_bytes,
                            prev.outbound_bytes,
                            interval_ms,
                        )),
                        Some(
                            (p.inbound_frames_in_error.saturating_sub(prev.frames_err)) as f64
                                / (interval_ms as f64 / 1000.0),
                        ),
                    )
                };
                if let Some(v) = in_mbps {
                    totals.inbound_mbps += v;
                }
                if let Some(v) = out_mbps {
                    totals.outbound_mbps += v;
                }
                self.prev_paths.insert(
                    p.name.clone(),
                    PrevPath {
                        inbound_bytes: p.inbound_bytes,
                        outbound_bytes: p.outbound_bytes,
                        frames_err: p.inbound_frames_in_error,
                    },
                );

                let severity = StateMachine::path_severity(p.available, p.online);
                match severity {
                    PathSeverity::Healthy => totals.paths_online += 1,
                    _ => totals.paths_down += 1,
                }
                totals.path_count += 1;
                totals.reader_count += p.reader_count;

                let srt_ids: Vec<String> = srt
                    .as_ref()
                    .map(|list| {
                        list.iter()
                            .filter(|c| c.path == p.name)
                            .map(|c| c.id.clone())
                            .collect()
                    })
                    .unwrap_or_default();

                let sample = PathSample {
                    name: p.name.clone(),
                    conf_name: p.conf_name.clone(),
                    available: p.available,
                    online: p.online,
                    source_type: p.source_type.clone(),
                    reader_count: p.reader_count,
                    codecs: p.codecs.clone(),
                    inbound_bytes: p.inbound_bytes,
                    outbound_bytes: p.outbound_bytes,
                    inbound_frames_in_error: p.inbound_frames_in_error,
                    inbound_mbps: in_mbps,
                    outbound_mbps: out_mbps,
                    frames_in_error_per_sec: err_rate,
                    online_since: p.online_since,
                    available_since: p.available_since,
                    severity,
                    srt_conn_ids: srt_ids,
                };

                let series = self
                    .path_series
                    .entry(p.name.clone())
                    .or_insert_with(|| SeriesSet::new(self.config.max_samples()));
                series.push(metric_key::PATH_INBOUND_MBPS, at_ms, in_mbps);
                series.push(metric_key::PATH_OUTBOUND_MBPS, at_ms, out_mbps);
                series.push(metric_key::PATH_READERS, at_ms, Some(p.reader_count as f64));
                series.push(metric_key::PATH_FRAMES_ERR, at_ms, err_rate);
                series.push(
                    metric_key::PATH_ONLINE,
                    at_ms,
                    Some(if p.online { 1.0 } else { 0.0 }),
                );

                path_patches.push(PathPatch { seq, sample });
            }
            // 消えたパスは前回値も消す。
            self.prev_paths.retain(|k, _| seen.contains(k));
        }

        // ---- SRT ----
        let mut srt_patches = Vec::new();
        let mut max_drop: Option<f64> = None;
        let mut min_recovery: Option<f64> = None;
        let mut worst: Option<SrtSeverity> = None;
        if let Some(list) = &srt {
            let mut seen = std::collections::HashSet::new();
            for c in list {
                seen.insert(c.id.clone());
                // 差分は前回の累積値から。`derived` は呼び出し前に計算済みだが、
                // ここでは間隔を確実に揃えるために再計算する。
                let derived = SrtDerived::compute(
                    self.prev_srt.get(&c.id),
                    &c.counters,
                    interval_ms,
                    &c.state,
                );
                self.prev_srt.insert(c.id.clone(), c.counters.clone());

                let severity = derived.severity(&self.config.srt);
                if let Some(v) = derived.drop_rate {
                    max_drop = Some(max_drop.map_or(v, |m: f64| m.max(v)));
                }
                if let Some(v) = derived.recovery_rate {
                    min_recovery = Some(min_recovery.map_or(v, |m: f64| m.min(v)));
                }
                worst = Some(match (worst, severity) {
                    (Some(a), b) => rank_max(a, b),
                    (None, b) => b,
                });

                let series = self
                    .srt_series
                    .entry(c.id.clone())
                    .or_insert_with(|| SeriesSet::new(self.config.max_samples()));
                series.push(metric_key::SRT_DROP_RATE, at_ms, derived.drop_rate);
                series.push(metric_key::SRT_RECOVERY_RATE, at_ms, derived.recovery_rate);
                series.push(metric_key::SRT_RECV_MBPS, at_ms, Some(derived.recv_mbps));
                series.push(
                    metric_key::SRT_SEND_MBPS,
                    at_ms,
                    Some(c.counters.mbps_send_rate),
                );
                series.push(metric_key::SRT_RTT_MS, at_ms, Some(derived.rtt_ms));
                series.push(
                    metric_key::SRT_LINK_CAPACITY,
                    at_ms,
                    Some(derived.link_capacity_mbps),
                );
                series.push(
                    metric_key::SRT_UNRECOVERED,
                    at_ms,
                    derived
                        .loss_packets
                        .zip(derived.recovered_packets)
                        .map(|(loss, rec)| (loss - rec).max(0.0)),
                );
                series.push(metric_key::SRT_NAK_RATE, at_ms, derived.nak_rate);

                srt_patches.push(SrtConnPatch {
                    seq,
                    sample: SrtConnSample {
                        id: c.id.clone(),
                        path: c.path.clone(),
                        remote_addr: c.remote_addr.clone(),
                        state: c.state.clone(),
                        created: c.created,
                        counters: c.counters.clone(),
                        derived,
                        severity,
                        from_metrics: c.from_metrics,
                    },
                });
            }
            self.prev_srt.retain(|k, _| seen.contains(k));
        }
        totals.srt_conn_count = srt.as_ref().map(Vec::len).unwrap_or(0);
        totals.srt_max_drop_rate = max_drop;
        totals.srt_min_recovery_rate = min_recovery;
        totals.srt_worst_severity = worst;

        // ---- サーバー系列 ----
        let s = &mut self.server_series;
        s.push(metric_key::API_RTT_MS, at_ms, api.rtt_ms);
        s.push(
            metric_key::PATHS_ONLINE,
            at_ms,
            Some(totals.paths_online as f64),
        );
        s.push(
            metric_key::PATHS_TOTAL,
            at_ms,
            Some(totals.path_count as f64),
        );
        s.push(metric_key::READERS, at_ms, Some(totals.reader_count as f64));
        s.push(
            metric_key::INBOUND_MBPS,
            at_ms,
            if paths.is_some() {
                Some(totals.inbound_mbps)
            } else {
                None
            },
        );
        s.push(
            metric_key::OUTBOUND_MBPS,
            at_ms,
            if paths.is_some() {
                Some(totals.outbound_mbps)
            } else {
                None
            },
        );
        s.push(
            metric_key::SRT_CONNS,
            at_ms,
            Some(totals.srt_conn_count as f64),
        );
        s.push(metric_key::SRT_DROP_RATE, at_ms, max_drop);

        // ---- イベント履歴 ----
        for e in &transition.events {
            if self.events.len() >= self.max_events {
                self.events.pop_front();
            }
            self.events.push_back(e.clone());
        }

        // スナップショット（フロント初期化・HTTP モードの同期）用に控えておく。
        self.last_srt = srt_patches.iter().map(|p| p.sample.clone()).collect();

        MonitorUpdate {
            seq,
            at,
            api,
            totals,
            paths: path_patches,
            srt: srt_patches,
            removed_paths: transition.removed_paths,
            removed_srt: transition.removed_srt,
            events: transition.events,
            server_version,
            full,
        }
    }

    /// 現在の全量スナップショット（フロント初期化用）。
    pub fn snapshot(&self, at: chrono::DateTime<chrono::FixedOffset>) -> MonitorUpdate {
        let mut paths = Vec::new();
        let mut srt = Vec::new();
        let mut totals = ServerTotals::default();
        for (name, series) in &self.path_series {
            let f = |m: &str| series.get(m).and_then(|r| r.last()).and_then(|p| p.value);
            let online = f(metric_key::PATH_ONLINE).unwrap_or(0.0) > 0.5;
            let severity = if online {
                PathSeverity::Healthy
            } else {
                PathSeverity::Down
            };
            if online {
                totals.paths_online += 1;
            } else {
                totals.paths_down += 1;
            }
            totals.path_count += 1;
            totals.reader_count += f(metric_key::PATH_READERS).unwrap_or(0.0) as usize;
            totals.inbound_mbps += f(metric_key::PATH_INBOUND_MBPS).unwrap_or(0.0);
            totals.outbound_mbps += f(metric_key::PATH_OUTBOUND_MBPS).unwrap_or(0.0);
            paths.push(PathPatch {
                seq: self.seq,
                sample: PathSample {
                    name: name.clone(),
                    conf_name: name.clone(),
                    available: online,
                    online,
                    source_type: String::new(),
                    reader_count: f(metric_key::PATH_READERS).unwrap_or(0.0) as usize,
                    codecs: Vec::new(),
                    inbound_bytes: 0,
                    outbound_bytes: 0,
                    inbound_frames_in_error: 0,
                    inbound_mbps: f(metric_key::PATH_INBOUND_MBPS),
                    outbound_mbps: f(metric_key::PATH_OUTBOUND_MBPS),
                    frames_in_error_per_sec: f(metric_key::PATH_FRAMES_ERR),
                    online_since: None,
                    available_since: None,
                    severity,
                    srt_conn_ids: Vec::new(),
                },
            });
        }

        // SRT 接続は直近の観測（パス名・状態・接続元を含む）をそのまま返す。
        // リングから作ると名前や状態が欠けるため、詳細は最後の更新から引き継ぐ。
        let mut max_drop: Option<f64> = None;
        let mut min_recovery: Option<f64> = None;
        let mut worst: Option<SrtSeverity> = None;
        for sample in self.last_srt.iter().cloned() {
            if let Some(v) = sample.derived.drop_rate {
                max_drop = Some(max_drop.map_or(v, |m: f64| m.max(v)));
            }
            if let Some(v) = sample.derived.recovery_rate {
                min_recovery = Some(min_recovery.map_or(v, |m: f64| m.min(v)));
            }
            worst = Some(match worst {
                Some(w) => rank_max(w, sample.severity),
                None => sample.severity,
            });
            srt.push(SrtConnPatch {
                seq: self.seq,
                sample,
            });
        }
        totals.srt_conn_count = srt.len();
        totals.srt_max_drop_rate = max_drop;
        totals.srt_min_recovery_rate = min_recovery;
        totals.srt_worst_severity = worst;

        MonitorUpdate {
            seq: self.seq,
            at,
            api: self.api.clone(),
            totals,
            paths,
            srt,
            removed_paths: Vec::new(),
            removed_srt: Vec::new(),
            events: Vec::new(),
            server_version: self.server_version.clone(),
            full: true,
        }
    }
}

fn rank_max(a: SrtSeverity, b: SrtSeverity) -> SrtSeverity {
    let rank = |s: SrtSeverity| match s {
        SrtSeverity::Ok => 0,
        SrtSeverity::Warning => 1,
        SrtSeverity::Critical => 2,
    };
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
}

/// 累積バイトの増分から Mbps を求める。カウンタリセット時は 0。
fn bytes_per_interval(cur: u64, prev: u64, interval_ms: i64) -> f64 {
    if interval_ms <= 0 {
        return 0.0;
    }
    let delta = cur.saturating_sub(prev) as f64;
    delta * 8.0 / (interval_ms as f64 / 1000.0) / 1_000_000.0
}

/// イベント重大度から通知すべきかを判断する補助。
pub fn should_notify(severity: EventSeverity) -> bool {
    matches!(severity, EventSeverity::Warning | EventSeverity::Critical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::Ring;
    use chrono::TimeZone;

    fn at(secs: i64) -> chrono::DateTime<chrono::FixedOffset> {
        chrono::FixedOffset::east_opt(9 * 3600)
            .unwrap()
            .timestamp_opt(1_760_000_000 + secs, 0)
            .unwrap()
    }

    fn obs_path(name: &str, online: bool, bytes: u64) -> ObservedPath {
        ObservedPath {
            name: name.into(),
            conf_name: name.into(),
            available: true,
            online,
            source_type: "srtConn".into(),
            reader_count: 1,
            codecs: vec!["H264".into()],
            inbound_bytes: bytes,
            outbound_bytes: bytes * 2,
            inbound_frames_in_error: 0,
            online_since: None,
            available_since: None,
        }
    }

    fn obs_srt(id: &str, path: &str, counters: SrtCounters) -> ObservedSrt {
        // 既定は publish（MediaMTX が受信側）。受信系カウンタを使うテストの前提。
        obs_srt_state(id, path, counters, "publish")
    }

    fn obs_srt_state(id: &str, path: &str, counters: SrtCounters, state: &str) -> ObservedSrt {
        ObservedSrt {
            id: id.into(),
            path: path.into(),
            remote_addr: "127.0.0.1:1".into(),
            state: state.into(),
            created: None,
            counters: counters.clone(),
            derived: SrtDerived::default(),
            severity: SrtSeverity::Ok,
            from_metrics: true,
        }
    }

    fn health_ok() -> ApiHealth {
        ApiHealth {
            ok: true,
            rtt_ms: Some(3.0),
            error: None,
            consecutive_failures: 0,
            metrics_ok: true,
            metrics_error: None,
        }
    }

    #[test]
    fn first_frame_registers_without_events() {
        let cfg = AppConfig::default();
        let mut fb = FrameBuilder::new(&cfg);
        let u = fb.ingest(
            at(0),
            health_ok(),
            Some("v1.21.2".into()),
            Some(vec![obs_path("cam1", true, 0)]),
            Some(vec![]),
            true,
        );
        assert!(u.events.is_empty());
        assert_eq!(u.totals.path_count, 1);
        assert!(u.paths[0].sample.inbound_mbps.is_none(), "初回は差分なし");
    }

    #[test]
    fn path_stop_emits_event_once() {
        let cfg = AppConfig::default();
        let mut fb = FrameBuilder::new(&cfg);
        fb.ingest(
            at(0),
            health_ok(),
            None,
            Some(vec![obs_path("cam1", true, 0)]),
            Some(vec![]),
            true,
        );
        let u = fb.ingest(
            at(2),
            health_ok(),
            None,
            Some(vec![obs_path("cam1", false, 100)]),
            Some(vec![]),
            false,
        );
        assert_eq!(u.events.len(), 1);
        assert_eq!(
            u.events[0].kind,
            crate::events::StatusEventKind::PathOnlineStopped
        );
        // 同じ状態が続いても再発火しない
        let u = fb.ingest(
            at(4),
            health_ok(),
            None,
            Some(vec![obs_path("cam1", false, 200)]),
            Some(vec![]),
            false,
        );
        assert!(u.events.is_empty());
    }

    #[test]
    fn bitrate_computed_from_counter_delta() {
        let cfg = AppConfig::default();
        let mut fb = FrameBuilder::new(&cfg);
        // 2 秒で 2.5 MB 増 → 10 Mbps
        fb.ingest(
            at(0),
            health_ok(),
            None,
            Some(vec![obs_path("cam1", true, 0)]),
            Some(vec![]),
            true,
        );
        let u = fb.ingest(
            at(2),
            health_ok(),
            None,
            Some(vec![obs_path("cam1", true, 2_500_000)]),
            Some(vec![]),
            false,
        );
        assert!((u.paths[0].sample.inbound_mbps.unwrap() - 10.0).abs() < 1e-6);
        assert!((u.totals.inbound_mbps - 10.0).abs() < 1e-6);
    }

    #[test]
    fn removed_path_is_reported() {
        let cfg = AppConfig::default();
        let mut fb = FrameBuilder::new(&cfg);
        fb.ingest(
            at(0),
            health_ok(),
            None,
            Some(vec![obs_path("a", true, 0), obs_path("b", true, 0)]),
            Some(vec![]),
            true,
        );
        let u = fb.ingest(
            at(2),
            health_ok(),
            None,
            Some(vec![obs_path("a", true, 0)]),
            Some(vec![]),
            false,
        );
        assert_eq!(u.removed_paths, vec!["b".to_string()]);
    }

    #[test]
    fn srt_counters_do_not_mix_between_connections() {
        let cfg = AppConfig::default();
        let mut fb = FrameBuilder::new(&cfg);
        let c1 = SrtCounters {
            packets_received: Some(0),
            packets_received_unique: Some(0),
            packets_received_drop: Some(0),
            packets_received_retrans: Some(0),
            packets_received_loss: Some(0),
            ..Default::default()
        };

        fb.ingest(
            at(0),
            health_ok(),
            None,
            Some(vec![]),
            Some(vec![obs_srt("a", "p", c1.clone())]),
            true,
        );
        let mut c2 = c1.clone();
        c2.packets_received = Some(1000);
        c2.packets_received_unique = Some(990);
        c2.packets_received_drop = Some(10);
        c2.packets_received_retrans = Some(8);
        c2.packets_received_loss = Some(10);
        let u = fb.ingest(
            at(2),
            health_ok(),
            None,
            Some(vec![]),
            Some(vec![obs_srt("a", "p", c2)]),
            false,
        );
        let d = &u.srt[0].sample.derived;
        assert!((d.drop_rate.unwrap() - 1.0).abs() < 1e-9);
        assert!((d.recovery_rate.unwrap() - 80.0).abs() < 1e-9);
        assert_eq!(u.totals.srt_max_drop_rate, d.drop_rate);
        assert_eq!(u.totals.srt_min_recovery_rate, d.recovery_rate);
    }

    /// read 接続（MediaMTX が視聴者へ送信する）では、送信側カウンタから
    /// ドロップ率とレートを作ること。
    ///
    /// MediaMTX の SRT 統計はソケットのカウンタなので、read でも
    /// `packetsReceived*` は制御パケットしか数えない。受信側だけで計算すると、
    /// 視聴側のドロップ率が常に 0 / null、受信レートが常に 0 になる。
    #[test]
    fn read_connection_uses_sent_counters() {
        let cfg = AppConfig::default();
        let mut fb = FrameBuilder::new(&cfg);
        let mut c1 = SrtCounters {
            packets_sent: Some(100_000),
            packets_sent_unique: Some(99_000),
            // 送信側カウンタは 0 始まりで揃える（`None` だと差分が取れない）
            packets_send_drop: Some(0),
            packets_retrans: Some(0),
            packets_send_loss: Some(0),
            packets_received_nak: Some(0),
            mbps_send_rate: 12.0,
            ..Default::default()
        };
        c1.mbps_receive_rate = 0.02; // 制御パケット分
        fb.ingest(
            at(0),
            health_ok(),
            None,
            Some(vec![]),
            Some(vec![obs_srt_state("r1", "cam1", c1.clone(), "read")]),
            true,
        );

        let mut c2 = c1.clone();
        c2.packets_sent = Some(101_000);
        c2.packets_sent_unique = Some(100_000);
        c2.packets_send_drop = Some(10);
        c2.packets_retrans = Some(40);
        c2.packets_send_loss = Some(50);
        c2.mbps_send_rate = 12.5;
        let u = fb.ingest(
            at(2),
            health_ok(),
            None,
            Some(vec![]),
            Some(vec![obs_srt_state("r1", "cam1", c2, "read")]),
            false,
        );
        let d = &u.srt[0].sample.derived;
        assert_eq!(d.recv_packets, Some(1000.0), "送信ユニーク数の差分");
        // 10 / 1000 × 100 = 1.0%
        assert!(
            (d.drop_rate.unwrap() - 1.0).abs() < 1e-9,
            "read のドロップ率が算出されていない: {:?}",
            d.drop_rate
        );
        // 40 / 50 × 100 = 80%
        assert!((d.recovery_rate.unwrap() - 80.0).abs() < 1e-9);
        // 受信レートは送信レート（12.5）を使う
        assert!(
            (d.recv_mbps - 12.5).abs() < 1e-9,
            "read のレートが送信レートになっていない: {}",
            d.recv_mbps
        );
        // 合計にも反映される
        assert_eq!(u.totals.srt_max_drop_rate, d.drop_rate);
        // 系列にも送信レートが載る
        let series = fb.srt_series("r1").expect("SRT 系列");
        assert!(
            (series
                .get(super::metric_key::SRT_RECV_MBPS)
                .and_then(|r| r.last())
                .and_then(|p| p.value)
                .unwrap()
                - 12.5)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn publish_connection_still_uses_received_counters() {
        // read 対応を入れても publish 側の計算が変わらないこと
        let cfg = AppConfig::default();
        let mut fb = FrameBuilder::new(&cfg);
        let first = SrtCounters {
            packets_received: Some(1000),
            packets_received_unique: Some(990),
            packets_received_nak: Some(0),
            packets_sent_nak: Some(0),
            packets_received_drop: Some(0),
            packets_received_retrans: Some(0),
            packets_received_loss: Some(0),
            mbps_receive_rate: 8.0,
            ..Default::default()
        };
        fb.ingest(
            at(0),
            health_ok(),
            None,
            Some(vec![]),
            Some(vec![obs_srt_state("p1", "cam2", first.clone(), "publish")]),
            true,
        );
        let mut second = first.clone();
        second.packets_received = Some(2000);
        second.packets_received_unique = Some(1980);
        second.packets_received_drop = Some(20);
        second.packets_received_retrans = Some(30);
        second.packets_received_loss = Some(60);
        let u = fb.ingest(
            at(2),
            health_ok(),
            None,
            Some(vec![]),
            Some(vec![obs_srt_state("p1", "cam2", second, "publish")]),
            false,
        );
        let d = &u.srt[0].sample.derived;
        assert_eq!(d.recv_packets, Some(990.0));
        assert!((d.drop_rate.unwrap() - 2.0).abs() < 1e-9);
        assert!((d.recovery_rate.unwrap() - 50.0).abs() < 1e-9);
        assert!((d.recv_mbps - 8.0).abs() < 1e-9);
    }

    #[test]
    fn srt_disappearance_is_dropped_event() {
        let cfg = AppConfig::default();
        let mut fb = FrameBuilder::new(&cfg);
        fb.ingest(
            at(0),
            health_ok(),
            None,
            Some(vec![]),
            Some(vec![obs_srt("a", "p", SrtCounters::default())]),
            true,
        );
        let u = fb.ingest(at(2), health_ok(), None, Some(vec![]), Some(vec![]), false);
        assert_eq!(u.removed_srt, vec!["a".to_string()]);
        assert_eq!(u.events.len(), 1);
        assert_eq!(
            u.events[0].kind,
            crate::events::StatusEventKind::SrtConnDropped
        );
    }

    #[test]
    fn events_history_is_bounded() {
        let cfg = AppConfig::default();
        let mut fb = FrameBuilder::new(&cfg);
        fb.ingest(
            at(0),
            health_ok(),
            None,
            Some(vec![obs_path("a", true, 0)]),
            Some(vec![]),
            true,
        );
        for i in 1..3000 {
            let online = i % 2 == 0;
            fb.ingest(
                at(i),
                health_ok(),
                None,
                Some(vec![obs_path("a", online, 0)]),
                Some(vec![]),
                false,
            );
        }
        assert!(fb.events(10_000).len() <= 2_000);
        assert_eq!(fb.events(3).len(), 3);
    }

    #[test]
    fn bytes_per_interval_handles_reset() {
        assert_eq!(bytes_per_interval(10, 100, 1000), 0.0);
        assert!((bytes_per_interval(125_000, 0, 1000) - 1.0).abs() < 1e-9);
    }

    /// 起動直後の読み取り用バッファが、監視側の履歴を取りこぼさないこと。
    ///
    /// 実機の流れは「起動時に空のバッファを置き、以降は毎周期 `merge_from` で
    /// 差分を写す」。`merge_from` がサーバー系列を**最新 1 点だけ** push する実装だと、
    /// 起動直後は 1 点しか無く、5 分窓のグラフが右端の点にしかならない
    /// （ドロップ率が「まだ記録されていません」に見える一因）。
    #[test]
    fn merge_from_catches_up_from_empty_buffer() {
        let cfg = AppConfig::default();
        let mut monitoring = FrameBuilder::new(&cfg);
        // アプリ起動より前から監視が走っていた状態（履歴がある）
        for i in 0..20 {
            monitoring.ingest(
                at(i),
                health_ok(),
                None,
                Some(vec![obs_path("a", true, 0)]),
                Some(vec![]),
                i == 0,
            );
        }

        // 起動時に置かれる空のバッファ
        let mut reading = FrameBuilder::new(&cfg);
        // 起動後の 1 周期目で同期する
        reading.merge_from(&monitoring);

        let mon_len = monitoring
            .server_series()
            .get(metric_key::API_RTT_MS)
            .map(Ring::len)
            .unwrap_or(0);
        let read_len = reading
            .server_series()
            .get(metric_key::API_RTT_MS)
            .map(Ring::len)
            .unwrap_or(0);
        assert_eq!(mon_len, 20);
        assert_eq!(
            read_len, mon_len,
            "起動直後の同期で履歴を取りこぼしている（read={read_len}, monitor={mon_len}）"
        );
    }
}
