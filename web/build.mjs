#!/usr/bin/env node
// フロントエンドのビルドを、どのディレクトリから呼ばれても正しく行う小さなラッパ。
// tauri.conf.json の beforeBuildCommand / beforeDevCommand から使う。
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const webDir = here;
const script = process.argv[2] ?? "build";
const result = spawnSync("pnpm", ["run", script], {
  cwd: webDir,
  stdio: "inherit",
  shell: process.platform === "win32",
});
process.exit(result.status ?? 1);