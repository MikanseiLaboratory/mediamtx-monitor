//! `ureq` による `HttpTransport` 実装。
//!
//! - 接続プール付きの `Agent` を使い、HTTP ステータスはエラーにせず
//!   そのまま返す（クライアント側で分類するため）。
//! - タイムアウトは要求ごとに指定する。
//! - TLS は OS 標準（Windows は Schannel）を使う。rustls の ring は
//!   環境によって C コンパイラでビルドできないことがあるため、
//!   依存の少ない native-tls を選んでいる。
//! - `allow_insecure_tls` が有効な場合は証明書検証を無効化する
//!   （`apiEncryption: true` の自己署名環境向け。既定は無効）。

use std::time::Duration;

use mtm_core::error::{ApiError, ApiErrorKind};
use mtm_core::transport::{HttpResponse, HttpTransport};
use ureq::http::Response;
use ureq::tls::{TlsConfig, TlsProvider};
use ureq::typestate::WithBody;
use ureq::{Agent, Body, RequestBuilder};

/// スレッドセーフな `ureq` 輸送。
pub struct UreqTransport {
    agent: Agent,
}

impl UreqTransport {
    pub fn new(allow_insecure_tls: bool) -> Self {
        let tls = TlsConfig::builder()
            .provider(TlsProvider::NativeTls)
            .disable_verification(allow_insecure_tls)
            .build();
        let config = Agent::config_builder()
            .tls_config(tls)
            .http_status_as_error(false)
            .user_agent(concat!("MediaMTX Monitor/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: config.into(),
        }
    }

    /// タイムアウトと認証ヘッダを適用する。
    fn decorate<Any>(
        req: RequestBuilder<Any>,
        authorization: Option<&str>,
        timeout: Duration,
    ) -> RequestBuilder<Any> {
        let mut req = req.config().timeout_global(Some(timeout)).build();
        req = req.header("Accept", "application/json, text/plain, */*");
        if let Some(auth) = authorization {
            req = req.header("Authorization", auth);
        }
        req
    }
}

impl HttpTransport for UreqTransport {
    fn request(
        &self,
        method: &str,
        url: &str,
        authorization: Option<&str>,
        body: Option<&str>,
        timeout_ms: u64,
    ) -> Result<HttpResponse, ApiError> {
        let timeout = Duration::from_millis(timeout_ms.max(100));

        // ureq 3 はボディの有無で型が変わる（typestate）。メソッドと
        // ボディ有無の組み合わせごとに分岐する。
        let result: Result<Response<Body>, ureq::Error> = match (method, body) {
            ("GET", None) => Self::decorate(self.agent.get(url), authorization, timeout).call(),
            ("DELETE", None) => {
                Self::decorate(self.agent.delete(url), authorization, timeout).call()
            }
            ("POST", Some(payload)) => {
                let req = Self::decorate(self.agent.post(url), authorization, timeout);
                send(req, payload)
            }
            ("PATCH", Some(payload)) => {
                let req = Self::decorate(self.agent.patch(url), authorization, timeout);
                send(req, payload)
            }
            ("PUT", Some(payload)) => {
                let req = Self::decorate(self.agent.put(url), authorization, timeout);
                send(req, payload)
            }
            (other, _) => {
                return Err(ApiError::new(
                    ApiErrorKind::Other,
                    format!("未対応の HTTP メソッドです: {other}"),
                ))
            }
        };

        match result {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let text = resp
                    .into_body()
                    .read_to_string()
                    .map_err(|e| classify(&e, url))?;
                Ok(HttpResponse { status, body: text })
            }
            Err(err) => Err(classify(&err, url)),
        }
    }
}

/// ボディ付きの送信。
fn send(req: RequestBuilder<WithBody>, payload: &str) -> Result<Response<Body>, ureq::Error> {
    req.send(payload)
}

fn classify(err: &ureq::Error, url: &str) -> ApiError {
    match err {
        ureq::Error::Timeout(_) => ApiError::timeout(format!(
            "メディアサーバー ({url}) への接続がタイムアウトしました。"
        )),
        ureq::Error::StatusCode(code) => {
            let mut e = ApiError::from_status(*code, "");
            e.message = format!("{} (HTTP {code})", e.message);
            e
        }
        ureq::Error::Io(io) if is_status_io_error(io) => {
            // `Io` に HTTP ステータスが載る構成もあるため、メッセージから拾う。
            match parse_status_line(&io.to_string()) {
                Some(code) => ApiError::from_status(code, "").with_detail(url),
                None if io.kind() == std::io::ErrorKind::TimedOut => {
                    ApiError::timeout(format!("応答の受信中にタイムアウトしました ({url})。"))
                }
                None => ApiError::transport(describe(err, url)),
            }
        }
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            ApiError::timeout(format!("応答の受信中にタイムアウトしました ({url})。"))
        }
        // `http_status_as_error(false)` の設定でも、一部の HTTP ステータスは
        // `Error::Protocol` として返ることがある。本文からステータスを拾って分類する。
        ureq::Error::Protocol(_) => match parse_status_line(&err.to_string()) {
            Some(code) => ApiError::from_status(code, "").with_detail(url),
            None => ApiError::transport(describe(err, url)),
        },
        other => ApiError::transport(describe(other, url)),
    }
}

fn is_status_io_error(err: &std::io::Error) -> bool {
    let text = err.to_string();
    text.contains("HTTP status") || text.contains("status code")
}

/// `http://...: HTTP status client error (404 Not Found)` のような文字列から
/// ステータスコードを取り出す。
fn parse_status_line(text: &str) -> Option<u16> {
    // 「HTTP status client error (404 Not Found)」と「HTTP/1.1 404」の両方を想定。
    let after_http = text.rsplit_once("HTTP ").map(|(_, rest)| rest)?;
    if let Some(rest) = after_http
        .strip_prefix("status client error (")
        .or_else(|| after_http.strip_prefix("status server error ("))
    {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        return digits
            .parse::<u16>()
            .ok()
            .filter(|c| (100..=599).contains(c));
    }
    let token = after_http.split_whitespace().next()?;
    token
        .parse::<u16>()
        .ok()
        .filter(|c| (100..=599).contains(c))
}

fn describe(err: &ureq::Error, url: &str) -> String {
    let text = err.to_string();
    let hint = if text.contains("certificate") || text.contains("Certificate") {
        "TLS 証明書の検証に失敗しました。自己署名証明書の場合は「証明書検証を省略」を有効にしてください。"
    } else if text.contains("dns") || text.contains("resolve") || text.contains("lookup") {
        "ホスト名を解決できませんでした。接続先 URL を確認してください。"
    } else {
        "MediaMTX に接続できませんでした。サーバーが起動しているか、`api: true` が設定されているか確認してください。"
    };
    format!("{hint} ({url}): {text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_status_from_protocol_error_text() {
        assert_eq!(
            parse_status_line(
                "http://127.0.0.1:9998/metrics: HTTP status client error (404 Not Found)"
            ),
            Some(404)
        );
        assert_eq!(parse_status_line("no status here"), None);
        assert_eq!(parse_status_line("HTTP "), None);
    }

    #[test]
    fn connection_refused_is_transport_error() {
        let t = UreqTransport::new(false);
        // 閉じたポートへ投げる（OS は即座に拒否する）。
        let err = t
            .request("GET", "http://127.0.0.1:9/v3/info", None, None, 500)
            .expect_err("接続できないはず");
        // OS によっては「拒否」ではなく「タイムアウト」に分類されることもあるので、
        // どちらでも再試行対象として扱えていることを確認する。
        assert!(
            matches!(err.kind, ApiErrorKind::Transport | ApiErrorKind::Timeout),
            "{err:?}"
        );
        assert!(err.kind.is_retryable());
        assert!(err.message.contains("127.0.0.1:9"));
    }

    #[test]
    fn rejects_unknown_method() {
        let t = UreqTransport::new(false);
        let err = t
            .request("BREW", "http://127.0.0.1:9/", None, None, 100)
            .expect_err("未対応メソッド");
        assert_eq!(err.kind, ApiErrorKind::Other);
    }
}
