import { defineConfig, devices } from "@playwright/test";

/// E2E（描画検証）設定。
///
/// ブラウザ向けにビルドしたものを `vite preview` で配信し、モックブリッジ
/// （`?mock=1`）で本番と同じコンポーネントを描画する。Tauri のネイティブ
/// ウィンドウは CI では掴めないため、UI の受け入れはここで行い、
/// バックエンドの実機検証は Rust の `live_mediamtx` テストが担当する。
export default defineConfig({
  testDir: "./e2e",
  timeout: 60_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? [["github"], ["list"]] : [["list"]],
  use: {
    baseURL: "http://127.0.0.1:4173",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    locale: "ja-JP",
    timezoneId: "Asia/Tokyo",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 960 } },
    },
  ],
  webServer: {
    command: "npm run preview",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: !process.env.CI,
    stdout: "pipe",
    stderr: "pipe",
    timeout: 120_000,
  },
});
