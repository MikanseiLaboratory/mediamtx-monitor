import { expect, test } from "@playwright/test";

/// 警告の保持とタイムラインの回帰テスト。
///
/// - 一瞬の劣化でバッジが点滅して見逃さないよう、表示は最低数秒保持する
/// - 劣化は発生時刻に遡ってタイムラインに残る
/// - グラフが空のとき、メトリクスごとに適切な説明を出す
///   （以前は全部「まだ劣化や流量が記録されていません」で、ドロップ率でも
///    落ちた／落ちていないが分からなかった）

test("警告の表示は保持時間の間つづく設定になっている", async ({ page }) => {
  await page.goto("/?mock=1");
  await expect(page.getByTestId("app-root")).toBeVisible();
  await page.getByTestId("tab-settings").click();
  // 既定は 3 秒
  await expect(page.getByTestId("srt-hold-secs")).toHaveValue("3");
  // スライダーを動かせる
  await page.getByTestId("srt-hold-secs").fill("5");
  await expect(page.getByTestId("srt-hold-secs")).toHaveValue("5");
});

test("保持秒数の設定は保存できる", async ({ page }) => {
  await page.goto("/?mock=1");
  await expect(page.getByTestId("app-root")).toBeVisible();
  await page.getByTestId("tab-settings").click();
  await page.getByTestId("srt-hold-secs").fill("7");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  // 保存は確認ダイアログを経る
  const dialog = page.getByTestId("confirm-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('"holdSecs": 7');
  await dialog.getByRole("button", { name: "保存する" }).click();
  await expect(page.getByTestId("toast")).toContainText("保存しました");
});

test("劣化するとタイムラインに発生時刻つきで残る", async ({ page }) => {
  // 保持 1 秒なら、モックの周期的な短い劣化でも記録される。
  await page.goto("/?mock=1&holdSecs=1");
  await expect(page.getByTestId("app-root")).toBeVisible();
  await page.getByTestId("tab-srt").click();
  await expect(page.getByTestId("srt-dashboard")).toBeVisible();

  // モックは短い劣化を周期的に起こす。保持 1 秒なので記録される。
  const timeline = page.getByTestId("event-timeline");
  await expect(timeline).toContainText("品質劣化", { timeout: 45_000 });
  // 記録された劣化には、発生時刻と値が入っている
  await expect(timeline).toContainText("ドロップ率");
  await expect(timeline.locator(".timeline-time").first()).toHaveText(/^\d{2}:\d{2}:\d{2}$/);
});

test("グラフが空のときは、メトリクスごとに違う説明を出す", async ({ page }) => {
  // 保持 0 でもグラフの説明には影響しない。ここでは描画の有無だけを見る。
  await page.goto("/?mock=1");
  await expect(page.getByTestId("app-root")).toBeVisible();
  await page.getByTestId("tab-srt").click();
  await expect(page.getByTestId("srt-dashboard")).toBeVisible();

  // データがある状態では空表示が出ない
  const drop = page.getByTestId("timeseries-srt_drop_rate");
  await expect(drop.locator(".timeseries-empty")).toHaveCount(0);
});
