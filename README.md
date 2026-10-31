# MediaMTX Monitor

MediaMTX の死活監視と SRT 回線の品質（ドロップ率・回復率・RTT）を可視化するデスクトップアプリです。

- Control API（`/v3`）と Prometheus メトリクス（`/metrics`）を定期取得し、パスごとの状態・視聴者数・ビットレート・SRT 品質をグラフ表示
- パスの作成・変更・削除、接続の切断、グローバル設定の変更に対応
- 切断・回線劣化時の OS 通知、グラフの CSV / SVG 出力

## ダウンロード

[Releases](https://github.com/MikanseiLaboratory/mediamtx-monitor/releases) でインストーラを配布しています。

- Windows: `MediaMTX.Monitor_*_x64-setup.exe`
- Debian / Ubuntu: `MediaMTX.Monitor_*_amd64.deb`

Windows 版は未署名のため、初回起動時に SmartScreen の警告が出ます。「詳細情報」→「実行」で起動してください。

## 事前準備

MediaMTX v1.15 以降が必要です。`mediamtx.yml` で API とメトリクスを有効にします。

```yaml
api: true
apiAddress: 127.0.0.1:9997
metrics: true
metricsAddress: 127.0.0.1:9998
```

## 設定

画面の「設定」タブ、または `%APPDATA%\io.github.mikanseilaboratory.mediamtx-monitor\settings.json` で指定します。

| 項目 | 既定値 | 内容 |
|---|---|---|
| `endpoint.baseUrl` | `http://127.0.0.1:9997` | MediaMTX API の URL |
| `endpoint.metricsUrl` | 自動（ポート 9998 の `/metrics`） | メトリクス取得先 |
| `endpoint.auth` | `none` | 認証方式（`none` / `basic` / `bearer`）。認証情報はメモリ保持のみで保存しません |
| `pollIntervalMs` | `2000` | 取得間隔（1000〜60000 ms） |
| `historyHours` | `4` | メモリ上の履歴保持時間（1〜72 時間） |
| `srt.dropRateWarn` / `dropRateCrit` | `0.5` / `2.0` | SRT ドロップ率（%）の警告・異常しきい値 |
| `srt.recoveryRateWarn` | `90` | SRT 回復率（%）の下限しきい値 |

SRT の各指標は取得周期ごとの増分（Δ）から算出します。

- 受信ドロップ率: `ΔpacketsReceivedDrop / ΔpacketsReceived × 100`
- 回復率: `ΔpacketsReceivedRetrans / ΔpacketsReceivedLoss × 100`
- NAK 率: `ΔpacketsSentNAK / ΔpacketsReceivedUnique × 100`

## 開発・ビルド

必要環境: Rust 1.85 以降、Node.js 22、pnpm 11（Windows は WebView2・MSVC ビルドツール）

```powershell
pnpm --dir web install

# 開発起動
cd crates/mediamtx-monitor
cargo tauri dev

# リリースビルド
cargo tauri build
```

テスト:

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm --dir web test
pnpm --dir web exec playwright test
```

## ライセンス

MIT
