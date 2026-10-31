/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri の開発時は固定ポートを使い、ビルドは相対パスで配信する。
export default defineConfig({
  plugins: [react()],
  // 本番ビルドとテストで React の解決先を分ける。
  // テスト（jsdom）では `act` を含む development 版が必要になる。
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  preview: { port: 4173, strictPort: true, host: "127.0.0.1" },
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: true,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    css: false,
  },
});
