/// フロントの単体テスト。
///
/// UI の受け入れ（描画・ダイアログ）は Playwright（`e2e/`）が担当する。
/// ここでは jsdom でも確実に検証できる「純粋なロジック」——差分計算・
/// 表示フォーマット・ストアの差分適用——を対象にする。

import { beforeEach, describe, expect, it } from "vitest";
import { diffConf, formToConf } from "../views/PathsView";
import { globalDiff } from "../views/OperationsView";
import {
  formatBytes,
  formatDuration,
  formatPercent,
  formatTime,
  parseDateTimeLocal,
  toDateTimeLocal,
} from "../lib/format";
import { resolveMetric } from "../lib/metrics";
import { resetStoreForTest, useAppStore } from "../store/useAppStore";
import type { MonitorUpdate, PathSample } from "../types";

function pathSample(name: string): PathSample {
  return {
    name,
    confName: name,
    available: true,
    online: true,
    sourceType: "srtConn",
    readerCount: 0,
    codecs: [],
    inboundBytes: 0,
    outboundBytes: 0,
    inboundFramesInError: 0,
    inboundMbps: 1,
    outboundMbps: 0,
    framesInErrorPerSec: 0,
    onlineSince: null,
    availableSince: null,
    severity: "healthy",
    srtConnIds: [],
  };
}

function update(seq: number, names: string[], removed: string[] = []): MonitorUpdate {
  return {
    seq,
    at: new Date().toISOString(),
    api: {
      ok: true,
      rttMs: 1,
      error: null,
      consecutiveFailures: 0,
      metricsOk: true,
      metricsError: null,
    },
    totals: {
      pathCount: names.length,
      pathsOnline: names.length,
      pathsDown: 0,
      readerCount: 0,
      inboundMbps: 0,
      outboundMbps: 0,
      srtConnCount: 0,
      srtMaxDropRate: null,
      srtMinRecoveryRate: null,
      srtWorstSeverity: null,
    },
    paths: names.map((n) => ({ seq, sample: pathSample(n) })),
    srt: [],
    removedPaths: removed,
    removedSrt: [],
    events: [],
    serverVersion: "v1.21.2",
    full: seq === 1,
  };
}

describe("パス設定の差分", () => {
  it("フォームから空欄を除いた JSON を作る", () => {
    const conf = formToConf({
      name: "cam1",
      source: "  rtsp://cam/1 ",
      sourceOnDemand: true,
      alwaysAvailable: false,
      maxReaders: "5",
      record: false,
      recordPath: "",
      overridePublisher: false,
    });
    expect(conf).toEqual({
      source: "rtsp://cam/1",
      sourceOnDemand: true,
      alwaysAvailable: false,
      maxReaders: 5,
      record: false,
      overridePublisher: false,
    });
  });

  it("PATCH 用の差分には変更したキーだけが入る", () => {
    const before = { source: "rtsp://a", maxReaders: 3, record: false };
    const diff = diffConf(before, { source: "rtsp://a", maxReaders: 5, record: false });
    expect(diff).toEqual({ maxReaders: 5 });
  });

  it("グローバル設定の差分は API の camelCase 名で送る", () => {
    const before = { logLevel: "info", api: true, srtAddress: ":8890" };
    const after = { logLevel: "debug", api: true, srtAddress: "" };
    expect(globalDiff(before, after)).toEqual({ logLevel: "debug" });
  });
});

describe("表示フォーマット", () => {
  it("欠測は — を出し、0 とは区別する", () => {
    expect(formatPercent(null)).toBe("—");
    expect(formatPercent(0)).toBe("0.00%");
    expect(formatBytes(null)).toBe("—");
    expect(formatBytes(1536)).toBe("1.5 KB");
  });

  it("時刻と期間を読みやすい形にする", () => {
    const iso = new Date(2026, 9, 31, 11, 5, 9).toISOString();
    expect(formatTime(iso)).toBe("11:05:09");
    const now = new Date(2026, 9, 31, 12, 7, 30).getTime();
    expect(formatDuration(new Date(2026, 9, 31, 11, 5, 0).toISOString(), now)).toBe("1時間02分");
  });

  it("datetime-local の往復", () => {
    const local = toDateTimeLocal(new Date(2026, 9, 31, 11, 30).getTime());
    expect(local).toBe("2026-10-31T11:30");
    expect(parseDateTimeLocal(local)).toBe(new Date(2026, 9, 31, 11, 30).getTime());
    expect(parseDateTimeLocal("")).toBeNull();
  });

  it("メトリクス定義を解決する（未知キーでも落ちない）", () => {
    expect(resolveMetric("srt_drop_rate").unit).toBe("%");
    expect(resolveMetric("srt_recovery_rate").label).toBe("回復率");
    expect(resolveMetric("unknown_metric").label).toBe("unknown_metric");
  });
});

describe("ストア（バックエンドの差分をそのまま反映する）", () => {
  beforeEach(() => resetStoreForTest());

  it("追加・削除を差分どおりに適用する", () => {
    const store = useAppStore.getState();
    store.applyUpdate(update(1, ["cam1", "cam2"]));
    expect(Object.keys(useAppStore.getState().paths).sort()).toEqual(["cam1", "cam2"]);
    store.applyUpdate(update(2, ["cam1"], ["cam2"]));
    expect(Object.keys(useAppStore.getState().paths)).toEqual(["cam1"]);
    expect(useAppStore.getState().seq).toBe(2);
  });

  it("イベントは末尾に追記される", () => {
    const store = useAppStore.getState();
    const u = update(1, []);
    u.events = [
      {
        seq: 1,
        at: new Date().toISOString(),
        kind: "srt_conn_dropped",
        target: "conn-1",
        label: "cam1",
        message: "SRT 接続が切断されました",
        severity: "critical",
      },
    ];
    store.applyUpdate(u);
    expect(useAppStore.getState().events).toHaveLength(1);
    expect(useAppStore.getState().events[0].kind).toBe("srt_conn_dropped");
  });

  it("トーストは追記され、閉じると消える", () => {
    const store = useAppStore.getState();
    store.pushToast("error", "失敗しました");
    const toasts = useAppStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    store.dismissToast(toasts[0].id);
    expect(useAppStore.getState().toasts).toHaveLength(0);
  });
});
