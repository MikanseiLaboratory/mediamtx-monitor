/// アプリの状態ストア。
///
/// **原則: サーバー由来のデータはここに「受信して置く」だけ**。
/// 監視状態の判定・差分計算はバックエンド（`mtm-core`）が行い、
/// フロントは受け取った最新値と描画用の系列バッファを保持する。
/// UI 固有の状態（どのタブ・どの時間窓・選択中の項目）だけがフロント発。

import { create } from "zustand";
import type {
  AppConfig,
  ApiHealth,
  ConnInfo,
  MetricPoint,
  MonitorStatus,
  MonitorUpdate,
  PathSample,
  ServerTotals,
  SrtConnSample,
  StatusEvent,
} from "../types";
import { EVENTS, api, getBridge } from "../bridge";

export type Tab =
  | "srt"
  | "dashboard"
  | "paths"
  | "connections"
  | "operations"
  | "settings";

export interface Toast {
  id: number;
  kind: "info" | "success" | "error";
  message: string;
}

export interface SeriesState {
  /** メトリクス名 → 点列（1 スコープで複数メトリクスを持つ）。 */
  byMetric: Record<string, MetricPoint[]>;
  loading: boolean;
  error: string | null;
}

/** グラフ用に 1 メトリクス分の点列を取り出す。 */
export function pointsOf(state: SeriesState | undefined, metric: string): MetricPoint[] {
  return state?.byMetric[metric] ?? [];
}

interface AppStore {
  // ---- バックエンド由来（受信して置くだけ） ----
  connected: boolean;
  bridgeKind: "tauri" | "mock" | "unknown";
  status: MonitorStatus | null;
  api: ApiHealth;
  totals: ServerTotals;
  paths: Record<string, PathSample>;
  srt: Record<string, SrtConnSample>;
  events: StatusEvent[];
  serverVersion: string | null;
  lastUpdateAt: string | null;
  seq: number;

  // ---- フロント固有（表示だけ） ----
  tab: Tab;
  windowMs: number;
  selectedSrt: string | null;
  selectedPath: string | null;
  series: Record<string, SeriesState>;
  connections: { kind: string; items: ConnInfo[]; loading: boolean; error: string | null; at: number };
  config: AppConfig | null;
  toasts: Toast[];

  // ---- アクション ----
  bootstrap: () => Promise<void>;
  applyUpdate: (update: MonitorUpdate) => void;
  applyStatus: (status: MonitorStatus) => void;
  setTab: (tab: Tab) => void;
  setWindowMs: (ms: number) => void;
  selectSrt: (id: string | null) => void;
  selectPath: (name: string | null) => void;
  loadSeries: (scope: string, metrics: string[], force?: boolean) => Promise<void>;
  /** 新しい観測 1 点を、取得済みの系列に追記する（毎秒の再取得を避ける）。 */
  appendPoints: (scope: string, atMs: number, values: Record<string, number | null>) => void;
  loadEvents: () => Promise<void>;
  loadConnections: (kind: string) => Promise<void>;
  loadConfig: () => Promise<void>;
  setConfig: (config: AppConfig) => void;
  pushToast: (kind: Toast["kind"], message: string) => void;
  dismissToast: (id: number) => void;
}

const emptyApi: ApiHealth = {
  ok: false,
  rttMs: null,
  error: null,
  consecutiveFailures: 0,
  metricsOk: false,
  metricsError: null,
};

const emptyTotals: ServerTotals = {
  pathCount: 0,
  pathsOnline: 0,
  pathsDown: 0,
  readerCount: 0,
  inboundMbps: 0,
  outboundMbps: 0,
  srtConnCount: 0,
  srtMaxDropRate: null,
  srtMinRecoveryRate: null,
  srtWorstSeverity: null,
};

let toastSeq = 1;
let unsubscribe: Array<() => void> = [];
/// スコープごとの取得世代。古い応答を捨てるために使う（状態には持たない）。
const loadGeneration = new Map<string, number>();

export const useAppStore = create<AppStore>((set, get) => ({
  connected: false,
  bridgeKind: "unknown",
  status: null,
  api: emptyApi,
  totals: emptyTotals,
  paths: {},
  srt: {},
  events: [],
  serverVersion: null,
  lastUpdateAt: null,
  seq: 0,

  tab: "srt",
  windowMs: 5 * 60_000,
  selectedSrt: null,
  selectedPath: null,
  series: {},
  connections: { kind: "srt", items: [], loading: false, error: null, at: 0 },
  config: null,
  toasts: [],

  async bootstrap() {
    if (get().connected) return;
    const bridge = await getBridge();
    set({ bridgeKind: bridge.kind });

    unsubscribe.forEach((u) => u());
    unsubscribe = [];
    unsubscribe.push(
      await bridge.listen<MonitorUpdate>(EVENTS.update, (update) => get().applyUpdate(update)),
    );
    unsubscribe.push(
      await bridge.listen<MonitorStatus>(EVENTS.status, (status) => get().applyStatus(status)),
    );

    try {
      const status = await api.getStatus();
      get().applyStatus(status);
      const config = await api.getSettings();
      set({ config });
      const events = await api.getEvents(200);
      set({ events });
      const snapshot = await api.getSnapshot();
      get().applyUpdate(snapshot);
      set({ connected: true });
    } catch (err) {
      set({
        connected: false,
        toasts: [
          ...get().toasts,
          { id: toastSeq++, kind: "error", message: `バックエンドに接続できません: ${String(err)}` },
        ],
      });
    }
  },

  applyUpdate(update) {
    const paths = { ...get().paths };
    const srt = { ...get().srt };
    for (const p of update.paths) paths[p.sample.name] = p.sample;
    for (const s of update.srt) srt[s.sample.id] = s.sample;
    for (const name of update.removedPaths) delete paths[name];
    for (const id of update.removedSrt) delete srt[id];
    const events = update.events.length ? [...get().events, ...update.events].slice(-500) : get().events;
    set({
      paths,
      srt,
      events,
      api: update.api,
      totals: update.totals,
      serverVersion: update.serverVersion,
      lastUpdateAt: update.at,
      seq: update.seq,
    });
  },

  applyStatus(status) {
    set({ status, api: status.api.ok || status.api.consecutiveFailures > 0 ? status.api : get().api });
  },

  setTab(tab) {
    set({ tab });
  },

  setWindowMs(ms) {
    set({ windowMs: ms });
  },

  selectSrt(id) {
    set({ selectedSrt: id });
  },

  selectPath(name) {
    set({ selectedPath: name });
  },

  /**
   * 1 スコープ分の系列を取得する。`force` で窓をいっぱい取り直す。
   * 以後の更新は `appendPoints` で末尾に足していく（毎秒の再取得を避ける）。
   *
   * **応答の順序**: 窓や接続を素早く切り替えると、古い要求の応答が新しい要求より
   * 後から届くことがある。そのまま書くと古い期間のデータが新しい窓に混ざるので、
   * スコープごとに世代番号を持ち、**最新の要求の応答だけ**を採用する。
   */
  async loadSeries(scope, metrics, force = false) {
    const to = Date.now();
    const from = to - get().windowMs;
    const cached = get().series[scope];
    const lastAt = cached?.byMetric[metrics[0]]?.at(-1)?.[0] ?? 0;
    if (!force && cached && !cached.loading && lastAt >= to - 10_000) return;

    // この要求の世代を確定させ、以降これより古い応答は捨てる。
    const generation = (loadGeneration.get(scope) ?? 0) + 1;
    loadGeneration.set(scope, generation);
    const isCurrent = () => loadGeneration.get(scope) === generation;

    set({
      series: {
        ...get().series,
        [scope]: { byMetric: cached?.byMetric ?? {}, loading: true, error: null },
      },
    });
    try {
      const resp = await api.getSeries({ scope, metrics, fromMs: from, toMs: to });
      if (!isCurrent()) return;
      const byMetric: Record<string, MetricPoint[]> = {};
      for (const [metric, points] of resp.series) byMetric[metric] = points;
      set({
        series: {
          ...get().series,
          [scope]: { byMetric, loading: false, error: null },
        },
      });
    } catch (err) {
      // 古い要求の失敗で、新しい要求の結果や読み込み中表示を壊さない。
      if (!isCurrent()) return;
      set({
        series: {
          ...get().series,
          [scope]: { byMetric: cached?.byMetric ?? {}, loading: false, error: String(err) },
        },
      });
    }
  },

  appendPoints(scope, atMs, values) {
    const cached = get().series[scope];
    if (!cached) return;
    // 取得中に追記すると、後から届く取得結果（追記より前の時点までのデータ）に
    // 上書きされ、追記分が消える。取得が終わるまでは追記を控える。
    if (cached.loading) return;
    const cutoff = atMs - get().windowMs - 60_000;
    const byMetric: Record<string, MetricPoint[]> = { ...cached.byMetric };
    for (const [metric, value] of Object.entries(values)) {
      const points = byMetric[metric];
      if (!points) continue;
      const next =
        points.length && points[points.length - 1][0] >= atMs
          ? [...points.slice(0, -1), [atMs, value] as MetricPoint]
          : [...points, [atMs, value] as MetricPoint];
      byMetric[metric] = next.filter((p) => p[0] >= cutoff);
    }
    set({
      series: {
        ...get().series,
        [scope]: { ...cached, byMetric },
      },
    });
  },

  async loadEvents() {
    try {
      const events = await api.getEvents(500);
      set({ events });
    } catch (err) {
      get().pushToast("error", `イベントの取得に失敗しました: ${String(err)}`);
    }
  },

  async loadConnections(kind) {
    set({ connections: { kind, items: [], loading: true, error: null, at: Date.now() } });
    try {
      const items = await api.listConnections(kind as never);
      set({ connections: { kind, items, loading: false, error: null, at: Date.now() } });
    } catch (err) {
      set({ connections: { kind, items: [], loading: false, error: String(err), at: Date.now() } });
    }
  },

  async loadConfig() {
    try {
      const config = await api.getSettings();
      set({ config });
    } catch (err) {
      get().pushToast("error", `設定の読み込みに失敗しました: ${String(err)}`);
    }
  },

  setConfig(config) {
    set({ config });
  },

  pushToast(kind, message) {
    const id = toastSeq++;
    set({ toasts: [...get().toasts, { id, kind, message }] });
    setTimeout(() => get().dismissToast(id), kind === "error" ? 8000 : 4000);
  },

  dismissToast(id) {
    set({ toasts: get().toasts.filter((t) => t.id !== id) });
  },
}));

/** テスト用のリセット。 */
export function resetStoreForTest(): void {
  unsubscribe.forEach((u) => u());
  unsubscribe = [];
  loadGeneration.clear();
  useAppStore.setState({
    connected: false,
    status: null,
    api: emptyApi,
    totals: emptyTotals,
    paths: {},
    srt: {},
    events: [],
    series: {},
    config: null,
    toasts: [],
    seq: 0,
  });
}
