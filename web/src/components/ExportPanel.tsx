/// 期間指定のグラフ書き出し。
///
/// 「特定期間のグラフだけ書き出したい」という用途に応えるため、
/// 開始/終了日時（datetime-local）を受け取り、CSV または SVG を
/// バックエンドへ依頼してファイルに書き出す。

import { useMemo, useState } from "react";
import { api } from "../bridge";
import { useAppStore } from "../store/useAppStore";
import { SERVER_METRICS, SRT_METRICS, WINDOWS } from "../lib/metrics";
import { parseDateTimeLocal, toDateTimeLocal } from "../lib/format";
import { ConfirmDialog } from "./ConfirmDialog";
import type { ExportResult } from "../types";

export interface ExportPanelProps {
  scope: string;
  title: string;
  defaultMetrics: string[];
  onClose: () => void;
}

const PRESETS = [
  { label: "直近 15 分", ms: 15 * 60_000 },
  { label: "直近 1 時間", ms: 60 * 60_000 },
  { label: "直近 4 時間", ms: 4 * 60 * 60_000 },
];

export function ExportPanel({ scope, title, defaultMetrics, onClose }: ExportPanelProps) {
  const now = Date.now();
  const [from, setFrom] = useState(toDateTimeLocal(now - 15 * 60_000));
  const [to, setTo] = useState(toDateTimeLocal(now));
  const [format, setFormat] = useState<"csv" | "svg">("csv");
  const [fileName, setFileName] = useState("");
  const [selected, setSelected] = useState<string[]>(defaultMetrics);
  const [preview, setPreview] = useState<ExportResult | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pushToast = useAppStore((s) => s.pushToast);

  const allMetrics = useMemo(
    () => (scope.startsWith("srt:") ? SRT_METRICS : SERVER_METRICS),
    [scope],
  );
  const available = useMemo(() => Object.values(allMetrics), [allMetrics]);

  const fromMs = parseDateTimeLocal(from);
  const toMs = parseDateTimeLocal(to);
  const valid = fromMs !== null && toMs !== null && toMs > fromMs && selected.length > 0;

  const toggle = (key: string) => {
    setSelected((prev) =>
      prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key],
    );
  };

  const applyPreset = (ms: number) => {
    const end = Date.now();
    setFrom(toDateTimeLocal(end - ms));
    setTo(toDateTimeLocal(end));
  };

  const requestExport = () => {
    if (!valid) return;
    setError(null);
    setConfirming(true);
  };

  const runExport = async () => {
    if (fromMs === null || toMs === null) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.exportRange({
        scope,
        metrics: selected,
        fromMs,
        toMs,
        format,
        fileName: fileName.trim() || null,
        title,
      });
      setPreview(result);
      setConfirming(false);
      pushToast("success", `${format.toUpperCase()} を書き出しました: ${result.path}`);
    } catch (err) {
      setError(String(err));
      pushToast("error", `書き出しに失敗しました: ${String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="panel export-panel" aria-label="グラフの書き出し" data-testid="export-panel">
      <div className="panel-head">
        <h2>期間を指定して書き出し</h2>
        <button type="button" className="btn btn-ghost" onClick={onClose} aria-label="閉じる">
          ✕
        </button>
      </div>

      <div className="export-grid">
        <label className="field">
          <span>開始</span>
          <input
            type="datetime-local"
            value={from}
            onChange={(e) => setFrom(e.target.value)}
            data-testid="export-from"
          />
        </label>
        <label className="field">
          <span>終了</span>
          <input
            type="datetime-local"
            value={to}
            onChange={(e) => setTo(e.target.value)}
            data-testid="export-to"
          />
        </label>
        <div className="field">
          <span>プリセット</span>
          <div className="chip-row">
            {PRESETS.map((p) => (
              <button key={p.label} type="button" className="chip" onClick={() => applyPreset(p.ms)}>
                {p.label}
              </button>
            ))}
            {WINDOWS.map((w) => (
              <button key={`w-${w.ms}`} type="button" className="chip" onClick={() => applyPreset(w.ms)}>
                {w.label}
              </button>
            ))}
          </div>
        </div>
        <label className="field">
          <span>形式</span>
          <select value={format} onChange={(e) => setFormat(e.target.value as "csv" | "svg")}>
            <option value="csv">CSV（表計算向け）</option>
            <option value="svg">SVG（図として保存）</option>
          </select>
        </label>
        <label className="field field-wide">
          <span>ファイル名（空なら自動）</span>
          <input
            type="text"
            value={fileName}
            placeholder={`mtm-${scope.replace(/[^a-z0-9]+/gi, "-")}-...`}
            onChange={(e) => setFileName(e.target.value)}
          />
        </label>
      </div>

      <fieldset className="metric-picker">
        <legend>書き出す計測値</legend>
        {available.map((m) => (
          <label key={m.key} className="check">
            <input
              type="checkbox"
              checked={selected.includes(m.key)}
              onChange={() => toggle(m.key)}
            />
            <span>
              {m.label}
              {m.unit ? <em> ({m.unit})</em> : null}
            </span>
          </label>
        ))}
      </fieldset>

      <div className="export-actions">
        <button type="button" className="btn btn-primary" onClick={requestExport} disabled={!valid}>
          書き出し
        </button>
        {!valid ? <span className="hint-error">期間と計測値を 1 つ以上指定してください。</span> : null}
      </div>

      {preview ? (
        <p className="export-result" data-testid="export-result">
          出力: <code>{preview.path}</code>（{preview.bytes.toLocaleString("ja-JP")} バイト）
        </p>
      ) : null}

      <ConfirmDialog
        open={confirming}
        title={`${format.toUpperCase()} を書き出しますか？`}
        target={scope}
        description={
          "指定した期間の計測値をファイルに保存します。MediaMTX の設定やデータは変更されません。"
        }
        diff={
          `期間: ${from} 〜 ${to}\n` +
          `計測値: ${selected.join(", ")}\n` +
          `形式: ${format.toUpperCase()}\n` +
          `保存先: 既定のドキュメントフォルダ`
        }
        confirmLabel="書き出す"
        busy={busy}
        error={error}
        onConfirm={runExport}
        onCancel={() => setConfirming(false)}
      />
    </section>
  );
}
