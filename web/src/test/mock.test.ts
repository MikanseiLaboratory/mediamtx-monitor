/// モックブリッジが返す系列のテスト。
///
/// 不具合の再発防止が目的:
/// - サーバー系スコープは**メトリクスごとに違う系列**を返す
///   （かつては全部 `srtMaxDropRate` を返し、ダッシュボードの 4 グラフが
///    同じ線になっていた）
/// - 要求した期間に対して、データがその期間の末尾まで入っている

import { describe, expect, it } from "vitest";
import { createMockBridge } from "../bridge/mock";
import type { SeriesResponse } from "../types";

async function fetchSeries(
  bridge: ReturnType<typeof createMockBridge>,
  scope: string,
  metrics: string[],
  windowMs = 5 * 60_000,
): Promise<SeriesResponse> {
  const to = Date.now();
  return bridge.invoke<SeriesResponse>("get_series", {
    request: { scope, metrics, fromMs: to - windowMs, toMs: to },
  });
}

describe("モックの系列", () => {
  it("サーバー系メトリクスはそれぞれ別の値域を返す", async () => {
    const bridge = createMockBridge();
    // 起動直後の一括投入を待つ
    await new Promise((r) => setTimeout(r, 30));

    const metrics = ["api_rtt_ms", "readers", "inbound_mbps", "srt_drop_rate"];
    const resp = await fetchSeries(bridge, "server", metrics);
    const byMetric = new Map(resp.series.map(([m, pts]) => [m, pts]));

    for (const m of metrics) {
      expect(byMetric.get(m)!.length).toBeGreaterThan(0);
    }

    const last = (m: string) => byMetric.get(m)!.at(-1)![1]!;
    // API 応答時間は数 ms 台、視聴者数は人、ビットレートは Mbps 台と、
    // 桁が違う。これが全部同じ値になっていたら不具合。
    expect(last("api_rtt_ms")).toBeLessThan(50);
    expect(last("readers")).toBeGreaterThanOrEqual(0);
    expect(last("inbound_mbps")).toBeGreaterThan(1);
    const values = metrics.map((m) => last(m));
    expect(new Set(values).size).toBeGreaterThan(1);
  });

  it("サーバー系列は互いに独立した形をする（同じ配列を共有しない）", async () => {
    const bridge = createMockBridge();
    await new Promise((r) => setTimeout(r, 30));
    const resp = await fetchSeries(bridge, "server", ["readers", "srt_drop_rate"]);
    const a = resp.series.find(([m]) => m === "readers")![1];
    const b = resp.series.find(([m]) => m === "srt_drop_rate")![1];
    // 値の並びが完全一致することはない
    expect(a.map((p) => p[1])).not.toEqual(b.map((p) => p[1]));
  });

  it("SRT スコープは接続ごとの系列を返す", async () => {
    const bridge = createMockBridge();
    await new Promise((r) => setTimeout(r, 30));
    const resp = await fetchSeries(bridge, "srt:mock-srt-cam1", [
      "srt_drop_rate",
      "srt_recovery_rate",
      "srt_recv_mbps",
      "srt_rtt_ms",
    ]);
    for (const [, pts] of resp.series) {
      expect(pts.length).toBeGreaterThan(0);
      expect(pts.every(([, v]) => v !== null)).toBe(true);
    }
  });

  it("read 接続でもレートが 0 にならない（送信側の値を返す）", async () => {
    // cam1 は read（MediaMTX が視聴者へ送信する側）。
    // 実機では `mbpsReceiveRate` が制御パケット分しか出ないため、
    // 送信レートを見ないと受信レートが常に 0 になる。
    const bridge = createMockBridge();
    await new Promise((r) => setTimeout(r, 30));
    const resp = await fetchSeries(bridge, "srt:mock-srt-cam1", ["srt_recv_mbps"]);
    const pts = resp.series[0][1];
    const last = pts.at(-1)![1];
    expect(last).not.toBeNull();
    expect(last!).toBeGreaterThan(1);
  });

  it("read 接続のカウンタは送信側に入る", async () => {
    const bridge = createMockBridge();
    await new Promise((r) => setTimeout(r, 30));
    const snap = await bridge.invoke<{
      srt: Array<{ sample: { id: string; state: string; counters: Record<string, number | null> } }>;
    }>("get_snapshot");
    const cam1 = snap.srt.find((s) => s.sample.id === "mock-srt-cam1")!;
    expect(cam1.sample.state).toBe("read");
    const c = cam1.sample.counters;
    // 送信側は有効、受信側は制御パケット分のみ
    expect(c.packetsSentUnique).toBeGreaterThan(0);
    expect(c.mbpsSendRate).toBeGreaterThan(1);
    expect(c.mbpsReceiveRate).toBeLessThan(0.1);
  });

  it("存在しない接続は null（欠測）を返し、他接続の値を混ぜない", async () => {
    const bridge = createMockBridge();
    await new Promise((r) => setTimeout(r, 30));
    const resp = await fetchSeries(bridge, "srt:does-not-exist", ["srt_drop_rate"]);
    const pts = resp.series[0][1];
    expect(pts.length).toBeGreaterThan(0);
    expect(pts.every(([, v]) => v === null)).toBe(true);
  });

  it("未知のメトリクスは null を返す（別メトリクスの値で埋めない）", async () => {
    const bridge = createMockBridge();
    await new Promise((r) => setTimeout(r, 30));
    const resp = await fetchSeries(bridge, "server", ["unknown_metric"]);
    const pts = resp.series[0][1];
    expect(pts.every(([, v]) => v === null)).toBe(true);
  });

  it("要求した期間の中にデータが入る", async () => {
    const bridge = createMockBridge();
    await new Promise((r) => setTimeout(r, 30));
    const to = Date.now();
    const from = to - 60_000;
    const resp = await fetchSeries(bridge, "server", ["api_rtt_ms"], 60_000);
    const pts = resp.series[0][1];
    expect(pts.length).toBeGreaterThan(0);
    for (const [t] of pts) {
      expect(t).toBeGreaterThanOrEqual(from);
      expect(t).toBeLessThanOrEqual(to);
    }
  });
});
