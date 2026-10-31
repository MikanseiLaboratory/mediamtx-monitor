/// SRT ダッシュボードが実際に描画できることのテスト。
///
/// 画面全体の見た目は E2E が担当するが、「接続が 0 件の状態と 1 件以上の状態を
/// 行き来できるか」は、フックの呼び出し順が壊れると React がエラーを出して
/// 画面ごと真っ白になるため、ここでも検証する。

import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SrtDashboard } from "../views/SrtDashboard";
import { resetStoreForTest, useAppStore } from "../store/useAppStore";
import { __setBridgeForTest } from "../bridge";
import type { MonitorUpdate, SrtConnSample } from "../types";

// uPlot は jsdom で描画できない（canvas / Path2D が無い）。
// ここで見たいのは「画面が組み上がるか」「フックの呼び出し順が壊れないか」なので、
// グラフは差し替えて描画処理を通さない。
vi.mock("../components/TimeSeries", () => ({
  TimeSeries: ({ metric }: { metric: { key: string; label: string } }) => (
    <div data-testid={`timeseries-${metric.key}`}>{metric.label}</div>
  ),
}));

function srtSample(id: string, severity: SrtConnSample["severity"]): SrtConnSample {
  return {
    id,
    path: id,
    remoteAddr: "127.0.0.1:1234",
    state: "read",
    created: null,
    counters: {
      packetsReceived: null,
      packetsReceivedUnique: null,
      packetsReceivedDrop: null,
      packetsReceivedRetrans: null,
      packetsReceivedLoss: null,
      packetsReceivedBelated: null,
      packetsReceivedUndecrypt: null,
      packetsSent: null,
      packetsSentUnique: null,
      packetsRetrans: null,
      packetsSendDrop: null,
      packetsSendLoss: null,
      packetsSentNak: null,
      packetsReceivedNak: null,
      msRtt: 5,
      mbpsReceiveRate: 10,
      mbpsSendRate: 0,
      mbpsLinkCapacity: 40,
      usPacketsSendPeriod: 1000,
      msReceiveBuf: 100,
      msSendBuf: null,
      packetsFlightSize: null,
      packetsFlowWindow: null,
      outboundFramesDiscarded: null,
    },
    derived: {
      intervalMs: 0,
      recvPackets: null,
      recoveredPackets: null,
      lossPackets: null,
      dropPackets: null,
      undecryptPackets: null,
      belatedPackets: null,
      dropRate: 0.1,
      recoveryRate: 99,
      unrecoveredRate: 1,
      nakRate: 0.2,
      recvMbps: 10,
      linkCapacityMbps: 40,
      headroomPercent: 75,
      rttMs: 5,
      estLatencyMs: 10,
      counterReset: false,
    },
    severity,
    fromMetrics: true,
  };
}

function updateWith(ids: string[], severity: SrtConnSample["severity"]): MonitorUpdate {
  return {
    seq: 1,
    at: new Date().toISOString(),
    api: { ok: true, rttMs: 1, error: null, consecutiveFailures: 0, metricsOk: true, metricsError: null },
    totals: {
      pathCount: ids.length,
      pathsOnline: ids.length,
      pathsDown: 0,
      readerCount: 0,
      inboundMbps: 0,
      outboundMbps: 0,
      srtConnCount: ids.length,
      srtMaxDropRate: 0.1,
      srtMinRecoveryRate: 99,
      srtWorstSeverity: severity,
    },
    paths: [],
    srt: ids.map((id) => ({ seq: 1, sample: srtSample(id, severity) })),
    removedPaths: [],
    removedSrt: [],
    events: [],
    serverVersion: "test",
    full: true,
  };
}

describe("SRT ダッシュボードの描画", () => {
  beforeEach(() => {
    resetStoreForTest();
    vi.restoreAllMocks();
    // 系列の取得は「まだ読んでいない」状態のままにする（描画だけを確認する）。
    __setBridgeForTest(undefined);
  });

  it("接続が無いときは案内を出す", async () => {
    render(<SrtDashboard />);
    expect(await screen.findByTestId("srt-dashboard")).toBeTruthy();
    expect(screen.getByText("SRT 接続がありません")).toBeTruthy();
  });

  it("接続が 0 件 → 1 件 → 0 件と変わっても描画が壊れない", async () => {
    render(<SrtDashboard />);
    await screen.findByText("SRT 接続がありません");

    // 接続が現れる
    act(() => useAppStore.getState().applyUpdate(updateWith(["cam1"], "ok")));
    await waitFor(() => expect(screen.getByTestId("stat-worst")).toBeTruthy());
    expect(screen.queryByText("SRT 接続がありません")).toBeNull();

    // さらに増える
    act(() => useAppStore.getState().applyUpdate(updateWith(["cam1", "cam2"], "ok")));
    await waitFor(() => expect(screen.getByTestId("stat-worst")).toBeTruthy());

    // 消える（削除は差分の `removedSrt` で伝える）
    const removed = updateWith([], "ok");
    removed.removedSrt = ["cam1", "cam2"];
    act(() => useAppStore.getState().applyUpdate(removed));
    await waitFor(() => expect(screen.getByText("SRT 接続がありません")).toBeTruthy());
  });

  it("警告になると総合状態の表示が変わる", async () => {
    act(() => useAppStore.getState().applyUpdate(updateWith(["cam1"], "ok")));
    render(<SrtDashboard />);
    const stat = await screen.findByTestId("stat-worst");
    expect(stat.textContent).toContain("良好");

    act(() => useAppStore.getState().applyUpdate(updateWith(["cam1"], "critical")));
    await waitFor(() => expect(screen.getByTestId("stat-worst").textContent).toContain("劣化"));
  });
});
