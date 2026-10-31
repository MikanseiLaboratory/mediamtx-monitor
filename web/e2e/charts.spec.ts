import { expect, test, type Page } from "@playwright/test";

/// グラフ描画の回帰テスト。
///
/// 検証しているのは「線が**窓いっぱい**に描かれているか」。
/// かつて次の不具合があり、どれもグラフが破綻して見えた。
/// - `setData` の直後に `setScale("x")` で固定窓を上書きし、データが右端に寄った
/// - 表示窓の基準が壁時計で、データが左へずれた
/// - モックの履歴が全点同じミリ秒に積まれ、1 本の縦線にしかならなかった
///
/// uPlot のインスタンスは DOM から直接辿れないので、canvas に描かれた
/// **彩色ピクセル（線と塗り）の水平分布**で判定する。

const MOCK = "/?mock=1";

interface Ink {
  key: string;
  width: number;
  inkCount: number;
  /** 描画領域に対する線の左端 / 右端（%） */
  leftPct: number;
  rightPct: number;
}

async function inkOf(page: Page, testId: string): Promise<Ink> {
  return page.evaluate((id) => {
    const card = document.querySelector(`[data-testid="${id}"]`);
    const canvas = card?.querySelector("canvas") as HTMLCanvasElement | null;
    if (!canvas) throw new Error(`canvas が見つかりません: ${id}`);
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("2d コンテキストが取れません");
    const { width, height } = canvas;
    const img = ctx.getImageData(0, 0, width, height).data;
    let minX = Infinity;
    let maxX = -Infinity;
    let count = 0;
    for (let y = 0; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        const i = (y * width + x) * 4;
        const r = img[i];
        const g = img[i + 1];
        const b = img[i + 2];
        if (img[i + 3] < 40) continue;
        const mx = Math.max(r, g, b);
        const mn = Math.min(r, g, b);
        // グリッド線・目盛り文字（灰色）を除くため、彩度のあるピクセルだけ数える。
        if (mx < 90 || (mx === 0 ? 0 : (mx - mn) / mx) < 0.35) continue;
        count += 1;
        if (x < minX) minX = x;
        if (x > maxX) maxX = x;
      }
    }
    return {
      key: id,
      width,
      inkCount: count,
      leftPct: count ? (minX / width) * 100 : 0,
      rightPct: count ? (maxX / width) * 100 : 0,
    };
  }, testId);
}

/** 線が窓全体に広がっていることを確かめる。 */
async function expectSpansWindow(page: Page, testId: string) {
  // 描画は非同期（uPlot の commit）なので、条件を満たすまで待つ。
  await expect
    .poll(async () => (await inkOf(page, testId)).leftPct, { timeout: 15_000 })
    .toBeLessThan(25);
  const ink = await inkOf(page, testId);
  expect(ink.inkCount).toBeGreaterThan(50);
  expect(ink.rightPct).toBeGreaterThan(85);
}

test.beforeEach(async ({ page }) => {
  await page.goto(MOCK);
  await expect(page.getByTestId("app-root")).toBeVisible();
});

test("SRT グラフは窓いっぱいに描かれる（右端に潰れない）", async ({ page }) => {
  await page.getByTestId("tab-srt").click();
  await expect(page.getByTestId("srt-dashboard")).toBeVisible();
  for (const key of ["srt_drop_rate", "srt_recovery_rate", "srt_recv_mbps", "srt_rtt_ms"]) {
    await expectSpansWindow(page, `timeseries-${key}`);
  }
});

test("時間窓を切り替えても窓いっぱいに描かれる", async ({ page }) => {
  await page.getByTestId("tab-srt").click();
  for (const label of ["1時間", "15分", "1分"]) {
    await page.getByRole("button", { name: label, exact: true }).click();
    await expectSpansWindow(page, "timeseries-srt_recovery_rate");
  }
});

test("窓を素早く切り替えても古い応答が新しい結果を上書きしない", async ({ page }) => {
  await page.getByTestId("tab-srt").click();
  await page.waitForTimeout(1500);
  // 1時間 → 1分 を連続で押す（先の要求が後から返っても結果を壊さないこと）
  await page.getByRole("button", { name: "1時間", exact: true }).click();
  await page.getByRole("button", { name: "1分", exact: true }).click();
  await expect(page.getByRole("button", { name: "1分", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  // 1 分窓でも線が窓いっぱいに出る＝1 時間分のデータが混ざっていない
  await expectSpansWindow(page, "timeseries-srt_recovery_rate");
});

test("ダッシュボードの 4 グラフはそれぞれ違う形になる", async ({ page }) => {
  await page.getByTestId("tab-dashboard").click();
  await expect(page.getByTestId("dashboard")).toBeVisible();
  const keys = ["api_rtt_ms", "readers", "inbound_mbps", "srt_drop_rate"];
  const inks: Ink[] = [];
  for (const key of keys) {
    await expectSpansWindow(page, `timeseries-${key}`);
    inks.push(await inkOf(page, `timeseries-${key}`));
  }
  // かつては全メトリクスに同じ値を返していたため、線の描画量が揃っていた。
  // 今は単位も値域も違うので、彩色ピクセル数は大きくばらける。
  const counts = inks.map((i) => i.inkCount);
  expect(new Set(counts).size).toBeGreaterThan(1);
  expect(Math.max(...counts) / Math.min(...counts)).toBeGreaterThan(1.5);
});
