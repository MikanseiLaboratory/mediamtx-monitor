//! 設定の読み書き。認証情報は保存しない（`AppConfig::endpoint` の
//! `password` / `token` は `#[serde(skip)]` なので JSON に出ない）。

use std::path::Path;

use mtm_core::config::AppConfig;

use crate::paths::AppPaths;

/// 設定を読み込む。ファイルが無ければ既定値を返す。
pub fn load(paths: &AppPaths) -> Result<AppConfig, String> {
    load_from(&paths.settings_file)
}

pub fn load_from(file: &Path) -> Result<AppConfig, String> {
    if !file.exists() {
        return Ok(AppConfig::default());
    }
    let text = std::fs::read_to_string(file).map_err(|e| {
        format!(
            "設定ファイルを読み込めませんでした ({}): {e}",
            file.display()
        )
    })?;
    if text.trim().is_empty() {
        return Ok(AppConfig::default());
    }
    let cfg: AppConfig = serde_json::from_str(&text)
        .map_err(|e| format!("設定ファイルの形式が不正です ({}): {e}", file.display()))?;
    Ok(cfg.sanitized())
}

/// 設定を保存する。親ディレクトリは自動作成し、一時ファイル経由で
/// 書き換える（書き込み中のクラッシュで壊れないように）。
pub fn save(paths: &AppPaths, cfg: &AppConfig) -> Result<(), String> {
    save_to(&paths.settings_file, cfg)
}

pub fn save_to(file: &Path, cfg: &AppConfig) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "設定フォルダを作成できませんでした ({}): {e}",
                parent.display()
            )
        })?;
    }
    let sanitized = cfg.clone().sanitized();
    let json = serde_json::to_string_pretty(&sanitized)
        .map_err(|e| format!("設定を JSON に変換できませんでした: {e}"))?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())
        .map_err(|e| format!("設定を書き込めませんでした ({}): {e}", tmp.display()))?;
    std::fs::rename(&tmp, file)
        .map_err(|e| format!("設定を確定できませんでした ({}): {e}", file.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtm_core::config::{AuthMode, EndpointConfig};

    #[test]
    fn roundtrip_preserves_settings_except_secrets() {
        let dir = crate::test_support::unique_temp_dir("settings");
        let _ = std::fs::remove_dir_all(&dir);
        let paths = AppPaths::for_test(dir.clone());
        let cfg = AppConfig {
            endpoint: EndpointConfig {
                auth: AuthMode::Basic,
                user: "alice".into(),
                password: "secret".into(),
                ..EndpointConfig::default()
            },
            poll_interval_ms: 3000,
            ..AppConfig::default()
        };
        save(&paths, &cfg).expect("save");
        let raw = std::fs::read_to_string(&paths.settings_file).unwrap();
        assert!(!raw.contains("secret"), "パスワードが保存されてはいけない");
        let back = load(&paths).expect("load");
        assert_eq!(back.poll_interval_ms, 3000);
        assert_eq!(back.endpoint.user, "alice");
        assert_eq!(back.endpoint.password, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = crate::test_support::unique_temp_dir("missing");
        let paths = AppPaths::for_test(dir.clone());
        let cfg = load(&paths).expect("default");
        assert_eq!(cfg.poll_interval_ms, AppConfig::default().poll_interval_ms);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn broken_json_is_reported() {
        let dir = crate::test_support::unique_temp_dir("broken");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        std::fs::write(&file, "{not json").unwrap();
        let err = load_from(&file).unwrap_err();
        assert!(err.contains("形式が不正"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
