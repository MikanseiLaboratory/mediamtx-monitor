//! システムトレイ。メニューと、監視状態を映すツールチップを持つ。
//!
//! トレイが使えない環境（権限など）でもアプリは起動させる。作成に失敗した
//! 場合は `None` を返し、呼び出し側が警告を出すだけにする。

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, Manager};

use crate::engine::Engine;

const MENU_SHOW: &str = "show";
const MENU_START: &str = "start";
const MENU_STOP: &str = "stop";
const MENU_QUIT: &str = "quit";

/// トレイを作る。失敗しても致命的ではないので `None` を返す。
pub fn install(app: &mut App, engine: Arc<Engine>) -> Option<TrayIcon> {
    let show = MenuItem::with_id(app, MENU_SHOW, "ウィンドウを表示", true, None::<&str>).ok()?;
    let start = MenuItem::with_id(app, MENU_START, "監視を開始", true, None::<&str>).ok()?;
    let stop = MenuItem::with_id(app, MENU_STOP, "監視を停止", true, None::<&str>).ok()?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "終了", true, None::<&str>).ok()?;
    let sep = PredefinedMenuItem::separator(app).ok()?;
    let menu = Menu::with_items(app, &[&show, &sep, &start, &stop, &sep, &quit]).ok()?;

    let handle = app.handle().clone();
    let engine_for_menu = engine.clone();
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(tooltip_for(false, false, 0, 0, 0))
        .on_menu_event(move |_app, event| match event.id.as_ref() {
            MENU_SHOW => {
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            MENU_START => {
                let cfg = engine_for_menu.config();
                if let Err(err) = engine_for_menu.start(cfg) {
                    log::warn!("監視を開始できませんでした: {err}");
                }
            }
            MENU_STOP => engine_for_menu.stop(),
            MENU_QUIT => {
                engine_for_menu.stop_quietly();
                handle.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button, .. } = event {
                if button == tauri::tray::MouseButton::Left {
                    if let Some(window) = tray.app_handle().get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    match builder.build(app) {
        Ok(tray) => Some(tray),
        Err(err) => {
            log::warn!("トレイアイコンを作成できませんでした（継続します）: {err}");
            None
        }
    }
}

/// 状態が変わるたびにツールチップを更新するスレッドを起こす。
pub fn spawn_tooltip_updater(tray: TrayIcon, engine: Arc<Engine>) {
    std::thread::Builder::new()
        .name("mtm-tray".to_string())
        .spawn(move || {
            let mut last = String::new();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let status = engine.status();
                let (online, total, srt) = engine.totals_summary();
                let text = tooltip_for(status.running, status.api.ok, online, total, srt);
                if text != last {
                    if tray.set_tooltip(Some(&text)).is_err() {
                        // トレイが消えたら終了する。
                        return;
                    }
                    last = text;
                }
            }
        })
        .ok();
}

/// ツールチップ文言を監視状態から組み立てる。
pub fn tooltip_for(
    running: bool,
    api_ok: bool,
    paths_online: usize,
    paths_total: usize,
    srt_conns: usize,
) -> String {
    if !running {
        return "MediaMTX Monitor — 監視停止中".to_string();
    }
    if !api_ok {
        return "MediaMTX Monitor — MediaMTX に接続できません".to_string();
    }
    format!("MediaMTX Monitor — パス {paths_online}/{paths_total} 稼働 / SRT 接続 {srt_conns}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_reflects_state() {
        assert!(tooltip_for(false, false, 0, 0, 0).contains("停止"));
        assert!(tooltip_for(true, false, 0, 0, 0).contains("接続できません"));
        let t = tooltip_for(true, true, 2, 3, 5);
        assert!(t.contains("2/3"), "{t}");
        assert!(t.contains("5"), "{t}");
    }
}
