/// モックブリッジ。
///
/// ブラウザで UI を確認するときと、Playwright の E2E（描画検証）で使う。
/// Tauri 側の `Monitor` と同じ形のデータを **同じイベント名**で流すので、
/// コンポーネントは本番と同じコードパスを通る。

import type {
  AppConfig,
  ConnInfo,
  ConnKind,
  ConnectionProbe,
  ExportRequest,
  ExportResult,
  MonitorStatus,
  MonitorUpdate,
  PathPatch,
  SeriesRequest,
  SeriesResponse,
  SrtConnPatch,
  StatusEvent,
} from "../types";
import { EVENTS, type Bridge } from "./index";

const DEFAULT_CONFIG: AppConfig = {
  endpoint: {
    baseUrl: "http://127.0.0.1:9997",
    metricsUrl: null,
    auth: "none",
    user: "",
    password: "",
    token: "",
    allowInsecureTls: false,
  },
  pollIntervalMs: 1000,
  requestTimeoutMs: 1500,
  historyHours: 4,
  failureThreshold: 3,
  srt: {
    dropRateWarn: 0.5,
    dropRateCrit: 2,
    recoveryRateWarn: 90,
    rttMsWarn: 200,
    cooldownSecs: 300,
  },
  notify: {
    enabled: true,
    srtConnDropped: true,
    srtConnAppeared: false,
    srtDegraded: true,
    pathDown: true,
    apiUnreachable: true,
  },
  minimizeToTray: true,
  autoStart: true,
  theme: "system",
};

/** 決定的な擬似乱数（E2E を安定させる）。 */
function lcg(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0xffffffff;
  };
}

interface MockConn {
  id: string;
  path: string;
  remoteAddr: string;
  state: string;
  counters: {
    receivedUnique: number;
    drop: number;
    retrans: number;
    loss: number;
    belated: number;
    undecrypt: number;
    sentNak: number;
  };
}

export function createMockBridge(): Bridge {
  let config: AppConfig = structuredClone(DEFAULT_CONFIG);
  let running = true;
  let seq = 0;
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const rand = lcg(42);
  const startedAt = new Date(Date.now() - 3600_000).toISOString();
  const history: Array<{ at: number; update: MonitorUpdate }> = [];

  const emit = (event: string, payload: unknown) => {
    for (const handler of listeners.get(event) ?? []) handler(payload);
  };

  const conns: MockConn[] = [
    {
      id: "mock-srt-cam1",
      path: "cam1",
      remoteAddr: "192.168.0.24:53512",
      state: "read",
      counters: { receivedUnique: 120_000, drop: 240, retrans: 1_100, loss: 1_260, belated: 3, undecrypt: 0, sentNak: 900 },
    },
    {
      id: "mock-srt-cam2",
      path: "cam2",
      remoteAddr: "192.168.0.31:51880",
      state: "publish",
      counters: { receivedUnique: 88_000, drop: 12, retrans: 40, loss: 52, belated: 0, undecrypt: 0, sentNak: 30 },
    },
  ];

  const baseRecvMbps = [12.4, 8.1];

  const nowStatus = (): MonitorStatus => ({
    running,
    intervalMs: config.pollIntervalMs,
    startedAt,
    lastTickAt: new Date().toISOString(),
    seq,
    serverVersion: "v1.21.2 (mock)",
    serverStarted: startedAt,
    api: { ok: true, rttMs: 2.5, error: null, consecutiveFailures: 0, metricsOk: true, metricsError: null },
    fatalError: null,
    totalSamples: history.length,
  });

  const tick = (): MonitorUpdate => {
    seq += 1;
    const at = Date.now();
    const srt: SrtConnPatch[] = conns.map((c, i) => {
      const jitter = (rand() - 0.5) * 0.004;
      const dropRate = Math.max(0, Math.min(5, 0.35 + jitter + (i === 0 ? Math.sin(at / 20000) * 0.4 : -0.3)));
      const recoveryRate = Math.max(80, Math.min(100, 95 + Math.sin(at / 15000 + i) * 4));
      const recvMbps = baseRecvMbps[i] + Math.sin(at / 7000 + i) * 0.6;
      return {
        seq,
        sample: {
          id: c.id,
          path: c.path,
          remoteAddr: c.remoteAddr,
          state: c.state,
          created: startedAt,
          counters: {
            packetsReceived: c.counters.receivedUnique,
            packetsReceivedUnique: c.counters.receivedUnique,
            packetsReceivedDrop: c.counters.drop,
            packetsReceivedRetrans: c.counters.retrans,
            packetsReceivedLoss: c.counters.loss,
            packetsReceivedBelated: c.counters.belated,
            packetsReceivedUndecrypt: c.counters.undecrypt,
            packetsSent: 0,
            packetsSentUnique: 0,
            packetsRetrans: 0,
            packetsSendDrop: 0,
            packetsSendLoss: 0,
            packetsSentNak: c.counters.sentNak,
            packetsReceivedNak: 0,
            msRtt: 8 + Math.sin(at / 11000 + i) * 3,
            mbpsReceiveRate: recvMbps,
            mbpsSendRate: 0,
            mbpsLinkCapacity: 40,
            usPacketsSendPeriod: 1000,
            msReceiveBuf: 120,
            msSendBuf: null,
            packetsFlightSize: 12,
            packetsFlowWindow: 8192,
            outboundFramesDiscarded: 0,
          },
          derived: {
            intervalMs: config.pollIntervalMs,
            recvPackets: 300,
            recoveredPackets: 2,
            lossPackets: 2,
            dropPackets: 1,
            undecryptPackets: 0,
            belatedPackets: 0,
            dropRate,
            recoveryRate,
            unrecoveredRate: 100 - recoveryRate,
            nakRate: dropRate * 1.2,
            recvMbps,
            linkCapacityMbps: 40,
            headroomPercent: ((40 - recvMbps) / 40) * 100,
            rttMs: 8 + Math.sin(at / 11000 + i) * 3,
            estLatencyMs: 12,
            counterReset: false,
          },
          severity: dropRate >= config.srt.dropRateCrit ? "critical" : dropRate >= config.srt.dropRateWarn ? "warning" : "ok",
          fromMetrics: true,
        },
      };
    });

    const paths: PathPatch[] = [
      ["cam1", "srtConn", 2, true, "mock-srt-cam1"],
      ["cam2", "srtConn", 1, true, "mock-srt-cam2"],
      ["relay", "rtspSource", 0, false, ""],
    ].map(([name, sourceType, readers, online, srtId]) => ({
      seq,
      sample: {
        name: name as string,
        confName: name as string,
        available: true,
        online: online as boolean,
        sourceType: sourceType as string,
        readerCount: readers as number,
        codecs: ["H264", "MPEG-4 Audio"],
        inboundBytes: 0,
        outboundBytes: 0,
        inboundFramesInError: 0,
        inboundMbps: (online ? 12 : null) as number | null,
        outboundMbps: (online ? 24 : null) as number | null,
        framesInErrorPerSec: 0,
        onlineSince: startedAt,
        availableSince: startedAt,
        severity: online ? "healthy" : "degraded",
        srtConnIds: srtId ? [srtId as string] : [],
      },
    }));

    const update: MonitorUpdate = {
      seq,
      at: new Date(at).toISOString(),
      api: { ok: true, rttMs: 2.4 + rand() * 0.6, error: null, consecutiveFailures: 0, metricsOk: true, metricsError: null },
      totals: {
        pathCount: 3,
        pathsOnline: 2,
        pathsDown: 1,
        readerCount: 3,
        inboundMbps: 20.5,
        outboundMbps: 41,
        srtConnCount: 2,
        srtMaxDropRate: srt[0].sample.derived.dropRate,
        srtMinRecoveryRate: Math.min(...srt.map((s) => s.sample.derived.recoveryRate ?? 100)),
        srtWorstSeverity: "warning",
      },
      paths,
      srt,
      removedPaths: [],
      removedSrt: [],
      events: [],
      serverVersion: "v1.21.2 (mock)",
      full: seq === 1,
    };

    history.push({ at, update });
    if (history.length > 7200) history.shift();
    emit(EVENTS.update, update);
    emit(EVENTS.status, nowStatus());

    // 累積値を進める（表示が「増えていく」ことを確認できる）。
    for (const c of conns) {
      c.counters.receivedUnique += 300;
      c.counters.drop += 1;
      c.counters.loss += 1;
      c.counters.retrans += 1;
    }
    return update;
  };

  const timer = setInterval(() => {
    if (running) tick();
  }, 1000);

  // SSR/テストでタイマーが残らないように。
  if (typeof timer === "object" && timer !== null && "unref" in timer) {
    (timer as unknown as { unref: () => void }).unref();
  }

  const seriesFor = (request: SeriesRequest): SeriesResponse => {
    const from = request.fromMs ?? Date.now() - 3600_000;
    const to = request.toMs ?? Date.now();
    const rows = history.filter((h) => h.at >= from && h.at <= to);
    const isSrt = request.scope.startsWith("srt:");
    const srtPatch = (u: MonitorUpdate) =>
      isSrt ? u.srt.find((s) => `srt:${s.sample.id}` === request.scope) : undefined;
    const series = request.metrics.map((metric) => {
      const points: Array<[number, number | null]> = rows.map((h) => {
        const s = srtPatch(h.update);
        let value: number | null = null;
        if (s) {
          const d = s.sample.derived;
          value =
            metric === "srt_drop_rate"
              ? d.dropRate
              : metric === "srt_recovery_rate"
                ? d.recoveryRate
                : metric === "srt_recv_mbps"
                  ? d.recvMbps
                  : metric === "srt_rtt_ms"
                    ? d.rttMs
                    : null;
        } else {
          value = h.update.totals.srtMaxDropRate;
        }
        return [h.at, value];
      });
      return [metric, points] as [string, Array<[number, number | null]>];
    });
    return { scope: request.scope, fromMs: from, toMs: to, series };
  };

  // 起動直後に 1 回分を流してグラフをすぐ描画できる状態にする。
  queueMicrotask(() => {
    for (let i = 0; i < 60; i += 1) {
      seq += 1;
      tick();
    }
    history.splice(0, history.length - 300);
  });

  return {
    kind: "mock",
    async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (command) {
        case "get_settings":
          return structuredClone(config) as T;
        case "save_settings":
          config = structuredClone((a as unknown as { config: AppConfig }).config);
          return structuredClone(config) as T;
        case "test_connection":
          return {
            version: "v1.21.2 (mock)",
            started: startedAt,
            apiRttMs: 2.4,
            elapsedMs: 24,
            metricsSamples: 214,
            metricsError: null,
          } satisfies ConnectionProbe as T;
        case "start_monitor":
          running = true;
          emit(EVENTS.status, nowStatus());
          return nowStatus() as T;
        case "stop_monitor":
          running = false;
          emit(EVENTS.status, nowStatus());
          return nowStatus() as T;
        case "get_monitor_status":
          return nowStatus() as T;
        case "get_snapshot":
          return structuredClone(history.at(-1)!.update) as T;
        case "get_series":
          return seriesFor((a as unknown as { request: SeriesRequest }).request) as T;
        case "get_events": {
          const events: StatusEvent[] = [
            {
              seq: seq - 5,
              at: new Date().toISOString(),
              kind: "srt_conn_appeared",
              target: "mock-srt-cam2",
              label: "cam2",
              message: "SRT 接続が確立しました（path=`cam2`）。",
              severity: "info",
            },
          ];
          return events as T;
        }
        case "list_paths":
          return [{ name: "cam1", confName: "cam1", source: "" }] as unknown as T;
        case "get_path_conf":
          return { name: "cam1", source: "" } as unknown as T;
        case "add_path":
        case "patch_path":
        case "delete_path":
        case "kick_connection":
        case "patch_global_config":
          return undefined as T;
        case "get_global_config":
          return {
            api: true,
            apiAddress: ":9997",
            metrics: true,
            metricsAddress: ":9998",
            logLevel: "info",
            logDestinations: ["stdout"],
            rtsp: false,
            rtspAddress: "",
            rtmp: false,
            rtmpAddress: "",
            srt: true,
            srtAddress: ":8890",
            hls: false,
            hlsAddress: "",
            webrtc: false,
            webrtcAddress: "",
          } as unknown as T;
        case "list_connections": {
          const kind = (a as unknown as { kind: ConnKind }).kind;
          const items: ConnInfo[] = conns
            .filter(() => kind === "srt")
            .map((c) => ({
              kind,
              id: c.id,
              path: c.path,
              remoteAddr: c.remoteAddr,
              state: c.state,
              created: startedAt,
              inboundBytes: c.counters.receivedUnique * 1316,
              outboundBytes: 0,
              detail: { msRTT: 8.2 },
            }));
          return items as T;
        }
        case "console_get":
          return [200, { status: "ok" }] as unknown as T;
        case "export_range": {
          const req = (a as unknown as { request: ExportRequest }).request;
          const result: ExportResult = {
            path: `C:\\Users\\Public\\Documents\\mtm-${req.format}-mock.${req.format}`,
            bytes: 2048,
          };
          return result as T;
        }
        case "save_png":
          return { path: "C:\\Users\\Public\\Documents\\mtm-graph-mock.png", bytes: 4096 } as T;
        default:
          throw new Error(`モック未実装のコマンド: ${command}`);
      }
    },
    async listen<T>(event: string, handler: (payload: T) => void) {
      const set = listeners.get(event) ?? new Set();
      set.add(handler as (payload: unknown) => void);
      listeners.set(event, set);
      return () => {
        set.delete(handler as (payload: unknown) => void);
      };
    },
  };
}
