//! テストでローカル TCP スタブを差し込めるようにする。

use crate::error::ApiError;

/// 単純な HTTP 応答。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// 輸送層。`Authorization` 等のヘッダは呼び出し側（`ApiClient`）が組み立てる。
pub trait HttpTransport: Send + Sync {
    /// `method`: `GET` / `POST` / `PATCH` / `DELETE`。
    /// `url`: 完全な URL（API とメトリクスは別ポートのことがある）。
    /// `body`: JSON 文字列（不要なら `None`）。
    fn request(
        &self,
        method: &str,
        url: &str,
        authorization: Option<&str>,
        body: Option<&str>,
        timeout_ms: u64,
    ) -> Result<HttpResponse, ApiError>;
}
