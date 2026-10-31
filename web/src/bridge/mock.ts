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
    holdSecs: 3,
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
  // `?holdSecs=N` で警告の保持秒数を差し替えられる（E2E で挙動を確かめるため）。
  if (typeof window !== "undefined") {
    const param = new URLSearchParams(window.location.search).get("holdSecs");
    if (param !== null && param !== "") {
      const n = Number(param);
      if (Number.isFinite(n)) config.srt.holdSecs = Math.max(0, Math.min(60, n));
    }
  }
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

  /// 接続ごとの「劣化が始まった時刻」と「記録済みか」（保持時間の判定用）。
  const mockQuality = new Map<string, { since: number; announced: boolean }>();

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

  /// 1 周期分の更新を作る。`atMs` を渡すと、その時刻の観測として記録する
  /// （起動直後に過去の履歴を埋めて、グラフが 1 点の縦線にならないようにする）。
  const tick = (atMs?: number): MonitorUpdate => {
    seq += 1;
    const at = atMs ?? Date.now();
    const srt: SrtConnPatch[] = conns.map((c, i) => {
      const jitter = (rand() - 0.5) * 0.004;
      // 約 45 秒周期で「数秒だけ劣化する」区間を作る。保持時間の挙動を
      // 画面で確認できるよう、短い劣化が確実に起きるようにしている。
      const burst = i === 0 && Math.sin(at / 7000) > 0.7 ? 1.2 : 0;
      const dropRate = Math.max(
        0,
        Math.min(5, 0.35 + jitter + (i === 0 ? Math.sin(at / 20000) * 0.4 : -0.3) + burst),
      );
      const recoveryRate = Math.max(80, Math.min(100, 95 + Math.sin(at / 15000 + i) * 4));
      const recvMbps = baseRecvMbps[i] + Math.sin(at / 7000 + i) * 0.6;
      // `read` は MediaMTX が視聴者へ送る側なので、送信系のカウンタが有効。
      // `publish` はその逆で、受信系が有効。実機の MediaMTX と同じ形にする。
      const sending = c.state === "read";
      const sentTotal = c.counters.receivedUnique;
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
            // 送信側（read 接続）の累積値。受信側と同じ規模で進む。
            packetsSent: sending ? sentTotal : 0,
            packetsSentUnique: sending ? sentTotal : 0,
            packetsRetrans: sending ? c.counters.retrans : 0,
            packetsSendDrop: sending ? c.counters.drop : 0,
            packetsSendLoss: sending ? c.counters.loss : 0,
            packetsSentNak: sending ? 0 : c.counters.sentNak,
            packetsReceivedNak: sending ? c.counters.sentNak : 0,
            msRtt: 8 + Math.sin(at / 11000 + i) * 3,
            // 送信側は `mbpsSendRate` を見る（`mbpsReceiveRate` は制御パケット分のみ）。
            mbpsReceiveRate: sending ? 0.02 : recvMbps,
            mbpsSendRate: sending ? recvMbps : 0,
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

    // 劣化イベントも流す（画面のタイムラインを確認できるように）。
    // 状態機械と同じ「保持時間」の考え方で、一定時間続いた劣化だけを記録する。
    //
    // ただし起動時の履歴の埋め戻し（過去へ遡って `tick` する）では判定しない。
    // ここで判定すると「1 時間前から劣化が続いていた」ことになり、いきなり
    // 大量のイベントが流れ込んでしまう。ライブの更新だけで判定する。
    const events: StatusEvent[] = [];
    const nowMs = at;
    const backfilling = atMs !== undefined && atMs < Date.now() - 5000;
    if (!backfilling) {
      for (const s of srt) {
        const c = s.sample;
        const degraded = c.severity !== "ok";
        const prev = mockQuality.get(c.id);
        if (!degraded) {
          // 劣化を記録した後に戻ったときだけ「回復」を記録する。
          // 保持時間に満たない一瞬の劣化は、劣化も回復も記録しない（ノイズ防止）。
          if (prev?.announced) {
            mockQuality.set(c.id, { since: 0, announced: false });
            events.push({
              seq,
              at: new Date(nowMs).toISOString(),
              kind: "srt_recovered",
              target: c.id,
              label: c.path,
              message: `SRT 品質が回復しました（path=\`${c.path}\`）。`,
              severity: "info",
            });
          } else if (prev) {
            mockQuality.delete(c.id);
          }
          continue;
        }
        const holdMs = Math.max(0, config.srt.holdSecs) * 1000;
        const since = prev && prev.since > 0 ? prev.since : nowMs;
        if (prev?.announced !== true && nowMs - since >= holdMs) {
          mockQuality.set(c.id, { since, announced: true });
          events.push({
            seq,
            at: new Date(since).toISOString(), // 発生時刻に遡って記録する
            kind: "srt_degraded",
            target: c.id,
            label: c.path,
            message: `SRT 品質が${c.severity === "critical" ? "劣化" : "注意"}（path=\`${c.path}\`）: ドロップ率 ${(c.derived.dropRate ?? 0).toFixed(2)}% / 回復率 ${(c.derived.recoveryRate ?? 100).toFixed(1)}%`,
            severity: c.severity === "critical" ? "critical" : "warning",
          });
        } else if (!prev) {
          mockQuality.set(c.id, { since, announced: false });
        }
      }
    }

    const update: MonitorUpdate = {
      seq,
      at: new Date(at).toISOString(),
      api: { ok: true, rttMs: 2.4 + rand() * 0.6, error: null, consecutiveFailures: 0, metricsOk: true, metricsError: null },
      totals: {
        pathCount: 3,
        pathsOnline: 2,
        pathsDown: 1,
        // 視聴者・ビットレートは実際に揺れる（グラフが直線にならないように）。
        readerCount: 3 + Math.round(Math.sin(at / 21000) * 1.4 + 1.4),
        inboundMbps: srt.reduce((sum, s) => sum + s.sample.derived.recvMbps, 0),
        outboundMbps: srt.reduce((sum, s) => sum + s.sample.derived.recvMbps * 2, 0),
        srtConnCount: 2,
        srtMaxDropRate: srt[0].sample.derived.dropRate,
        srtMinRecoveryRate: Math.min(...srt.map((s) => s.sample.derived.recoveryRate ?? 100)),
        srtWorstSeverity: "warning",
      },
      paths,
      srt,
      removedPaths: [],
      removedSrt: [],
      events,
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

  /// サーバー系メトリクスの値。`frame.rs` がリングへ積むキーと同じ名前で返す。
  /// かつては全メトリクスに `srtMaxDropRate` を返しており、
  /// ダッシュボードの 4 グラフがすべて同じ線になっていた。
  const serverMetricValue = (u: MonitorUpdate, metric: string): number | null => {
    const t = u.totals;
    switch (metric) {
      case "api_rtt_ms":
        return u.api.rttMs;
      case "paths_online":
        return t.pathsOnline;
      case "paths_total":
        return t.pathCount;
      case "readers":
        return t.readerCount;
      case "inbound_mbps":
        return t.inboundMbps;
      case "outbound_mbps":
        return t.outboundMbps;
      case "srt_conns":
        return t.srtConnCount;
      case "srt_drop_rate":
        return t.srtMaxDropRate;
      default:
        return null;
    }
  };

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
        if (isSrt) {
          if (!s) return [h.at, null];
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
                    : metric === "srt_link_capacity"
                      ? d.linkCapacityMbps
                      : metric === "srt_nak_rate"
                        ? d.nakRate
                        : metric === "srt_unrecovered_packets"
                          ? d.lossPackets != null && d.recoveredPackets != null
                            ? Math.max(0, d.lossPackets - d.recoveredPackets)
                            : null
                          : metric === "srt_send_mbps"
                            ? s.sample.counters.mbpsSendRate
                            : null;
        } else {
          value = serverMetricValue(h.update, metric);
        }
        return [h.at, value];
      });
      return [metric, points] as [string, Array<[number, number | null]>];
    });
    return { scope: request.scope, fromMs: from, toMs: to, series };
  };

  // 起動直後に、過去 1 時間ぶんの履歴を 1 秒間隔で埋めておく。
  // ここで `tick()` を連続で呼ぶと全点が同じミリ秒に積まれ、グラフが
  // 右端の 1 点の縦線にしかならず、線の形も確認できなくなる。
  // 時刻を遡って与えることで、どの時間窓でも形が見えるようにする。
  queueMicrotask(() => {
    const now = Date.now();
    const stepMs = config.pollIntervalMs || 1000;
    const backfill = Math.floor(3600_000 / stepMs);
    for (let i = backfill; i > 0; i -= 1) {
      tick(now - i * stepMs);
    }
    // 最新は「今」として 1 点入れておく。
    tick(now);
    history.splice(0, Math.max(0, history.length - 3700));
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
