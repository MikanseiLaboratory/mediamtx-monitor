/// 1 本の時系列を uPlot で描くコンポーネント。
///
/// データは親から渡される（フロントで集計しない）。1 秒周期の更新でも
/// 再マウントせず `setData` だけ呼ぶので、描画がちらつかない。

import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import type { MetricPoint } from "../types";
import type { MetricDef } from "../lib/metrics";

export interface TimeSeriesProps {
  metric: MetricDef;
  points: MetricPoint[];
  /** 表示する時間窓（ms）。x 軸を固定するために使う。 */
  windowMs: number;
  height?: number;
  /** しきい値線（% のグラフに赤線を引く）。 */
  threshold?: number;
  emptyLabel?: string;
}

function buildOptions(
  metric: MetricDef,
  height: number,
  threshold?: number,
): uPlot.Options {
  const axisColor = "#8b98a9";
  const gridColor = "rgba(120, 140, 165, 0.16)";
  return {
    width: 10,
    height,
    padding: [8, 8, 0, 0],
    cursor: {
      drag: { x: true, y: false, setScale: true },
      sync: { key: "mikansei" },
    },
    legend: { show: false },
    scales: {
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

function toColumnar(points: MetricPoint[], windowMs: number): uPlot.AlignedData {
  const now = Date.now();
  const start = now - windowMs;
  const visible = points.filter(([t]) => t >= start - windowMs * 0.1);
  const xs = new Array<number>(visible.length);
  const ys = new Array<number | null>(visible.length);
  for (let i = 0; i < visible.length; i += 1) {
    xs[i] = visible[i][0];
    ys[i] = visible[i][1];
  }
  return [xs, ys];
}

export function TimeSeries({ metric, points, windowMs, height = 180, threshold, emptyLabel }: TimeSeriesProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const plotRef = useRef<uPlot | null>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const opts = buildOptions(metric, height, threshold);
    const plot = new uPlot(opts, [[], []], host);
    plotRef.current = plot;
    const observer = new ResizeObserver(() => {
      const width = host.clientWidth;
      if (width > 0) plot.setSize({ width, height });
    });
    observer.observe(host);
    return () => {
      observer.disconnect();
      plot.destroy();
      plotRef.current = null;
    };
  }, [metric.key, windowMs, height, threshold]);

  useEffect(() => {
    const plot = plotRef.current;
    if (!plot) return;
    plot.setData(toColumnar(points, windowMs));
    const now = Date.now() / 1000;
    plot.setScale("x", { min: now - windowMs / 1000, max: now });
  }, [points, windowMs]);

  const hasData = points.some(([, v]) => v !== null);

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
