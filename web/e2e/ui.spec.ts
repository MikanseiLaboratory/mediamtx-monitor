import { expect, test } from "@playwright/test";

/// 描画の受け入れテスト。値の計算はバックエンドの責任なので、ここでは
/// 「届いた値が正しく画面に出るか」「確認ダイアログを経ずに変更が起きないか」
/// を確認する。

const MOCK = "/?mock=1";

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(msg.text());
  });
  await page.goto(MOCK);
  await expect(page.getByTestId("app-root")).toBeVisible();
  expect(errors).toEqual([]);
});

test("SRT 品質モニターが接続カードとグラフを描画する", async ({ page }) => {
  await page.getByTestId("tab-srt").click();
  await expect(page.getByTestId("srt-dashboard")).toBeVisible();

  // 接続カード（モックでは cam1 / cam2）
  await expect(page.getByTestId("srt-card-cam1")).toBeVisible();
  await expect(page.getByTestId("srt-card-cam2")).toBeVisible();

  // 主目的: ドロップ率と回復率のグラフが uPlot で描画されている
  for (const key of ["srt_drop_rate", "srt_recovery_rate"]) {
    const chart = page.getByTestId(`timeseries-${key}`);
    await expect(chart).toBeVisible();
    await expect(chart.locator("canvas")).toHaveCount(1);
  }

  // 数値が出ている（初期は「—」ではない）
  await expect(page.getByTestId("stat-max-drop")).not.toContainText("—");
  await expect(page.getByTestId("stat-min-recovery")).not.toContainText("—");
});

test("ライブ更新でヘッダの更新時刻と数値が進む", async ({ page }) => {
  const before = await page.getByTestId("header-updated").innerText();
  await expect
    .poll(async () => page.getByTestId("header-updated").innerText(), { timeout: 10_000 })
    .not.toBe(before);
});

test("パス削除は確認ダイアログを経てから実行される", async ({ page }) => {
  await page.getByTestId("tab-paths").click();
  await expect(page.getByTestId("paths-view")).toBeVisible();
  // 設定一覧のモック行は 1 件
  const deleteButtons = page.getByRole("button", { name: "削除" });
  await deleteButtons.first().click();

  const dialog = page.getByTestId("confirm-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("パスを削除しますか？");
  await expect(dialog).toContainText("取り消せません");

  // キャンセルするとダイアログが閉じるだけで、何も起きない
  await dialog.getByRole("button", { name: "キャンセル" }).click();
  await expect(dialog).toBeHidden();

  // もう一度開いて実行
  await deleteButtons.first().click();
  await page.getByTestId("confirm-dialog").getByRole("button", { name: "削除する" }).click();
  await expect(page.getByTestId("toast")).toContainText("削除しました");
});

test("パス追加フォームは送信前に差分プレビュー付きの確認を出す", async ({ page }) => {
  await page.getByTestId("tab-paths").click();
  await page.getByRole("button", { name: "パスを追加" }).click();
  await page.getByTestId("path-name").fill("cam-new");
  await page.getByTestId("path-source").fill("rtsp://camera.local/stream");
  await page.getByRole("button", { name: "作成内容を確認" }).click();

  const dialog = page.getByTestId("confirm-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("パスを作成しますか？");
  await expect(dialog).toContainText("cam-new");
  // 送信内容のプレビューが出ている
  await expect(dialog.locator(".dialog-diff")).toContainText("rtsp://camera.local/stream");

  // キャンセルでは何も起きない（ダイアログが閉じるだけ）
  await dialog.getByRole("button", { name: "キャンセル" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("path-name")).toBeVisible();
});

test("グローバル設定の変更は差分だけを確認ダイアログに出す", async ({ page }) => {
  await page.getByTestId("tab-operations").click();
  await expect(page.getByTestId("operations-view")).toBeVisible();
  // ログレベルを debug に変える
  const logLevel = page.getByLabel("ログレベル");
  await logLevel.selectOption("debug");
  await page.getByRole("button", { name: /変更を確認（1 件）/ }).click();
  const dialog = page.getByTestId("confirm-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('"logLevel": "debug"');
  await dialog.getByRole("button", { name: "適用する" }).click();
  await expect(page.getByTestId("toast")).toContainText("グローバル設定を更新しました");
});

test("GET コンソールが結果を表示する", async ({ page }) => {
  await page.getByTestId("tab-operations").click();
  await page.getByTestId("console-path").fill("/v3/info");
  await page.getByRole("button", { name: "送信" }).click();
  await expect(page.getByTestId("console-output")).toContainText("HTTP 200");
});

test("設定画面で接続テストの結果が表示される", async ({ page }) => {
  await page.getByTestId("tab-settings").click();
  await expect(page.getByTestId("settings-view")).toBeVisible();
  await page.getByRole("button", { name: "接続を確認" }).click();
  await expect(page.getByTestId("probe-result")).toContainText("接続成功");
});

test("期間指定の書き出しは確認ダイアログを経てファイルを書き出す", async ({ page }) => {
  await page.getByTestId("tab-srt").click();
  await page.getByRole("button", { name: "期間を指定して書き出し" }).click();
  await expect(page.getByTestId("export-panel")).toBeVisible();
  await page.getByRole("button", { name: "書き出し", exact: true }).click();
  const dialog = page.getByTestId("confirm-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("CSV を書き出しますか？");
  await dialog.getByRole("button", { name: "書き出す" }).click();
  await expect(page.getByTestId("export-result")).toContainText("mock.csv");
});
