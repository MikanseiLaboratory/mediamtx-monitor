/// Tauri バックエンドとのワイヤ型。
///
/// ここに定義された型は `mtm-core` の `serde` 定義と 1:1 で対応する。
/// **フロントはこの型を「受け取って描画する」だけ**で、派生値の計算は
/// 一切しない（バックエンドが唯一の正本）。

export type ApiErrorKind =
  | "transport"
  | "timeout"
  | "unauthorized"
  | "not_found"
  | "bad_request"
  | "server"
  | "http"
  | "decode"
  | "config"
  | "other";

export interface ApiError {
  kind: ApiErrorKind;
  message: string;
  status?: number | null;
  detail?: string | null;
}

export interface ApiHealth {
  ok: boolean;
  rttMs: number | null;
  error: ApiError | null;
  consecutiveFailures: number;
  metricsOk: boolean;
  metricsError: ApiError | null;
}

export type SrtSeverity = "ok" | "warning" | "critical";

export interface SrtCounters {
  packetsReceived: number | null;
  packetsReceivedUnique: number | null;
  packetsReceivedDrop: number | null;
  packetsReceivedRetrans: number | null;
  packetsReceivedLoss: number | null;
  packetsReceivedBelated: number | null;
  packetsReceivedUndecrypt: number | null;
  packetsSent: number | null;
  packetsSentUnique: number | null;
  packetsRetrans: number | null;
  packetsSendDrop: number | null;
  packetsSendLoss: number | null;
  packetsSentNak: number | null;
  packetsReceivedNak: number | null;
  msRtt: number;
  mbpsReceiveRate: number;
  mbpsSendRate: number;
  mbpsLinkCapacity: number;
  usPacketsSendPeriod: number;
  msReceiveBuf: number | null;
  msSendBuf: number | null;
  packetsFlightSize: number | null;
  packetsFlowWindow: number | null;
  outboundFramesDiscarded: number | null;
}

export interface SrtDerived {
  intervalMs: number;
  recvPackets: number | null;
  recoveredPackets: number | null;
  lossPackets: number | null;
  dropPackets: number | null;
  undecryptPackets: number | null;
  belatedPackets: number | null;
  dropRate: number | null;
  recoveryRate: number | null;
  unrecoveredRate: number | null;
  nakRate: number | null;
  recvMbps: number;
  linkCapacityMbps: number;
  headroomPercent: number | null;
  rttMs: number;
  estLatencyMs: number;
  counterReset: boolean;
}

export interface SrtConnSample {
  id: string;
  path: string;
  remoteAddr: string;
  state: string;
  created: string | null;
  counters: SrtCounters;
  derived: SrtDerived;
  severity: SrtSeverity;
  fromMetrics: boolean;
}

export type PathSeverity = "healthy" | "degraded" | "down";

export interface PathSample {
  name: string;
  confName: string;
  available: boolean;
  online: boolean;
  sourceType: string;
  readerCount: number;
  codecs: string[];
  inboundBytes: number;
  outboundBytes: number;
  inboundFramesInError: number;
  inboundMbps: number | null;
  outboundMbps: number | null;
  framesInErrorPerSec: number | null;
  onlineSince: string | null;
  availableSince: string | null;
  severity: PathSeverity;
  srtConnIds: string[];
}

export interface ServerTotals {
  pathCount: number;
  pathsOnline: number;
  pathsDown: number;
  readerCount: number;
  inboundMbps: number;
  outboundMbps: number;
  srtConnCount: number;
  srtMaxDropRate: number | null;
  srtMinRecoveryRate: number | null;
  srtWorstSeverity: SrtSeverity | null;
}

export type StatusEventKind =
  | "srt_conn_appeared"
  | "srt_conn_dropped"
  | "srt_degraded"
  | "srt_recovered"
  | "path_down"
  | "path_recovered"
  | "path_online_stopped"
  | "path_online_started"
  | "source_changed"
  | "api_unreachable"
  | "api_restored"
  | "monitor_started"
  | "monitor_stopped";

export type EventSeverity = "info" | "warning" | "critical";

export interface StatusEvent {
  seq: number;
  at: string;
  kind: StatusEventKind;
  target: string;
  label: string;
  message: string;
  severity: EventSeverity;
}

export interface PathPatch {
  seq: number;
  sample: PathSample;
}

export interface SrtConnPatch {
  seq: number;
  sample: SrtConnSample;
}

export interface MonitorUpdate {
  seq: number;
  at: string;
  api: ApiHealth;
  totals: ServerTotals;
  paths: PathPatch[];
  srt: SrtConnPatch[];
  removedPaths: string[];
  removedSrt: string[];
  events: StatusEvent[];
  serverVersion: string | null;
  full: boolean;
}

export interface MonitorStatus {
  running: boolean;
  intervalMs: number;
  startedAt: string | null;
  lastTickAt: string | null;
  seq: number;
  serverVersion: string | null;
  serverStarted: string | null;
  api: ApiHealth;
  fatalError: string | null;
  totalSamples: number;
}

// ---- 設定 ----

export type AuthMode = "none" | "basic" | "bearer";

export interface EndpointConfig {
  baseUrl: string;
  metricsUrl: string | null;
  auth: AuthMode;
  user: string;
  password: string;
  token: string;
  allowInsecureTls: boolean;
}

export interface SrtThresholds {
  dropRateWarn: number;
  dropRateCrit: number;
  recoveryRateWarn: number;
  rttMsWarn: number;
  cooldownSecs: number;
  /** 劣化／回復を確定するまでの保持時間（秒）。瞬時の変化をノイズにしない。 */
  holdSecs: number;
}

export interface NotifyRules {
  enabled: boolean;
  srtConnDropped: boolean;
  srtConnAppeared: boolean;
  srtDegraded: boolean;
  pathDown: boolean;
  apiUnreachable: boolean;
}

export type Theme = "dark" | "light" | "system";

export interface AppConfig {
  endpoint: EndpointConfig;
  pollIntervalMs: number;
  requestTimeoutMs: number;
  historyHours: number;
  failureThreshold: number;
  srt: SrtThresholds;
  notify: NotifyRules;
  minimizeToTray: boolean;
  autoStart: boolean;
  theme: Theme;
}

export interface ConnectionProbe {
  version: string;
  started: string | null;
  apiRttMs: number;
  elapsedMs: number;
  metricsSamples: number | null;
  metricsError: string | null;
}

export type ConnKind = "rtsp" | "rtsps" | "rtmp" | "rtmps" | "srt" | "web_rtc" | "hls" | "moq";

export interface ConnInfo {
  kind: ConnKind;
  id: string;
  path: string;
  remoteAddr: string;
  state: string;
  created: string | null;
  inboundBytes: number;
  outboundBytes: number;
  detail: Record<string, unknown>;
}

export type MetricPoint = [number, number | null];

export interface SeriesRequest {
  scope: string;
  metrics: string[];
  fromMs?: number;
  toMs?: number;
}

export interface SeriesResponse {
  scope: string;
  fromMs: number;
  toMs: number;
  series: Array<[string, MetricPoint[]]>;
}

export interface ExportRequest {
  scope: string;
  metrics: string[];
  fromMs: number;
  toMs: number;
  format: "csv" | "svg";
  fileName?: string | null;
  dir?: string | null;
  title?: string | null;
}

export interface ExportResult {
  path: string;
  bytes: number;
}
