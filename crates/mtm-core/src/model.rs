//! - 未知フィールドは無視する（MediaMTX のバージョン差を吸収するため）。
//! - 数値は JSON 上で整数/浮動小数の揺れがあるので、`u64` 系は小数を許容する
//!   デシリアライザで受ける。
//! - 日時は MediaMTX の RFC3339（`2026-10-31T11:00:00.123456789+09:00`）を
//!   `DateTime<FixedOffset>` で受ける。

use chrono::{DateTime, FixedOffset};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

/// `u64` への変換で小数・文字列を許容する。
fn de_u64_flexible<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    match v {
        serde_json::Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().map(|f| f.max(0.0).round() as u64))
            .ok_or_else(|| D::Error::custom("数値として解釈できません")),
        serde_json::Value::String(s) => s
            .parse::<u64>()
            .map_err(|_| D::Error::custom("数値文字列として解釈できません")),
        serde_json::Value::Null => Ok(0),
        other => Err(D::Error::custom(format!("期待しない型の値です: {other}"))),
    }
}

fn de_opt_u64_flexible<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Option::<serde_json::Value>::deserialize(deserializer)?;
    match v {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => Ok(n
            .as_u64()
            .or_else(|| n.as_f64().map(|f| f.max(0.0).round() as u64))),
        Some(serde_json::Value::String(s)) => s
            .parse::<u64>()
            .map(Some)
            .map_err(|_| D::Error::custom("数値文字列として解釈できません")),
        Some(other) => Err(D::Error::custom(format!("期待しない型の値です: {other}"))),
    }
}

fn de_f64_flexible<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    match v {
        serde_json::Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| D::Error::custom("浮動小数として解釈できません")),
        serde_json::Value::String(s) => s
            .parse::<f64>()
            .map_err(|_| D::Error::custom("数値文字列として解釈できません")),
        serde_json::Value::Null => Ok(0.0),
        other => Err(D::Error::custom(format!("期待しない型の値です: {other}"))),
    }
}

fn de_opt_time<'de, D>(deserializer: D) -> Result<Option<DateTime<FixedOffset>>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Option::<String>::deserialize(deserializer)?;
    match v {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => DateTime::parse_from_rfc3339(s.trim())
            .map(Some)
            .map_err(|e| D::Error::custom(format!("日時として解釈できません ({e})"))),
    }
}

/// `GET /v3/info`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Info {
    #[serde(default)]
    pub version: String,
    #[serde(default, deserialize_with = "de_opt_time")]
    pub started: Option<DateTime<FixedOffset>>,
}

/// パスのリーダー（視聴側）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathReader {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default)]
    pub reader_type: String,
}

/// パスのソース（配信元）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathSource {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default)]
    pub source_type: String,
}

/// トラック（コーデック情報）。`codecProps` は形式が多様なので生 JSON で保持する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathTrack {
    #[serde(default)]
    pub codec: String,
    #[serde(default, rename = "codecProps")]
    pub codec_props: Option<serde_json::Value>,
}

/// `GET /v3/paths/list` の `items[]`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Path {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "confName")]
    pub conf_name: String,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub online: bool,
    #[serde(default, rename = "availableTime", deserialize_with = "de_opt_time")]
    pub available_time: Option<DateTime<FixedOffset>>,
    #[serde(default, rename = "onlineTime", deserialize_with = "de_opt_time")]
    pub online_time: Option<DateTime<FixedOffset>>,
    #[serde(default)]
    pub source: Option<PathSource>,
    #[serde(default)]
    pub readers: Vec<PathReader>,
    #[serde(default)]
    pub tracks2: Vec<PathTrack>,
    #[serde(default, rename = "inboundBytes", deserialize_with = "de_u64_flexible")]
    pub inbound_bytes: u64,
    #[serde(
        default,
        rename = "outboundBytes",
        deserialize_with = "de_u64_flexible"
    )]
    pub outbound_bytes: u64,
    #[serde(
        default,
        rename = "inboundFramesInError",
        deserialize_with = "de_u64_flexible"
    )]
    pub inbound_frames_in_error: u64,
}

impl Path {
    /// 現在の視聴者数。
    pub fn reader_count(&self) -> usize {
        self.readers.len()
    }

    /// ソースの種別（未設定なら空文字）。
    pub fn source_type(&self) -> &str {
        self.source
            .as_ref()
            .map(|s| s.source_type.as_str())
            .unwrap_or("")
    }
}

/// ページング付きリストの共通形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListResponse<T> {
    #[serde(default, rename = "itemCount")]
    pub item_count: usize,
    #[serde(default, rename = "pageCount")]
    pub page_count: usize,
    pub items: Vec<T>,
}

impl<T: serde::de::DeserializeOwned> ListResponse<T> {
    /// フィールド欠落に耐えるパース。`items` が無ければ空リスト、
    /// `pageCount` が無ければ 1 とみなす。
    pub fn parse(body: &str) -> Result<Self, serde_json::Error> {
        let mut v: serde_json::Value = serde_json::from_str(body)?;
        let obj = v
            .as_object_mut()
            .ok_or_else(|| serde_json::Error::io(std::io::Error::other("object expected")))?;
        obj.entry("items")
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        obj.entry("pageCount").or_insert(serde_json::Value::from(1));
        obj.entry("itemCount").or_insert(serde_json::Value::from(0));
        serde_json::from_value(v)
    }
}

/// `GET /v3/srt/conns/list` の `items[]`。累積カウンタと瞬間値を持つ。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SrtConn {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub path: String,
    #[serde(default, rename = "remoteAddr")]
    pub remote_addr: String,
    /// `idle` / `read` / `publish`
    #[serde(default)]
    pub state: String,
    #[serde(default, deserialize_with = "de_opt_time")]
    pub created: Option<DateTime<FixedOffset>>,
    #[serde(default, rename = "expiresAt", deserialize_with = "de_opt_time")]
    pub expires_at: Option<DateTime<FixedOffset>>,

    #[serde(
        default,
        rename = "packetsReceived",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_received: Option<u64>,
    #[serde(
        default,
        rename = "packetsReceivedUnique",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_received_unique: Option<u64>,
    #[serde(
        default,
        rename = "packetsReceivedDrop",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_received_drop: Option<u64>,
    #[serde(
        default,
        rename = "packetsReceivedRetrans",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_received_retrans: Option<u64>,
    #[serde(
        default,
        rename = "packetsReceivedLoss",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_received_loss: Option<u64>,
    #[serde(
        default,
        rename = "packetsReceivedBelated",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_received_belated: Option<u64>,
    #[serde(
        default,
        rename = "packetsReceivedUndecrypt",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_received_undecrypt: Option<u64>,
    #[serde(
        default,
        rename = "packetsSent",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_sent: Option<u64>,
    #[serde(
        default,
        rename = "packetsSentUnique",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_sent_unique: Option<u64>,
    #[serde(
        default,
        rename = "packetsRetrans",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_retrans: Option<u64>,
    #[serde(
        default,
        rename = "packetsSendDrop",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_send_drop: Option<u64>,
    #[serde(
        default,
        rename = "packetsSendLoss",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_send_loss: Option<u64>,
    #[serde(
        default,
        rename = "packetsSentNAK",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_sent_nak: Option<u64>,
    #[serde(
        default,
        rename = "packetsReceivedNAK",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_received_nak: Option<u64>,
    #[serde(
        default,
        rename = "packetsReceiveBuf",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_receive_buf: Option<u64>,
    #[serde(
        default,
        rename = "packetsSendBuf",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_send_buf: Option<u64>,
    #[serde(
        default,
        rename = "packetsFlightSize",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_flight_size: Option<u64>,
    #[serde(
        default,
        rename = "packetsFlowWindow",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub packets_flow_window: Option<u64>,

    #[serde(
        default,
        rename = "mbpsLinkCapacity",
        deserialize_with = "de_f64_flexible"
    )]
    pub mbps_link_capacity: f64,
    #[serde(default, rename = "mbpsMaxBW", deserialize_with = "de_f64_flexible")]
    pub mbps_max_bw: f64,
    #[serde(
        default,
        rename = "mbpsReceiveRate",
        deserialize_with = "de_f64_flexible"
    )]
    pub mbps_receive_rate: f64,
    #[serde(default, rename = "mbpsSendRate", deserialize_with = "de_f64_flexible")]
    pub mbps_send_rate: f64,
    #[serde(default, rename = "msRTT", deserialize_with = "de_f64_flexible")]
    pub ms_rtt: f64,
    #[serde(
        default,
        rename = "msReceiveBuf",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub ms_receive_buf: Option<u64>,
    #[serde(
        default,
        rename = "msSendBuf",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub ms_send_buf: Option<u64>,
    #[serde(
        default,
        rename = "msReceiveTsbPdDelay",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub ms_receive_tsb_pd_delay: Option<u64>,
    #[serde(
        default,
        rename = "msSendTsbPdDelay",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub ms_send_tsb_pd_delay: Option<u64>,
    #[serde(
        default,
        rename = "usPacketsSendPeriod",
        deserialize_with = "de_f64_flexible"
    )]
    pub us_packets_send_period: f64,
    #[serde(
        default,
        rename = "outboundFramesDiscarded",
        deserialize_with = "de_opt_u64_flexible"
    )]
    pub outbound_frames_discarded: Option<u64>,
}

/// 接続/セッション一覧で扱う種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnKind {
    Rtsp,
    Rtsps,
    Rtmp,
    Rtmps,
    Srt,
    WebRtc,
    Hls,
    Moq,
}

impl ConnKind {
    pub const ALL: [ConnKind; 8] = [
        ConnKind::Srt,
        ConnKind::Rtsp,
        ConnKind::Rtsps,
        ConnKind::Rtmp,
        ConnKind::Rtmps,
        ConnKind::WebRtc,
        ConnKind::Hls,
        ConnKind::Moq,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ConnKind::Rtsp => "RTSP",
            ConnKind::Rtsps => "RTSPS",
            ConnKind::Rtmp => "RTMP",
            ConnKind::Rtmps => "RTMPS",
            ConnKind::Srt => "SRT",
            ConnKind::WebRtc => "WebRTC",
            ConnKind::Hls => "HLS",
            ConnKind::Moq => "MoQ",
        }
    }

    /// 一覧 API のパス（v1.21 系の実装で確認済み）。
    pub fn list_path(self) -> &'static str {
        match self {
            ConnKind::Rtsp => "/v3/rtsp/conns/list",
            ConnKind::Rtsps => "/v3/rtsps/conns/list",
            ConnKind::Rtmp => "/v3/rtmp/conns/list",
            ConnKind::Rtmps => "/v3/rtmps/conns/list",
            ConnKind::Srt => "/v3/srt/conns/list",
            ConnKind::WebRtc => "/v3/webrtc/sessions/list",
            ConnKind::Hls => "/v3/hls/sessions/list",
            ConnKind::Moq => "/v3/moq/sessions/list",
        }
    }

    pub fn kick_path(self, id: &str) -> String {
        match self {
            ConnKind::Rtsp => format!("/v3/rtsp/conns/kick/{id}"),
            ConnKind::Rtsps => format!("/v3/rtsps/conns/kick/{id}"),
            ConnKind::Rtmp => format!("/v3/rtmp/conns/kick/{id}"),
            ConnKind::Rtmps => format!("/v3/rtmps/conns/kick/{id}"),
            ConnKind::Srt => format!("/v3/srt/conns/kick/{id}"),
            ConnKind::WebRtc => format!("/v3/webrtc/sessions/kick/{id}"),
            ConnKind::Hls => format!("/v3/hls/sessions/kick/{id}"),
            ConnKind::Moq => format!("/v3/moq/sessions/kick/{id}"),
        }
    }
}

/// 接続一覧の共通表示形。種別ごとにフィールドが違うので必要分のみ正規化する。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnInfo {
    pub kind: ConnKind,
    pub id: String,
    pub path: String,
    pub remote_addr: String,
    pub state: String,
    pub created: Option<DateTime<FixedOffset>>,
    pub inbound_bytes: u64,
    pub outbound_bytes: u64,
    /// プロトコル固有の補足（SRT: rtt/drop 等、WebRTC: jitter 等）。
    pub detail: serde_json::Value,
}

impl ConnInfo {
    pub fn from_value(kind: ConnKind, v: &serde_json::Value) -> Self {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let u = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)))
                .unwrap_or(0)
        };
        let created = v
            .get("created")
            .and_then(|x| x.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
        let inbound = if v.get("inboundBytes").is_some() {
            u("inboundBytes")
        } else {
            u("bytesReceived")
        };
        let outbound = if v.get("outboundBytes").is_some() {
            u("outboundBytes")
        } else {
            u("bytesSent")
        };
        let mut detail = v.clone();
        if let Some(map) = detail.as_object_mut() {
            for key in [
                "id",
                "path",
                "remoteAddr",
                "state",
                "created",
                "inboundBytes",
                "outboundBytes",
                "bytesReceived",
                "bytesSent",
            ] {
                map.remove(key);
            }
        }
        Self {
            kind,
            id: s("id"),
            path: s("path"),
            remote_addr: s("remoteAddr"),
            state: s("state"),
            created,
            inbound_bytes: inbound,
            outbound_bytes: outbound,
            detail,
        }
    }
}

/// グローバル設定の GUI 対象項目の要約。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GlobalConfSummary {
    pub api: Option<bool>,
    pub api_address: Option<String>,
    pub metrics: Option<bool>,
    pub metrics_address: Option<String>,
    pub log_level: Option<String>,
    pub log_destinations: Option<Vec<String>>,
    pub log_file: Option<String>,
    pub rtsp: Option<bool>,
    pub rtsp_address: Option<String>,
    pub rtmp: Option<bool>,
    pub rtmp_address: Option<String>,
    pub srt: Option<bool>,
    pub srt_address: Option<String>,
    pub hls: Option<bool>,
    pub hls_address: Option<String>,
    pub webrtc: Option<bool>,
    pub webrtc_address: Option<String>,
    pub playback: Option<bool>,
    pub playback_address: Option<String>,
}

/// 生のグローバル設定から要約を抜き出す。型が想定外でも取れた分だけ返す。
pub fn summarize_global_conf(raw: &serde_json::Value) -> GlobalConfSummary {
    let get_bool = |k: &str| raw.get(k).and_then(|v| v.as_bool());
    let get_str = |k: &str| raw.get(k).and_then(|v| v.as_str()).map(str::to_string);
    GlobalConfSummary {
        api: get_bool("api"),
        api_address: get_str("apiAddress"),
        metrics: get_bool("metrics"),
        metrics_address: get_str("metricsAddress"),
        log_level: get_str("logLevel"),
        log_destinations: raw.get("logDestinations").and_then(|v| {
            v.as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
        }),
        log_file: get_str("logFile"),
        rtsp: get_bool("rtsp"),
        rtsp_address: get_str("rtspAddress"),
        rtmp: get_bool("rtmp"),
        rtmp_address: get_str("rtmpAddress"),
        srt: get_bool("srt"),
        srt_address: get_str("srtAddress"),
        hls: get_bool("hls"),
        hls_address: get_str("hlsAddress"),
        webrtc: get_bool("webrtc"),
        webrtc_address: get_str("webrtcAddress"),
        playback: get_bool("playback"),
        playback_address: get_str("playbackAddress"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_path_item() {
        let raw = r#"{
            "name": "cam1",
            "confName": "cam1",
            "available": true,
            "online": true,
            "availableTime": "2026-10-31T10:00:00.123456789+09:00",
            "onlineTime": null,
            "source": {"type": "srtConn", "id": "b3a5"},
            "readers": [{"type": "hlsSession", "id": "1"}, {"type": "webRTCSession", "id": "2"}],
            "tracks2": [{"codec": "H264", "codecProps": {"width": 1920, "height": 1080, "level": "4.1"}}],
            "bytesReceived": 10,
            "bytesSent": 5,
            "inboundBytes": 1024,
            "outboundBytes": 2048,
            "inboundFramesInError": 3,
            "ready": true,
            "readyTime": "2026-10-31T10:00:00.123456789+09:00",
            "unknownFutureField": {"x": 1}
        }"#;
        let p: Path = serde_json::from_str(raw).unwrap();
        assert_eq!(p.name, "cam1");
        assert_eq!(p.conf_name, "cam1");
        assert!(p.available && p.online);
        assert_eq!(p.reader_count(), 2);
        assert_eq!(p.source_type(), "srtConn");
        assert_eq!(p.inbound_bytes, 1024);
        assert_eq!(p.outbound_bytes, 2048);
        assert_eq!(p.inbound_frames_in_error, 3);
        assert_eq!(p.tracks2[0].codec, "H264");
        assert!(p.online_time.is_none());
        assert!(p.available_time.is_some());
    }

    #[test]
    fn flex_number_parsing() {
        let raw = r#"{"name":"x","inboundBytes": 1.0e3, "outboundBytes": "512"}"#;
        let p: Path = serde_json::from_str(raw).unwrap();
        assert_eq!(p.inbound_bytes, 1000);
        assert_eq!(p.outbound_bytes, 512);
    }

    #[test]
    fn missing_fields_default() {
        let p: Path = serde_json::from_str(r#"{"name":"only-name"}"#).unwrap();
        assert!(!p.available);
        assert!(p.source.is_none());
        assert!(p.readers.is_empty());
        assert_eq!(p.source_type(), "");
    }

    #[test]
    fn parses_info() {
        let i: Info =
            serde_json::from_str(r#"{"version":"v1.21.2","started":"2026-10-31T09:00:00Z"}"#)
                .unwrap();
        assert_eq!(i.version, "v1.21.2");
        assert!(i.started.is_some());
    }

    #[test]
    fn parses_srt_conn() {
        let raw = r#"{
            "id":"6f1d","path":"cam1","remoteAddr":"10.0.0.2:53512","state":"read",
            "created":"2026-10-31T09:00:00.5+09:00","expiresAt":null,
            "packetsReceived":12345,"packetsReceivedUnique":12000,
            "packetsReceivedDrop":12,"packetsReceivedRetrans":300,"packetsReceivedLoss":320,
            "packetsSent":0,"packetsSentUnique":0,"packetsRetrans":0,
            "packetsSendDrop":0,"packetsSendLoss":0,"packetsSentNAK":0,"packetsReceivedNAK":0,
            "mbpsLinkCapacity":40.5,"mbpsMaxBW":0,"mbpsReceiveRate":12.25,"mbpsSendRate":0,
            "msRTT":8.75,"usPacketsSendPeriod":1000,"msReceiveBuf":120,
            "outboundFramesDiscarded":0
        }"#;
        let c: SrtConn = serde_json::from_str(raw).unwrap();
        assert_eq!(c.id, "6f1d");
        assert_eq!(c.remote_addr, "10.0.0.2:53512");
        assert_eq!(c.packets_received_unique, Some(12000));
        assert_eq!(c.packets_received_loss, Some(320));
        assert_eq!(c.ms_rtt, 8.75);
        assert_eq!(c.mbps_receive_rate, 12.25);
        assert!(c.created.is_some());
    }

    #[test]
    fn list_response_tolerates_missing_keys() {
        let l = ListResponse::<std::collections::HashMap<String, serde_json::Value>>::parse(
            r#"{"itemCount":0,"pageCount":1,"items":[]}"#,
        )
        .unwrap();
        assert!(l.items.is_empty());
        let l = ListResponse::<serde_json::Value>::parse(r#"{"items":[{"a":1}]}"#).unwrap();
        assert_eq!(l.items.len(), 1);
        assert_eq!(l.page_count, 1);
    }

    #[test]
    fn conn_kind_paths() {
        assert_eq!(ConnKind::Srt.list_path(), "/v3/srt/conns/list");
        assert_eq!(ConnKind::WebRtc.list_path(), "/v3/webrtc/sessions/list");
        assert_eq!(ConnKind::Srt.kick_path("abc"), "/v3/srt/conns/kick/abc");
        for k in ConnKind::ALL {
            assert!(k.list_path().starts_with("/v3/"));
            assert!(k.kick_path("x").starts_with("/v3/"));
        }
    }

    #[test]
    fn conn_info_normalizes_legacy_fields() {
        let v = serde_json::json!({
            "id": "1", "path": "p", "remoteAddr": "1.2.3.4:5", "state": "read",
            "created": "2026-10-31T09:00:00Z", "bytesReceived": 7, "bytesSent": 9,
            "msRTT": 12.5
        });
        let info = ConnInfo::from_value(ConnKind::Srt, &v);
        assert_eq!(info.inbound_bytes, 7);
        assert_eq!(info.outbound_bytes, 9);
        assert!(info.created.is_some());
        assert_eq!(
            info.detail.get("msRTT").and_then(|x| x.as_f64()),
            Some(12.5)
        );
        assert!(info.detail.get("path").is_none());
    }

    #[test]
    fn summarizes_global_conf() {
        let raw = serde_json::json!({
            "api": true, "apiAddress": ":9997", "metrics": false,
            "logLevel": "info", "logDestinations": ["stdout","file"],
            "srt": true, "srtAddress": ":8890", "unknown": 1
        });
        let s = summarize_global_conf(&raw);
        assert_eq!(s.api, Some(true));
        assert_eq!(s.metrics, Some(false));
        assert_eq!(s.srt_address.as_deref(), Some(":8890"));
        assert_eq!(s.log_destinations.as_ref().map(|v| v.len()), Some(2));
    }
}
