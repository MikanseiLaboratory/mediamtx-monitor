//! 監視エンジン。HTTP 輸送を差し込んで 1 周期分の観測を組み立てる。
//!
//! Tauri アダプタはこの `Monitor` を専用スレッドから呼ぶだけ。時刻と輸送も
//! 輸送も注入できるので、テストでは固定時刻 + 偽輸送で完全に決定的に
//! 駆動できる。

use chrono::{DateTime, FixedOffset, Utc};
use std::collections::HashMap;

use crate::client::ApiClient;
use crate::config::AppConfig;
use crate::error::ApiError;
use crate::events::{ApiHealth, MonitorUpdate};
use crate::frame::FrameBuilder;
use crate::srt::SrtCounters;
use crate::state::{ObservedPath, ObservedSrt};
use crate::transport::HttpTransport;

/// 1 周期分のポーリング。
pub struct Monitor<'t> {
    transport: &'t dyn HttpTransport,
    config: AppConfig,
    builder: FrameBuilder,
    metrics_enabled: bool,
    started_at: Option<DateTime<FixedOffset>>,
}

/// 周期の結果。`update` は emit 用、`api` は状態表示用。
pub struct CycleResult {
    pub update: MonitorUpdate,
    pub fatal_error: Option<String>,
}

impl<'t> Monitor<'t> {
    pub fn new(transport: &'t dyn HttpTransport, config: AppConfig) -> Self {
        let builder = FrameBuilder::new(&config);
        Self {
            transport,
            config,
            builder,
            metrics_enabled: true,
            started_at: None,
        }
    }

    pub fn builder(&self) -> &FrameBuilder {
        &self.builder
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn mark_started(&mut self, at: DateTime<FixedOffset>) {
        self.started_at = Some(at);
    }

    pub fn started_at(&self) -> Option<DateTime<FixedOffset>> {
        self.started_at
    }

    /// 1 周期。API が落ちていても panic せず、`ApiHealth` に理由を載せる。
    pub fn tick(&mut self, at: DateTime<FixedOffset>) -> CycleResult {
        let client = match ApiClient::new(
            self.transport,
            &self.config.endpoint,
            self.config.request_timeout_ms,
        ) {
            Ok(c) => c,
            Err(e) => {
                let health = ApiHealth {
                    ok: false,
                    error: Some(e),
                    ..ApiHealth::default()
                };
                let update = self.builder.ingest(at, health, None, None, None, true);
                return CycleResult {
                    update,
                    fatal_error: None,
                };
            }
        };

        // ---- info ----
        let (info_version, rtt) = match client.info() {
            Ok((info, rtt)) => (Some(info.version), Some(rtt as f64)),
            Err(_) => (None, None),
        };

        let mut api = ApiHealth {
            rtt_ms: rtt,
            ..ApiHealth::default()
        };

        // ---- paths ----
        let paths = match client.list_paths() {
            Ok(list) => {
                api.ok = true;
                api.error = None;
                api.consecutive_failures = 0;
                Some(
                    list.iter()
                        .map(|p| ObservedPath {
                            name: p.name.clone(),
                            conf_name: p.conf_name.clone(),
                            available: p.available,
                            online: p.online,
                            source_type: p.source_type().to_string(),
                            reader_count: p.reader_count(),
                            codecs: p.tracks2.iter().map(|t| t.codec.clone()).collect(),
                            inbound_bytes: p.inbound_bytes,
                            outbound_bytes: p.outbound_bytes,
                            inbound_frames_in_error: p.inbound_frames_in_error,
                            online_since: p.online_time,
                            available_since: p.available_time,
                        })
                        .collect(),
                )
            }
            Err(e) => {
                api.ok = false;
                api.consecutive_failures = self.builder.api().consecutive_failures + 1;
                api.error = Some(e);
                None
            }
        };

        // ---- srt ----
        let srt_api = client.list_srt_conns().ok();
        let mut srt = None;
        if api.ok {
            let api_conns = srt_api.unwrap_or_default();
            let metrics = if self.metrics_enabled {
                match client.metrics() {
                    Ok(m) => {
                        api.metrics_ok = !m.is_empty();
                        api.metrics_error = None;
                        // 空の応答は一時的に起こり得る（起動直後・設定変更直後）ので、
                        // ここで恒久的に無効化しない。恒久エラー（404 など）のみ止める。
                        Some(m)
                    }
                    Err(e) => {
                        api.metrics_ok = false;
                        // 404 など恒久エラーなら以降は叩かない。
                        if !e.kind.is_retryable() {
                            self.metrics_enabled = false;
                        }
                        api.metrics_error = Some(e);
                        None
                    }
                }
            } else {
                None
            };
            let series = metrics.as_ref().map(|m| m.srt_conn_series());
            let mut list: Vec<ObservedSrt> = Vec::new();
            let mut seen_ids = std::collections::HashSet::new();

            for conn in &api_conns {
                seen_ids.insert(conn.id.clone());
                let m = series
                    .as_ref()
                    .and_then(|s| s.get(&conn.id))
                    .cloned()
                    .unwrap_or_default();
                let counters = merge_counters(&conn_to_counters(conn), &m);
                list.push(ObservedSrt {
                    id: conn.id.clone(),
                    path: conn.path.clone(),
                    remote_addr: conn.remote_addr.clone(),
                    state: conn.state.clone(),
                    created: conn.created,
                    counters,
                    derived: crate::srt::SrtDerived::default(),
                    severity: crate::srt::SrtSeverity::Ok,
                    from_metrics: series.as_ref().is_some_and(|s| s.contains_key(&conn.id)),
                });
            }
            // API 一覧に無いがメトリクスにだけ現れる接続も拾う（状態取得の取りこぼし対策）。
            if let Some(series) = &series {
                for (id, m) in series {
                    if seen_ids.contains(id) || id.is_empty() {
                        continue;
                    }
                    let counters = SrtCounters::from_metric_map(m);
                    list.push(ObservedSrt {
                        id: id.clone(),
                        // メトリクスのラベルには path も付くが、ここでは取得しない。
                        path: String::new(),
                        remote_addr: String::new(),
                        state: "unknown".into(),
                        created: None,
                        counters,
                        derived: crate::srt::SrtDerived::default(),
                        severity: crate::srt::SrtSeverity::Ok,
                        from_metrics: true,
                    });
                }
            }
            srt = Some(list);
        }

        let update = self
            .builder
            .ingest(at, api, info_version, paths, srt, false);
        CycleResult {
            update,
            fatal_error: None,
        }
    }

    /// 接続確認（監視とは独立）。バージョン・起動時刻・API 応答時間を返す。
    pub fn test_connection(
        transport: &'t dyn HttpTransport,
        config: &AppConfig,
    ) -> Result<ConnectionProbe, ApiError> {
        let client = ApiClient::new(transport, &config.endpoint, config.request_timeout_ms)?;
        let started = std::time::Instant::now();
        let (info, rtt) = client.info()?;
        // メトリクスは「補助情報」なので、失敗しても接続テスト自体は成功扱いにする。
        let metrics = match client.metrics() {
            Ok(m) => Ok(m.sample_count()),
            Err(e) => Err(e.message),
        };
        Ok(ConnectionProbe {
            version: info.version,
            started: info.started,
            api_rtt_ms: rtt as f64,
            elapsed_ms: started.elapsed().as_millis() as u64,
            metrics_samples: metrics.as_ref().ok().copied(),
            metrics_error: metrics.err(),
        })
    }

    /// 現在時刻を取得（テストで差し替えたければ `tick` の引数を使う）。
    pub fn now() -> DateTime<FixedOffset> {
        Utc::now().fixed_offset()
    }
}

/// 接続テストの結果（GUI の「接続確認」ボタン用）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProbe {
    pub version: String,
    pub started: Option<DateTime<FixedOffset>>,
    pub api_rtt_ms: f64,
    pub elapsed_ms: u64,
    /// メトリクスが取れた場合のサンプル数。
    pub metrics_samples: Option<usize>,
    pub metrics_error: Option<String>,
}

fn conn_to_counters(conn: &crate::model::SrtConn) -> SrtCounters {
    SrtCounters {
        packets_received: conn.packets_received,
        packets_received_unique: conn.packets_received_unique,
        packets_received_drop: conn.packets_received_drop,
        packets_received_retrans: conn.packets_received_retrans,
        packets_received_loss: conn.packets_received_loss,
        packets_received_belated: conn.packets_received_belated,
        packets_received_undecrypt: conn.packets_received_undecrypt,
        packets_sent: conn.packets_sent,
        packets_sent_unique: conn.packets_sent_unique,
        packets_retrans: conn.packets_retrans,
        packets_send_drop: conn.packets_send_drop,
        packets_send_loss: conn.packets_send_loss,
        packets_sent_nak: conn.packets_sent_nak,
        packets_received_nak: conn.packets_received_nak,
        ms_rtt: conn.ms_rtt,
        mbps_receive_rate: conn.mbps_receive_rate,
        mbps_send_rate: conn.mbps_send_rate,
        mbps_link_capacity: conn.mbps_link_capacity,
        us_packets_send_period: conn.us_packets_send_period,
        ms_receive_buf: conn.ms_receive_buf,
        ms_send_buf: conn.ms_send_buf,
        packets_flight_size: conn.packets_flight_size,
        packets_flow_window: conn.packets_flow_window,
        outbound_frames_discarded: conn.outbound_frames_discarded,
    }
}

/// API とメトリクスの値をマージする。API を正とし、欠けている項目だけ
/// メトリクスで補う。
fn merge_counters(api: &SrtCounters, metrics: &HashMap<String, f64>) -> SrtCounters {
    let m = SrtCounters::from_metric_map(metrics);
    if metrics.is_empty() {
        return api.clone();
    }
    let pick_u = |a: Option<u64>, b: Option<u64>| a.or(b);
    let pick_f = |a: f64, b: f64| if a != 0.0 { a } else { b };
    SrtCounters {
        packets_received: pick_u(api.packets_received, m.packets_received),
        packets_received_unique: pick_u(api.packets_received_unique, m.packets_received_unique),
        packets_received_drop: pick_u(api.packets_received_drop, m.packets_received_drop),
        packets_received_retrans: pick_u(api.packets_received_retrans, m.packets_received_retrans),
        packets_received_loss: pick_u(api.packets_received_loss, m.packets_received_loss),
        packets_received_belated: pick_u(api.packets_received_belated, m.packets_received_belated),
        packets_received_undecrypt: pick_u(
            api.packets_received_undecrypt,
            m.packets_received_undecrypt,
        ),
        packets_sent: pick_u(api.packets_sent, m.packets_sent),
        packets_sent_unique: pick_u(api.packets_sent_unique, m.packets_sent_unique),
        packets_retrans: pick_u(api.packets_retrans, m.packets_retrans),
        packets_send_drop: pick_u(api.packets_send_drop, m.packets_send_drop),
        packets_send_loss: pick_u(api.packets_send_loss, m.packets_send_loss),
        packets_sent_nak: pick_u(api.packets_sent_nak, m.packets_sent_nak),
        packets_received_nak: pick_u(api.packets_received_nak, m.packets_received_nak),
        ms_rtt: pick_f(api.ms_rtt, m.ms_rtt),
        mbps_receive_rate: pick_f(api.mbps_receive_rate, m.mbps_receive_rate),
        mbps_send_rate: pick_f(api.mbps_send_rate, m.mbps_send_rate),
        mbps_link_capacity: pick_f(api.mbps_link_capacity, m.mbps_link_capacity),
        us_packets_send_period: pick_f(api.us_packets_send_period, m.us_packets_send_period),
        ms_receive_buf: pick_u(api.ms_receive_buf, m.ms_receive_buf),
        ms_send_buf: pick_u(api.ms_send_buf, m.ms_send_buf),
        packets_flight_size: pick_u(api.packets_flight_size, m.packets_flight_size),
        packets_flow_window: pick_u(api.packets_flow_window, m.packets_flow_window),
        outbound_frames_discarded: pick_u(
            api.outbound_frames_discarded,
            m.outbound_frames_discarded,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_prefers_api_and_fills_gaps() {
        let api = SrtCounters {
            packets_received_unique: Some(100),
            ms_rtt: 0.0,
            ..Default::default()
        };
        let mut m = HashMap::new();
        m.insert("packets_received_unique".to_string(), 999.0);
        m.insert("ms_rtt".to_string(), 12.0);
        let merged = merge_counters(&api, &m);
        assert_eq!(merged.packets_received_unique, Some(100));
        assert_eq!(merged.ms_rtt, 12.0);
    }
}
