/// SRT ダッシュボード（アプリの主画面）。
///
/// 目的は **SRT 回線のパケットドロップと回復率の常時監視**。接続ごとに
/// 現在値をカードで見せ、履歴はバックエンドのリングから範囲取得して描く。
/// 値の計算（差分・率）はすべてバックエンドで済んでいる。

import { useEffect, useMemo, useState } from "react";
import { useAppStore } from "../store/useAppStore";
import { SRT_METRICS, WINDOWS, SEVERITY } from "../lib/metrics";
import { formatDuration, formatMbps, formatMs, formatNumber, formatPercent, srtStateLabel } from "../lib/format";
import { useHeldTone } from "../lib/hold";
import { EmptyState, SeverityBadge, Stat } from "../components/Badges";
import { TimeSeries } from "../components/TimeSeries";
import { EventTimeline } from "../components/EventTimeline";
import { ExportPanel } from "../components/ExportPanel";
import type { SrtConnSample, SrtSeverity } from "../types";

/** グラフに並べる順。ドロップ率と回復率を最優先（主目的）。 */
const CHART_METRICS = [
  "srt_drop_rate",
  "srt_recovery_rate",
  "srt_recv_mbps",
  "srt_rtt_ms",
] as const;

/// 計測値が空のときの説明。メトリクスごとに「何が無いのか」を書き分ける。
/// 以前は全グラフ共通で「まだ劣化や流量が記録されていません」と出しており、
/// ドロップ率のグラフでも落ちた／落ちていないが分からなかった。
const EMPTY_LABELS: Partial<Record<(typeof CHART_METRICS)[number], string>> = {
  srt_drop_rate: "まだ計測が始まっていません（監視開始後に記録されます）",
  srt_recovery_rate: "まだ計測が始まっていません（監視開始後に記録されます）",
  srt_recv_mbps: "まだ受信レートが記録されていません",
  srt_rtt_ms: "まだ RTT が記録されていません",
};

function worstSeverity(items: SrtConnSample[]): SrtSeverity | null {
  let worst: SrtSeverity | null = null;
  const rank = (s: SrtSeverity) => (s === "critical" ? 2 : s === "warning" ? 1 : 0);
  for (const item of items) {
    if (!worst || rank(item.severity) > rank(worst)) worst = item.severity;
  }
  return worst;
}

function tone(severity: SrtSeverity | null): "ok" | "warn" | "crit" | "neutral" {
  switch (severity) {
    case "ok":
      return "ok";
    case "warning":
      return "warn";
    case "critical":
      return "crit";
    default:
      return "neutral";
  }
}

export function SrtDashboard() {
  const srt = useAppStore((s) => s.srt);
  const totals = useAppStore((s) => s.totals);
  const events = useAppStore((s) => s.events);
  const api = useAppStore((s) => s.api);
  const selected = useAppStore((s) => s.selectedSrt);
  const selectSrt = useAppStore((s) => s.selectSrt);
  const windowMs = useAppStore((s) => s.windowMs);
  const setWindowMs = useAppStore((s) => s.setWindowMs);
  const series = useAppStore((s) => s.series);
  const loadSeries = useAppStore((s) => s.loadSeries);
  const appendPoints = useAppStore((s) => s.appendPoints);
  const holdSecs = useAppStore((s) => s.config?.srt.holdSecs ?? 0);
  const [exportOpen, setExportOpen] = useState(false);

  const conns = useMemo(
    () => Object.values(srt).sort((a, b) => a.path.localeCompare(b.path)),
    [srt],
  );

  // 選択中の接続が消えたら先頭へ戻す。
  useEffect(() => {
    if (conns.length === 0) return;
    if (!selected || !srt[selected]) selectSrt(conns[0].id);
  }, [conns, selected, srt, selectSrt]);

  const active = selected ? srt[selected] : undefined;
  const activeId = active?.id ?? "";

  // 接続・窓が変わったら系列を取り直す。
  // バックエンドは接続単位のスコープ（`srt:<id>`）で履歴を持つので、
  // メトリクスはまとめて 1 回で問い合わせる。
  useEffect(() => {
    if (!activeId) return;
    void loadSeries(`srt:${activeId}`, [...CHART_METRICS], true);
  }, [activeId, windowMs, loadSeries]);

  // 新しいサンプルが来たら、取得済みの系列へ最新値を追記する
  // （毎秒の全量再取得や、グラフのチラつきを避ける。値はバックエンド計算済み）。
  const seq = useAppStore((s) => s.seq);
  const lastUpdateAt = useAppStore((s) => s.lastUpdateAt);
  useEffect(() => {
    if (!activeId) return;
    const at = Date.parse(lastUpdateAt ?? "");
    if (Number.isNaN(at)) return;
    const c = useAppStore.getState().srt[activeId];
    if (!c) return;
    const values: Record<string, number | null> = {
      srt_drop_rate: c.derived.dropRate,
      srt_recovery_rate: c.derived.recoveryRate,
      srt_recv_mbps: c.derived.recvMbps,
      srt_rtt_ms: c.derived.rttMs,
    };
    appendPoints(`srt:${activeId}`, at, values);
  }, [seq, activeId, lastUpdateAt, appendPoints]);

  const worst = worstSeverity(conns);
  // 一瞬の劣化を見逃さないよう、警告表示は最低 holdSecs 秒は保つ。
  // フックは早期 return より前に置く（接続の増減でフック数が変わらないように）。
  const shownWorst = useHeldTone(tone(worst), holdSecs * 1000);

  if (conns.length === 0) {
    return (
      <section className="view" data-testid="srt-dashboard">
        <EmptyState
          title="SRT 接続がありません"
          hint={
            api.ok
              ? "MediaMTX に SRT で配信（または視聴）すると、ここに接続が現れます。"
              : "MediaMTX API に接続できていません。設定タブで接続先を確認してください。"
          }
        />
        <EventTimeline events={events} />
      </section>
    );
  }

  return (
    <section className="view" data-testid="srt-dashboard">
      <div className="view-head">
        <h1>SRT 品質モニター</h1>
        <div className="view-head-actions">
          <div className="segmented" role="group" aria-label="表示する時間範囲">
            {WINDOWS.map((w) => (
              <button
                key={w.ms}
                type="button"
                className={windowMs === w.ms ? "active" : ""}
                onClick={() => setWindowMs(w.ms)}
                aria-pressed={windowMs === w.ms}
              >
                {w.label}
              </button>
            ))}
          </div>
          <button type="button" className="btn" onClick={() => setExportOpen((v) => !v)}>
            期間を指定して書き出し
          </button>
        </div>
      </div>

      <div className="stat-row">
        <Stat label="SRT 接続" value={formatNumber(totals.srtConnCount, 0)} unit="本" />
        <Stat
          label="最大ドロップ率"
          value={formatPercent(totals.srtMaxDropRate)}
          tone={shownWorst}
          hint="全接続中の最大値（区間）"
          testId="stat-max-drop"
        />
        <Stat
          label="最小回復率"
          value={formatPercent(totals.srtMinRecoveryRate, 1)}
          tone={shownWorst}
          hint="再送受信 / 損失検出 × 100"
          testId="stat-min-recovery"
        />
        <Stat
          label="総合状態"
          value={worst ? SEVERITY[worst].label : "—"}
          tone={shownWorst}
          hint={
            shownWorst !== tone(worst)
              ? "直前に警告がありました（表示を保持しています）"
              : worst === "ok"
                ? "劣化は検出されていません"
                : "接続カードの詳細を確認してください"
          }
          testId="stat-worst"
        />
      </div>

      {exportOpen && active ? (
        <ExportPanel
          scope={`srt:${active.id}`}
          title={`SRT ${active.path}`}
          defaultMetrics={[...CHART_METRICS]}
          onClose={() => setExportOpen(false)}
        />
      ) : null}

      <div className="srt-layout">
        <div className="srt-list" role="list">
          {conns.map((conn) => (
            <button
              key={conn.id}
              type="button"
              role="listitem"
              className={`srt-card ${conn.id === active?.id ? "active" : ""} sev-border-${conn.severity}`}
              onClick={() => selectSrt(conn.id)}
              data-testid={`srt-card-${conn.path || conn.id}`}
            >
              <div className="srt-card-head">
                <span className="srt-path">{conn.path || "(パス不明)"}</span>
                <SeverityBadge severity={conn.severity} />
              </div>
              <div className="srt-card-state">
                {srtStateLabel(conn.state)} ・ {conn.remoteAddr || "接続元不明"}
                {conn.fromMetrics ? "" : "（API のみ）"}
              </div>
              <div className="srt-card-metrics">
                <div>
                  <span className="k">ドロップ率</span>
                  <span className={`v sev-text-${conn.severity}`}>{formatPercent(conn.derived.dropRate)}</span>
                </div>
                <div>
                  <span className="k">回復率</span>
                  <span className="v">{formatPercent(conn.derived.recoveryRate, 1)}</span>
                </div>
                <div>
                  <span className="k">受信</span>
                  <span className="v">{formatMbps(conn.derived.recvMbps)}</span>
                </div>
                <div>
                  <span className="k">RTT</span>
                  <span className="v">{formatMs(conn.derived.rttMs)}</span>
                </div>
              </div>
              <div className="srt-card-foot">
                <span>確立から {formatDuration(conn.created)}</span>
                {conn.derived.counterReset ? <span className="warn-text">カウンタリセット検出</span> : null}
              </div>
            </button>
          ))}
        </div>

        <div className="srt-detail">
          {active ? (
            <>
              <div className="srt-detail-head">
                <h2>
                  {active.path} <code>{active.id.slice(0, 8)}</code>
                </h2>
                <div className="srt-detail-facts">
                  <span>受信 <b>{formatMbps(active.derived.recvMbps)}</b></span>
                  <span>回線容量 <b>{formatMbps(active.derived.linkCapacityMbps)}</b></span>
                  <span>帯域余裕 <b>{formatPercent(active.derived.headroomPercent, 0)}</b></span>
                  <span>推定遅延 <b>{formatMs(active.derived.estLatencyMs, 0)}</b></span>
                  <span>
                    区間の損失 <b>{formatNumber(active.derived.lossPackets)}</b> / 回復{" "}
                    <b>{formatNumber(active.derived.recoveredPackets)}</b>
                  </span>
                </div>
              </div>
              <div className="chart-grid">
                {CHART_METRICS.map((key) => {
                  const def = SRT_METRICS[key];
                  const state = series[`srt:${active.id}`];
                  const pts = state?.byMetric[key] ?? [];
                  return (
                    <TimeSeries
                      key={key}
                      metric={def}
                      points={pts}
                      windowMs={windowMs}
                      threshold={key === "srt_drop_rate" ? 0.5 : undefined}
                      emptyLabel={
                        state?.loading
                          ? "履歴を読み込み中…"
                          : EMPTY_LABELS[key] ?? "計測値がまだありません"
                      }
                    />
                  );
                })}
              </div>
            </>
          ) : null}
        </div>
      </div>

      <EventTimeline events={events.filter((e) => e.target === active?.id || e.target === active?.path)} />
    </section>
  );
}
