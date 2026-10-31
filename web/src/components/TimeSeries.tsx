/// 1 本の時系列を uPlot で描くコンポーネント。
///
/// データは親から渡される（フロントで集計しない）。1 秒周期の更新でも
/// 再マウントせず `setData` だけ呼ぶので、描画がちらつかない。
///
/// ## x スケールの扱い
///
/// uPlot の `setData` は、渡したデータの先頭と末尾から x 範囲を自動決定する。
/// データ側で「窓の左端〜右端」を網羅していないと窓が縮むので、ここでは
/// **データに窓の左端の点を 1 つ補ってから `setData` に渡し**、自動決定に任せる。
///
/// かつては `setData` の直後に `setScale("x", …)` で固定窓を上書きしていたが、
/// これだとユーザーのドラッグ拡大が毎秒の更新で打ち消され、さらに
/// 窓の基準が壁時計になるためデータが窓の右端に寄って見えていた。
///
/// ## カーソル同期
///
/// 全グラフが同じ `sync` キーを共有すると、uPlot はモジュール内の
/// グローバルレジストリで **同一ページの全グラフのカーソルを連動**させる。
/// 縦軸の単位も値域も違うグラフを並べているので、画面ごとに独立したキーを作る。

import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import type { MetricPoint } from "../types";
import type { MetricDef } from "../lib/metrics";
import { toColumnar as toColumnarData, windowRange } from "../lib/series";

export interface TimeSeriesProps {
  metric: MetricDef;
  points: MetricPoint[];
  /** 表示する時間窓（ms）。 */
  windowMs: number;
  height?: number;
  /** しきい値線（% のグラフに赤線を引く）。 */
  threshold?: number;
  emptyLabel?: string;
  /**
   * カーソル同期のグループ。同じキーのグラフだけがカーソルを連動させる。
   * 未指定ならグラフごとに独立する（同期しない）。
   */
  syncKey?: string;
}

function buildOptions(
  metric: MetricDef,
  height: number,
  threshold: number | undefined,
  syncKey: string | undefined,
): uPlot.Options {
  const axisColor = "#8b98a9";
  const gridColor = "rgba(120, 140, 165, 0.16)";
  return {
    width: 10,
    height,
    padding: [8, 8, 0, 0],
    cursor: {
      drag: { x: true, y: false, setScale: true },
      // 未指定なら `key` を渡さない（uPlot はキー無しのグループに登録するが、
      // キー付きのグループとは混ざらない）。
      ...(syncKey ? { sync: { key: syncKey } } : {}),
    },
    legend: { show: false },
    scales: {
      // `time: true` にして time 軸の目盛りを使うが、範囲はデータから決める。
      x: { time: true },
      y: {
        auto: true,
        range: ((_u: uPlot, _min: number, max: number): uPlot.Range.MinMax => {
          if (metric.percent) return [0, Math.max(100, max)];
          return [metric.min ?? 0, Math.max(max * 1.15, 1)];
        }) as unknown as uPlot.Scale["range"],
      },
    },
    axes: [
      {
        stroke: axisColor,
        grid: { stroke: gridColor, width: 1 },
        ticks: { stroke: gridColor, width: 1 },
        font: "11px 'Segoe UI', 'Yu Gothic UI', sans-serif",
        size: 26,
      },
      {
        stroke: axisColor,
        grid: { stroke: gridColor, width: 1 },
        ticks: { stroke: gridColor, width: 1 },
        font: "11px 'Segoe UI', 'Yu Gothic UI', sans-serif",
        size: 52,
        values: (_u, ticks) => ticks.map((v) => (metric.percent ? `${v.toFixed(0)}%` : `${v}`)),
      },
    ],
    series: [
      {},
      {
        label: metric.label,
        stroke: metric.color,
        width: 1.8,
        fill: `${metric.color}22`,
        spanGaps: false,
        points: { show: false },
      },
    ],
    hooks: {
      draw: [
        (u) => {
          if (threshold === undefined) return;
          const y = u.valToPos(threshold, "y", true);
          const ctx = u.ctx;
          ctx.save();
          ctx.strokeStyle = "#f25f5c";
          ctx.setLineDash([6, 4]);
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(u.bbox.left, y);
          ctx.lineTo(u.bbox.left + u.bbox.width, y);
          ctx.stroke();
          ctx.restore();
        },
      ],
    },
  };
}

/// 窓の左端の点を補い、データの範囲がちょうど表示窓になるようにする。
///
/// x 軸の自動決定はデータの先頭/末尾で行われるので、左端の点を足せば
/// 「窓いっぱい」のスケールになる。値は `null` なので線は引かれない
/// （`spanGaps: false`）。データが 1 点も無いときは何も足さない。
export function padToWindow(
  columnar: [number[], Array<number | null>],
  points: MetricPoint[],
  windowMs: number,
  nowMs: number = Date.now(),
): [number[], Array<number | null>] {
  const [xs, ys] = columnar;
  if (xs.length === 0) return columnar;
  const [left, right] = windowRange(points, windowMs, nowMs);
  const outX: number[] = [];
  const outY: Array<number | null> = [];
  if (xs[0] > left) {
    outX.push(left);
    outY.push(null);
  }
  for (let i = 0; i < xs.length; i += 1) {
    outX.push(xs[i]);
    outY.push(ys[i]);
  }
  // データが右端より手前で止まっていても、窓の右端まで見せる。
  const lastX = outX[outX.length - 1];
  if (lastX < right) {
    outX.push(right);
    outY.push(null);
  }
  return [outX, outY];
}

export function TimeSeries({
  metric,
  points,
  windowMs,
  height = 180,
  threshold,
  emptyLabel,
  syncKey,
}: TimeSeriesProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const plotRef = useRef<uPlot | null>(null);
  /// ユーザーがドラッグで拡大している間は、データ更新で自動スケールしない。
  const zoomedRef = useRef(false);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const opts = buildOptions(metric, height, threshold, syncKey);
    const plot = new uPlot(opts, [[], []], host);
    plotRef.current = plot;
    zoomedRef.current = false;

    // uPlot は `mousedown` を over 要素で、`mouseup` を document で受けて
    // ドラッグ範囲を `drag.setScale` として適用する。拡大の確定は
    // document の `mouseup` の後なので、そこで拡大中かどうかを記録する。
    // （`setScale` フックは自動スケールでも発火するので使えない）
    const onMouseUp = () => {
      zoomedRef.current = plot.select.width > 0;
    };
    // 拡大を解除する操作: ダブルクリックで窓表示に戻す。
    const onDoubleClick = () => {
      zoomedRef.current = false;
      plot.setData(plot.data, true);
    };
    document.addEventListener("mouseup", onMouseUp);
    plot.over.addEventListener("dblclick", onDoubleClick);

    const observer = new ResizeObserver(() => {
      const width = host.clientWidth;
      if (width > 0) plot.setSize({ width, height });
    });
    observer.observe(host);
    return () => {
      document.removeEventListener("mouseup", onMouseUp);
      plot.over.removeEventListener("dblclick", onDoubleClick);
      observer.disconnect();
      plot.destroy();
      plotRef.current = null;
    };
  }, [metric.key, height, threshold, syncKey]);

  useEffect(() => {
    const plot = plotRef.current;
    if (!plot) return;
    const columnar = toColumnarData(points, windowMs);
    // 窓が変わったときは拡大状態を解除して、窓表示に戻す。
    plot.setData(padToWindow(columnar, points, windowMs), !zoomedRef.current);
  }, [points, windowMs]);

  // 窓・メトリクスが変わったら拡大状態は解除する。
  useEffect(() => {
    zoomedRef.current = false;
  }, [windowMs, metric.key]);

  // 「グラフに描ける点があるか」で判定する。窓の外の古い点だけがある場合や、
  // 窓内がすべて欠測（null）の場合は、何も描かれていないので空表示にする。
  const hasData = toColumnarData(points, windowMs)[1].some((v) => v !== null);

  return (
    <div className="timeseries" data-testid={`timeseries-${metric.key}`}>
      <div className="timeseries-head">
        <span className="timeseries-dot" style={{ background: metric.color }} aria-hidden="true" />
        <span className="timeseries-label">{metric.label}</span>
        {metric.unit ? <span className="timeseries-unit">{metric.unit}</span> : null}
      </div>
      <div className="timeseries-host" ref={hostRef} />
      {!hasData ? (
        <div className="timeseries-empty" role="status">
          {emptyLabel ?? "データを待っています…"}
        </div>
      ) : null}
    </div>
  );
}
