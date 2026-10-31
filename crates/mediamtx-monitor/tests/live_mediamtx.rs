//! 実機（ローカルの MediaMTX）に対する結合テスト。
//!
//! 既定では実行しない。開発機で `MediaMTX Monitor/.dev/mediamtx.yml` を
//! 起動し、SRT で配信してから次のように実行する:
//!
//! ```text
//! MTM_LIVE=1 cargo test -p mediamtx-monitor --test live_mediamtx -- --nocapture --test-threads=1
//! ```
//!
//! 期待する流れ:
//! 1. `/v3/info` でバージョンが取れる
//! 2. `/v3/paths/list` と `/metrics` が取れる
//! 3. SRT 接続が現れ、2 周期分の差分からドロップ率・回復率が算出される
//! 4. パスの add → patch → delete が往復する
//! 5. 期間指定の CSV / SVG を書き出せる

use std::time::Duration;

use chrono::Utc;
use mediamtx_monitor::transport_ureq::UreqTransport;
use mtm_core::client::ApiClient;
use mtm_core::config::{AppConfig, EndpointConfig};
use mtm_core::export::{CsvColumn, ExportFormat, SvgSeries, SvgSize};
use mtm_core::monitor::Monitor;

fn live_enabled() -> bool {
    std::env::var("MTM_LIVE").map(|v| v == "1").unwrap_or(false)
}

fn config() -> AppConfig {
    let base = std::env::var("MTM_LIVE_URL").unwrap_or_else(|_| "http://127.0.0.1:9997".into());
    AppConfig {
        endpoint: EndpointConfig {
            base_url: base,
            ..EndpointConfig::default()
        },
        poll_interval_ms: 500,
        request_timeout_ms: 2000,
        ..AppConfig::default()
    }
}

#[test]
fn live_mediamtx_end_to_end() {
    if !live_enabled() {
        eprintln!("MTM_LIVE=1 で有効になります。スキップします。");
        return;
    }
    let cfg = config();
    let transport = UreqTransport::new(false);

    // 1) info
    let probe = Monitor::test_connection(&transport, &cfg).expect("info");
    assert!(!probe.version.is_empty(), "バージョンが空です");
    // メトリクスは補助情報。取れなくても接続テストは成功する設計なので、ここでは警告に留める。
    if probe.metrics_samples.is_none() {
        eprintln!("警告: メトリクスが取れません: {:?}", probe.metrics_error);
    }
    eprintln!("MediaMTX {} started={:?}", probe.version, probe.started);

    // 2) 一覧
    let client = ApiClient::new(&transport, &cfg.endpoint, cfg.request_timeout_ms).expect("client");
    let paths = client.list_paths().expect("paths");
    eprintln!("paths={}", paths.len());
    match client.metrics() {
        Ok(metrics) => {
            assert!(!metrics.is_empty(), "メトリクスが空です");
            eprintln!("srt conns by metrics={}", metrics.srt_conn_series().len());
        }
        Err(err) => {
            // メトリクスは補助情報。404 はポート設定違いのことが多いので警告に留める。
            eprintln!("警告: メトリクスが取れません: {err}");
            assert_eq!(
                err.status,
                Some(404),
                "404 以外のメトリクス障害はテスト失敗にする"
            );
        }
    }

    // 3) 2 周期回して SRT の派生値を確認（配信が無くても監視自体は回る）。
    let mut monitor = Monitor::new(&transport, cfg.clone());
    let first = monitor.tick(Utc::now().fixed_offset());
    assert!(
        first.update.api.ok,
        "API に到達できません: {:?}",
        first.update.api.error
    );
    std::thread::sleep(Duration::from_millis(1200));
    let second = monitor.tick(Utc::now().fixed_offset());
    assert_eq!(second.update.seq, first.update.seq + 1);
    for conn in &second.update.srt {
        eprintln!(
            "srt {} state={} drop={:?}% recovery={:?}% rtt={}ms recv={}Mbps",
            conn.sample.id,
            conn.sample.state,
            conn.sample.derived.drop_rate,
            conn.sample.derived.recovery_rate,
            conn.sample.derived.rtt_ms,
            conn.sample.derived.recv_mbps
        );
        // 2 周期目は差分が出ているはず（初回は None）。
        assert!(
            conn.sample.derived.recv_packets.is_some(),
            "差分が算出されていません"
        );
    }
    if let Some(conn) = second.update.srt.first() {
        assert!(
            conn.sample.severity == mtm_core::srt::SrtSeverity::Ok
                || conn.sample.derived.drop_rate.is_some()
        );
    }

    // 4) パスの作成・更新・削除
    let test_name = format!("mtm-e2e-{}", std::process::id());
    let _ = client.delete_path(&test_name); // 前回の残骸を掃除
    client
        .add_path(
            &test_name,
            &serde_json::json!({"source": "publisher", "maxReaders": 3}),
        )
        .expect("add path");
    let conf = client.get_path_conf(&test_name).expect("get conf");
    assert_eq!(conf.get("maxReaders").and_then(|v| v.as_u64()), Some(3));
    client
        .patch_path(&test_name, &serde_json::json!({"maxReaders": 5}))
        .expect("patch path");
    let conf = client
        .get_path_conf(&test_name)
        .expect("get conf after patch");
    assert_eq!(conf.get("maxReaders").and_then(|v| v.as_u64()), Some(5));
    client.delete_path(&test_name).expect("delete path");
    assert!(
        client.get_path_conf(&test_name).is_err(),
        "削除後は 404 のはず"
    );

    // 5) 書き出し（期間指定）
    let now = Utc::now().timestamp_millis();
    let cols = vec![CsvColumn {
        metric: "srt_drop_rate".to_string(),
        unit: "%".to_string(),
        points: (0..10)
            .map(|i| mtm_core::ring::SamplePoint {
                at_ms: now - 10_000 + i * 1_000,
                value: Some(i as f64 * 0.1),
            })
            .collect(),
    }];
    let csv = mtm_core::export::to_csv(&cols, 540);
    assert!(csv.contains("srt_drop_rate"));
    assert_eq!(ExportFormat::parse("SVG"), Some(ExportFormat::Svg));
    let svg = mtm_core::export::to_svg(
        "live test",
        &[SvgSeries {
            label: "ドロップ率".into(),
            unit: "%".into(),
            color: "#f25f5c".into(),
            points: cols[0].points.clone(),
        }],
        SvgSize::default(),
        540,
    );
    assert!(svg.starts_with("<svg"));
    // 書き出し先は作業ディレクトリ配下の `target/live-export`（CI でも書き込める）。
    // 同名ファイルを繰り返し上書きしないよう、実行ごとに番号を付ける。
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/live-export")
        .join(format!("{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("live.csv"), csv).expect("write csv");
    std::fs::write(dir.join("live.svg"), svg).expect("write svg");
    eprintln!("wrote exports to {}", dir.display());
}

#[test]
fn live_srt_quality_is_computed() {
    if !live_enabled() {
        eprintln!("MTM_LIVE=1 で有効になります。スキップします。");
        return;
    }
    let cfg = config();
    let transport = UreqTransport::new(false);
    let mut monitor = Monitor::new(&transport, cfg);
    let mut last = monitor.tick(Utc::now().fixed_offset());
    // SRT 接続が現れるまで最大 20 秒待つ（配信中ならすぐ）。
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(500));
        last = monitor.tick(Utc::now().fixed_offset());
        if last
            .update
            .srt
            .iter()
            .any(|c| c.sample.derived.recv_packets.is_some())
        {
            break;
        }
    }
    let conn = last
        .update
        .srt
        .iter()
        .find(|c| c.sample.state == "publish")
        .or_else(|| last.update.srt.first())
        .expect("SRT 接続がありません（ffmpeg で SRT 配信してから実行してください）");
    eprintln!(
        "live SRT: path={} state={} recv_packets={:?} drop_rate={:?} recovery={:?}",
        conn.sample.path,
        conn.sample.state,
        conn.sample.derived.recv_packets,
        conn.sample.derived.drop_rate,
        conn.sample.derived.recovery_rate
    );
    assert!(
        conn.sample.derived.recv_packets.unwrap_or(0.0) > 0.0,
        "受信パケットが 0 です"
    );
    assert!(
        conn.sample.derived.drop_rate.is_some(),
        "ドロップ率が未算出"
    );
    assert!(
        conn.sample.derived.recovery_rate.is_some(),
        "回復率が未算出"
    );
}
