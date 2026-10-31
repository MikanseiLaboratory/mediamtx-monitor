import { expect, test } from "@playwright/test";

/// 画面ごとのスクリーンショットを撮り、CI のアーティファクトに残す。
/// 見た目の退行を人間が確認できるようにするためのもので、比較判定はしない。

const PAGES: Array<{ name: string; tab: string }> = [
  { name: "01-srt", tab: "srt" },
  { name: "02-dashboard", tab: "dashboard" },
  { name: "03-paths", tab: "paths" },
  { name: "04-connections", tab: "connections" },
  { name: "05-operations", tab: "operations" },
  { name: "06-settings", tab: "settings" },
];

test("全画面のスクリーンショットを保存する", async ({ page }, testInfo) => {
  await page.goto("/?mock=1");
  await expect(page.getByTestId("app-root")).toBeVisible();
  // グラフの初期描画を待つ
  await page.waitForTimeout(1200);
  for (const p of PAGES) {
    await page.getByTestId(`tab-${p.tab}`).click();
    await page.waitForTimeout(600);
    await page.screenshot({
      path: testInfo.outputPath(`${p.name}.png`),
      fullPage: false,
    });
  }

  // 確認ダイアログも 1 枚（変更系操作アラートの見た目）
  await page.getByTestId("tab-paths").click();
  await page.getByRole("button", { name: "削除" }).first().click();
  await expect(page.getByTestId("confirm-dialog")).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("07-confirm.png") });
});
