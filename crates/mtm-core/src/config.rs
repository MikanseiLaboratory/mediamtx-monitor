//! 認証情報は **保存しない** 方針なので `password` / `token` は
//! `#[serde(skip)]` で永続化対象から外してある（実行中はメモリ上に保持）。

use serde::{Deserialize, Serialize};

/// API の認証方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// 認証なし。
    #[default]
    None,
    /// HTTP Basic（MediaMTX の internal / http 認証で一般的）。
    Basic,
    /// Bearer トークン（JWT 認証など）。
    Bearer,
}

impl AuthMode {
    pub const ALL: [AuthMode; 3] = [AuthMode::None, AuthMode::Basic, AuthMode::Bearer];

    pub fn label(self) -> &'static str {
        match self {
            AuthMode::None => "なし",
            AuthMode::Basic => "Basic",
            AuthMode::Bearer => "Bearer",
        }
    }
}

/// 接続先エンドポイント。`base_url` から API と メトリクスの URL を導出する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EndpointConfig {
    /// 例: `http://127.0.0.1:9997`
    pub base_url: String,
    /// メトリクスの URL。未指定なら `base_url` のポートを 9998（既定）に置換する。
    pub metrics_url: Option<String>,
    /// api のアドレスと metrics のアドレスが別ホストの場合のホスト名。
    pub auth: AuthMode,
    pub user: String,
    /// 保存されない（`[serde(skip)]`）。
    #[serde(skip)]
    pub password: String,
    /// 保存されない（`[serde(skip)]`）。
    #[serde(skip)]
    pub token: String,
    /// 自己署名証明書を許可する（`apiEncryption: true` の開発環境用）。
    pub allow_insecure_tls: bool,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:9997".to_string(),
            metrics_url: None,
            auth: AuthMode::None,
            user: String::new(),
            password: String::new(),
            token: String::new(),
            allow_insecure_tls: false,
        }
    }
}

/// 入力された base_url を検証して、末尾スラッシュを除去した正規形を返す。
///
/// 対応形式: `http[s]://host[:port][/prefix]`。スキームを省略した場合は
/// `http://` を補う（`127.0.0.1:9997` のような入力を受け付ける）。
pub fn normalize_base_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("接続先 URL が空です。".to_string());
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let scheme_end = with_scheme
        .find("://")
        .ok_or_else(|| "URL の形式が不正です。".to_string())?;
    let scheme = with_scheme[..scheme_end].to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "プロトコルは http または https のみ対応しています（入力: {scheme}）。"
        ));
    }
    let rest = &with_scheme[scheme_end + 3..];
    let host_and_path = rest.trim_end_matches('/');
    if host_and_path.is_empty() || host_and_path.starts_with('/') {
        return Err("URL にホスト名が含まれていません。".to_string());
    }
    if host_and_path.contains(char::is_whitespace) {
        return Err("URL に空白が含まれています。".to_string());
    }
    Ok(format!("{scheme}://{host_and_path}"))
}

/// メトリクス URL を解決する。
///
/// - `metrics_url` が指定されていれば、その URL そのもの（`/metrics` を含む完全な URL）を使う。
/// - 未指定なら API の URL のポートを 9998 に置換し、末尾に `/metrics` を付ける。
///
/// MediaMTX のメトリクスは `http://<host>:<metricsAddress>/metrics` で提供される。
pub fn resolve_metrics_url(cfg: &EndpointConfig) -> Result<String, String> {
    if let Some(url) = cfg
        .metrics_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let normalized = normalize_base_url(url)?;
        return Ok(if has_metrics_path(&normalized) {
            normalized
        } else {
            format!("{normalized}/metrics")
        });
    }
    let base = normalize_base_url(&cfg.base_url)?;
    let scheme_end = base.find("://").unwrap_or(0) + 3;
    let (scheme, hostport) = base.split_at(scheme_end);
    // API の URL にパス接頭辞がある場合（リバースプロキシ配下など）は引き継がない。
    let (authority, _path) = split_authority(hostport);
    let (host, port) = split_host_port(authority);
    let new_port = match port {
        Some("9997") => "9998".to_string(),
        Some(other) => other.to_string(),
        None => "9998".to_string(),
    };
    Ok(format!("{scheme}{host}:{new_port}/metrics"))
}

/// URL が `/metrics`（または `/metrics/`）で終わっているか。
fn has_metrics_path(url: &str) -> bool {
    url.ends_with("/metrics") || url.ends_with("/metrics/")
}

/// ホスト部とパス部を分ける。
fn split_authority(host_and_path: &str) -> (&str, &str) {
    match host_and_path.find('/') {
        Some(idx) => (&host_and_path[..idx], &host_and_path[idx..]),
        None => (host_and_path, ""),
    }
}

fn split_host_port(hostport: &str) -> (&str, Option<&str>) {
    // IPv6 リテラル `[::1]:9997` に対応。
    if let Some(rest) = hostport.strip_prefix('[') {
        if let Some(close) = rest.find(']') {
            let host = &hostport[..close + 2];
            let after = &hostport[close + 2..];
            let port = after.strip_prefix(':').filter(|p| !p.is_empty());
            return (host, port);
        }
        return (hostport, None);
    }
    match hostport.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (hostport, None),
    }
}

/// SRT 品質の警告しきい値。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SrtThresholds {
    /// ドロップ率 (%) の警告値。
    pub drop_rate_warn: f64,
    /// ドロップ率 (%) の危険値。
    pub drop_rate_crit: f64,
    /// 回復率 (%) の下限警告値。
    pub recovery_rate_warn: f64,
    /// RTT (ms) の警告値。
    pub rtt_ms_warn: f64,
    /// 警告の再発火を抑えるクールダウン（秒）。
    pub cooldown_secs: u64,
    /// 劣化／回復を確定するまでの保持時間（秒）。
    ///
    /// 1 周期だけ黄色になったような瞬時の変化をそのままイベントにすると、
    /// 「劣化」と「回復」が交互に並んでノイズになる。この秒数だけ状態が
    /// 続いてから、劣化・回復を確定して記録する（0 で従来どおり即時）。
    /// 記録する時刻は、状態が最初に変わった時点まで遡る。
    pub hold_secs: u64,
}

impl Default for SrtThresholds {
    fn default() -> Self {
        Self {
            drop_rate_warn: 0.5,
            drop_rate_crit: 2.0,
            recovery_rate_warn: 90.0,
            rtt_ms_warn: 200.0,
            cooldown_secs: 300,
            hold_secs: 3,
        }
    }
}

/// 通知（OS トースト）の ON/OFF 規則。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotifyRules {
    pub enabled: bool,
    pub srt_conn_dropped: bool,
    pub srt_conn_appeared: bool,
    pub srt_degraded: bool,
    pub path_down: bool,
    pub api_unreachable: bool,
}

impl Default for NotifyRules {
    fn default() -> Self {
        Self {
            enabled: true,
            srt_conn_dropped: true,
            srt_conn_appeared: false,
            srt_degraded: true,
            path_down: true,
            api_unreachable: true,
        }
    }
}

/// アプリ全体の設定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    pub endpoint: EndpointConfig,
    /// ポーリング間隔 (ms)。1000..=60000。
    pub poll_interval_ms: u64,
    /// 1 リクエストのタイムアウト (ms)。
    pub request_timeout_ms: u64,
    /// 時系列の保持時間 (時間)。1..=72。
    pub history_hours: u32,
    /// この回数連続で API 取得に失敗したら「API 到達不可」と判定する。
    pub failure_threshold: u32,
    pub srt: SrtThresholds,
    pub notify: NotifyRules,
    pub minimize_to_tray: bool,
    pub auto_start: bool,
    pub theme: Theme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
}

impl AppConfig {
    /// 値域を補正する。スライダー等で範囲外の値が来ても破綻しないように。
    pub fn sanitized(mut self) -> Self {
        self.poll_interval_ms = self.poll_interval_ms.clamp(1_000, 60_000);
        self.request_timeout_ms = self.request_timeout_ms.clamp(500, 30_000);
        // 取得間隔よりタイムアウトが長いと次周期が詰まるので上限を合わせる。
        self.request_timeout_ms = self.request_timeout_ms.min(self.poll_interval_ms.max(500));
        self.history_hours = self.history_hours.clamp(1, 72);
        self.failure_threshold = self.failure_threshold.clamp(1, 100);
        self.srt.drop_rate_warn = self.srt.drop_rate_warn.clamp(0.0, 100.0);
        self.srt.drop_rate_crit = self
            .srt
            .drop_rate_crit
            .clamp(self.srt.drop_rate_warn, 100.0);
        self.srt.recovery_rate_warn = self.srt.recovery_rate_warn.clamp(0.0, 100.0);
        self.srt.rtt_ms_warn = self.srt.rtt_ms_warn.clamp(1.0, 60_000.0);
        self.srt.cooldown_secs = self.srt.cooldown_secs.clamp(0, 86_400);
        // 保持は 1 分まで（それ以上は「劣化」と呼べない長さになる）。
        self.srt.hold_secs = self.srt.hold_secs.clamp(0, 60);
        self.endpoint.base_url = self.endpoint.base_url.trim().to_string();
        self
    }

    /// ポーリング間隔から算出した最大サンプル数。
    pub fn max_samples(&self) -> usize {
        let span_secs = u64::from(self.history_hours) * 3600;
        let interval = self.poll_interval_ms.max(1);
        ((span_secs * 1000 / interval) as usize).clamp(60, 500_000)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            endpoint: EndpointConfig::default(),
            poll_interval_ms: 2_000,
            request_timeout_ms: 1_500,
            history_hours: 4,
            failure_threshold: 3,
            srt: SrtThresholds::default(),
            notify: NotifyRules::default(),
            minimize_to_tray: true,
            auto_start: true,
            theme: Theme::System,
        }
    }
}

/// 永続化用のシリアライズ時に認証情報が出ないことを保証するためのヘルパ。
impl EndpointConfig {
    /// 認証ヘッダ値を組み立てる。認証不要なら `None`。
    pub fn authorization_value(&self) -> Option<String> {
        match self.auth {
            AuthMode::None => None,
            AuthMode::Bearer => {
                let token = self.token.trim();
                if token.is_empty() {
                    None
                } else {
                    Some(format!("Bearer {token}"))
                }
            }
            AuthMode::Basic => {
                let user = self.user.trim();
                if user.is_empty() && self.password.is_empty() {
                    None
                } else {
                    Some(format!(
                        "Basic {}",
                        base64_encode(format!("{user}:{}", self.password).as_bytes())
                    ))
                }
            }
        }
    }
}

/// 最小限の Base64 エンコーダ（Basic 認証ヘッダ用。外部クレートを増やさない）。
pub fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_examples() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
    }

    #[test]
    fn normalize_accepts_scheme_less_input() {
        assert_eq!(
            normalize_base_url("127.0.0.1:9997").unwrap(),
            "http://127.0.0.1:9997"
        );
        assert_eq!(
            normalize_base_url(" https://mtx.example.com/ ").unwrap(),
            "https://mtx.example.com"
        );
    }

    #[test]
    fn normalize_rejects_bad_input() {
        assert!(normalize_base_url("").is_err());
        assert!(normalize_base_url("ftp://host").is_err());
        assert!(normalize_base_url("http://").is_err());
        assert!(normalize_base_url("http://has space").is_err());
    }

    #[test]
    fn metrics_url_derivation() {
        let mut cfg = EndpointConfig::default();
        assert_eq!(
            resolve_metrics_url(&cfg).unwrap(),
            "http://127.0.0.1:9998/metrics"
        );

        cfg.base_url = "http://10.0.0.5:9997".into();
        assert_eq!(
            resolve_metrics_url(&cfg).unwrap(),
            "http://10.0.0.5:9998/metrics"
        );

        cfg.base_url = "http://10.0.0.5".into();
        assert_eq!(
            resolve_metrics_url(&cfg).unwrap(),
            "http://10.0.0.5:9998/metrics"
        );

        cfg.base_url = "http://[::1]:9997".into();
        assert_eq!(
            resolve_metrics_url(&cfg).unwrap(),
            "http://[::1]:9998/metrics"
        );

        // パス付きで指定した場合は `/metrics` を補う。
        cfg.metrics_url = Some("http://10.0.0.9:9100/custom".into());
        assert_eq!(
            resolve_metrics_url(&cfg).unwrap(),
            "http://10.0.0.9:9100/custom/metrics"
        );
        // 完全な URL（/metrics を含む）はそのまま使う。
        cfg.metrics_url = Some("http://10.0.0.9:9100/metrics".into());
        assert_eq!(
            resolve_metrics_url(&cfg).unwrap(),
            "http://10.0.0.9:9100/metrics"
        );
    }

    #[test]
    fn credentials_are_not_serialized() {
        let cfg = AppConfig {
            endpoint: EndpointConfig {
                auth: AuthMode::Basic,
                user: "alice".into(),
                password: "s3cret".into(),
                token: "tok".into(),
                ..EndpointConfig::default()
            },
            ..AppConfig::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(!json.contains("s3cret"), "{json}");
        assert!(!json.contains("\"tok\""), "{json}");
        assert!(json.contains("alice"));

        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.endpoint.password, "");
        assert_eq!(back.endpoint.auth, AuthMode::Basic);
    }

    #[test]
    fn sanitize_clamps_ranges() {
        let mut cfg = AppConfig {
            poll_interval_ms: 10,
            request_timeout_ms: 60_000,
            history_hours: 1000,
            ..AppConfig::default()
        };
        cfg.srt.drop_rate_warn = 5.0;
        cfg.srt.drop_rate_crit = 1.0;
        let cfg = cfg.sanitized();
        assert_eq!(cfg.poll_interval_ms, 1_000);
        assert!(cfg.request_timeout_ms <= 1_000);
        assert_eq!(cfg.history_hours, 72);
        assert!(cfg.srt.drop_rate_crit >= cfg.srt.drop_rate_warn);
    }

    #[test]
    fn max_samples_bounds() {
        let cfg = AppConfig {
            poll_interval_ms: 2_000,
            history_hours: 4,
            ..AppConfig::default()
        };
        assert_eq!(cfg.max_samples(), 7_200);
    }

    #[test]
    fn authz_header() {
        let mut ep = EndpointConfig::default();
        assert_eq!(ep.authorization_value(), None);
        ep.auth = AuthMode::Bearer;
        ep.token = "abc".into();
        assert_eq!(ep.authorization_value().as_deref(), Some("Bearer abc"));
        ep.auth = AuthMode::Basic;
        ep.user = "u".into();
        ep.password = "p".into();
        assert_eq!(ep.authorization_value().as_deref(), Some("Basic dTpw"));
    }
}
