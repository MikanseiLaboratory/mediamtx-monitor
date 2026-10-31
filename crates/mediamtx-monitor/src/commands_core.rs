//! Tauri 非依存のコマンド実装。
//!
//! Tauri の `invoke` ハンドラと、`--browser` 配信の HTTP 経由の両方から
//! 呼ばれる。状態（`Engine`）と設定保存先だけを受け取るので、UI 側の
//! 事情に依存しない。

use std::path::{Path, PathBuf};

use mtm_core::client::ApiClient;
use mtm_core::config::AppConfig;
use mtm_core::error::ApiError;
use mtm_core::events::{MonitorStatus, MonitorUpdate, StatusEvent};
use mtm_core::export::{self, CsvColumn, ExportFormat, SvgSeries, SvgSize};
use mtm_core::model::{ConnInfo, ConnKind};
use mtm_core::monitor::Monitor;
use serde::Serialize;
use serde_json::{json, Value};

use crate::engine::Engine;
use crate::transport_ureq::UreqTransport;

/// コマンドの実行結果（エラーは利用者向けの文字列）。
pub type CmdResult<T> = std::result::Result<T, String>;

fn transport_for(cfg: &AppConfig) -> UreqTransport {
    UreqTransport::new(cfg.endpoint.allow_insecure_tls)
}

fn client<'a>(transport: &'a UreqTransport, cfg: &AppConfig) -> CmdResult<ApiClient<'a>> {
    ApiClient::new(transport, &cfg.endpoint, cfg.request_timeout_ms).map_err(|e| e.message)
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn arg_object(args: &Value, key: &str) -> CmdResult<Value> {
    match args.get(key) {
        Some(v) if v.is_object() => Ok(v.clone()),
        Some(_) => Err(format!("`{key}` は JSON オブジェクトで指定してください。")),
        None => Err(format!("`{key}` が指定されていません。")),
    }
}

fn to_value<T: Serialize>(value: T) -> CmdResult<Value> {
    serde_json::to_value(value).map_err(|e| format!("応答を JSON に変換できませんでした: {e}"))
}

/// `command` を実行して JSON を返す。
pub fn dispatch(engine: &Engine, command: &str, args: &Value) -> CmdResult<Value> {
    match command {
        "get_settings" => to_value(engine.config()),
        "save_settings" | "apply_settings" => {
            let parsed: AppConfig = serde_json::from_value(
                args.get("config")
                    .cloned()
                    .ok_or_else(|| "`config` がありません。".to_string())?,
            )
            .map_err(|e| format!("設定の形式が不正です: {e}"))?;
            // 認証情報は保存しないので、直前までのメモリ上の値を引き継ぐ。
            let merged = merge_secrets(engine.config(), parsed);
            engine.apply_config(merged.clone())?;
            to_value(merged)
        }
        "test_connection" => {
            let parsed: AppConfig = serde_json::from_value(
                args.get("config")
                    .cloned()
                    .ok_or_else(|| "`config` がありません。".to_string())?,
            )
            .map_err(|e| format!("設定の形式が不正です: {e}"))?;
            let cfg = parsed.sanitized();
            let transport = transport_for(&cfg);
            let probe = Monitor::test_connection(&transport, &cfg).map_err(api_err)?;
            to_value(probe)
        }
        "start_monitor" => {
            let cfg = engine.config();
            engine.start(cfg)?;
            to_value(engine.status())
        }
        "stop_monitor" => {
            engine.stop();
            to_value(engine.status())
        }
        "get_monitor_status" => to_value(engine.status()),
        "get_snapshot" => engine
            .snapshot()
            .ok_or_else(|| "監視が停止しているためスナップショットを取得できません。".to_string())
            .and_then(to_value),
        "get_events" => {
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(200)
                .clamp(1, 2000) as usize;
            to_value(engine.events(limit))
        }
        "get_series" => {
            let req = args
                .get("request")
                .cloned()
                .ok_or_else(|| "`request` がありません。".to_string())?;
            let scope =
                arg_str(&req, "scope").ok_or_else(|| "`scope` がありません。".to_string())?;
            let metrics: Vec<String> = req
                .get("metrics")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let to = req
                .get("toMs")
                .and_then(|v| v.as_i64())
                .unwrap_or_else(now_ms);
            let from = req
                .get("fromMs")
                .and_then(|v| v.as_i64())
                .unwrap_or(to - 3_600_000);
            let series = engine
                .series(&scope, &metrics, from, to)
                .ok_or_else(|| format!("スコープ `{scope}` の系列がありません。"))?;
            to_value(json!({
                "scope": scope,
                "fromMs": from,
                "toMs": to,
                "series": series,
            }))
        }
        "list_paths" => {
            let cfg = engine.config();
            let source = arg_str(args, "source").unwrap_or_else(|| "runtime".to_string());
            let transport = transport_for(&cfg);
            let c = client(&transport, &cfg)?;
            if source == "config" {
                c.list_config_paths().map_err(api_err).and_then(to_value)
            } else {
                c.list_paths().map_err(api_err).and_then(to_value)
            }
        }
        "get_path_conf" => {
            let name = arg_str(args, "name").ok_or_else(|| "`name` がありません。".to_string())?;
            let cfg = engine.config();
            let transport = transport_for(&cfg);
            client(&transport, &cfg)?
                .get_path_conf(&name)
                .map_err(api_err)
        }
        "add_path" => {
            let name = arg_str(args, "name").ok_or_else(|| "`name` がありません。".to_string())?;
            validate_path_name(&name)?;
            let conf = arg_object(args, "conf")?;
            let cfg = engine.config();
            let transport = transport_for(&cfg);
            client(&transport, &cfg)?
                .add_path(&name, &conf)
                .map_err(api_err)?;
            Ok(Value::Null)
        }
        "patch_path" => {
            let name = arg_str(args, "name").ok_or_else(|| "`name` がありません。".to_string())?;
            validate_path_name(&name)?;
            let diff = arg_object(args, "diff").or_else(|_| arg_object(args, "conf"))?;
            if diff.as_object().is_some_and(|m| m.is_empty()) {
                return Err("変更項目がありません。".to_string());
            }
            let cfg = engine.config();
            let transport = transport_for(&cfg);
            client(&transport, &cfg)?
                .patch_path(&name, &diff)
                .map_err(api_err)?;
            Ok(Value::Null)
        }
        "delete_path" => {
            let name = arg_str(args, "name").ok_or_else(|| "`name` がありません。".to_string())?;
            validate_path_name(&name)?;
            let cfg = engine.config();
            let transport = transport_for(&cfg);
            client(&transport, &cfg)?
                .delete_path(&name)
                .map_err(api_err)?;
            Ok(Value::Null)
        }
        "get_global_config" => {
            let cfg = engine.config();
            let transport = transport_for(&cfg);
            client(&transport, &cfg)?.global_config().map_err(api_err)
        }
        "patch_global_config" => {
            let patch = arg_object(args, "patch")?;
            if patch.as_object().is_some_and(|m| m.is_empty()) {
                return Err("変更項目がありません。".to_string());
            }
            let cfg = engine.config();
            let transport = transport_for(&cfg);
            client(&transport, &cfg)?
                .patch_global_config(&patch)
                .map_err(api_err)?;
            Ok(Value::Null)
        }
        "list_connections" => {
            let kind = parse_kind(&arg_str(args, "kind").unwrap_or_default())?;
            let cfg = engine.config();
            let transport = transport_for(&cfg);
            let items: Vec<ConnInfo> = client(&transport, &cfg)?
                .list_conns(kind)
                .map_err(api_err)?;
            to_value(items)
        }
        "kick_connection" => {
            let kind = parse_kind(&arg_str(args, "kind").unwrap_or_default())?;
            let id = arg_str(args, "id").ok_or_else(|| "`id` がありません。".to_string())?;
            let cfg = engine.config();
            let transport = transport_for(&cfg);
            client(&transport, &cfg)?
                .kick_conn(kind, &id)
                .map_err(api_err)?;
            Ok(Value::Null)
        }
        "console_get" => {
            let path = arg_str(args, "path").ok_or_else(|| "`path` がありません。".to_string())?;
            let cfg = engine.config();
            let transport = transport_for(&cfg);
            let (status, body) = client(&transport, &cfg)?
                .console_get(&path)
                .map_err(api_err)?;
            Ok(json!([status, body]))
        }
        "export_range" => {
            let req = args
                .get("request")
                .cloned()
                .ok_or_else(|| "`request` がありません。".to_string())?;
            export_range(engine, &req)
        }
        other => Err(format!("未対応のコマンドです: {other}")),
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn api_err(err: ApiError) -> String {
    err.message
}

fn merge_secrets(previous: AppConfig, next: AppConfig) -> AppConfig {
    let mut merged = next.sanitized();
    if merged.endpoint.password.is_empty() {
        merged.endpoint.password = previous.endpoint.password;
    }
    if merged.endpoint.token.is_empty() {
        merged.endpoint.token = previous.endpoint.token;
    }
    merged
}

fn validate_path_name(name: &str) -> CmdResult<()> {
    if name.trim().is_empty() {
        return Err("パス名が空です。".to_string());
    }
    if name.trim() != name {
        return Err("パス名の前後に空白は使えません。".to_string());
    }
    if name.len() > 256 {
        return Err("パス名が長すぎます（256 文字以内）。".to_string());
    }
    Ok(())
}

fn parse_kind(kind: &str) -> CmdResult<ConnKind> {
    ConnKind::ALL
        .into_iter()
        .find(|k| {
            serde_json::to_value(k)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .is_some_and(|s| s == kind)
        })
        .ok_or_else(|| format!("未知の接続種別です: {kind}"))
}

fn export_range(engine: &Engine, req: &Value) -> CmdResult<Value> {
    let scope = arg_str(req, "scope").ok_or_else(|| "`scope` がありません。".to_string())?;
    let metrics: Vec<String> = req
        .get("metrics")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if metrics.is_empty() {
        return Err("書き出すメトリクスが指定されていません。".to_string());
    }
    let from_ms = req
        .get("fromMs")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "`fromMs` がありません。".to_string())?;
    let to_ms = req
        .get("toMs")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "`toMs` がありません。".to_string())?;
    if to_ms <= from_ms {
        return Err("期間の終了は開始より後にしてください。".to_string());
    }
    let format = ExportFormat::parse(&arg_str(req, "format").unwrap_or_default())
        .ok_or_else(|| "対応する形式は csv / svg です。".to_string())?;

    let frames = engine
        .series(&scope, &metrics, from_ms, to_ms)
        .ok_or_else(|| format!("スコープ `{scope}` の系列がありません。"))?;

    let dir = default_export_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("出力フォルダを作成できません ({}): {e}", dir.display()))?;
    let offset = chrono::Local::now().offset().local_minus_utc() / 60;

    let points_for = |metric: &str| -> Vec<mtm_core::ring::SamplePoint> {
        frames
            .iter()
            .find(|(m, _)| m == metric)
            .map(|(_, p)| {
                p.iter()
                    .map(|(at, v)| mtm_core::ring::SamplePoint {
                        at_ms: *at,
                        value: *v,
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    let file = match format {
        ExportFormat::Csv => {
            let columns: Vec<CsvColumn> = metrics
                .iter()
                .map(|m| CsvColumn {
                    metric: m.clone(),
                    unit: unit_for(m).to_string(),
                    points: points_for(m),
                })
                .collect();
            let path = unique_path(&dir, &scope, "csv");
            std::fs::write(&path, export::to_csv(&columns, offset).as_bytes())
                .map_err(|e| format!("書き出しに失敗しました ({}): {e}", path.display()))?;
            path
        }
        ExportFormat::Svg => {
            let series: Vec<SvgSeries> = metrics
                .iter()
                .enumerate()
                .map(|(i, m)| SvgSeries {
                    label: format!("{} ({})", label_for(m), unit_for(m)),
                    unit: unit_for(m).to_string(),
                    color: palette()[i % palette().len()].to_string(),
                    points: points_for(m),
                })
                .collect();
            let title = format!("MediaMTX Monitor — {}", scope_label(&scope));
            let path = unique_path(&dir, &scope, "svg");
            std::fs::write(
                &path,
                export::to_svg(&title, &series, SvgSize::default(), offset).as_bytes(),
            )
            .map_err(|e| format!("書き出しに失敗しました ({}): {e}", path.display()))?;
            path
        }
    };
    let bytes = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    Ok(json!({ "path": file.display().to_string(), "bytes": bytes }))
}

fn default_export_dir() -> PathBuf {
    // テスト時は作業ディレクトリ配下、通常はユーザーのドキュメント。
    if let Ok(dir) = std::env::var("MTM_EXPORT_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Documents"))
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Documents")))
        .unwrap_or_else(std::env::temp_dir)
}

fn unique_path(dir: &Path, scope: &str, ext: &str) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    dir.join(format!("mtm-{}-{stamp}.{ext}", scope_slug(scope)))
}

fn scope_slug(scope: &str) -> String {
    let slug: String = scope
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "export".to_string()
    } else {
        slug
    }
}

fn palette() -> &'static [&'static str] {
    &[
        "#4cc9f0", "#f72585", "#f9c74f", "#90be6d", "#b5179e", "#f8961e", "#577590", "#43aa8b",
    ]
}

fn unit_for(metric: &str) -> &'static str {
    match metric {
        "srt_drop_rate" | "srt_recovery_rate" | "srt_nak_rate" => "%",
        "srt_recv_mbps" | "srt_send_mbps" | "srt_link_capacity" | "inbound_mbps"
        | "outbound_mbps" => "Mbps",
        "srt_rtt_ms" | "api_rtt_ms" => "ms",
        "srt_unrecovered_packets"
        | "readers"
        | "paths_online"
        | "paths_total"
        | "srt_conns"
        | "frames_in_error_per_sec" => "個",
        _ => "",
    }
}

fn label_for(metric: &str) -> &str {
    match metric {
        "srt_drop_rate" => "ドロップ率",
        "srt_recovery_rate" => "回復率",
        "srt_nak_rate" => "NAK 率",
        "srt_recv_mbps" => "受信レート",
        "srt_send_mbps" => "送信レート",
        "srt_link_capacity" => "回線容量",
        "srt_rtt_ms" => "RTT",
        "srt_unrecovered_packets" => "未回復パケット",
        "api_rtt_ms" => "API 応答時間",
        "inbound_mbps" => "受信ビットレート",
        "outbound_mbps" => "送信ビットレート",
        "readers" => "視聴者数",
        "paths_online" => "稼働パス数",
        "paths_total" => "パス数",
        "srt_conns" => "SRT 接続数",
        "frames_in_error_per_sec" => "フレームエラー/秒",
        "online" => "配信中",
        other => other,
    }
}

fn scope_label(scope: &str) -> String {
    if let Some(rest) = scope.strip_prefix("srt:") {
        format!("SRT 接続 {rest}")
    } else if let Some(rest) = scope.strip_prefix("path:") {
        format!("パス {rest}")
    } else {
        "サーバー全体".to_string()
    }
}

/// Tauri の状態型に依存しないための型エイリアス（`commands` から使う）。
pub type MonitorStates = (MonitorStatus, MonitorUpdate, StatusEvent);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct Nop;
    impl crate::engine::Sink for Nop {
        fn update(&self, _u: &MonitorUpdate) {}
        fn status(&self, _s: &MonitorStatus) {}
    }

    fn engine() -> Engine {
        Engine::new(AppConfig::default(), Arc::new(Nop))
    }

    #[test]
    fn get_settings_returns_config() {
        let v = dispatch(&engine(), "get_settings", &Value::Null).expect("ok");
        assert_eq!(v.get("pollIntervalMs").and_then(|x| x.as_u64()), Some(2000));
    }

    #[test]
    fn invalid_path_names_are_rejected() {
        let e = engine();
        assert!(dispatch(&e, "add_path", &json!({"name": "  x", "conf": {}})).is_err());
        assert!(dispatch(&e, "add_path", &json!({"name": "", "conf": {}})).is_err());
        assert!(dispatch(&e, "add_path", &json!({"name": "ok", "conf": []})).is_err());
    }

    #[test]
    fn unknown_kind_is_rejected() {
        assert!(parse_kind("srt").is_ok());
        assert!(parse_kind("web_rtc").is_ok());
        assert!(parse_kind("bogus").is_err());
    }

    #[test]
    fn export_requires_valid_range() {
        let e = engine();
        let err = dispatch(
            &e,
            "export_range",
            &json!({"request": {"scope": "server:api_rtt_ms", "metrics": ["api_rtt_ms"], "fromMs": 10, "toMs": 5, "format": "csv"}}),
        )
        .unwrap_err();
        assert!(err.contains("期間"), "{err}");
    }

    #[test]
    fn units_and_labels() {
        assert_eq!(unit_for("srt_drop_rate"), "%");
        assert_eq!(label_for("srt_recovery_rate"), "回復率");
        assert_eq!(scope_slug("srt:abc-def"), "srt-abc-def");
        assert_eq!(scope_label("server:api_rtt_ms"), "サーバー全体");
    }
}
