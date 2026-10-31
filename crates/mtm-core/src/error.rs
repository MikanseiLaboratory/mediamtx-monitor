//! エラー型。UI にそのまま出せる日本語メッセージを持つ。Kind で再試行可否や表示文言を分岐できる。

use std::fmt;

/// 失敗の種類。フロントは `kind` で分岐して表示文言や再試行可否を変える。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorKind {
    /// 接続拒否・名前解決失敗など到達不能。
    Transport,
    /// タイムアウト。
    Timeout,
    /// 認証エラー (401/403)。
    Unauthorized,
    /// 対象が存在しない (404)。
    NotFound,
    /// リクエスト不正 (400)。
    BadRequest,
    /// サーバー内部エラー (5xx)。
    Server,
    /// その他の HTTP ステータス。
    Http,
    /// JSON / テキストの解釈失敗。
    Decode,
    /// 設定不備（URL が不正など）。
    Config,
    /// その他。
    Other,
}

impl ApiErrorKind {
    /// 再試行に意味があるか（ネットワーク系のみ true）。
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Transport | Self::Timeout | Self::Server)
    }
}

/// API 呼び出しの失敗。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApiError {
    pub kind: ApiErrorKind,
    /// 利用者向けの日本語メッセージ。
    pub message: String,
    /// HTTP ステータス（あれば）。
    pub status: Option<u16>,
    /// サーバーが返したエラー本文の抜粋（あれば）。
    pub detail: Option<String>,
}

impl ApiError {
    pub fn new(kind: ApiErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status: None,
            detail: None,
        }
    }

    pub fn transport(message: impl Into<String>) -> Self {
        Self::new(ApiErrorKind::Transport, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ApiErrorKind::Timeout, message)
    }

    pub fn decode(message: impl Into<String>) -> Self {
        Self::new(ApiErrorKind::Decode, message)
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::new(ApiErrorKind::Config, message)
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let text = detail.into();
        let text = text.trim();
        if text.is_empty() {
            return self;
        }
        let clipped: String = text.chars().take(600).collect();
        self.detail = Some(clipped);
        self
    }

    /// HTTP ステータスからエラーを組み立てる。MediaMTX のエラー本文
    /// (`{"status":"error","error":"..."}`) があればメッセージに混ぜる。
    pub fn from_status(status: u16, body: &str) -> Self {
        let server_message = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.as_str())
                    .map(|s| s.to_string())
            });
        let kind = match status {
            401 | 403 => ApiErrorKind::Unauthorized,
            404 => ApiErrorKind::NotFound,
            400 | 422 => ApiErrorKind::BadRequest,
            500..=599 => ApiErrorKind::Server,
            _ => ApiErrorKind::Http,
        };
        let base = match status {
            401 => "認証に失敗しました。ユーザー名・パスワードを確認してください。",
            403 => "この操作は許可されていません (403)。",
            404 => "対象が見つかりませんでした (404)。",
            400 => "リクエストが拒否されました (400)。",
            500..=599 => "MediaMTX 側でエラーが発生しました。",
            _ => "MediaMTX API がエラーを返しました。",
        };
        let message = match server_message {
            Some(m) if !m.is_empty() => format!("{base} 詳細: {m}"),
            _ => base.to_string(),
        };
        Self {
            kind,
            message,
            status: Some(status),
            detail: if body.trim().is_empty() {
                None
            } else {
                Some(body.chars().take(600).collect())
            },
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ApiError {}

/// コア内で使う汎用エラー（設定ファイル入出力・エクスポート等）。
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("入出力エラー: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON エラー: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl From<CoreError> for String {
    fn from(value: CoreError) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping() {
        let e = ApiError::from_status(404, r#"{"status":"error","error":"path not found"}"#);
        assert_eq!(e.kind, ApiErrorKind::NotFound);
        assert!(e.message.contains("path not found"));

        let e = ApiError::from_status(401, "");
        assert_eq!(e.kind, ApiErrorKind::Unauthorized);
        assert!(e.detail.is_none());

        let e = ApiError::from_status(500, "boom");
        assert_eq!(e.kind, ApiErrorKind::Server);
        assert!(e.kind.is_retryable());
    }

    #[test]
    fn status_body_is_clipped() {
        let body = "x".repeat(5000);
        let e = ApiError::from_status(500, &body);
        assert_eq!(e.detail.as_deref().map(str::len), Some(600));
    }
}
