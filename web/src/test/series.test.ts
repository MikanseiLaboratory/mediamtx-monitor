/// グラフの列データ組み立てと、ストアの系列取得のテスト。
///
/// 不具合の再発防止が目的:
/// - 表示窓の基準が壁時計だとしても、データが窓の左にずれない
/// - 窓を素早く切り替えたとき、古い応答が新しい結果を上書きしない
/// - 取得中に追記した点が、後から届く取得結果で消えない

import { beforeEach, describe, expect, it, vi } from "vitest";
import { padToWindow } from "../components/TimeSeries";
import { toColumnar, windowAnchorMs, windowRange } from "../lib/series";
import { api } from "../bridge";
import { resetStoreForTest, useAppStore } from "../store/useAppStore";
import type { MetricPoint, SeriesResponse } from "../types";

const NOW = 1_700_000_000_000;

/** `count` 点を 1 秒間隔で作り、最後の点を `endMs` に置く。 */
function pointsEndingAt(endMs: number, count: number, stepMs = 1000): MetricPoint[] {
  const out: MetricPoint[] = [];
  for (let i = count - 1; i >= 0; i -= 1) out.push([endMs - i * stepMs, i]);
  return out;
}

describe("表示窓の基準", () => {
  it("データの最大時刻を基準にする（壁時計ではない）", () => {
    const pts = pointsEndingAt(NOW - 30_000, 10);
    // 30 秒前が最新でも、基準はその点になる。
    expect(windowAnchorMs(pts, NOW)).toBe(NOW - 30_000);
  });

  it("未来の点が混ざっても壁時計で頭打ちにする", () => {
    const pts: MetricPoint[] = [
      [NOW - 1000, 1],
      [NOW + 600_000, 2],
    ];
    expect(windowAnchorMs(pts, NOW)).toBe(NOW);
  });

  it("データが無ければ壁時計を使う", () => {
    expect(windowAnchorMs([], NOW)).toBe(NOW);
  });

  it("窓の右端はデータの最新点、左端はそこから窓長ぶん前", () => {
    const pts = pointsEndingAt(NOW - 45_000, 10);
    const [left, right] = windowRange(pts, 60_000, NOW);
    expect(right).toBe((NOW - 45_000) / 1000);
    expect(left).toBe((NOW - 105_000) / 1000);
  });

  it("列データの x は秒へ変換される（ミリ秒のまま渡さない）", () => {
    const pts: MetricPoint[] = [[NOW - 1000, 5]];
    const [xs, ys] = toColumnar(pts, 60_000, NOW);
    expect(xs).toEqual([(NOW - 1000) / 1000]);
    expect(ys).toEqual([5]);
    // uPlot の time 軸は秒を前提にするので、10 桁台（1970 年付近）にならない。
    expect(xs[0]).toBeGreaterThan(1_000_000_000);
  });

  it("窓より古い点は捨てる（端の点は 10% 余分に残す）", () => {
    const pts: MetricPoint[] = [
      [NOW - 10 * 60_000, 99], // 10 分前 → 窓外
      [NOW - 60_000, 1],
      [NOW - 30_000, 2],
    ];
    const [xs] = toColumnar(pts, 60_000, NOW);
    expect(xs).toHaveLength(2);
  });
});

describe("窓いっぱいへの補完", () => {
  it("左端に null を足して、窓全体をデータ範囲にする", () => {
    const pts: MetricPoint[] = [
      [NOW - 40_000, 1],
      [NOW - 20_000, 2],
    ];
    const columnar = toColumnar(pts, 60_000, NOW);
    const [xs, ys] = padToWindow(columnar, pts, 60_000, NOW);
    const [left] = windowRange(pts, 60_000, NOW);
    // 左端は null で埋め、右端はデータの最終点（= 窓の右端）のまま
    expect(xs[0]).toBe(left);
    expect(ys[0]).toBeNull();
    expect(xs).toHaveLength(3);
    expect(ys[ys.length - 1]).toBe(2);
  });

  it("データが右端より古ければ、右端にも null を足す", () => {
    const pts: MetricPoint[] = [
      [NOW - 50_000, 1],
      [NOW - 30_000, 2], // 最新が 30 秒前 → 右端はここ
    ];
    const [xs, ys] = padToWindow(toColumnar(pts, 60_000, NOW), pts, 60_000, NOW);
    // 右端 = データの最終点なので、追加は左端の 1 点だけ
    expect(xs).toHaveLength(3);
    expect(ys[ys.length - 1]).toBe(2);
  });

  it("両端が既に窓に一致していれば何も足さない", () => {
    const pts: MetricPoint[] = [
      [NOW - 60_000, 1],
      [NOW, 2],
    ];
    const columnar = toColumnar(pts, 60_000, NOW);
    const [xs] = padToWindow(columnar, pts, 60_000, NOW);
    expect(xs).toHaveLength(2);
  });

  it("データが無ければ空のまま返す", () => {
    const [xs] = padToWindow([[], []], [], 60_000, NOW);
    expect(xs).toHaveLength(0);
  });
});

describe("系列の取得（競合と追記）", () => {
  beforeEach(() => {
    resetStoreForTest();
    vi.restoreAllMocks();
  });

  function response(scope: string, value: number): SeriesResponse {
    return {
      scope,
      fromMs: 0,
      toMs: NOW,
      series: [["api_rtt_ms", [[NOW - 1000, value]]]],
    };
  }

  it("古い要求の応答が後から届いても、新しい結果を上書きしない", async () => {
    const resolvers: Array<(r: SeriesResponse) => void> = [];
    vi.spyOn(api, "getSeries").mockImplementation(
      () => new Promise<SeriesResponse>((resolve) => resolvers.push(resolve)),
    );

    const store = useAppStore.getState();
    const first = store.loadSeries("server", ["api_rtt_ms"], true);
    // 窓を切り替えた体で 2 回目の要求（こちらが最新）
    const second = store.loadSeries("server", ["api_rtt_ms"], true);

    // 新しい要求が先に返り、古い要求が後から返る
    resolvers[1](response("server", 999));
    await second;
    resolvers[0](response("server", 111));
    await first;

    const pts = useAppStore.getState().series.server.byMetric.api_rtt_ms;
    expect(pts).toEqual([[NOW - 1000, 999]]);
  });

  it("古い要求の失敗で、新しい結果をエラー表示にしない", async () => {
    const rejecters: Array<(e: unknown) => void> = [];
    const resolvers: Array<(r: SeriesResponse) => void> = [];
    vi.spyOn(api, "getSeries").mockImplementation(
      () =>
        new Promise<SeriesResponse>((resolve, reject) => {
          resolvers.push(resolve);
          rejecters.push(reject);
        }),
    );

    const store = useAppStore.getState();
    const first = store.loadSeries("server", ["api_rtt_ms"], true);
    const second = store.loadSeries("server", ["api_rtt_ms"], true);

    resolvers[1](response("server", 42));
    await second;
    rejecters[0](new Error("timeout"));
    await first;

    const state = useAppStore.getState().series.server;
    expect(state.error).toBeNull();
    expect(state.loading).toBe(false);
    expect(state.byMetric.api_rtt_ms).toEqual([[NOW - 1000, 42]]);
  });

  it("取得中は追記を控える（後から届く結果で追記分が消えない）", async () => {
    let resolveReq: ((r: SeriesResponse) => void) | null = null;
    vi.spyOn(api, "getSeries").mockImplementation(
      () =>
        new Promise<SeriesResponse>((resolve) => {
          resolveReq = resolve;
        }),
    );
    const store = useAppStore.getState();
    const pending = store.loadSeries("server", ["api_rtt_ms"], true);
    expect(useAppStore.getState().series.server.loading).toBe(true);

    useAppStore.getState().appendPoints("server", NOW - 500, { api_rtt_ms: 77 });
    resolveReq!(response("server", 1));
    await pending;

    // 取得結果だけが残る（追記は取得完了後に改めて届いた分が載る）
    const pts = useAppStore.getState().series.server.byMetric.api_rtt_ms;
    expect(pts).toEqual([[NOW - 1000, 1]]);
  });

  it("取得が終わっていれば追記は反映される", async () => {
    vi.spyOn(api, "getSeries").mockResolvedValue(response("server", 1));
    const store = useAppStore.getState();
    await store.loadSeries("server", ["api_rtt_ms"], true);

    useAppStore.getState().appendPoints("server", NOW - 500, { api_rtt_ms: 77 });
    const pts = useAppStore.getState().series.server.byMetric.api_rtt_ms;
    expect(pts).toHaveLength(2);
    expect(pts[1]).toEqual([NOW - 500, 77]);
  });
});
