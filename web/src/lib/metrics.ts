/// 系列（メトリクス）の定義。**どのグラフを何色で描くかだけ**を決める。
/// 値の計算（ドロップ率・回復率）はバックエンドが行う。

import type { SrtSeverity } from "../types";

export interface MetricDef {
  key: string;
  label: string;
  unit: string;
  /** 表示しない場合の色 */
  color: string;
  /** 0–100% に固定して描く（ドロップ率など）。 */
  percent?: boolean;
  /** y 軸の最小値 */
  min?: number;
}

/** バックエンドの `frame::metric_key` と一致させる。 */
export const SRT_METRICS: Record<string, MetricDef> = {
  srt_drop_rate: {
    key: "srt_drop_rate",
    label: "ドロップ率",
    unit: "%",
    color: "#f25f5c",
    percent: true,
    min: 0,
  },
  srt_recovery_rate: {
    key: "srt_recovery_rate",
    label: "回復率",
    unit: "%",
    color: "#4cc9f0",
    percent: true,
    min: 0,
  },
  srt_recv_mbps: {
    key: "srt_recv_mbps",
    label: "受信レート",
    unit: "Mbps",
    color: "#90be6d",
    min: 0,
  },
  srt_link_capacity: {
    key: "srt_link_capacity",
    label: "回線容量",
    unit: "Mbps",
    color: "#f9c74f",
    min: 0,
  },
  srt_rtt_ms: {
    key: "srt_rtt_ms",
    label: "RTT",
    unit: "ms",
    color: "#b5179e",
    min: 0,
  },
  srt_nak_rate: {
    key: "srt_nak_rate",
    label: "NAK 率",
    unit: "%",
    color: "#f8961e",
    percent: true,
    min: 0,
  },
  srt_unrecovered_packets: {
    key: "srt_unrecovered_packets",
    label: "未回復パケット",
    unit: "個",
    color: "#f72585",
    min: 0,
  },
  srt_send_mbps: {
    key: "srt_send_mbps",
    label: "送信レート",
    unit: "Mbps",
    color: "#577590",
    min: 0,
  },
};

export const SERVER_METRICS: Record<string, MetricDef> = {
  api_rtt_ms: {
    key: "api_rtt_ms",
    label: "API 応答時間",
    unit: "ms",
    color: "#4cc9f0",
    min: 0,
  },
  paths_online: {
    key: "paths_online",
    label: "稼働パス数",
    unit: "個",
    color: "#90be6d",
    min: 0,
  },
  paths_total: {
    key: "paths_total",
    label: "パス数",
    unit: "個",
    color: "#577590",
    min: 0,
  },
  readers: {
    key: "readers",
    label: "視聴者数",
    unit: "人",
    color: "#f9c74f",
    min: 0,
  },
  inbound_mbps: {
    key: "inbound_mbps",
    label: "受信ビットレート",
    unit: "Mbps",
    color: "#43aa8b",
    min: 0,
  },
  outbound_mbps: {
    key: "outbound_mbps",
    label: "送信ビットレート",
    unit: "Mbps",
    color: "#f8961e",
    min: 0,
  },
  srt_conns: {
    key: "srt_conns",
    label: "SRT 接続数",
    unit: "本",
    color: "#b5179e",
    min: 0,
  },
  srt_drop_rate: {
    key: "srt_drop_rate",
    label: "最大ドロップ率",
    unit: "%",
    color: "#f25f5c",
    percent: true,
    min: 0,
  },
};

/** uPlot の系列色（SVG エクスポートと同じ 8 色）。 */
export const PALETTE = [
  "#4cc9f0",
  "#f72585",
  "#f9c74f",
  "#90be6d",
  "#b5179e",
  "#f8961e",
  "#577590",
  "#43aa8b",
] as const;

/** 重大度の日本語ラベルと CSS クラス。 */
export const SEVERITY: Record<SrtSeverity, { label: string; className: string }> = {
  ok: { label: "良好", className: "sev-ok" },
  warning: { label: "注意", className: "sev-warning" },
  critical: { label: "劣化", className: "sev-critical" },
};

/** グラフの時間窓（ミリ秒）。 */
export const WINDOWS: Array<{ label: string; ms: number }> = [
  { label: "1分", ms: 60_000 },
  { label: "5分", ms: 5 * 60_000 },
  { label: "15分", ms: 15 * 60_000 },
  { label: "1時間", ms: 60 * 60_000 },
  { label: "4時間", ms: 4 * 60 * 60_000 },
];

/** 選択されたメトリクスを定義に解決する。未知のキーは最小構成で描く。 */
export function resolveMetric(key: string): MetricDef {
  return (
    SRT_METRICS[key] ??
    SERVER_METRICS[key] ?? { key, label: key, unit: "", color: "#8b98a9", min: 0 }
  );
}
