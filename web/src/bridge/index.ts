/// Tauri バックエンドとのブリッジ。
///
/// 通信は **Tauri 公式の方法だけ** を使う。
/// - コマンド呼び出し: `@tauri-apps/api/core` の `invoke`
/// - リアルタイム更新: `EventTarget` ではなく Tauri の `listen`（イベント購読）
///
/// ブラウザ（`vite dev` / Playwright）には Tauri の IPC が無いので、そのときだけ
/// **モックブリッジ**（同じシグネチャの擬似実装）に切り替える。本番の経路は常に
/// Tauri の IPC で、フロントはここ以外でバックエンドに触らない。

import type {
  AppConfig,
  ConnInfo,
  ConnKind,
  ConnectionProbe,
  ExportRequest,
  ExportResult,
  MonitorStatus,
  MonitorUpdate,
  SeriesRequest,
  SeriesResponse,
  StatusEvent,
} from "../types";

/** バックエンドに送るコマンド名（`mediamtx-monitor/src/commands.rs` と一致）。 */
export type Command =
  | "get_settings"
  | "save_settings"
  | "test_connection"
  | "start_monitor"
  | "stop_monitor"
  | "get_monitor_status"
  | "get_snapshot"
  | "get_series"
  | "get_events"
  | "list_paths"
  | "get_path_conf"
  | "add_path"
  | "patch_path"
  | "delete_path"
  | "get_global_config"
  | "patch_global_config"
  | "list_connections"
  | "kick_connection"
  | "console_get"
  | "export_range";

export interface Bridge {
  readonly kind: "tauri" | "mock";
  invoke<T>(command: Command, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

/** イベント名。バックエンドの `events` モジュールと一致。 */
export const EVENTS = {
  /** 1 周期分の差分更新。 */
  update: "monitor://update",
  /** 監視の開始・停止・接続状態。 */
  status: "monitor://status",
} as const;

function tauriAvailable(): boolean {
  if (typeof window === "undefined") return false;
  const w = window as unknown as { __TAURI_INTERNALS__?: unknown; __TAURI__?: unknown };
  return Boolean(w.__TAURI_INTERNALS__ ?? w.__TAURI__);
}

function mockRequested(): boolean {
  if (typeof window === "undefined") return false;
  const params = new URLSearchParams(window.location.search);
  return params.get("mock") === "1" || params.get("mock") === "true";
}

const tauriBridge: Bridge = {
  kind: "tauri",
  async invoke<T>(command: Command, args?: Record<string, unknown>): Promise<T> {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(command, args ?? {});
  },
  async listen<T>(event: string, handler: (payload: T) => void) {
    const { listen } = await import("@tauri-apps/api/event");
    // Tauri のイベント購読。戻り値の `unlisten` で解除する。
    return listen<T>(event, (e) => handler(e.payload));
  },
};

let bridge: Bridge | undefined;

/** ブリッジ取得。Tauri 以外（ブラウザ・テスト）ではモックに切り替える。 */
export async function getBridge(): Promise<Bridge> {
  if (bridge) return bridge;
  if (!mockRequested() && tauriAvailable()) {
    bridge = tauriBridge;
  } else {
    const { createMockBridge } = await import("./mock");
    bridge = createMockBridge();
  }
  return bridge;
}

/** テスト用にブリッジを差し替える。 */
export function __setBridgeForTest(next: Bridge | undefined): void {
  bridge = next;
}

// ---- 便利ラッパ ----

export const api = {
  async getSettings(): Promise<AppConfig> {
    return (await getBridge()).invoke<AppConfig>("get_settings");
  },
  async saveSettings(config: AppConfig): Promise<AppConfig> {
    return (await getBridge()).invoke<AppConfig>("save_settings", { config });
  },
  async testConnection(config: AppConfig): Promise<ConnectionProbe> {
    return (await getBridge()).invoke<ConnectionProbe>("test_connection", { config });
  },
  async startMonitor(): Promise<MonitorStatus> {
    return (await getBridge()).invoke<MonitorStatus>("start_monitor");
  },
  async stopMonitor(): Promise<MonitorStatus> {
    return (await getBridge()).invoke<MonitorStatus>("stop_monitor");
  },
  async getStatus(): Promise<MonitorStatus> {
    return (await getBridge()).invoke<MonitorStatus>("get_monitor_status");
  },
  async getSnapshot(): Promise<MonitorUpdate> {
    return (await getBridge()).invoke<MonitorUpdate>("get_snapshot");
  },
  async getSeries(request: SeriesRequest): Promise<SeriesResponse> {
    return (await getBridge()).invoke<SeriesResponse>("get_series", { request });
  },
  async getEvents(limit = 200): Promise<StatusEvent[]> {
    return (await getBridge()).invoke<StatusEvent[]>("get_events", { limit });
  },
  async listPaths(source: "runtime" | "config" = "runtime"): Promise<Record<string, unknown>[]> {
    return (await getBridge()).invoke("list_paths", { source });
  },
  async getPathConf(name: string): Promise<Record<string, unknown>> {
    return (await getBridge()).invoke("get_path_conf", { name });
  },
  async addPath(name: string, conf: Record<string, unknown>): Promise<void> {
    return (await getBridge()).invoke("add_path", { name, conf });
  },
  async patchPath(name: string, diff: Record<string, unknown>): Promise<void> {
    return (await getBridge()).invoke("patch_path", { name, diff });
  },
  async deletePath(name: string): Promise<void> {
    return (await getBridge()).invoke("delete_path", { name });
  },
  async getGlobalConfig(): Promise<Record<string, unknown>> {
    return (await getBridge()).invoke("get_global_config");
  },
  async patchGlobalConfig(patch: Record<string, unknown>): Promise<void> {
    return (await getBridge()).invoke("patch_global_config", { patch });
  },
  async listConnections(kind: ConnKind): Promise<ConnInfo[]> {
    return (await getBridge()).invoke("list_connections", { kind });
  },
  async kickConnection(kind: ConnKind, id: string): Promise<void> {
    return (await getBridge()).invoke("kick_connection", { kind, id });
  },
  async consoleGet(path: string): Promise<[number, unknown]> {
    return (await getBridge()).invoke("console_get", { path });
  },
  async exportRange(request: ExportRequest): Promise<ExportResult> {
    return (await getBridge()).invoke("export_range", { request });
  },
};
