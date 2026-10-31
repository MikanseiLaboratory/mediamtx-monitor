/// 警告の「最低表示時間」を保つためのフック。
///
/// 1 秒周期の監視では、劣化が一瞬だけ起きてすぐ戻ると、画面を見た瞬間には
/// もう黄／赤が消えていることがある。バッジが一瞬だけ点滅して見逃すと
/// 「壊れたこと」自体に気づけないので、一度でも警告が出たら一定時間は
/// 表示を保持し、その間に気づけるようにする。
///
/// 保持するのは**表示だけ**で、値そのものは常に最新を使う。

import { useEffect, useRef, useState } from "react";

export type Tone = "ok" | "warn" | "crit" | "neutral";

const RANK: Record<Tone, number> = { neutral: 0, ok: 1, warn: 2, crit: 3 };

/**
 * `tone` が一定時間 `holdMs` 以上続かない場合でも、最低 `holdMs` は
 * 警告表示（warn / crit）を保つ。
 *
 * - 新しい警告が出たら、その時点から `holdMs` の保持を開始する
 * - 保持中にさらに強い警告（warn → crit）が出たら、強度と期限を更新する
 * - 保持中に戻っても、`holdMs` が経つまでは警告表示のままにする
 * - `holdMs` が経過すると最新の状態へ戻す
 */
export function useHeldTone(tone: Tone, holdMs = 3000): Tone {
  const [held, setHeld] = useState<Tone>(tone);
  const latest = useRef<Tone>(tone);
  const until = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    latest.current = tone;
    if (holdMs <= 0) {
      // 保持しない設定：そのまま反映する。
      if (timer.current) clearTimeout(timer.current);
      timer.current = null;
      until.current = 0;
      setHeld(tone);
      return;
    }

    const isWarning = (t: Tone) => t === "warn" || t === "crit";
    if (isWarning(tone)) {
      // 警告が出た：強度と期限を更新して、少なくとも holdMs は表示する。
      const nextUntil = Date.now() + holdMs;
      if (nextUntil > until.current || RANK[tone] > RANK[held]) {
        until.current = nextUntil;
        setHeld(tone);
      }
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        timer.current = null;
        // 期限が来たら、その時点の最新状態へ戻す。
        setHeld(latest.current);
      }, holdMs);
      return;
    }

    // 正常／中立に戻った：保持中なら期限まで待つ。
    const remain = until.current - Date.now();
    if (remain > 0) {
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        timer.current = null;
        setHeld(latest.current);
      }, remain);
      return;
    }
    setHeld(tone);
  }, [tone, holdMs]);

  useEffect(() => {
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, []);

  return held;
}
