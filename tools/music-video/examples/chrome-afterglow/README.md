# Chrome Afterglow

Sonalloyのシンセ5音源とドラム4音源で演奏した曲の、冒頭30秒を紹介する制作例です。音源定義は`presets/`から参照し、演奏パターンは`patterns/`、音声生成スクリプトとデモの構成定義はこのディレクトリに置いています。

| 設定 | 表示する音源 | 出力フォルダ（リポジトリルート基準） |
|---|---|---|
| [config/project-with-drums.json](config/project-with-drums.json) | シンセ5音源と、小さなドラム4音源の表示 | `tools/music-video/out/demo-video/with-drums/` |
| [config/project-synths.json](config/project-synths.json) | シンセ5音源 | `tools/music-video/out/demo-video/synths/` |

どちらも同じミックス音声を使い、128 BPMの曲を30秒で切り出し、末尾0.5秒をフェードします。各設定の`tracks`に音源名・ステム・演奏パターンを、`sections`に7.5秒ごとの場面を記載しています。

## 再生成する

リポジトリのルートで音声を作り、その後に映像を生成します。Sonalloyのビルド環境はルートの[README](../../../../README.md)、Remotionの標準テンプレートと共通コマンドは[Music Video](../../README.md)を参照してください。

```powershell
cargo build -p sonalloy-cli
pwsh -File tools/music-video/examples/chrome-afterglow/render-audio.ps1
cd tools/music-video
npm ci
node scripts/render.mjs still examples/chrome-afterglow/config/project-with-drums.json 24
node scripts/render.mjs render examples/chrome-afterglow/config/project-with-drums.json
```

音声とステムが`tools/music-video/out/audio/chrome-afterglow/`に揃っている場合は、音声生成を省略できます。シンセだけを表示する場合は、コマンドの設定パスを`config/project-synths.json`に置き換えます。横型動画の出力は設定ごとに別のフォルダへ保存されます。

完成済みの紹介動画は`tools/music-video/out/demo-video/`に保存しています。`out/`の音声・動画はGit管理外です。

縦型のビジュアライザー設定と共通の生成方法は、[音声解析を使う動画](../../README.md#解析データを使う動画)にまとめています。
