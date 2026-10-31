//! 監視エンジンと HTTP 輸送を Tauri 非依存で組み立てる中間層。
//!
//! `web` サブコマンド（ブラウザ向け配信）は Tauri を起動せずに監視を回す
//! 必要があるため、イベント配信先を `Emitter` トレイトで抽象化する。
//! Tauri 側は薄いアダプタ（`TauriEmitter`）を渡すだけ。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use chrono::{DateTime, FixedOffset, Utc};
use mtm_core::config::AppConfig;
use mtm_core::events::{MonitorStatus, MonitorUpdate, StatusEvent};
use mtm_core::frame::FrameBuilder;
use mtm_core::monitor::Monitor;
use mtm_core::transport::HttpTransport;

use crate::transport_ureq::UreqTransport;

/// 監視の出力先。Tauri のイベントとブラウザ配信の両方で使う。
pub trait Sink: Send + Sync + 'static {
    fn update(&self, update: &MonitorUpdate);
    fn status(&self, status: &MonitorStatus);
    fn events(&self, events: &[StatusEvent]) {
        let _ = events;
    }
}

/// 系列 1 本分（メトリクス名と、時刻・値の組）。
pub type SeriesPoints = (String, Vec<(i64, Option<f64>)>);

/// 監視の状態（バックエンドが唯一の正本）。
pub struct Engine {
    config: Mutex<AppConfig>,
    shared: Arc<Shared>,
    sink: Arc<dyn Sink>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    wake: Arc<(Mutex<bool>, Condvar)>,
}

/// スレッドと共有する制御情報。
struct Shared {
    stop: AtomicBool,
    seq: AtomicU64,
    last_status: Mutex<Option<MonitorStatus>>,
    /// 監視スレッドが書き込み、読み取り側が参照する。
    builder: Mutex<Option<FrameBuilder>>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            stop: AtomicBool::new(false),
            seq: AtomicU64::new(0),
            last_status: Mutex::new(None),
            builder: Mutex::new(None),
        }
    }
}

impl Engine {
    pub fn new(config: AppConfig, sink: Arc<dyn Sink>) -> Self {
        Self {
            config: Mutex::new(config.sanitized()),
            shared: Arc::new(Shared::default()),
            sink,
            thread: Mutex::new(None),
            wake: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    pub fn config(&self) -> AppConfig {
        self.config.lock().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn is_running(&self) -> bool {
        self.thread.lock().map(|t| t.is_some()).unwrap_or(false)
    }

    /// 設定を差し替える。実行中なら再起動して反映する。
    pub fn apply_config(&self, cfg: AppConfig) -> Result<(), String> {
        let running = self.is_running();
        if let Ok(mut slot) = self.config.lock() {
            *slot = cfg.clone().sanitized();
        }
        if running {
            self.stop_inner();
            self.start_inner(cfg)?;
        }
        Ok(())
    }

    pub fn start(&self, cfg: AppConfig) -> Result<(), String> {
        if self.is_running() {
            self.stop_inner();
        }
        self.start_inner(cfg)
    }

    fn start_inner(&self, cfg: AppConfig) -> Result<(), String> {
        let cfg = cfg.sanitized();
        if let Ok(mut slot) = self.config.lock() {
            *slot = cfg.clone();
        }
        let shared = self.shared.clone();
        let wake = self.wake.clone();
        let sink = self.sink.clone();
        let started_at = Utc::now().fixed_offset();
        self.shared.stop.store(false, Ordering::SeqCst);
        {
            let (lock, _) = &*wake;
            if let Ok(mut flag) = lock.lock() {
                *flag = false;
            }
        }
        if let Ok(mut slot) = self.shared.builder.lock() {
            *slot = Some(FrameBuilder::new(&cfg));
        }

        let thread = std::thread::Builder::new()
            .name("mtm-monitor".to_string())
            .spawn(move || run_loop(cfg, shared, wake, sink, started_at))
            .map_err(|e| format!("監視スレッドを開始できませんでした: {e}"))?;
        if let Ok(mut slot) = self.thread.lock() {
            *slot = Some(thread);
        }
        self.publish_status(true);
        Ok(())
    }

    pub fn stop(&self) {
        self.stop_inner();
        self.publish_status(false);
    }

    /// アプリ終了時など、イベント配信を伴わずに止めたい場合。
    pub fn stop_quietly(&self) {
        self.stop_inner();
    }

    fn stop_inner(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        {
            let (lock, cvar) = &*self.wake;
            if let Ok(mut flag) = lock.lock() {
                *flag = true;
                cvar.notify_all();
            }
        }
        if let Some(handle) = self.thread.lock().ok().and_then(|mut t| t.take()) {
            let _ = handle.join();
        }
    }

    /// 現在の状態。
    pub fn status(&self) -> MonitorStatus {
        let running = self.is_running();
        let cfg = self.config();
        let mut status = self
            .shared
            .last_status
            .lock()
            .ok()
            .and_then(|s| s.clone())
            .unwrap_or_default();
        status.running = running;
        status.interval_ms = cfg.poll_interval_ms;
        status.seq = self.shared.seq.load(Ordering::SeqCst);
        status
    }

    fn publish_status(&self, running: bool) {
        let mut status = self.status();
        status.running = running;
        self.sink.status(&status);
    }

    /// 読み取り専用コマンド用に `FrameBuilder` を借りる。
    pub fn with_builder<T>(&self, f: impl FnOnce(&FrameBuilder) -> T) -> Option<T> {
        let guard = self.shared.builder.lock().ok()?;
        guard.as_ref().map(f)
    }

    /// 最新の全量スナップショット。
    pub fn snapshot(&self) -> Option<MonitorUpdate> {
        self.with_builder(|b| b.snapshot(Utc::now().fixed_offset()))
    }

    /// 指定スコープの系列。
    pub fn series(
        &self,
        scope: &str,
        metrics: &[String],
        from_ms: i64,
        to_ms: i64,
    ) -> Option<Vec<SeriesPoints>> {
        let result = self.with_builder(|b| {
            // スコープは `server` / `path:<名前>` / `srt:<ID>`。
            // フロントは「1 グラフ 1 スコープ」のため `<scope>:<metric>` で問い合わせる
            // ことがあるので、その場合も元のスコープとして扱う。
            let base_scope = scope
                .strip_suffix(&format!(
                    ":{}",
                    metrics.first().map(String::as_str).unwrap_or("")
                ))
                .unwrap_or(scope);
            let set = if base_scope == "server" {
                Some(b.server_series())
            } else if let Some(name) = base_scope.strip_prefix("path:") {
                b.path_series(name)
            } else if let Some(id) = base_scope.strip_prefix("srt:") {
                b.srt_series(id)
            } else {
                None
            }?;
            let mut out = Vec::new();
            for metric in metrics {
                let points = set
                    .range(metric, Some(from_ms), Some(to_ms))
                    .into_iter()
                    .map(|p| (p.at_ms, p.value))
                    .collect::<Vec<_>>();
                out.push((metric.clone(), points));
            }
            Some(out)
        });
        result.flatten()
    }

    pub fn events(&self, limit: usize) -> Vec<StatusEvent> {
        self.with_builder(|b| b.events(limit)).unwrap_or_default()
    }

    /// トレイ表示用の要約（稼働パス数・全パス数・SRT 接続数）。
    pub fn totals_summary(&self) -> (usize, usize, usize) {
        self.with_builder(|b| {
            let mut online = 0usize;
            let mut total = 0usize;
            for name in b.path_names() {
                total += 1;
                let flag = b
                    .path_series(name)
                    .and_then(|s| s.get("online").and_then(|r| r.last()))
                    .and_then(|p| p.value)
                    .unwrap_or(0.0);
                if flag > 0.5 {
                    online += 1;
                }
            }
            (online, total, b.srt_conn_count())
        })
        .unwrap_or((0, 0, 0))
    }
}

fn sample_count(builder: &FrameBuilder) -> usize {
    builder.server_series().sample_count()
}

fn run_loop(
    cfg: AppConfig,
    shared: Arc<Shared>,
    wake: Arc<(Mutex<bool>, Condvar)>,
    sink: Arc<dyn Sink>,
    started_at: DateTime<FixedOffset>,
) {
    let transport: Box<dyn HttpTransport> =
        Box::new(UreqTransport::new(cfg.endpoint.allow_insecure_tls));
    let mut monitor = Monitor::new(transport.as_ref(), cfg.clone());
    monitor.mark_started(started_at);
    let mut first = true;

    while !shared.stop.load(Ordering::SeqCst) {
        let at = Utc::now().fixed_offset();
        let result = monitor.tick(at);
        let mut update = result.update;
        update.full = first;
        shared.seq.store(update.seq, Ordering::SeqCst);

        // リング全体を複製せず、対象ごとに読み取り用バッファへ写す。
        if let Ok(mut slot) = shared.builder.lock() {
            match slot.as_mut() {
                Some(buf) => buf.merge_from(monitor.builder()),
                None => *slot = Some(monitor.builder().clone()),
            }
        }

        let status = MonitorStatus {
            running: true,
            interval_ms: cfg.poll_interval_ms,
            started_at: Some(started_at),
            last_tick_at: Some(at),
            seq: update.seq,
            server_version: update.server_version.clone(),
            server_started: None,
            api: update.api.clone(),
            fatal_error: None,
            total_samples: sample_count(monitor.builder()),
        };
        if let Ok(mut slot) = shared.last_status.lock() {
            *slot = Some(status.clone());
        }
        sink.status(&status);

        if !update.events.is_empty() {
            sink.events(&update.events);
        }
        sink.update(&update);
        first = false;

        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        // 次周期まで待つ。停止要求は 50ms 粒度で検知できるので、これで十分。
        // （`Condvar` を使うと状態遷移の取りこぼしで待機が素通りすることがあった）
        let deadline = std::time::Instant::now() + Duration::from_millis(cfg.poll_interval_ms);
        while std::time::Instant::now() < deadline {
            if shared.stop.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let _ = wake;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用のシンク（受信内容を記録する）。
    #[derive(Default)]
    struct RecordingSink {
        statuses: Mutex<Vec<MonitorStatus>>,
        updates: Mutex<Vec<u64>>,
    }

    impl Sink for RecordingSink {
        fn update(&self, update: &MonitorUpdate) {
            self.updates.lock().unwrap().push(update.seq);
        }
        fn status(&self, status: &MonitorStatus) {
            self.statuses.lock().unwrap().push(status.clone());
        }
    }

    #[test]
    fn start_and_stop_emit_status() {
        let sink = Arc::new(RecordingSink::default());
        let engine = Engine::new(AppConfig::default(), sink.clone());
        assert!(!engine.is_running());
        engine.start(AppConfig::default()).expect("start");
        assert!(engine.is_running());
        engine.stop();
        assert!(!engine.is_running());
        let statuses = sink.statuses.lock().unwrap();
        assert!(statuses.iter().any(|s| s.running));
        assert!(statuses.iter().any(|s| !s.running));
    }

    #[test]
    fn stopped_engine_has_no_snapshot() {
        let sink = Arc::new(RecordingSink::default());
        let engine = Engine::new(AppConfig::default(), sink);
        assert!(engine.snapshot().is_none());
        assert_eq!(engine.totals_summary(), (0, 0, 0));
    }
}
