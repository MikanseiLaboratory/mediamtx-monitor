/// 変更系・作成系操作の確認ダイアログ。
///
/// このアプリでは **状態を変えるすべての操作**（パス追加/更新/削除、接続切断、
/// グローバル設定の変更、監視の開始/停止）でこのダイアログを必ず通す。
/// `window.confirm` は使わない（見た目の統一と、変更内容の diff を見せるため）。

import { useEffect, useRef } from "react";

export interface ConfirmDialogProps {
  open: boolean;
  title: string;
  /** 対象（パス名など）。 */
  target?: string;
  /** 何が起きるかの説明。 */
  description: string;
  /** 送信する変更内容のプレビュー（JSON など）。 */
  diff?: string;
  /** 破壊的な操作か（削除・切断など）。ボタンが赤くなる。 */
  destructive?: boolean;
  confirmLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
  busy?: boolean;
  error?: string | null;
}

export function ConfirmDialog({
  open,
  title,
  target,
  description,
  diff,
  destructive = false,
  confirmLabel = "実行する",
  onConfirm,
  onCancel,
  busy = false,
  error = null,
}: ConfirmDialogProps) {
  const confirmRef = useRef<HTMLButtonElement | null>(null);
  const cancelRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (!open) return;
    // 破壊的操作の既定フォーカスは「キャンセル」にする（誤操作防止）。
    (destructive ? cancelRef.current : confirmRef.current)?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, destructive, onCancel, busy]);

  if (!open) return null;

  return (
    <div className="dialog-backdrop" role="presentation" onMouseDown={(e) => e.target === e.currentTarget && !busy && onCancel()}>
      <div
        className="dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        aria-describedby="confirm-desc"
        data-testid="confirm-dialog"
      >
        <div className={`dialog-icon ${destructive ? "danger" : "warn"}`} aria-hidden="true">
          {destructive ? "!" : "?"}
        </div>
        <h2 id="confirm-title">{title}</h2>
        {target ? (
          <p className="dialog-target">
            対象: <code>{target}</code>
          </p>
        ) : null}
        <p id="confirm-desc" className="dialog-desc">
          {description}
        </p>
        {diff ? (
          <pre className="dialog-diff" aria-label="変更内容">
            {diff}
          </pre>
        ) : null}
        {error ? (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        ) : null}
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onCancel} disabled={busy} ref={cancelRef}>
            キャンセル
          </button>
          <button
            type="button"
            className={`btn ${destructive ? "btn-danger" : "btn-primary"}`}
            onClick={onConfirm}
            disabled={busy}
            ref={confirmRef}
          >
            {busy ? "実行中…" : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
