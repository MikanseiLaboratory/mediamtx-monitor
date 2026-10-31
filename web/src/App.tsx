/// アプリのルート。タブ切替と全体状態の表示だけを担う。

import { useEffect } from "react";
import { useAppStore } from "./store/useAppStore";
import { Dashboard } from "./views/Dashboard";
import { SrtDashboard } from "./views/SrtDashboard";
import { PathsView } from "./views/PathsView";
import { ConnectionsView } from "./views/ConnectionsView";
import { OperationsView } from "./views/OperationsView";
import { SettingsView } from "./views/SettingsView";
import { formatTime } from "./lib/format";

const TABS = [
  { id: "srt", label: "SRT 品質" },
  { id: "dashboard", label: "ダッシュボード" },
  { id: "paths", label: "パス管理" },
  { id: "connections", label: "接続" },
  { id: "operations", label: "API 操作" },
  { id: "settings", label: "設定" },
] as const;

export function App() {
  const tab = useAppStore((s) => s.tab);
  const setTab = useAppStore((s) => s.setTab);
  const bootstrap = useAppStore((s) => s.bootstrap);
  const status = useAppStore((s) => s.status);
  const api = useAppStore((s) => s.api);
  const bridgeKind = useAppStore((s) => s.bridgeKind);
  const lastUpdateAt = useAppStore((s) => s.lastUpdateAt);
  const toasts = useAppStore((s) => s.toasts);
  const dismissToast = useAppStore((s) => s.dismissToast);
  const totals = useAppStore((s) => s.totals);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  const statusLabel = !status
    ? "未接続"
    : status.running
      ? api.ok
        ? "監視中"
        : "監視中（API 不可）"
      : "監視停止";

  return (
    <div className="app" data-testid="app-root">
      <header className="app-header">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true" />
          <span className="brand-name">MediaMTX Monitor</span>
          <span className="brand-sub">MediaMTX 監視</span>
        </div>
        <nav className="tabs" role="tablist" aria-label="画面">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              className={tab === t.id ? "active" : ""}
              onClick={() => setTab(t.id)}
              data-testid={`tab-${t.id}`}
            >
              {t.label}
            </button>
          ))}
        </nav>
        <div className="header-status">
          <span
            className={`pill ${api.ok && status?.running ? "pill-ok" : status?.running ? "pill-crit" : "pill-idle"}`}
            data-testid="header-state"
          >
            {statusLabel}
          </span>
          <span className="muted" data-testid="header-updated">
            更新 {formatTime(lastUpdateAt)}
          </span>
          {bridgeKind === "mock" ? <span className="pill pill-mock">モック</span> : null}
        </div>
      </header>

      <main className="app-main">
        {tab === "srt" ? <SrtDashboard /> : null}
        {tab === "dashboard" ? <Dashboard /> : null}
        {tab === "paths" ? <PathsView /> : null}
        {tab === "connections" ? <ConnectionsView /> : null}
        {tab === "operations" ? <OperationsView /> : null}
        {tab === "settings" ? <SettingsView /> : null}
      </main>

      <footer className="app-footer">
        <span>
          SRT {totals.srtConnCount} 本 / パス {totals.pathsOnline}/{totals.pathCount} 稼働
        </span>
        {status?.serverVersion ? <span>MediaMTX {status.serverVersion}</span> : null}
      </footer>

      <div className="toasts" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast toast-${t.kind}`} role="status" data-testid="toast">
            <span>{t.message}</span>
            <button type="button" className="toast-close" onClick={() => dismissToast(t.id)} aria-label="閉じる">
              ✕
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
