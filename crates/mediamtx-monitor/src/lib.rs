//! Tauri v2 アダプタ。
//!
//! 監視の実体は `engine`（Tauri 非依存）にあり、ここでは Tauri のイベントへの配信、
//! `invoke` ハンドラの登録、ウィンドウ・トレイ・通知だけを扱う。
//! 状態の正本は常にバックエンド側にある。
//!
//! フロントとの通信は Tauri 標準の IPC（`invoke` コマンドと `listen` イベント）だけを使う。

pub mod cli;
pub mod commands;
pub mod commands_core;
pub mod engine;
pub mod notify;
pub mod paths;
pub mod settings_store;
pub mod transport_ureq;
pub mod tray;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use mtm_core::events::{MonitorStatus, MonitorUpdate, StatusEvent};
use tauri::{Emitter, Manager};

use crate::engine::{Engine, Sink};

/// テスト用の補助（プロセス内で一意な一時ディレクトリ）。
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 一意なディレクトリを作って返す。`%TEMP%` が使えない環境では
    /// 作業ディレクトリ配下の `target` にフォールバックする。
    pub fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let name = format!("mtm-test-{tag}-{}-{n}", std::process::id());
        let mut candidates = vec![std::env::temp_dir().join(&name)];
        candidates.push(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target")
                .join(&name),
        );
        for dir in candidates {
            if std::fs::create_dir_all(&dir).is_ok() {
                return dir;
            }
        }
        panic!("テスト用ディレクトリを作成できません");
    }
}
/// フロントへ配るイベント名。`web/src/bridge/index.ts` と一致させる。
pub mod events {
    pub const UPDATE: &str = "monitor://update";
    pub const STATUS: &str = "monitor://status";
}

/// アプリの共有状態。
pub struct AppState {
    pub engine: Arc<Engine>,
    pub paths: paths::AppPaths,
    pub shutdown: Arc<AtomicBool>,
    /// 通知設定（設定変更時に更新する）。
    pub notify: Arc<Mutex<mtm_core::config::NotifyRules>>,
}

/// Tauri イベントへの配信アダプタ。
struct TauriSink {
    app: tauri::AppHandle,
    notify: Arc<Mutex<mtm_core::config::NotifyRules>>,
}

impl Sink for TauriSink {
    fn update(&self, update: &MonitorUpdate) {
        let _ = self.app.emit(events::UPDATE, update);
    }
    fn status(&self, status: &MonitorStatus) {
        let _ = self.app.emit(events::STATUS, status);
    }
    fn events(&self, events: &[StatusEvent]) {
        if let Ok(rules) = self.notify.lock() {
            notify::dispatch(&self.app, &rules, events);
        }
    }
}

/// アプリ起動（ネイティブウィンドウ）。
pub fn run(smoke_exit_after_ms: Option<u64>) -> tauri::Result<()> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let notify_rules = Arc::new(Mutex::new(mtm_core::config::NotifyRules::default()));
    let notify_for_setup = notify_rules.clone();
    let shutdown_for_setup = shutdown.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let paths = paths::AppPaths::resolve(&handle);
            let config = settings_store::load(&paths).unwrap_or_default();
            if let Ok(mut rules) = notify_for_setup.lock() {
                *rules = config.notify.clone();
            }

            let sink = Arc::new(TauriSink {
                app: handle.clone(),
                notify: notify_for_setup.clone(),
            });
            let engine = Arc::new(Engine::new(config.clone(), sink));
            app.manage(AppState {
                engine: engine.clone(),
                paths: paths.clone(),
                shutdown: shutdown_for_setup.clone(),
                notify: notify_for_setup.clone(),
            });

            // ウィンドウは設定ファイルからの自動生成ではなく、ここで明示的に作る。
            // 失敗しても理由を残して起動を続ける（画面が出ない原因を追えるように）。
            if let Some(cfg) = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
            {
                let mut cfg = cfg;
                // 画面外に開かないよう、プライマリモニタの中央に置く。
                if let Ok(Some(monitor)) = handle.primary_monitor() {
                    let scale = monitor.scale_factor();
                    let size = monitor.size().to_logical::<f64>(scale);
                    let (w, h) = (cfg.width, cfg.height);
                    if size.width > w && size.height > h {
                        cfg.x = Some(((size.width - w) / 2.0).floor());
                        cfg.y = Some(((size.height - h) / 2.0).floor());
                    }
                }
                match tauri::WebviewWindowBuilder::from_config(&handle, &cfg) {
                    Ok(builder) => match builder.build() {
                        Ok(window) => {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                        Err(err) => {
                            eprintln!("メインウィンドウを作成できませんでした: {err}");
                            log::error!("メインウィンドウを作成できませんでした: {err}");
                        }
                    },
                    Err(err) => {
                        eprintln!("ウィンドウ設定が不正です: {err}");
                        log::error!("ウィンドウ設定が不正です: {err}");
                    }
                }
            }

            if let Some(tray) = tray::install(app, engine.clone()) {
                tray::spawn_tooltip_updater(tray, engine.clone());
            }

            if config.auto_start {
                if let Err(err) = engine.start(config.clone()) {
                    log::error!("監視を開始できませんでした: {err}");
                }
            }

            if let Some(ms) = smoke_exit_after_ms {
                let handle = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                    log::info!("smoke-exit: 指定時間に達したため終了します");
                    handle.exit(0);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.app_handle().state::<AppState>();
                if state.engine.config().minimize_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::apply_settings,
            commands::test_connection,
            commands::start_monitor,
            commands::stop_monitor,
            commands::get_monitor_status,
            commands::get_snapshot,
            commands::get_update,
            commands::get_series,
            commands::get_events,
            commands::list_paths,
            commands::get_path_conf,
            commands::add_path,
            commands::patch_path,
            commands::delete_path,
            commands::get_global_config,
            commands::patch_global_config,
            commands::list_connections,
            commands::kick_connection,
            commands::console_get,
            commands::export_range,
        ])
        .run(tauri::generate_context!())?;
    let _ = shutdown;
    Ok(())
}
