/// uPlot に渡す列データの組み立て。
///
/// ストアの系列は **Unix ミリ秒**（バックエンドの `at_ms`）で持つが、
/// uPlot の time 軸は **Unix 秒** を前提にしている。ここで単位を揃えないと
/// x 軸のスケール（秒）とデータ（ミリ秒）が 1000 倍ずれて、線が描かれない。
///
/// 表示窓の基準は **壁時計（`Date.now()`）ではなく系列の最大時刻**にする。
/// 壁時計を基準にすると、バックエンドのポーリングが一時的に止まったときや
/// 取得直後に「窓の右端が今より先」になり、データが窓の左へずれて
/// 左端に空白ができる。データの最新点を右端に置けば、データの有無に
/// 関わらず線は常に窓いっぱいに描かれる。

import type { MetricPoint } from "../types";

/** 表示窓の基準時刻（Unix ミリ秒）。データが無ければ壁時計を返す。 */
export function windowAnchorMs(points: MetricPoint[], nowMs: number = Date.now()): number {
  let latest = Number.NEGATIVE_INFINITY;
  for (const [t] of points) {
    if (t > latest) latest = t;
  }
  // 未来の点（時計ずれ）が 1 点混ざっても窓が吹き飛ばないよう、壁時計で頭打ちにする。
  return Number.isFinite(latest) ? Math.min(latest, nowMs) : nowMs;
}

/** 表示窓に含まれる点を [x(秒), y] の列に変換する。 */
export function toColumnar(
  points: MetricPoint[],
  windowMs: number,
  nowMs: number = Date.now(),
): [number[], Array<number | null>] {
  const anchor = windowAnchorMs(points, nowMs);
  // 端の点が窓の外に出ないよう、10% だけ余分に残す。
  const cutoff = anchor - windowMs * 1.1;
  const visible = points.filter(([t]) => t >= cutoff);
  const xs = new Array<number>(visible.length);
  const ys = new Array<number | null>(visible.length);
  for (let i = 0; i < visible.length; i += 1) {
    xs[i] = visible[i][0] / 1000;
    ys[i] = visible[i][1];
  }
  return [xs, ys];
}

/** 表示窓の x 範囲（Unix 秒）。データの最大時刻を右端に揃える。 */
export function windowRange(
  points: MetricPoint[],
  windowMs: number,
  nowMs: number = Date.now(),
): [number, number] {
  const anchor = windowAnchorMs(points, nowMs);
  return [(anchor - windowMs) / 1000, anchor / 1000];
}
