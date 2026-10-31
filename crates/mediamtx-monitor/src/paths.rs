//! アプリが使うファイルパスの解決。

use std::path::PathBuf;
use tauri::Manager;

/// 設定・ログ・既定のエクスポート先。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// `%APPDATA%\<identifier>\settings.json`
    pub settings_file: PathBuf,
    /// `%APPDATA%\<identifier>\logs`
    pub log_dir: PathBuf,
    /// エクスポートの既定フォルダ（ドキュメント）。
    pub export_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &tauri::AppHandle) -> Self {
        let config_dir = app
            .path()
            .app_config_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("mtm-laboratory"));
        let export_dir = app.path().document_dir();
        let export_dir = export_dir.unwrap_or_else(|_| std::env::temp_dir());
        Self {
            settings_file: config_dir.join("settings.json"),
            log_dir: config_dir.join("logs"),
            export_dir,
        }
    }

    /// テスト用にディレクトリを指定して組み立てる。
    pub fn for_test(root: PathBuf) -> Self {
        Self {
            settings_file: root.join("settings.json"),
            log_dir: root.join("logs"),
            export_dir: root.join("exports"),
        }
    }
}
