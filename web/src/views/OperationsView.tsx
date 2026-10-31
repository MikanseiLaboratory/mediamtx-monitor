/// API 操作（グローバル設定の表示・変更と、任意エンドポイントの GET コンソール）。
/// 変更系はすべて確認ダイアログを挟む。

import { useEffect, useMemo, useState } from "react";
import { api } from "../bridge";
import { useAppStore } from "../store/useAppStore";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Spinner } from "../components/Badges";

interface GlobalForm {
  api: boolean;
  apiAddress: string;
  metrics: boolean;
  metricsAddress: string;
  logLevel: string;
  rtsp: boolean;
  rtspAddress: string;
  rtmp: boolean;
  rtmpAddress: string;
  srt: boolean;
  srtAddress: string;
  hls: boolean;
  hlsAddress: string;
  webrtc: boolean;
  webrtcAddress: string;
}

const FIELDS: Array<{ key: keyof GlobalForm; label: string; type: "bool" | "text" | "select"; options?: string[] }> = [
  { key: "api", label: "Control API", type: "bool" },
  { key: "apiAddress", label: "API アドレス", type: "text" },
  { key: "metrics", label: "メトリクス", type: "bool" },
  { key: "metricsAddress", label: "メトリクス アドレス", type: "text" },
  { key: "logLevel", label: "ログレベル", type: "select", options: ["error", "warn", "info", "debug"] },
  { key: "rtsp", label: "RTSP サーバー", type: "bool" },
  { key: "rtspAddress", label: "RTSP アドレス", type: "text" },
  { key: "rtmp", label: "RTMP サーバー", type: "bool" },
  { key: "rtmpAddress", label: "RTMP アドレス", type: "text" },
  { key: "srt", label: "SRT サーバー", type: "bool" },
  { key: "srtAddress", label: "SRT アドレス", type: "text" },
  { key: "hls", label: "HLS サーバー", type: "bool" },
  { key: "hlsAddress", label: "HLS アドレス", type: "text" },
  { key: "webrtc", label: "WebRTC サーバー", type: "bool" },
  { key: "webrtcAddress", label: "WebRTC アドレス", type: "text" },
];

/** 送信する差分（変更のあったキーだけ、API の camelCase 名で送る）。 */
export function globalDiff(before: Record<string, unknown>, after: Record<string, unknown>): Record<string, unknown> {
  const diff: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(after)) {
    if (value === undefined || value === "") continue;
    if (JSON.stringify(before[key]) !== JSON.stringify(value)) diff[key] = value;
  }
  return diff;
}

export function OperationsView() {
  const pushToast = useAppStore((s) => s.pushToast);
  const [raw, setRaw] = useState<Record<string, unknown> | null>(null);
  const [form, setForm] = useState<GlobalForm | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [patch, setPatch] = useState<Record<string, unknown> | null>(null);
  const [busy, setBusy] = useState(false);
  const [patchError, setPatchError] = useState<string | null>(null);
  const [consolePath, setConsolePath] = useState("/v3/paths/list");
  const [consoleResult, setConsoleResult] = useState<string>("");

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const conf = await api.getGlobalConfig();
      setRaw(conf);
      // 取得できなかったキーは「触らない」扱いにするため、フォームの値を
      // undefined にして送らない（空文字を PATCH して設定を消さないため）。
      const str = (key: string, fallback = "") =>
        typeof conf[key] === "string" ? String(conf[key]) : fallback;
      const bool = (key: string) =>
        typeof conf[key] === "boolean" ? Boolean(conf[key]) : false;
      setForm({
        api: bool("api"),
        apiAddress: str("apiAddress"),
        metrics: bool("metrics"),
        metricsAddress: str("metricsAddress"),
        logLevel: str("logLevel", "info"),
        rtsp: bool("rtsp"),
        rtspAddress: str("rtspAddress"),
        rtmp: bool("rtmp"),
        rtmpAddress: str("rtmpAddress"),
        srt: bool("srt"),
        srtAddress: str("srtAddress"),
        hls: bool("hls"),
        hlsAddress: str("hlsAddress"),
        webrtc: bool("webrtc"),
        webrtcAddress: str("webrtcAddress"),
      });
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const diff = useMemo(() => (raw && form ? globalDiff(raw, { ...form }) : {}), [raw, form]);

  const requestPatch = () => {
    if (!raw || !form) return;
    if (Object.keys(diff).length === 0) {
      pushToast("info", "変更項目がありません。");
      return;
    }
    setPatchError(null);
    setPatch({ ...diff });
  };

  const applyPatch = async () => {
    if (!patch) return;
    setBusy(true);
    setPatchError(null);
    try {
      await api.patchGlobalConfig(patch);
      pushToast("success", "グローバル設定を更新しました。");
      setPatch(null);
      await load();
    } catch (err) {
      setPatchError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const runConsole = async () => {
    try {
      const [status, body] = await api.consoleGet(consolePath.trim());
      setConsoleResult(`HTTP ${status}\n${JSON.stringify(body, null, 2)}`);
    } catch (err) {
      setConsoleResult(`エラー: ${String(err)}`);
    }
  };

  return (
    <section className="view" data-testid="operations-view">
      <div className="view-head">
        <h1>API 操作</h1>
        <div className="view-head-actions">
          <button type="button" className="btn" onClick={() => void load()} disabled={loading}>
            再読み込み
          </button>
        </div>
      </div>

      {error ? (
        <div className="banner banner-error" role="alert">
          {error}
        </div>
      ) : null}
      {loading ? <Spinner /> : null}

      {form ? (
        <form
          className="panel"
          onSubmit={(e) => {
            e.preventDefault();
            requestPatch();
          }}
        >
          <div className="panel-head">
            <h2>グローバル設定</h2>
            <span className="muted">変更した項目だけを PATCH で送ります（初期状態から未取得の項目が {Object.keys(diff).length} 件あります）。</span>
          </div>
          <div className="form-grid">
            {FIELDS.map((f) => {
              const value = form[f.key];
              return f.type === "bool" ? (
                <label key={f.key} className="check">
                  <input
                    type="checkbox"
                    checked={Boolean(value)}
                    onChange={(e) => setForm({ ...form, [f.key]: e.target.checked })}
                  />
                  <span>{f.label}</span>
                </label>
              ) : f.type === "select" ? (
                <label key={f.key} className="field">
                  <span>{f.label}</span>
                  <select
                    value={String(value)}
                    onChange={(e) => setForm({ ...form, [f.key]: e.target.value })}
                  >
                    {f.options!.map((o) => (
                      <option key={o} value={o}>
                        {o}
                      </option>
                    ))}
                  </select>
                </label>
              ) : (
                <label key={f.key} className="field">
                  <span>{f.label}</span>
                  <input
                    type="text"
                    value={String(value)}
                    onChange={(e) => setForm({ ...form, [f.key]: e.target.value })}
                  />
                </label>
              );
            })}
          </div>
          <div className="form-actions">
            <button type="submit" className="btn btn-primary" disabled={Object.keys(diff).length === 0}>
              変更を確認（{Object.keys(diff).length} 件）
            </button>
          </div>
        </form>
      ) : null}

      <div className="panel">
        <div className="panel-head">
          <h2>GET コンソール</h2>
          <span className="muted">読み取り専用（`/v3/...` のみ）</span>
        </div>
        <div className="console-row">
          <input
            type="text"
            className="mono"
            value={consolePath}
            onChange={(e) => setConsolePath(e.target.value)}
            aria-label="リクエストパス"
            data-testid="console-path"
          />
          <button type="button" className="btn" onClick={() => void runConsole()}>
            送信
          </button>
        </div>
        <pre className="console-out" data-testid="console-output">
          {consoleResult || "結果はここに表示されます。"}
        </pre>
      </div>

      <ConfirmDialog
        open={patch !== null}
        title="グローバル設定を変更しますか？"
        description="MediaMTX のグローバル設定を部分的に更新します。サーバーやアドレスを切り替えると、接続中の配信や視聴が切断される可能性があります。"
        diff={patch ? JSON.stringify(patch, null, 2) : undefined}
        confirmLabel="適用する"
        busy={busy}
        error={patchError}
        onConfirm={() => void applyPatch()}
        onCancel={() => setPatch(null)}
      />
    </section>
  );
}
