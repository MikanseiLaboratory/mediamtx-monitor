# MediaMTX Monitor

MediaMTX の **死活監視** と **SRT 回線の品質（パケットドロップ・回復率）** を見続けるための
デスクトップアプリです。Tauri v2（Rust）+ React で作られています。

- MediaMTX の Control API（`/v3`）と Prometheus メトリクス（`/metrics`）を一定間隔で取得
- パスごとの UP/DOWN、視聴者数、ビットレート、SRT 接続ごとのドロップ率・回復率・RTT をグラフ表示
- パスの作成・変更・削除、接続の切断、グローバル設定の変更を GUI から（**すべて確認ダイアログ付き**）
- 指定した期間のグラフを CSV / SVG として書き出し

> このアプリは MediaMTX を **外から観測するだけ** です。MediaMTX 本体の設定変更や再起動は
> 行いません（API 経由の操作は、ユーザーが明示的に実行したときだけ送ります）。

## ダウンロード

[Releases](https://github.com/MikanseiLaboratory/mediamtx-monitor/releases) から取得できます。

| ファイル | 対象 |
|---|---|
| `MediaMTX.Monitor_*_x64-setup.exe` | Windows（インストーラ） |
| `MediaMTX.Monitor_*_amd64.deb` | Debian / Ubuntu |

> **v0.1.0-alpha.1 はプレリリースです。** 署名をしていないため、初回起動時に
> Windows SmartScreen の警告（「Windows によって PC が保護されました」）が出ます。
> 「詳細情報」→「実行」で進めてください。

## 構成

```
crates/mtm-core/           UI も Tauri も知らない中核
  config.rs                設定・認証・しきい値
  client.rs                MediaMTX API クライアント（ページング・エラー分類）
  prom.rs                  /metrics のパーサ
  model.rs                 API のデータモデル（未知フィールド耐性あり）
  srt.rs                   SRT の差分計算（ドロップ率・回復率・カウンタリセット検出）
  ring.rs                  時系列リング
  state.rs                 監視の状態機械（遷移イベント）
  frame.rs                 1 周期分の更新・スナップショット・集計
  export.rs                期間指定の CSV / SVG 生成
  monitor.rs               API + メトリクスを 1 周期分取り込む

crates/mediamtx-monitor/   Tauri アダプタ
  engine.rs                監視スレッドと状態の正本（Tauri 非依存の Sink 経由で配信）
  commands_core.rs         コマンド実装（Tauri の invoke から呼ぶ）
  commands.rs              Tauri の invoke ハンドラ
  transport_ureq.rs        HTTP 輸送（ureq / OS の TLS）
  tray.rs / notify.rs      トレイ・通知
  tauri.conf.json         ウィンドウ・バンドル設定

web/                       フロントエンド（React + TypeScript + uPlot）
  build.mjs                フロントのビルド入口（Tauri のフックから呼ぶ）
  src/bridge/              Tauri の invoke / listen ラッパ（ブラウザ時はモック）
  src/store/               受信データと描画バッファ
  src/views/               画面（SRT 品質・ダッシュボード・パス管理・接続・API 操作・設定）
  e2e/                     Playwright の受け入れテストとスクリーンショット
```

### 設計の要点

- **状態の正本はバックエンド**。フロントは受け取った値を描画するだけで、差分計算や
  判定は行いません。更新は Tauri のイベント（`monitor://update`）で、1 周期ごとに
  変わった対象だけを配ります。
- **フロントとバックエンドの通信は Tauri 標準の IPC だけ**（`@tauri-apps/api` の
  `invoke` と `listen`）。自前の HTTP サーバーを立てたり `fetch` で叩いたりはしません。
- **変更系操作は必ず確認ダイアログ**を通します（対象・送信内容の diff・影響を提示）。
- 認証情報は **ディスクに保存しません**（実行中のみメモリに保持）。
- 履歴はメモリ保持です（既定 4 時間、設定で変更可）。永続化は CSV / SVG の書き出しで代替します。

## 必要要件

- MediaMTX **v1.15 以降**（`/v3` API）。監視には `mediamtx.yml` で次の 2 つを有効にしてください。

```yaml
api: true
apiAddress: 127.0.0.1:9997
metrics: true
metricsAddress: 127.0.0.1:9998
```

- ビルド: Rust 1.85 以降、Node.js 22、pnpm 11。Windows は WebView2 と MSVC ビルドツール。

## ビルドと起動

```powershell
pnpm --dir web install

# 開発（Vite と Tauri を同時に起動）
cd crates/mediamtx-monitor
cargo tauri dev

# リリースビルド（インストーラまで）
cargo tauri build
```

ビルド成果物は `target/release/mediamtx-monitor.exe`（Windows）と
`target/release/bundle/` 配下のインストーラです。

その他: `--smoke-exit-after-ms <ms>`（起動確認用の自動終了）、`--help`、`--version`。

## 設定

画面の「設定」タブ、または `%APPDATA%\io.github.mikanseilaboratory.mediamtx-monitor\settings.json`
で指定します。

| 項目 | 既定 | 説明 |
|---|---|---|
| `endpoint.baseUrl` | `http://127.0.0.1:9997` | MediaMTX API |
| `endpoint.metricsUrl` | （自動） | 未指定なら API のポートを 9998 にして `/metrics` |
| `endpoint.auth` | `none` | `none` / `basic` / `bearer`（資格情報は保存されません） |
| `pollIntervalMs` | 2000 | 取得間隔（1〜60 秒） |
| `historyHours` | 4 | 履歴の保持時間（1〜72 時間） |
| `srt.dropRateWarn` / `dropRateCrit` | 0.5 / 2.0 | ドロップ率(%) の警告・劣化しきい値 |
| `srt.recoveryRateWarn` | 90 | 回復率(%) の下限 |
| `notify.*` | 切断・劣化・停止・API 不可を ON | OS 通知の規則 |

### SRT の指標の定義

| 指標 | 計算 |
|---|---|
| 受信ドロップ率 | Δ`packetsReceivedDrop` ÷ Δ`packetsReceived` × 100 |
| 回復率 | Δ`packetsReceivedRetrans` ÷ Δ`packetsReceivedLoss` × 100 |
| 未回復率 | 100 − 回復率（0% 下限） |
| NAK 率 | Δ`packetsSentNAK` ÷ Δ`packetsReceivedUnique` × 100 |

累積カウンタのリセットを検出した区間は「値なし」として扱います（0 と混同しません）。

## テスト

```powershell
cargo test --workspace                              # Rust（コア 60 / アプリ 22 / 結合 3）
cargo clippy --workspace --all-targets -- -D warnings
pnpm --dir web test                                 # Vitest
pnpm --dir web exec playwright test                 # 画面の受け入れ（モック）
```

### 実機（本物の MediaMTX）に対する結合テスト

`MTM_LIVE=1` を付けたときだけ実行されます。

```powershell
# 1) MediaMTX を api/metrics 有効で起動（.dev/mediamtx.yml 参照）
# 2) ffmpeg で SRT 配信と視聴を開始
ffmpeg -re -f lavfi -i testsrc2=size=640x360:rate=30 `
  -c:v libx264 -preset ultrafast -tune zerolatency -f mpegts `
  "srt://127.0.0.1:8890?streamid=publish:e2e&mode=caller&latency=1000000"
ffmpeg -i "srt://127.0.0.1:8890?streamid=read:e2e&mode=caller&latency=1000000" -c copy -f mpegts $env:TEMP\out.ts
# 3) テスト実行
$env:MTM_LIVE = "1"
cargo test -p mediamtx-monitor --test live_mediamtx -- --nocapture --test-threads=1
```

## CI / 実機 E2E

- `.github/workflows/ci.yml`: Rust の fmt / clippy / test（Windows + Linux）、
  フロントの型検査・単体テスト・ビルド、Playwright の受け入れとスクリーンショット保存。
- `.github/workflows/e2e.yml`: **GitHub ホステッドの Ubuntu 実機**で本物の MediaMTX と
  ffmpeg を動かし、SRT のドロップ率・回復率が算出されること、パス操作とエクスポートを検証
  （毎日 + 手動実行）。ログと書き出しファイルをアーティファクトに保存します。
- `.github/workflows/release.yml`: **Tauri 公式の [`tauri-apps/tauri-action`](https://github.com/tauri-apps/tauri-action)**
  でビルドとリリース作成を行います。`v*` タグの push で起動し、タグ名に `-`（`v0.1.0-alpha.1` など）が
  含まれる場合は自動でプレリリースになります。手動実行（`workflow_dispatch`）ではタグ・
  プレリリース・ドラフトを指定できます。

  > WiX（msi）はプレリリース識別子を扱えないため、Windows は NSIS のセットアップ exe のみを
  > 生成します（`tauri.conf.json` の `bundle.targets` も `nsis` と `deb` のみ）。

## 制限事項

- 認証情報の永続化（OS の資格情報ストア）は未対応です。
- 履歴はメモリ保持のみ（再起動で消えます）。長期間の保管は CSV 書き出しを使ってください。
- 通知は OS 標準のトーストのみ（音声・メール・Slack 連携は未対応）。
- PNG の書き出しは未対応です（CSV と SVG を用意しています）。

## ライセンス

MIT
