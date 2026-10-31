/// 警告表示の保持（`useHeldTone`）のテスト。
///
/// 一瞬だけ起きた劣化でバッジが点滅して見逃されるのを防ぐための仕組み。
/// タイマーを使うので、偽の時刻で進める。

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useHeldTone } from "../lib/hold";

type Tone = "ok" | "warn" | "crit";

describe("警告バッジの保持", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("一瞬の警告でも holdMs の間は表示し続ける", () => {
    const { result, rerender } = renderHook(
      ({ tone }: { tone: Tone }) => useHeldTone(tone, 3000),
      { initialProps: { tone: "ok" as Tone } },
    );
    expect(result.current).toBe("ok");

    // 警告が出た
    act(() => {
      rerender({ tone: "warn" });
    });
    expect(result.current).toBe("warn");

    // すぐ正常に戻っても、保持時間中は警告のまま
    act(() => {
      rerender({ tone: "ok" });
    });
    expect(result.current).toBe("warn");

    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(result.current).toBe("warn");

    // 保持時間が過ぎたら、最新の状態へ戻る
    act(() => {
      vi.advanceTimersByTime(1100);
    });
    expect(result.current).toBe("ok");
  });

  it("保持中に深刻化したら、より強い警告を表示する", () => {
    const { result, rerender } = renderHook(
      ({ tone }: { tone: Tone }) => useHeldTone(tone, 3000),
      { initialProps: { tone: "ok" as Tone } },
    );
    act(() => {
      rerender({ tone: "warn" });
    });
    act(() => {
      rerender({ tone: "crit" });
    });
    expect(result.current).toBe("crit");

    // 回復しても保持時間までは crit のまま
    act(() => {
      rerender({ tone: "ok" });
    });
    expect(result.current).toBe("crit");
  });

  it("警告が続いている間は、保持期限を延ばし続ける", () => {
    const { result, rerender } = renderHook(
      ({ tone }: { tone: Tone }) => useHeldTone(tone, 3000),
      { initialProps: { tone: "warn" as Tone } },
    );
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    // まだ警告中。期限が延びる
    act(() => {
      rerender({ tone: "warn" });
    });
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(result.current).toBe("warn");

    // 正常に戻ってから 3 秒で解除
    act(() => {
      rerender({ tone: "ok" });
    });
    act(() => {
      vi.advanceTimersByTime(3100);
    });
    expect(result.current).toBe("ok");
  });

  it("holdMs が 0 なら即座に反映する（保持しない設定）", () => {
    const { result, rerender } = renderHook(
      ({ tone }: { tone: Tone }) => useHeldTone(tone, 0),
      { initialProps: { tone: "ok" as Tone } },
    );
    act(() => {
      rerender({ tone: "warn" });
    });
    expect(result.current).toBe("warn");
    act(() => {
      rerender({ tone: "ok" });
    });
    expect(result.current).toBe("ok");
  });

  it("保持中に新しい警告が起きたら、そこから数え直す", () => {
    const { result, rerender } = renderHook(
      ({ tone }: { tone: Tone }) => useHeldTone(tone, 3000),
      { initialProps: { tone: "ok" as Tone } },
    );
    act(() => {
      rerender({ tone: "warn" });
    });
    act(() => {
      rerender({ tone: "ok" });
    });
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    // 期限前にもう一度警告が出る
    act(() => {
      rerender({ tone: "warn" });
    });
    act(() => {
      rerender({ tone: "ok" });
    });
    // 2 回目の警告から 3 秒は保持される
    act(() => {
      vi.advanceTimersByTime(2500);
    });
    expect(result.current).toBe("warn");
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(result.current).toBe("ok");
  });
});
