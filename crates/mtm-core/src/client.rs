//! 輸送は `HttpTransport` に委譲する。ここでは
//! - URL 組み立て（**パス名はセグメント全体を percent-encode**）
//! - 認証ヘッダ
//! - 全ページ取得（`pageCount` を見て収束させる）
//! - 応答の解釈とエラー分類
//!
//! を担当する。

use std::time::Instant;

use serde::de::DeserializeOwned;

use crate::config::EndpointConfig;
use crate::error::{ApiError, ApiErrorKind};
use crate::model::{ConnInfo, ConnKind, Info, ListResponse, Path, SrtConn};
use crate::prom::{self, Metrics};
use crate::transport::{HttpResponse, HttpTransport};

/// ページング要求の上限（暴走防止）。
const MAX_PAGES: usize = 200;
/// 1 ページで要求する件数。
const ITEMS_PER_PAGE: usize = 1000;

/// API クライアント。
pub struct ApiClient<'a> {
    transport: &'a dyn HttpTransport,
    base_url: String,
    metrics_url: String,
    auth: Option<String>,
    timeout_ms: u64,
}

/// 1 回の API 呼び出しの結果。RTT 計測用に経過時間も含む。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timed {
    pub http: HttpResponse,
    pub elapsed_ms: u64,
}

impl<'a> ApiClient<'a> {
    pub fn new(
        transport: &'a dyn HttpTransport,
        endpoint: &EndpointConfig,
        timeout_ms: u64,
    ) -> Result<Self, ApiError> {
        let base_url =
            crate::config::normalize_base_url(&endpoint.base_url).map_err(ApiError::config)?;
        let metrics_url = crate::config::resolve_metrics_url(endpoint).map_err(ApiError::config)?;
        Ok(Self {
            transport,
            base_url,
            metrics_url,
            auth: endpoint.authorization_value(),
            timeout_ms,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn metrics_url(&self) -> &str {
        &self.metrics_url
    }

    /// 生の HTTP リクエスト。`path` は `/v3/...` 形式。
    pub fn raw(&self, method: &str, path: &str, body: Option<&str>) -> Result<Timed, ApiError> {
        let url = format!("{}{}", self.base_url, path);
        self.raw_url(method, &self.base_url, &url, body)
    }

    fn raw_url(
        &self,
        method: &str,
        _base: &str,
        url: &str,
        body: Option<&str>,
    ) -> Result<Timed, ApiError> {
        let started = Instant::now();
        let resp =
            self.transport
                .request(method, url, self.auth.as_deref(), body, self.timeout_ms)?;
        Ok(Timed {
            http: resp,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// `GET` して JSON にデコード。非 2xx は `ApiError`。
    fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<(T, u64), ApiError> {
        let timed = self.raw("GET", path, None)?;
        let elapsed = timed.elapsed_ms;
        if !timed.http.is_success() {
            return Err(ApiError::from_status(timed.http.status, &timed.http.body));
        }
        let value = serde_json::from_str::<T>(&timed.http.body).map_err(|e| {
            ApiError::decode(format!("応答を JSON として解釈できませんでした: {e}"))
                .with_status(timed.http.status)
                .with_detail(&timed.http.body)
        })?;
        Ok((value, elapsed))
    }

    /// 変更系。成功時は 200 + `{"status":"ok"}` を期待する。
    fn mutate(&self, method: &str, path: &str, body: Option<&str>) -> Result<(), ApiError> {
        let timed = self.raw(method, path, body)?;
        if !timed.http.is_success() {
            return Err(ApiError::from_status(timed.http.status, &timed.http.body));
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // 読み取り
    // ------------------------------------------------------------------

    pub fn info(&self) -> Result<(Info, u64), ApiError> {
        self.get_json("/v3/info")
    }

    /// 全ページを取得する。`pageCount` が増減しても、空ページが返ったら止める。
    fn get_all<T: DeserializeOwned>(&self, base_path: &str) -> Result<Vec<T>, ApiError> {
        let mut all: Vec<T> = Vec::new();
        let mut seen_pages = 0usize;
        for page in 0..MAX_PAGES {
            let sep = if base_path.contains('?') { '&' } else { '?' };
            let path = format!("{base_path}{sep}page={page}&itemsPerPage={ITEMS_PER_PAGE}");
            let timed = self.raw("GET", &path, None)?;
            if !timed.http.is_success() {
                return Err(ApiError::from_status(timed.http.status, &timed.http.body));
            }
            let list = ListResponse::<T>::parse(&timed.http.body).map_err(|e| {
                ApiError::decode(format!("応答を JSON として解釈できませんでした: {e}"))
                    .with_status(timed.http.status)
                    .with_detail(&timed.http.body)
            })?;
            seen_pages += 1;
            let page_count = list.page_count;
            let items_len = list.items.len();
            all.extend(list.items);
            if items_len == 0 || seen_pages >= page_count.max(1) {
                break;
            }
        }
        Ok(all)
    }

    pub fn list_paths(&self) -> Result<Vec<Path>, ApiError> {
        self.get_all("/v3/paths/list")
    }

    pub fn list_config_paths(&self) -> Result<Vec<serde_json::Value>, ApiError> {
        self.get_all("/v3/config/paths/list")
    }

    pub fn path_defaults(&self) -> Result<serde_json::Value, ApiError> {
        self.get_json("/v3/config/path-defaults/get")
            .map(|(v, _)| v)
    }

    pub fn get_path_conf(&self, name: &str) -> Result<serde_json::Value, ApiError> {
        self.get_json(&format!("/v3/config/paths/get/{}", encode_path_name(name)))
            .map(|(v, _)| v)
    }

    pub fn global_config(&self) -> Result<serde_json::Value, ApiError> {
        self.get_json("/v3/config/global/get").map(|(v, _)| v)
    }

    pub fn list_srt_conns(&self) -> Result<Vec<SrtConn>, ApiError> {
        Ok(self
            .get_all::<serde_json::Value>(ConnKind::Srt.list_path())?
            .iter()
            .filter_map(|v| serde_json::from_value::<SrtConn>(v.clone()).ok())
            .collect())
    }

    pub fn list_conns(&self, kind: ConnKind) -> Result<Vec<ConnInfo>, ApiError> {
        let raw = self.get_all::<serde_json::Value>(kind.list_path())?;
        Ok(raw.iter().map(|v| ConnInfo::from_value(kind, v)).collect())
    }

    /// `/metrics` を取得してパースする。
    ///
    /// メトリクスが無効なときは 404 が返る（これは正常な応答）。接続自体に
    /// 失敗したときだけ、API と同じポートの `/metrics` も 1 度だけ試す。
    pub fn metrics(&self) -> Result<Metrics, ApiError> {
        match self.fetch_metrics(&self.metrics_url) {
            Ok(m) => Ok(m),
            Err(err) if err.status.is_none() => {
                if let Some(url) = self.fallback_metrics_url() {
                    if let Ok(m) = self.fetch_metrics(&url) {
                        return Ok(m);
                    }
                }
                Err(err)
            }
            Err(err) => Err(self.metrics_disabled_error(err)),
        }
    }

    /// メトリクスの代替 URL（API と同じポートの `/metrics`）。
    fn fallback_metrics_url(&self) -> Option<String> {
        let same_port = format!("{}/metrics", self.base_url);
        if same_port == self.metrics_url {
            None
        } else {
            Some(same_port)
        }
    }

    /// 1 回分の取得。ステータス異常も `ApiError` として返す。
    fn fetch_metrics(&self, url: &str) -> Result<Metrics, ApiError> {
        let timed = self.raw_url("GET", url, url, None)?;
        if !timed.http.is_success() {
            return Err(ApiError::from_status(timed.http.status, &timed.http.body).with_detail(url));
        }
        Ok(prom::parse(&timed.http.body))
    }

    fn metrics_disabled_error(&self, err: ApiError) -> ApiError {
        let mut err = err;
        if err.status == Some(404) {
            err.message =
                "メトリクスが無効です。mediamtx.yml の `metrics: true` を確認してください。"
                    .to_string();
        }
        err
    }

    // ------------------------------------------------------------------
    // 変更系
    // ------------------------------------------------------------------

    /// パス設定の追加。`conf` は JSON オブジェクト。
    pub fn add_path(&self, name: &str, conf: &serde_json::Value) -> Result<(), ApiError> {
        let body = serde_json::to_string(conf)
            .map_err(|e| ApiError::decode(format!("設定を JSON に変換できません: {e}")))?;
        self.mutate(
            "POST",
            &format!("/v3/config/paths/add/{}", encode_path_name(name)),
            Some(&body),
        )
    }

    pub fn patch_path(&self, name: &str, conf: &serde_json::Value) -> Result<(), ApiError> {
        let body = serde_json::to_string(conf)
            .map_err(|e| ApiError::decode(format!("設定を JSON に変換できません: {e}")))?;
        self.mutate(
            "PATCH",
            &format!("/v3/config/paths/patch/{}", encode_path_name(name)),
            Some(&body),
        )
    }

    pub fn delete_path(&self, name: &str) -> Result<(), ApiError> {
        self.mutate(
            "DELETE",
            &format!("/v3/config/paths/delete/{}", encode_path_name(name)),
            None,
        )
    }

    pub fn patch_global_config(&self, patch: &serde_json::Value) -> Result<(), ApiError> {
        let body = serde_json::to_string(patch)
            .map_err(|e| ApiError::decode(format!("設定を JSON に変換できません: {e}")))?;
        self.mutate("PATCH", "/v3/config/global/patch", Some(&body))
    }

    /// 接続を切断する。
    pub fn kick_conn(&self, kind: ConnKind, id: &str) -> Result<(), ApiError> {
        if id.trim().is_empty() {
            return Err(ApiError::new(
                ApiErrorKind::BadRequest,
                "切断対象の ID が空です。",
            ));
        }
        self.mutate("POST", &kind.kick_path(id), None)
    }

    /// 任意エンドポイントの GET（API 操作コンソール用）。
    pub fn console_get(&self, path: &str) -> Result<(u16, serde_json::Value), ApiError> {
        let normalized = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        if !normalized.starts_with("/v3/") && !normalized.starts_with("/metrics") {
            return Err(ApiError::new(
                ApiErrorKind::BadRequest,
                "パスは `/v3/...` で指定してください。",
            ));
        }
        let timed = self.raw("GET", &normalized, None)?;
        let status = timed.http.status;
        let value = serde_json::from_str(&timed.http.body)
            .unwrap_or_else(|_| serde_json::json!({ "raw": timed.http.body }));
        if !timed.http.is_success() {
            return Err(ApiError::from_status(status, &timed.http.body));
        }
        Ok((status, value))
    }
}

/// パス名を URL セグメントとして安全にエンコードする。
///
/// MediaMTX の `*name` ルートはスラッシュを含めて 1 セグメントとして受け取る
/// ため、スラッシュ・`~`・`^` などを含むパス名（例 `~^prefix`、`cam/1`）も
/// そのまま渡せるようにエンコードする。RFC 3986 の unreserved 以外はすべて
/// `%XX` にする。
pub fn encode_path_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_path_names() {
        assert_eq!(encode_path_name("cam1"), "cam1");
        assert_eq!(encode_path_name("cam/1"), "cam%2F1");
        assert_eq!(encode_path_name("~^prefix"), "~%5Eprefix");
        assert_eq!(encode_path_name("日本語"), "%E6%97%A5%E6%9C%AC%E8%AA%9E");
        assert_eq!(encode_path_name("a b"), "a%20b");
    }

    #[test]
    fn url_is_built_from_normalized_base() {
        struct Dummy;
        impl HttpTransport for Dummy {
            fn request(
                &self,
                _m: &str,
                _u: &str,
                _a: Option<&str>,
                _b: Option<&str>,
                _t: u64,
            ) -> Result<HttpResponse, ApiError> {
                Err(ApiError::transport("unused"))
            }
        }
        let ep = EndpointConfig {
            base_url: "127.0.0.1:9997".into(),
            ..EndpointConfig::default()
        };
        let c = ApiClient::new(&Dummy, &ep, 1000).unwrap();
        assert_eq!(c.base_url(), "http://127.0.0.1:9997");
        assert_eq!(c.metrics_url(), "http://127.0.0.1:9998/metrics");
    }
}
