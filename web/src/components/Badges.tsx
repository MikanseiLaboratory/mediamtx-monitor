/// 状態バッジなどの小さな表示部品。

import type { PathSeverity, SrtSeverity } from "../types";
import { SEVERITY } from "../lib/metrics";

export function SeverityBadge({ severity, label }: { severity: SrtSeverity; label?: string }) {
  const def = SEVERITY[severity];
  return (
    <span className={`badge ${def.className}`} data-testid={`severity-${severity}`}>
      {label ?? def.label}
    </span>
  );
}

export function PathBadge({ severity }: { severity: PathSeverity }) {
  switch (severity) {
    case "healthy":
      return <span className="badge sev-ok">稼働</span>;
    case "degraded":
      return <span className="badge sev-warning">待機</span>;
    case "down":
    default:
      return <span className="badge sev-critical">停止</span>;
  }
}

export function ApiBadge({ ok, label }: { ok: boolean; label?: string }) {
  return (
    <span className={`badge ${ok ? "sev-ok" : "sev-critical"}`} data-testid="api-badge">
      <span className={`dot ${ok ? "dot-ok" : "dot-critical"}`} aria-hidden="true" />
      {label ?? (ok ? "API 接続 OK" : "API 接続不可")}
    </span>
  );
}

/** 数値と単位を大きく見せる KPI カード。0 と「データなし」を区別する。 */
export function Stat({
  label,
  value,
  unit,
  tone = "neutral",
  hint,
  testId,
}: {
  label: string;
  value: string;
  unit?: string;
  tone?: "neutral" | "ok" | "warn" | "crit";
  hint?: string;
  testId?: string;
}) {
  return (
    <div className={`stat stat-${tone}`} data-testid={testId}>
      <div className="stat-label">{label}</div>
      <div className="stat-value">
        {value}
        {unit ? <span className="stat-unit">{unit}</span> : null}
      </div>
      {hint ? <div className="stat-hint">{hint}</div> : null}
    </div>
  );
}

export function EmptyState({ title, hint }: { title: string; hint?: string }) {
  return (
    <div className="empty-state" role="status">
      <div className="empty-title">{title}</div>
      {hint ? <div className="empty-hint">{hint}</div> : null}
    </div>
  );
}

export function Spinner({ label = "読み込み中…" }: { label?: string }) {
  return (
    <span className="spinner" role="status" aria-label={label}>
      <span className="spinner-ring" aria-hidden="true" />
      {label}
    </span>
  );
}
