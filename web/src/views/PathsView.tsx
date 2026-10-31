/// パス管理（MediaMTX のパス設定に対する作成・更新・削除）。
///
/// 作成/更新/削除は **必ず確認ダイアログ** を通してから実行する。
/// フォームは主要項目のみを扱い、上級者向けに JSON を直接編集する
/// 「詳細（JSON）」タブも用意する。

import { useEffect, useMemo, useState } from "react";
import { api } from "../bridge";
import { useAppStore } from "../store/useAppStore";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EmptyState, Spinner } from "../components/Badges";

type Mode = "list" | "create" | "edit";

interface FormState {
  name: string;
  source: string;
  sourceOnDemand: boolean;
  alwaysAvailable: boolean;
  maxReaders: string;
  record: boolean;
  recordPath: string;
  overridePublisher: boolean;
}

const EMPTY_FORM: FormState = {
  name: "",
  source: "",
  sourceOnDemand: false,
  alwaysAvailable: false,
  maxReaders: "",
  record: false,
  recordPath: "",
  overridePublisher: false,
};

/** フォーム → API に送る JSON（空欄は送らない）。 */
export function formToConf(form: FormState): Record<string, unknown> {
  const conf: Record<string, unknown> = {};
  if (form.source.trim()) conf.source = form.source.trim();
  conf.sourceOnDemand = form.sourceOnDemand;
  conf.alwaysAvailable = form.alwaysAvailable;
  if (form.maxReaders.trim()) {
    const n = Number(form.maxReaders);
    if (Number.isFinite(n) && n >= 0) conf.maxReaders = Math.trunc(n);
  }
  conf.record = form.record;
  if (form.recordPath.trim()) conf.recordPath = form.recordPath.trim();
  conf.overridePublisher = form.overridePublisher;
  return conf;
}

/** 既存差分（PATCH 用）をフォームの値と比べて作る。 */
export function diffConf(
  before: Record<string, unknown>,
  after: Record<string, unknown>,
): Record<string, unknown> {
  const diff: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(after)) {
    if (JSON.stringify(before[key]) !== JSON.stringify(value)) diff[key] = value;
  }
  return diff;
}

export function confToForm(name: string, conf: Record<string, unknown>): FormState {
  const str = (v: unknown, fallback = "") => (typeof v === "string" ? v : fallback);
  return {
    name,
    source: str(conf.source),
    sourceOnDemand: Boolean(conf.sourceOnDemand),
    alwaysAvailable: Boolean(conf.alwaysAvailable),
    maxReaders: typeof conf.maxReaders === "number" ? String(conf.maxReaders) : "",
    record: Boolean(conf.record),
    recordPath: str(conf.recordPath),
    overridePublisher: Boolean(conf.overridePublisher),
  };
}

export function PathsView() {
  const pushToast = useAppStore((s) => s.pushToast);
  const paths = useAppStore((s) => s.paths);
  const [mode, setMode] = useState<Mode>("list");
  const [rows, setRows] = useState<Record<string, unknown>[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [advanced, setAdvanced] = useState(false);
  const [rawJson, setRawJson] = useState("{}");
  const [original, setOriginal] = useState<Record<string, unknown>>({});
  const [dialog, setDialog] = useState<null | { kind: "add" | "patch" | "delete"; name: string; body: Record<string, unknown> }>(null);
  const [busy, setBusy] = useState(false);
  const [dialogError, setDialogError] = useState<string | null>(null);

  const reload = async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await api.listPaths("config");
      setRows(list);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  const runtimeNames = useMemo(() => new Set(Object.keys(paths)), [paths]);

  const startCreate = () => {
    setForm(EMPTY_FORM);
    setRawJson("{\n  \"source\": \"\",\n  \"sourceOnDemand\": false,\n  \"alwaysAvailable\": false\n}");
    setAdvanced(false);
    setMode("create");
  };

  const startEdit = async (name: string) => {
    try {
      const conf = await api.getPathConf(name);
      setOriginal(conf);
      setForm(confToForm(name, conf));
      setRawJson(JSON.stringify(conf, null, 2));
      setAdvanced(false);
      setMode("edit");
    } catch (err) {
      pushToast("error", `設定を取得できませんでした: ${String(err)}`);
    }
  };

  const askDelete = (name: string) => {
    setDialogError(null);
    setDialog({ kind: "delete", name, body: {} });
  };

  const askSubmit = () => {
    if (!form.name.trim()) {
      pushToast("error", "パス名を入力してください。");
      return;
    }
    setDialogError(null);
    if (advanced) {
      try {
        const parsed = JSON.parse(rawJson) as Record<string, unknown>;
        if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("オブジェクトではありません");
        setDialog({ kind: mode === "create" ? "add" : "patch", name: form.name.trim(), body: parsed });
      } catch (err) {
        pushToast("error", `JSON が不正です: ${String(err)}`);
      }
      return;
    }
    const after = formToConf(form);
    if (mode === "create") {
      setDialog({ kind: "add", name: form.name.trim(), body: after });
    } else {
      const diff = diffConf(original, after);
      if (Object.keys(diff).length === 0) {
        pushToast("info", "変更項目がありません。");
        return;
      }
      setDialog({ kind: "patch", name: form.name.trim(), body: diff });
    }
  };

  const execute = async () => {
    if (!dialog) return;
    setBusy(true);
    setDialogError(null);
    try {
      if (dialog.kind === "add") await api.addPath(dialog.name, dialog.body);
      if (dialog.kind === "patch") await api.patchPath(dialog.name, dialog.body);
      if (dialog.kind === "delete") await api.deletePath(dialog.name);
      pushToast(
        "success",
        dialog.kind === "add"
          ? `パス「${dialog.name}」を作成しました。`
          : dialog.kind === "patch"
            ? `パス「${dialog.name}」を更新しました。`
            : `パス「${dialog.name}」を削除しました。`,
      );
      setDialog(null);
      setMode("list");
      await reload();
    } catch (err) {
      setDialogError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="view" data-testid="paths-view">
      <div className="view-head">
        <h1>パス管理</h1>
        <div className="view-head-actions">
          <button type="button" className="btn" onClick={() => void reload()} disabled={loading}>
            再読み込み
          </button>
          <button type="button" className="btn btn-primary" onClick={startCreate}>
            パスを追加
          </button>
        </div>
      </div>

      {error ? (
        <div className="banner banner-error" role="alert">
          {error}
        </div>
      ) : null}

      {mode === "list" ? (
        loading && rows.length === 0 ? (
          <Spinner />
        ) : rows.length === 0 ? (
          <EmptyState
            title="設定されたパスがありません"
            hint="「パスを追加」から `source` などを指定して作成できます。"
          />
        ) : (
          <table className="table" data-testid="path-table">
            <thead>
              <tr>
                <th>パス名</th>
                <th>ソース</th>
                <th>オンデマンド</th>
                <th>常時利用可能</th>
                <th>録画</th>
                <th>稼働状態</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const name = typeof row.name === "string" ? row.name : "";
                return (
                  <tr key={name}>
                    <td>
                      <code>{name}</code>
                    </td>
                    <td className="truncate">{typeof row.source === "string" && row.source ? row.source : "—"}</td>
                    <td>{row.sourceOnDemand ? "はい" : "いいえ"}</td>
                    <td>{row.alwaysAvailable ? "はい" : "いいえ"}</td>
                    <td>{row.record ? "はい" : "いいえ"}</td>
                    <td>{runtimeNames.has(name) ? "実行中" : "待機（未接続）"}</td>
                    <td className="row-actions">
                      <button type="button" className="btn btn-small" onClick={() => void startEdit(name)}>
                        編集
                      </button>
                      <button type="button" className="btn btn-small btn-danger" onClick={() => askDelete(name)}>
                        削除
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )
      ) : (
        <form
          className="panel path-form"
          onSubmit={(e) => {
            e.preventDefault();
            askSubmit();
          }}
        >
          <div className="panel-head">
            <h2>{mode === "create" ? "パスを作成" : `パスを編集: ${form.name}`}</h2>
            <button type="button" className="btn btn-ghost" onClick={() => setMode("list")}>
              戻る
            </button>
          </div>

          <div className="form-grid">
            <label className="field">
              <span>パス名 *</span>
              <input
                type="text"
                value={form.name}
                disabled={mode === "edit"}
                placeholder="cam1"
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                data-testid="path-name"
              />
            </label>
            <label className="field field-wide">
              <span>source（例: srt://0.0.0.0:8890?mode=listener, rtsp://... ）</span>
              <input
                type="text"
                value={form.source}
                placeholder="rtsp://camera.local/stream"
                onChange={(e) => setForm({ ...form, source: e.target.value })}
                data-testid="path-source"
              />
            </label>
            <label className="field">
              <span>maxReaders</span>
              <input
                type="number"
                min={0}
                value={form.maxReaders}
                placeholder="0（無制限）"
                onChange={(e) => setForm({ ...form, maxReaders: e.target.value })}
              />
            </label>
            <label className="field field-wide">
              <span>recordPath</span>
              <input
                type="text"
                value={form.recordPath}
                placeholder="./recordings/%path/%Y-%m-%d_%H-%M-%S-%f"
                onChange={(e) => setForm({ ...form, recordPath: e.target.value })}
              />
            </label>
          </div>

          <div className="check-row">
            <label className="check">
              <input
                type="checkbox"
                checked={form.sourceOnDemand}
                onChange={(e) => setForm({ ...form, sourceOnDemand: e.target.checked })}
              />
              <span>sourceOnDemand（視聴時のみ接続）</span>
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={form.alwaysAvailable}
                onChange={(e) => setForm({ ...form, alwaysAvailable: e.target.checked })}
              />
              <span>alwaysAvailable（配信が無くても常に利用可能）</span>
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={form.record}
                onChange={(e) => setForm({ ...form, record: e.target.checked })}
              />
              <span>録画する</span>
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={form.overridePublisher}
                onChange={(e) => setForm({ ...form, overridePublisher: e.target.checked })}
              />
              <span>配信者の乗っ取りを許可</span>
            </label>
          </div>

          <details className="advanced" open={advanced} onToggle={(e) => setAdvanced(e.currentTarget.open)}>
            <summary>詳細（JSON を直接編集）</summary>
            <textarea
              className="mono"
              rows={12}
              value={rawJson}
              onChange={(e) => setRawJson(e.target.value)}
              spellCheck={false}
            />
            <p className="muted">
              ここを開いている間は、フォームではなくこの JSON がそのまま API に送られます。
            </p>
          </details>

          <div className="form-actions">
            <button type="submit" className="btn btn-primary">
              {mode === "create" ? "作成内容を確認" : "変更を確認"}
            </button>
          </div>
        </form>
      )}

      <ConfirmDialog
        open={dialog !== null}
        title={
          dialog?.kind === "delete"
            ? "パスを削除しますか？"
            : dialog?.kind === "add"
              ? "パスを作成しますか？"
              : "パスの設定を変更しますか？"
        }
        target={dialog?.name}
        destructive={dialog?.kind === "delete"}
        description={
          dialog?.kind === "delete"
            ? "MediaMTX の設定からこのパスを削除します。配信中の場合は接続が切れます。この操作は取り消せません。"
            : dialog?.kind === "add"
              ? "MediaMTX の設定に新しいパスを追加します。設定は実行中のサーバーに即時反映されます。"
              : "MediaMTX の設定を部分的に更新します。配信中の接続に影響する可能性があります。"
        }
        diff={dialog && dialog.kind !== "delete" ? JSON.stringify(dialog.body, null, 2) : undefined}
        confirmLabel={dialog?.kind === "delete" ? "削除する" : "実行する"}
        busy={busy}
        error={dialogError}
        onConfirm={() => void execute()}
        onCancel={() => setDialog(null)}
      />
    </section>
  );
}
