/// 設定（接続先・ポーリング・SRT しきい値・通知・表示）。
/// 保存はバックエンドが行い、監視中なら設定変更で監視が再起動される。

import { useEffect, useState } from "react";
import { api } from "../bridge";
import { useAppStore } from "../store/useAppStore";
import { ConfirmDialog } from "../components/ConfirmDialog";
import type { AppConfig, AuthMode } from "../types";

const AUTH_MODES: Array<{ value: AuthMode; label: string }> = [
  { value: "none", label: "なし" },
  { value: "basic", label: "Basic 認証" },
  { value: "bearer", label: "Bearer トークン" },
];

export function SettingsView() {
  const config = useAppStore((s) => s.config);
  const setConfig = useAppStore((s) => s.setConfig);
  const pushToast = useAppStore((s) => s.pushToast);
  const status = useAppStore((s) => s.status);
  const [draft, setDraft] = useState<AppConfig | null>(config);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [probe, setProbe] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    setDraft(config);
  }, [config]);

  const update = (patch: Partial<AppConfig>) => {
    if (!draft) return;
    setDraft({ ...draft, ...patch });
  };

  const updateEndpoint = (patch: Partial<AppConfig["endpoint"]>) => {
    if (!draft) return;
    setDraft({ ...draft, endpoint: { ...draft.endpoint, ...patch } });
  };

  const updateSrt = (patch: Partial<AppConfig["srt"]>) => {
    if (!draft) return;
    setDraft({ ...draft, srt: { ...draft.srt, ...patch } });
  };

  const updateNotify = (patch: Partial<AppConfig["notify"]>) => {
    if (!draft) return;
    setDraft({ ...draft, notify: { ...draft.notify, ...patch } });
  };

  const test = async () => {
    if (!draft) return;
    setTesting(true);
    setProbe(null);
    try {
      const result = await api.testConnection(draft);
      const metrics =
        result.metricsSamples !== null
          ? `メトリクス ${result.metricsSamples} サンプル取得`
          : `メトリクス取得不可: ${result.metricsError ?? "理由不明"}`;
      setProbe(`接続成功: MediaMTX ${result.version} / API 応答 ${result.apiRttMs.toFixed(1)}ms / ${metrics}`);
    } catch (err) {
      setProbe(`接続できません: ${String(err)}`);
    } finally {
      setTesting(false);
    }
  };

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      const saved = await api.saveSettings(draft);
      setConfig(saved);
      setConfirming(false);
      pushToast("success", "設定を保存しました。");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const startStop = async () => {
    try {
      if (status?.running) {
        await api.stopMonitor();
        pushToast("info", "監視を停止しました。");
      } else {
        await api.startMonitor();
        pushToast("success", "監視を開始しました。");
      }
    } catch (err) {
      pushToast("error", `監視の切り替えに失敗しました: ${String(err)}`);
    }
  };

  if (!draft) {
    return (
      <section className="view" data-testid="settings-view">
        <p className="muted">設定を読み込み中です…</p>
      </section>
    );
  }

  return (
    <section className="view" data-testid="settings-view">
      <div className="view-head">
        <h1>設定</h1>
        <div className="view-head-actions">
          <button type="button" className="btn" onClick={() => setDraft(config)}>
            変更を破棄
          </button>
          <button type="button" className="btn" onClick={() => void startStop()}>
            {status?.running ? "監視を停止" : "監視を開始"}
          </button>
          <button type="button" className="btn btn-primary" onClick={() => setConfirming(true)}>
            保存
          </button>
        </div>
      </div>

      <div className="panel">
        <div className="panel-head">
          <h2>接続先</h2>
        </div>
        <div className="form-grid">
          <label className="field field-wide">
            <span>MediaMTX API の URL</span>
            <input
              type="text"
              value={draft.endpoint.baseUrl}
              onChange={(e) => updateEndpoint({ baseUrl: e.target.value })}
              placeholder="http://127.0.0.1:9997"
              data-testid="settings-base-url"
            />
          </label>
          <label className="field field-wide">
            <span>メトリクス URL（空なら API のポートを 9998 に置換）</span>
            <input
              type="text"
              value={draft.endpoint.metricsUrl ?? ""}
              onChange={(e) => updateEndpoint({ metricsUrl: e.target.value || null })}
              placeholder="http://127.0.0.1:9998"
            />
          </label>
          <label className="field">
            <span>認証</span>
            <select
              value={draft.endpoint.auth}
              onChange={(e) => updateEndpoint({ auth: e.target.value as AuthMode })}
            >
              {AUTH_MODES.map((m) => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </select>
          </label>
          {draft.endpoint.auth === "basic" ? (
            <>
              <label className="field">
                <span>ユーザー名</span>
                <input
                  type="text"
                  value={draft.endpoint.user}
                  onChange={(e) => updateEndpoint({ user: e.target.value })}
                />
              </label>
              <label className="field">
                <span>パスワード（保存されません）</span>
                <input
                  type="password"
                  value={draft.endpoint.password}
                  onChange={(e) => updateEndpoint({ password: e.target.value })}
                />
              </label>
            </>
          ) : null}
          {draft.endpoint.auth === "bearer" ? (
            <label className="field field-wide">
              <span>トークン（保存されません）</span>
              <input
                type="password"
                value={draft.endpoint.token}
                onChange={(e) => updateEndpoint({ token: e.target.value })}
              />
            </label>
          ) : null}
          <label className="check">
            <input
              type="checkbox"
              checked={draft.endpoint.allowInsecureTls}
              onChange={(e) => updateEndpoint({ allowInsecureTls: e.target.checked })}
            />
            <span>TLS 証明書の検証を省略する（自己署名用）</span>
          </label>
        </div>
        <div className="form-actions">
          <button type="button" className="btn" onClick={() => void test()} disabled={testing}>
            {testing ? "確認中…" : "接続を確認"}
          </button>
          {probe ? (
            <span className={probe.startsWith("接続成功") ? "ok-text" : "warn-text"} data-testid="probe-result">
              {probe}
            </span>
          ) : null}
        </div>
      </div>

      <div className="panel">
        <div className="panel-head">
          <h2>ポーリングと履歴</h2>
        </div>
        <div className="form-grid">
          <label className="field">
            <span>取得間隔: {(draft.pollIntervalMs / 1000).toFixed(0)} 秒</span>
            <input
              type="range"
              min={1000}
              max={30000}
              step={1000}
              value={draft.pollIntervalMs}
              onChange={(e) => update({ pollIntervalMs: Number(e.target.value) })}
            />
          </label>
          <label className="field">
            <span>タイムアウト: {draft.requestTimeoutMs} ms</span>
            <input
              type="range"
              min={500}
              max={30000}
              step={100}
              value={draft.requestTimeoutMs}
              onChange={(e) => update({ requestTimeoutMs: Number(e.target.value) })}
            />
          </label>
          <label className="field">
            <span>履歴保持: {draft.historyHours} 時間</span>
            <input
              type="range"
              min={1}
              max={72}
              step={1}
              value={draft.historyHours}
              onChange={(e) => update({ historyHours: Number(e.target.value) })}
            />
          </label>
          <label className="field">
            <span>連続失敗で「API 不可」: {draft.failureThreshold} 回</span>
            <input
              type="range"
              min={1}
              max={10}
              step={1}
              value={draft.failureThreshold}
              onChange={(e) => update({ failureThreshold: Number(e.target.value) })}
            />
          </label>
        </div>
      </div>

      <div className="panel">
        <div className="panel-head">
          <h2>SRT 品質のしきい値</h2>
          <span className="muted">グラフの色と通知の判定に使います。</span>
        </div>
        <div className="form-grid">
          <label className="field">
            <span>ドロップ率 注意: {draft.srt.dropRateWarn}%</span>
            <input
              type="range"
              min={0}
              max={10}
              step={0.1}
              value={draft.srt.dropRateWarn}
              onChange={(e) => updateSrt({ dropRateWarn: Number(e.target.value) })}
            />
          </label>
          <label className="field">
            <span>ドロップ率 劣化: {draft.srt.dropRateCrit}%</span>
            <input
              type="range"
              min={0}
              max={20}
              step={0.1}
              value={draft.srt.dropRateCrit}
              onChange={(e) => updateSrt({ dropRateCrit: Number(e.target.value) })}
            />
          </label>
          <label className="field">
            <span>回復率の下限: {draft.srt.recoveryRateWarn}%</span>
            <input
              type="range"
              min={0}
              max={100}
              step={1}
              value={draft.srt.recoveryRateWarn}
              onChange={(e) => updateSrt({ recoveryRateWarn: Number(e.target.value) })}
            />
          </label>
          <label className="field">
            <span>RTT 警告: {draft.srt.rttMsWarn} ms</span>
            <input
              type="range"
              min={10}
              max={2000}
              step={10}
              value={draft.srt.rttMsWarn}
              onChange={(e) => updateSrt({ rttMsWarn: Number(e.target.value) })}
            />
          </label>
        </div>
      </div>

      <div className="panel">
        <div className="panel-head">
          <h2>通知</h2>
        </div>
        <div className="check-row">
          <label className="check">
            <input
              type="checkbox"
              checked={draft.notify.enabled}
              onChange={(e) => updateNotify({ enabled: e.target.checked })}
            />
            <span>通知を有効にする</span>
          </label>
          {(
            [
              ["srtConnDropped", "SRT 接続の切断"],
              ["srtDegraded", "SRT 品質の劣化"],
              ["pathDown", "配信の停止"],
              ["apiUnreachable", "MediaMTX に接続できない"],
            ] as const
          ).map(([key, label]) => (
            <label key={key} className="check">
              <input
                type="checkbox"
                checked={draft.notify[key]}
                disabled={!draft.notify.enabled}
                onChange={(e) => updateNotify({ [key]: e.target.checked } as never)}
              />
              <span>{label}</span>
            </label>
          ))}
        </div>
      </div>

      <div className="panel">
        <div className="panel-head">
          <h2>動作</h2>
        </div>
        <div className="check-row">
          <label className="check">
            <input
              type="checkbox"
              checked={draft.minimizeToTray}
              onChange={(e) => update({ minimizeToTray: e.target.checked })}
            />
            <span>ウィンドウを閉じたらトレイに常駐する</span>
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.autoStart}
              onChange={(e) => update({ autoStart: e.target.checked })}
            />
            <span>起動時に監視を自動開始する</span>
          </label>
        </div>
        <p className="muted">
          認証情報（パスワード・トークン）はディスクに保存されません。再起動後に再入力してください。
        </p>
      </div>

      <ConfirmDialog
        open={confirming}
        title="設定を保存しますか？"
        description={
          status?.running
            ? "設定を保存し、監視を新しい設定で再起動します。収集中の短い区間（数秒）が欠測になります。"
            : "設定を保存します。"
        }
        diff={JSON.stringify(
          {
            baseUrl: draft.endpoint.baseUrl,
            metricsUrl: draft.endpoint.metricsUrl,
            auth: draft.endpoint.auth,
            pollIntervalMs: draft.pollIntervalMs,
            historyHours: draft.historyHours,
            srt: draft.srt,
            notify: draft.notify,
            minimizeToTray: draft.minimizeToTray,
            autoStart: draft.autoStart,
          },
          null,
          2,
        )}
        confirmLabel="保存する"
        busy={busy}
        error={error}
        onConfirm={() => void save()}
        onCancel={() => setConfirming(false)}
      />
    </section>
  );
}
