/// 接続・セッション一覧（SRT / RTSP / RTMP / WebRTC / HLS / MoQ）。

import { useEffect, useState } from "react";
import { api } from "../bridge";
import { useAppStore } from "../store/useAppStore";
import { formatBytes, formatTime } from "../lib/format";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EmptyState, Spinner } from "../components/Badges";
import type { ConnKind } from "../types";

const KINDS: Array<{ kind: ConnKind; label: string }> = [
  { kind: "srt", label: "SRT" },
  { kind: "rtsp", label: "RTSP" },
  { kind: "rtsps", label: "RTSPS" },
  { kind: "rtmp", label: "RTMP" },
  { kind: "rtmps", label: "RTMPS" },
  { kind: "web_rtc", label: "WebRTC" },
  { kind: "hls", label: "HLS" },
  { kind: "moq", label: "MoQ" },
];

export function ConnectionsView() {
  const connections = useAppStore((s) => s.connections);
  const loadConnections = useAppStore((s) => s.loadConnections);
  const pushToast = useAppStore((s) => s.pushToast);
  const [kind, setKind] = useState<ConnKind>("srt");
  const [pending, setPending] = useState<{ id: string; path: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void loadConnections(kind);
  }, [kind, loadConnections]);

  const kick = async () => {
    if (!pending) return;
    setBusy(true);
    setError(null);
    try {
      await api.kickConnection(kind, pending.id);
      pushToast("success", `接続を切断しました（path=${pending.path || "不明"}）。`);
      setPending(null);
      await loadConnections(kind);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="view" data-testid="connections-view">
      <div className="view-head">
        <h1>接続・セッション</h1>
        <div className="view-head-actions">
          <button type="button" className="btn" onClick={() => void loadConnections(kind)}>
            再読み込み
          </button>
        </div>
      </div>

      <div className="tabs" role="tablist">
        {KINDS.map((k) => (
          <button
            key={k.kind}
            type="button"
            role="tab"
            aria-selected={kind === k.kind}
            className={kind === k.kind ? "active" : ""}
            onClick={() => setKind(k.kind)}
          >
            {k.label}
          </button>
        ))}
      </div>

      {connections.loading ? <Spinner /> : null}
      {connections.error ? (
        <div className="banner banner-error" role="alert">
          {connections.error}
        </div>
      ) : null}

      {!connections.loading && !connections.error && connections.items.length === 0 ? (
        <EmptyState title="接続はありません" hint="このプロトコルの接続・セッションは現在ありません。" />
      ) : (
        <table className="table" data-testid="conn-table">
          <thead>
            <tr>
              <th>パス</th>
              <th>状態</th>
              <th>接続元</th>
              <th>開始</th>
              <th>受信</th>
              <th>送信</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {connections.items.map((c) => (
              <tr key={c.id}>
                <td>
                  <code>{c.path || "—"}</code>
                </td>
                <td>{c.state || "—"}</td>
                <td>{c.remoteAddr || "—"}</td>
                <td>{formatTime(c.created)}</td>
                <td>{formatBytes(c.inboundBytes)}</td>
                <td>{formatBytes(c.outboundBytes)}</td>
                <td className="row-actions">
                  <button
                    type="button"
                    className="btn btn-small btn-danger"
                    onClick={() => setPending({ id: c.id, path: c.path })}
                  >
                    切断
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <ConfirmDialog
        open={pending !== null}
        title="この接続を切断しますか？"
        target={pending?.path}
        destructive
        description="MediaMTX 側でこの接続を強制終了します。視聴者の再生が止まる可能性があります。"
        diff={pending ? `接続 ID: ${pending.id}` : undefined}
        confirmLabel="切断する"
        busy={busy}
        error={error}
        onConfirm={() => void kick()}
        onCancel={() => setPending(null)}
      />
    </section>
  );
}
