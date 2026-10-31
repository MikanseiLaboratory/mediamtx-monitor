//! フロントへ流すワイヤ型（`serde` で camelCase JSON に変換）。
//!
//! フロントはこれらを受け取って描画するだけで、状態の計算は一切しない。
//! 1 秒周期の `MonitorUpdate` は差分（変更のあった対象だけ）を含むので、
//! フロントは受信バッファにマージするだけで最新状態を保てる。

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::srt::{SrtDerived, SrtSeverity};

/// API 到達性。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiHealth {
    pub ok: bool,
    pub rtt_ms: Option<f64>,
    pub error: Option<ApiError>,
    /// この回数連続で失敗している（成功で 0 に戻る）。
    pub consecutive_failures: u32,
    /// メトリクス取得の可否。
    pub metrics_ok: bool,
    pub metrics_error: Option<ApiError>,
}

/// 監視エンジンの実行状態。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatus {
    pub running: bool,
    pub interval_ms: u64,
    pub started_at: Option<DateTime<FixedOffset>>,
    pub last_tick_at: Option<DateTime<FixedOffset>>,
    /// 完了した周期の通し番号（フロントは更新の取りこぼし検出に使う）。
    pub seq: u64,
    pub server_version: Option<String>,
    pub server_started: Option<DateTime<FixedOffset>>,
    pub api: ApiHealth,
    /// 最後にエンジンが致命エラーで止まった場合のメッセージ。
    pub fatal_error: Option<String>,
    pub total_samples: usize,
}

/// パスの 1 スナップショット（フロントのカード表示用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathSample {
    pub name: String,
    pub conf_name: String,
    /// API 上の `available`（パスが利用可能か）。
    pub available: bool,
    /// API 上の `online`（ソース接続中か）。死活監視の主判定。
    pub online: bool,
    pub source_type: String,
    pub reader_count: usize,
    pub codecs: Vec<String>,
    pub inbound_bytes: u64,
    pub outbound_bytes: u64,
    pub inbound_frames_in_error: u64,
    /// 受信ビットレート (Mbps)。差分から算出（初回は None）。
    pub inbound_mbps: Option<f64>,
    pub outbound_mbps: Option<f64>,
    /// フレームエラーの毎秒増分。
    pub frames_in_error_per_sec: Option<f64>,
    pub online_since: Option<DateTime<FixedOffset>>,
    pub available_since: Option<DateTime<FixedOffset>>,
    pub severity: PathSeverity,
    /// このパスに紐づく SRT 接続 ID。
    pub srt_conn_ids: Vec<String>,
}

/// パスの状態区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathSeverity {
    /// online=true
    Healthy,
    /// online=false だが available=true（ソース待ち）
    Degraded,
    /// online=false かつ available=false、またはパス消失
    Down,
}

/// SRT 接続のスナップショット（SRT ダッシュボードの主データ）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SrtConnSample {
    pub id: String,
    pub path: String,
    pub remote_addr: String,
    pub state: String,
    pub created: Option<DateTime<FixedOffset>>,
    pub counters: crate::srt::SrtCounters,
    pub derived: SrtDerived,
    pub severity: SrtSeverity,
    /// サーバーメトリクス由来か（API のみの場合は false）。
    pub from_metrics: bool,
}

/// ダッシュボード用の全体集計。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerTotals {
    pub path_count: usize,
    pub paths_online: usize,
    pub paths_down: usize,
    pub reader_count: usize,
    pub inbound_mbps: f64,
    pub outbound_mbps: f64,
    pub srt_conn_count: usize,
    /// 全 SRT 接続のうち最大のドロップ率 (%)。
    pub srt_max_drop_rate: Option<f64>,
    /// 全 SRT 接続のうち最小の回復率 (%)。
    pub srt_min_recovery_rate: Option<f64>,
    pub srt_worst_severity: Option<SrtSeverity>,
}

/// パス単位の差分。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathPatch {
    pub seq: u64,
    pub sample: PathSample,
}

/// SRT 接続単位の差分。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SrtConnPatch {
    pub seq: u64,
    pub sample: SrtConnSample,
}

/// 1 周期分の更新（イベント `monitor://update`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorUpdate {
    pub seq: u64,
    pub at: DateTime<FixedOffset>,
    pub api: ApiHealth,
    pub totals: ServerTotals,
    pub paths: Vec<PathPatch>,
    pub srt: Vec<SrtConnPatch>,
    /// 消えたパス名。
    pub removed_paths: Vec<String>,
    /// 消えた SRT 接続 ID。
    pub removed_srt: Vec<String>,
    /// その周期で発生した状態遷移（通常 0 件）。
    pub events: Vec<StatusEvent>,
    pub server_version: Option<String>,
    pub full: bool,
}

/// 状態遷移イベント。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusEvent {
    pub seq: u64,
    pub at: DateTime<FixedOffset>,
    pub kind: StatusEventKind,
    /// 対象（SRT 接続 ID またはパス名）。
    pub target: String,
    /// 対象の表示名（パス名など）。
    pub label: String,
    pub message: String,
    /// 通知すべき重大度。
    pub severity: EventSeverity,
}

/// イベントの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusEventKind {
    SrtConnAppeared,
    SrtConnDropped,
    SrtDegraded,
    SrtRecovered,
    PathDown,
    PathRecovered,
    PathOnlineStopped,
    PathOnlineStarted,
    SourceChanged,
    ApiUnreachable,
    ApiRestored,
    MonitorStarted,
    MonitorStopped,
}

impl StatusEventKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::SrtConnAppeared => "srt_conn_appeared",
            Self::SrtConnDropped => "srt_conn_dropped",
            Self::SrtDegraded => "srt_degraded",
            Self::SrtRecovered => "srt_recovered",
            Self::PathDown => "path_down",
            Self::PathRecovered => "path_recovered",
            Self::PathOnlineStopped => "path_online_stopped",
            Self::PathOnlineStarted => "path_online_started",
            Self::SourceChanged => "source_changed",
            Self::ApiUnreachable => "api_unreachable",
            Self::ApiRestored => "api_restored",
            Self::MonitorStarted => "monitor_started",
            Self::MonitorStopped => "monitor_stopped",
        }
    }
}

/// イベントの重大度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSeverity {
    Info,
    Warning,
    Critical,
}
