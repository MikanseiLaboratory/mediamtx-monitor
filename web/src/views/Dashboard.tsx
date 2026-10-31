/// ダッシュボード（MediaMTX 全体の死活監視）。

import { useEffect, useMemo } from "react";
import { pointsOf, useAppStore } from "../store/useAppStore";
import { SERVER_METRICS } from "../lib/metrics";
import { formatCount, formatMbps, formatMs, formatPercent, formatTime } from "../lib/format";
import { ApiBadge, EmptyState, PathBadge, Stat } from "../components/Badges";
import { PathCard } from "../components/PathCard";
import { TimeSeries } from "../components/TimeSeries";
import { EventTimeline } from "../components/EventTimeline";

const SERVER_CHARTS = ["api_rtt_ms", "readers", "inbound_mbps", "srt_drop_rate"] as const;

export function Dashboard() {
  const paths = useAppStore((s) => s.paths);
  const totals = useAppStore((s) => s.totals);
  const api = useAppStore((s) => s.api);
  const status = useAppStore((s) => s.status);
  const events = useAppStore((s) => s.events);
  const series = useAppStore((s) => s.series);
  const windowMs = useAppStore((s) => s.windowMs);
  const loadSeries = useAppStore((s) => s.loadSeries);
  const appendPoints = useAppStore((s) => s.appendPoints);
  const selectedPath = useAppStore((s) => s.selectedPath);
  const selectPath = useAppStore((s) => s.selectPath);

  const list = useMemo(
    () => Object.values(paths).sort((a, b) => Number(a.online) - Number(b.online) || a.name.localeCompare(b.name)),
    [paths],
  );

  // サーバー系列は表示時と窓の変更時に取り直す（追記は下の効果で行う）。
  useEffect(() => {
    for (const key of SERVER_CHARTS) {
      void loadSeries(`server:${key}`, [key], true);
    }
  }, [loadSeries, windowMs]);

  // 最新値の追記（再取得せずに伸ばす）。
  const seq = useAppStore((s) => s.seq);
  useEffect(() => {
    const at = Date.parse(useAppStore.getState().lastUpdateAt ?? "");
    if (Number.isNaN(at)) return;
    const t = useAppStore.getState();
    const values: Record<string, number | null> = {
      api_rtt_ms: t.api.rttMs,
      readers: t.totals.readerCount,
      inbound_mbps: t.totals.inboundMbps,
      srt_drop_rate: t.totals.srtMaxDropRate,
    };
    for (const key of SERVER_CHARTS) {
      appendPoints(`server:${key}`, at, values);
    }
  }, [seq, appendPoints]);

  const running = status?.running ?? false;

  return (
    <section className="view" data-testid="dashboard">
      <div className="view-head">
        <h1>全体ダッシュボード</h1>
        <div className="view-head-actions">
          <ApiBadge ok={api.ok} />
          <span className={`badge ${running ? "sev-ok" : "sev-warning"}`} data-testid="monitor-state">
            監視 {running ? "稼働中" : "停止"}
          </span>
          {status?.serverVersion ? (
            <span className="muted">MediaMTX {status.serverVersion}</span>
          ) : null}
        </div>
      </div>

      {!api.ok && api.error ? (
        <div className="banner banner-error" role="alert">
          <strong>MediaMTX に接続できません。</strong>
          <span>{api.error.message}</span>
          {api.error.detail ? <code>{api.error.detail}</code> : null}
        </div>
      ) : null}
      {api.ok && !api.metricsOk && api.metricsError ? (
        <div className="banner banner-warn" role="status">
          <strong>メトリクスを取得できません（死活監視のみ継続）。</strong>
          <span>{api.metricsError.message}</span>
        </div>
      ) : null}

      <div className="stat-row">
        <Stat
          label="稼働パス"
          value={`${totals.pathsOnline}/${totals.pathCount}`}
          tone={totals.pathsDown > 0 ? "warn" : "ok"}
          testId="stat-paths"
        />
        <Stat label="視聴者" value={formatCount(totals.readerCount)} unit="人" />
        <Stat label="受信合計" value={formatMbps(totals.inboundMbps)} />
        <Stat
          label="API 応答"
          value={formatMs(api.rttMs, 1)}
          tone={api.rttMs !== null && api.rttMs > 500 ? "crit" : "neutral"}
          hint={`最終取得 ${formatTime(status?.lastTickAt)}`}
        />
        <Stat
          label="SRT 最大ドロップ"
          value={formatPercent(totals.srtMaxDropRate)}
          tone={
            totals.srtWorstSeverity === "critical"
              ? "crit"
              : totals.srtWorstSeverity === "warning"
                ? "warn"
                : "ok"
          }
        />
      </div>

      <div className="chart-grid chart-grid-server">
        {SERVER_CHARTS.map((key) => (
          <TimeSeries
            key={key}
            metric={SERVER_METRICS[key]}
            points={pointsOf(series[`server`], key)}
            windowMs={windowMs}
            emptyLabel="履歴を読み込み中…"
          />
        ))}
      </div>

      <h2 className="section-title">パス一覧</h2>
      {list.length === 0 ? (
        <EmptyState
          title="パスが見つかりません"
          hint="MediaMTX にパス設定が無いか、まだ配信がありません。パス管理タブから追加できます。"
        />
      ) : (
        <div className="path-grid" data-testid="path-grid">
          {list.map((p) => (
            <PathCard
              key={p.name}
              path={p}
              active={selectedPath === p.name}
              onSelect={(name) => selectPath(name === selectedPath ? null : name)}
            />
          ))}
        </div>
      )}

      <div className="two-col">
        <div>
          <h2 className="section-title">状態遷移</h2>
          <EventTimeline events={events} />
        </div>
        <div>
          <h2 className="section-title">パスの内訳</h2>
          <table className="table">
            <thead>
              <tr>
                <th>パス</th>
                <th>状態</th>
                <th>ソース</th>
                <th>視聴者</th>
                <th>受信</th>
              </tr>
            </thead>
            <tbody>
              {list.map((p) => (
                <tr key={p.name}>
                  <td>{p.name}</td>
                  <td>
                    <PathBadge severity={p.severity} />
                  </td>
                  <td>{p.sourceType || "—"}</td>
                  <td>{formatCount(p.readerCount)}</td>
                  <td>{formatMbps(p.inboundMbps)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </section>
  );
}
