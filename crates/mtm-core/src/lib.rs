//! mtm-core — MediaMTX Monitor の中核。
//! このクレートは **UI も Tauri も知らない**。MediaMTX の Control API と
//! Prometheus メトリクスから観測値を取り込み、SRT の品質指標（パケットの
//! ドロップ率・回復率など）を差分計算し、監視状態機械・時系列リング・
//! 期間指定エクスポートまでを担う。Tauri アダプタ (`mediamtx-monitor`) は
//! HTTP 輸送とイベント配信だけを差し込む薄い層として実装される。
//!
//! 層構成:
//!
//! ```text
//! transport ──> client ──> frame ──> monitor
//!                  │                      │
//!          prom / model / srt        state / events
//!                                        │
//!                              ring / export / config
//! ```

pub mod client;
pub mod config;
pub mod error;
pub mod events;
pub mod export;
pub mod frame;
pub mod model;
pub mod monitor;
pub mod prom;
pub mod ring;
pub mod srt;
pub mod state;
pub mod transport;

pub use config::{AppConfig, AuthMode, EndpointConfig, SrtThresholds};
pub use error::{ApiError, ApiErrorKind};
pub use events::{
    ApiHealth, MonitorStatus, MonitorUpdate, PathPatch, PathSample, ServerTotals, SrtConnPatch,
    SrtConnSample, StatusEvent, StatusEventKind,
};
pub use monitor::Monitor;
