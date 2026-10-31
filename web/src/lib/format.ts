/// 表示フォーマットのヘルパ。計算はバックエンドで済んでいる前提で、
/// ここでは「見やすくする」ことだけを行う。

export function formatNumber(value: number | null | undefined, digits = 2): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return value.toFixed(digits);
}

export function formatPercent(value: number | null | undefined, digits = 2): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return `${value.toFixed(digits)}%`;
}

export function formatMbps(value: number | null | undefined, digits = 2): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return `${value.toFixed(digits)} Mbps`;
}

export function formatMs(value: number | null | undefined, digits = 1): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return `${value.toFixed(digits)} ms`;
}

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes)) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

export function formatCount(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return new Intl.NumberFormat("ja-JP").format(Math.round(value));
}

/** RFC3339 → `HH:mm:ss`（ローカル）。 */
export function formatTime(iso: string | null | undefined, withDate = false): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  if (!withDate) return `${hh}:${mm}:${ss}`;
  const y = d.getFullYear();
  const mo = String(d.getMonth() + 1).padStart(2, "0");
  const da = String(d.getDate()).padStart(2, "0");
  return `${y}/${mo}/${da} ${hh}:${mm}:${ss}`;
}

/** 経過時間 `1時間02分` 形式。 */
export function formatDuration(fromIso: string | null | undefined, nowMs = Date.now()): string {
  if (!fromIso) return "—";
  const from = new Date(fromIso).getTime();
  if (Number.isNaN(from)) return "—";
  const secs = Math.max(0, Math.floor((nowMs - from) / 1000));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (h > 0) return `${h}時間${String(m).padStart(2, "0")}分`;
  if (m > 0) return `${m}分${String(s).padStart(2, "0")}秒`;
  return `${s}秒`;
}

/** `2026-10-31T11:30` (datetime-local 入力) → ms。 */
export function parseDateTimeLocal(value: string): number | null {
  if (!value) return null;
  const ms = new Date(value).getTime();
  return Number.isNaN(ms) ? null : ms;
}

/** ms → `2026-10-31T11:30` (datetime-local 入力)。 */
export function toDateTimeLocal(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** SRT の状態（read/publish/idle）の日本語ラベル。 */
export function srtStateLabel(state: string): string {
  switch (state) {
    case "read":
      return "視聴中 (read)";
    case "publish":
      return "配信中 (publish)";
    case "idle":
      return "待機 (idle)";
    default:
      return state || "不明";
  }
}
