//! SRT 品質会計（パケットドロップ率・回復率・RTT・帯域余裕の算出）。
//! MediaMTX の `srt_conns_*` は **累積カウンタ** なので、そのまま表示しても
//! 「いま詰まっているか」は分からない。ここでは前回サンプルとの差分から
//! 区間ごとの品質を算出する。定義は SRT の標準的な意味に沿う:
//!
//! | 指標 | 定義 |
//! |---|---|
//! | 受信ドロップ率 | Δ`packets_received_drop` / Δ`packets_received` × 100 |
//! | 回復率 | Δ`packets_received_retrans` / Δ`packets_received_loss` × 100（>100% もあり得る） |
//! | 未回復率 | 100 − 回復率 |
//! | NAK 率 | Δ`packets_sent_nak` / Δ`packets_received_unique` × 100（送信側の要求率） |
//!
//! カウンタがリセットされた（Δ < 0）場合は差分を `None` として扱い、0 と
//! 混同しない。初回観測も同様に `None`。

use serde::{Deserialize, Serialize};

/// SRT 接続の 1 サンプル（累積値）。MediaMTX の `/v3/srt/conns/list` と
/// `/metrics` の `srt_conns_*` のどちらからでも作れる。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SrtCounters {
    pub packets_received: Option<u64>,
    pub packets_received_unique: Option<u64>,
    pub packets_received_drop: Option<u64>,
    pub packets_received_retrans: Option<u64>,
    pub packets_received_loss: Option<u64>,
    pub packets_received_belated: Option<u64>,
    pub packets_received_undecrypt: Option<u64>,
    pub packets_sent: Option<u64>,
    pub packets_sent_unique: Option<u64>,
    pub packets_retrans: Option<u64>,
    pub packets_send_drop: Option<u64>,
    pub packets_send_loss: Option<u64>,
    pub packets_sent_nak: Option<u64>,
    pub packets_received_nak: Option<u64>,
    // 瞬間値（差分不要）
    pub ms_rtt: f64,
    pub mbps_receive_rate: f64,
    pub mbps_send_rate: f64,
    pub mbps_link_capacity: f64,
    pub us_packets_send_period: f64,
    pub ms_receive_buf: Option<u64>,
    pub ms_send_buf: Option<u64>,
    pub packets_flight_size: Option<u64>,
    pub packets_flow_window: Option<u64>,
    pub outbound_frames_discarded: Option<u64>,
}

/// 区間ごとの派生値。`None` は「算出できない（初回・カウンタリセット・欠測）」。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SrtDerived {
    /// 観測区間の長さ (ms)。
    pub interval_ms: i64,
    /// 新規受信（ユニーク）パケット数。
    pub recv_packets: Option<f64>,
    /// 再送で回復したパケット数。
    pub recovered_packets: Option<f64>,
    /// 損失として検出されたパケット数（区間値）。
    pub loss_packets: Option<f64>,
    /// 受信側で捨てられたパケット数（区間値）。
    pub drop_packets: Option<f64>,
    /// 復号失敗（区間値）。
    pub undecrypt_packets: Option<f64>,
    /// 遅延到着（区間値）。
    pub belated_packets: Option<f64>,
    /// 受信ドロップ率 (%)。
    pub drop_rate: Option<f64>,
    /// 回復率 (%) = 再送受信 / 損失検出。
    pub recovery_rate: Option<f64>,
    /// 未回復率 (%) = 100 − 回復率（下限 0）。
    pub unrecovered_rate: Option<f64>,
    /// NAK 要求率 (%)。
    pub nak_rate: Option<f64>,
    /// 受信レート (Mbps)。累積バイトが取れない場面でも、
    /// `packets_received_unique` × MSS 相当の概算に頼らず MediaMTX の瞬間値を優先する。
    pub recv_mbps: f64,
    pub link_capacity_mbps: f64,
    /// 帯域余裕率 (%) = (link − recv) / link × 100。
    pub headroom_percent: Option<f64>,
    pub rtt_ms: f64,
    /// 推定遅延 (ms) = RTT + 4 × パケット送信間隔。
    pub est_latency_ms: f64,
    /// カウンタのリセットを検出したか。
    pub counter_reset: bool,
}

impl SrtDerived {
    /// 差分から算出する。`prev` が `None`（初回）または `interval_ms <= 0` の
    /// ときは差分系を `None` にし、瞬間値だけを埋める。
    pub fn compute(prev: Option<&SrtCounters>, cur: &SrtCounters, interval_ms: i64) -> Self {
        let mut d = Self {
            interval_ms,
            recv_mbps: finite(cur.mbps_receive_rate),
            link_capacity_mbps: finite(cur.mbps_link_capacity),
            rtt_ms: finite(cur.ms_rtt),
            est_latency_ms: finite(cur.ms_rtt) + 4.0 * finite(cur.us_packets_send_period) / 1000.0,
            ..Default::default()
        };
        d.headroom_percent = if d.link_capacity_mbps > 0.0 {
            Some(((d.link_capacity_mbps - d.recv_mbps) / d.link_capacity_mbps) * 100.0)
        } else {
            None
        };

        let Some(prev) = prev.filter(|_| interval_ms > 0) else {
            return d;
        };

        let d_recv = delta(prev.packets_received, cur.packets_received);
        let d_unique = delta(prev.packets_received_unique, cur.packets_received_unique);
        let d_drop = delta(prev.packets_received_drop, cur.packets_received_drop);
        let d_retrans = delta(prev.packets_received_retrans, cur.packets_received_retrans);
        let d_loss = delta(prev.packets_received_loss, cur.packets_received_loss);
        let d_undecrypt = delta(
            prev.packets_received_undecrypt,
            cur.packets_received_undecrypt,
        );
        let d_belated = delta(prev.packets_received_belated, cur.packets_received_belated);
        let d_nak_sent = delta(prev.packets_sent_nak, cur.packets_sent_nak);
        let d_nak_received = delta(prev.packets_received_nak, cur.packets_received_nak);

        if is_reset(d_recv)
            || is_reset(d_unique)
            || is_reset(d_drop)
            || is_reset(d_retrans)
            || is_reset(d_loss)
        {
            d.counter_reset = true;
            return d;
        }

        d.recv_packets = d_unique.or(d_recv);
        d.recovered_packets = d_retrans;
        d.loss_packets = d_loss;
        d.drop_packets = d_drop;
        d.undecrypt_packets = d_undecrypt;
        d.belated_packets = d_belated;

        d.drop_rate = match (d_drop, d_recv.filter(|v| *v > 0.0)) {
            (Some(drop), Some(recv)) => Some(drop / recv * 100.0),
            _ => None,
        };
        d.recovery_rate = match (d_retrans, d_loss) {
            (Some(retrans), Some(loss)) if loss > 0.0 => Some(retrans / loss * 100.0),
            // 損失 0 の区間は「劣化なし」。回復率 100 とみなす（分母 0 の扱いを明示）。
            (Some(_), Some(0.0)) => Some(100.0),
            (Some(_), None) => None,
            _ => None,
        };
        d.unrecovered_rate = d.recovery_rate.map(|r| (100.0 - r).max(0.0));
        d.nak_rate = match (d_nak_sent.or(d_nak_received), d_recv.filter(|v| *v > 0.0)) {
            (Some(nak), Some(recv)) => Some(nak / recv * 100.0),
            _ => None,
        };
        d
    }

    /// 劣化判定（UI の色分け用）。しきい値は設定から渡す。
    pub fn severity(&self, warn: &crate::config::SrtThresholds) -> SrtSeverity {
        let drop = self.drop_rate.unwrap_or(0.0);
        let recovery = self.recovery_rate.unwrap_or(100.0);
        if drop >= warn.drop_rate_crit || self.rtt_ms >= warn.rtt_ms_warn * 2.0 {
            return SrtSeverity::Critical;
        }
        if drop >= warn.drop_rate_warn
            || (self.recovery_rate.is_some() && recovery < warn.recovery_rate_warn)
            || self.rtt_ms >= warn.rtt_ms_warn
            || self.undecrypt_packets.is_some_and(|v| v > 0.0)
        {
            return SrtSeverity::Warning;
        }
        SrtSeverity::Ok
    }
}

/// 品質の 3 段階。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SrtSeverity {
    Ok,
    Warning,
    Critical,
}

impl SrtSeverity {
    pub fn label(self) -> &'static str {
        match self {
            SrtSeverity::Ok => "良好",
            SrtSeverity::Warning => "注意",
            SrtSeverity::Critical => "劣化",
        }
    }
}

fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

/// `cur - prev`。どちらかが欠測なら `None`。
fn delta(prev: Option<u64>, cur: Option<u64>) -> Option<f64> {
    match (prev, cur) {
        (Some(p), Some(c)) => Some(c as f64 - p as f64),
        _ => None,
    }
}

fn is_reset(d: Option<f64>) -> bool {
    d.is_some_and(|v| v < 0.0)
}

impl SrtCounters {
    /// MediaMTX の `SRTConn` JSON から累積カウンタ部分を取り出す。
    pub fn from_json(v: &serde_json::Value) -> Self {
        let u = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f.max(0.0) as u64)))
        };
        let f = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
        Self {
            packets_received: u("packetsReceived"),
            packets_received_unique: u("packetsReceivedUnique"),
            packets_received_drop: u("packetsReceivedDrop"),
            packets_received_retrans: u("packetsReceivedRetrans"),
            packets_received_loss: u("packetsReceivedLoss"),
            packets_received_belated: u("packetsReceivedBelated"),
            packets_received_undecrypt: u("packetsReceivedUndecrypt"),
            packets_sent: u("packetsSent"),
            packets_sent_unique: u("packetsSentUnique"),
            packets_retrans: u("packetsRetrans"),
            packets_send_drop: u("packetsSendDrop"),
            packets_send_loss: u("packetsSendLoss"),
            packets_sent_nak: u("packetsSentNAK"),
            packets_received_nak: u("packetsReceivedNAK"),
            ms_rtt: f("msRTT"),
            mbps_receive_rate: f("mbpsReceiveRate"),
            mbps_send_rate: f("mbpsSendRate"),
            mbps_link_capacity: f("mbpsLinkCapacity"),
            us_packets_send_period: f("usPacketsSendPeriod"),
            ms_receive_buf: u("msReceiveBuf"),
            ms_send_buf: u("msSendBuf"),
            packets_flight_size: u("packetsFlightSize"),
            packets_flow_window: u("packetsFlowWindow"),
            outbound_frames_discarded: u("outboundFramesDiscarded"),
        }
    }

    /// `/metrics` の `srt_conns_<suffix>` 群から作る。
    pub fn from_metric_map(m: &std::collections::HashMap<String, f64>) -> Self {
        let u = |k: &str| {
            m.get(k)
                .filter(|v| v.is_finite())
                .map(|v| v.max(0.0).round() as u64)
        };
        let f = |k: &str| m.get(k).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
        Self {
            packets_received: u("packets_received"),
            packets_received_unique: u("packets_received_unique"),
            packets_received_drop: u("packets_received_drop"),
            packets_received_retrans: u("packets_received_retrans"),
            packets_received_loss: u("packets_received_loss"),
            packets_received_belated: u("packets_received_belated"),
            packets_received_undecrypt: u("packets_received_undecrypt"),
            packets_sent: u("packets_sent"),
            packets_sent_unique: u("packets_sent_unique"),
            packets_retrans: u("packets_retrans"),
            packets_send_drop: u("packets_send_drop"),
            packets_send_loss: u("packets_send_loss"),
            packets_sent_nak: u("packets_sent_nak"),
            packets_received_nak: u("packets_received_nak"),
            ms_rtt: f("ms_rtt"),
            mbps_receive_rate: f("mbps_receive_rate"),
            mbps_send_rate: f("mbps_send_rate"),
            mbps_link_capacity: f("mbps_link_capacity"),
            us_packets_send_period: f("us_packets_send_period"),
            ms_receive_buf: u("ms_receive_buf"),
            ms_send_buf: u("ms_send_buf"),
            packets_flight_size: u("packets_flight_size"),
            packets_flow_window: u("packets_flow_window"),
            outbound_frames_discarded: u("outbound_frames_discarded"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SrtThresholds;

    fn counters(recv: u64, unique: u64, drop: u64, retrans: u64, loss: u64) -> SrtCounters {
        SrtCounters {
            packets_received: Some(recv),
            packets_received_unique: Some(unique),
            packets_received_drop: Some(drop),
            packets_received_retrans: Some(retrans),
            packets_received_loss: Some(loss),
            ..Default::default()
        }
    }

    #[test]
    fn first_sample_has_no_deltas() {
        let cur = counters(1000, 900, 10, 5, 20);
        let d = SrtDerived::compute(None, &cur, 2000);
        assert!(d.drop_rate.is_none());
        assert!(d.recovery_rate.is_none());
        assert!(d.recv_packets.is_none());
        assert!(!d.counter_reset);
    }

    #[test]
    fn computes_drop_and_recovery_rates() {
        let prev = counters(10_000, 9_900, 100, 50, 200);
        // Δrecv=1000, Δunique=1000, Δdrop=5, Δretrans=45, Δloss=50
        let cur = counters(11_000, 10_900, 105, 95, 250);
        let d = SrtDerived::compute(Some(&prev), &cur, 2000);
        assert_eq!(d.recv_packets, Some(1000.0));
        assert_eq!(d.drop_packets, Some(5.0));
        assert_eq!(d.recovered_packets, Some(45.0));
        assert_eq!(d.loss_packets, Some(50.0));
        assert!((d.drop_rate.unwrap() - 0.5).abs() < 1e-9);
        assert!((d.recovery_rate.unwrap() - 90.0).abs() < 1e-9);
        assert!((d.unrecovered_rate.unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn loss_free_interval_reports_full_recovery() {
        let prev = counters(0, 0, 0, 0, 0);
        let cur = counters(100, 100, 0, 0, 0);
        let d = SrtDerived::compute(Some(&prev), &cur, 1000);
        assert_eq!(d.recovery_rate, Some(100.0));
        assert_eq!(d.unrecovered_rate, Some(0.0));
        assert_eq!(d.drop_rate, Some(0.0));
    }

    #[test]
    fn zero_received_interval_has_no_drop_rate() {
        let prev = counters(100, 100, 0, 0, 0);
        let cur = counters(100, 100, 0, 0, 0);
        let d = SrtDerived::compute(Some(&prev), &cur, 1000);
        assert!(d.drop_rate.is_none());
        assert_eq!(d.drop_packets, Some(0.0));
    }

    #[test]
    fn counter_reset_is_flagged() {
        let prev = counters(10_000, 9_900, 100, 50, 200);
        let cur = counters(10, 10, 0, 0, 0);
        let d = SrtDerived::compute(Some(&prev), &cur, 1000);
        assert!(d.counter_reset);
        assert!(d.drop_rate.is_none());
        assert!(d.recovery_rate.is_none());
    }

    #[test]
    fn recovery_can_exceed_100_percent() {
        let prev = counters(0, 0, 0, 0, 0);
        let cur = counters(1000, 990, 10, 30, 20);
        let d = SrtDerived::compute(Some(&prev), &cur, 1000);
        assert!(d.recovery_rate.unwrap() > 100.0);
        assert_eq!(d.unrecovered_rate, Some(0.0), "未回復率は 0 で下限");
    }

    #[test]
    fn zero_interval_is_treated_as_no_delta() {
        let prev = counters(0, 0, 0, 0, 0);
        let cur = counters(100, 100, 1, 1, 1);
        let d = SrtDerived::compute(Some(&prev), &cur, 0);
        assert!(d.recv_packets.is_none());
    }

    #[test]
    fn severity_thresholds() {
        let th = SrtThresholds::default();
        let mut d = SrtDerived {
            drop_rate: Some(0.1),
            recovery_rate: Some(99.0),
            rtt_ms: 20.0,
            ..Default::default()
        };
        assert_eq!(d.severity(&th), SrtSeverity::Ok);
        d.drop_rate = Some(0.6);
        assert_eq!(d.severity(&th), SrtSeverity::Warning);
        d.drop_rate = Some(3.0);
        assert_eq!(d.severity(&th), SrtSeverity::Critical);
        d.drop_rate = Some(0.0);
        d.recovery_rate = Some(80.0);
        assert_eq!(d.severity(&th), SrtSeverity::Warning);
        d.recovery_rate = Some(100.0);
        d.rtt_ms = 250.0;
        assert_eq!(d.severity(&th), SrtSeverity::Warning);
    }

    #[test]
    fn builds_from_metric_map() {
        let mut m = std::collections::HashMap::new();
        m.insert("packets_received_unique".to_string(), 1234.0);
        m.insert("ms_rtt".to_string(), 12.5);
        m.insert("packets_received_drop".to_string(), f64::NAN);
        let c = SrtCounters::from_metric_map(&m);
        assert_eq!(c.packets_received_unique, Some(1234));
        assert_eq!(c.ms_rtt, 12.5);
        assert_eq!(c.packets_received_drop, None, "NaN は欠測扱い");
    }

    #[test]
    fn builds_from_api_json() {
        let v = serde_json::json!({
            "packetsReceivedUnique": 5000,
            "packetsReceivedLoss": 10,
            "packetsReceivedRetrans": 8,
            "msRTT": 9.5,
            "mbpsReceiveRate": 12.25,
            "mbpsLinkCapacity": 50.0,
            "usPacketsSendPeriod": 1000.0
        });
        let c = SrtCounters::from_json(&v);
        assert_eq!(c.packets_received_unique, Some(5000));
        assert_eq!(c.ms_rtt, 9.5);
        let d = SrtDerived::compute(None, &c, 1000);
        assert_eq!(d.est_latency_ms, 9.5 + 4.0);
        assert_eq!(d.headroom_percent, Some((50.0 - 12.25) / 50.0 * 100.0));
    }
}
