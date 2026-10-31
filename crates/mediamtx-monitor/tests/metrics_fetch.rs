//! MediaMTX の `/metrics` 取得まわりの回帰テスト。
//!
//! 実機では「メトリクスが 404 扱いになる」不具合があったため、
//! ローカルの簡易 HTTP サーバーを立てて挙動を固定する。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

use mediamtx_monitor::transport_ureq::UreqTransport;
use mtm_core::client::ApiClient;
use mtm_core::config::EndpointConfig;
use mtm_core::transport::{HttpResponse, HttpTransport};

/// `/metrics` だけを返す 1 リクエスト用サーバー。返す URL を返す。
fn serve_once(body: &'static str, status_line: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let body = Arc::new(body);
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let response = format!(
                "{status_line}\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    format!("http://{addr}/metrics")
}

fn client_for(metrics_url: String, transport: &UreqTransport) -> ApiClient<'_> {
    let ep = EndpointConfig {
        base_url: "http://127.0.0.1:9".to_string(),
        metrics_url: Some(metrics_url),
        ..EndpointConfig::default()
    };
    ApiClient::new(transport, &ep, 3000).expect("client")
}

#[test]
fn metrics_are_fetched_from_the_configured_url() {
    let body = "# SRT connections\nsrt_conns{id=\"a\",state=\"read\",path=\"p\",remoteAddr=\"1.2.3.4:5\"} 1\nsrt_conns_packets_received_unique{id=\"a\",state=\"read\",path=\"p\",remoteAddr=\"1.2.3.4:5\"} 100\n";
    let url = serve_once(body, "HTTP/1.1 200 OK");
    let transport = UreqTransport::new(false);
    let client = client_for(url, &transport);
    let metrics = client.metrics().expect("metrics");
    assert_eq!(metrics.sample_count(), 2);
    assert_eq!(metrics.srt_conn_series().len(), 1);
}

#[test]
fn metrics_404_reports_disabled() {
    let url = serve_once("404 page not found", "HTTP/1.1 404 Not Found");
    let transport = UreqTransport::new(false);
    let client = client_for(url, &transport);
    let err = client.metrics().expect_err("404 は失敗");
    assert_eq!(err.status, Some(404));
    assert!(err.message.contains("metrics: true"), "{}", err.message);
}

#[test]
fn transport_sees_text_plain_body() {
    // ureq 側が本文を読めることを直接確認する（Content-Type に依存しないこと）。
    let body = "paths{name=\"e2e\",state=\"ready\"} 1\n";
    let url = serve_once(body, "HTTP/1.1 200 OK");
    let transport = UreqTransport::new(false);
    let resp: HttpResponse = transport
        .request("GET", &url, None, None, 3000)
        .expect("request");
    assert_eq!(resp.status, 200);
    assert!(resp.body.contains("paths{"), "body={}", resp.body);
}
