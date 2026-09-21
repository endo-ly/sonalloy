# 音楽動画ツール

音声、ステム、ノート情報から音楽動画を生成します。音源ごとの設定は`projects/<id>/project.json`にまとめ、横型と縦型を同じコマンドから生成します。入力音声の生成方法には依存せず、動画ツールは用意された音声素材を読み込みます。

## 配置

- `projects/<id>/project.json`: 音源、トラック、ノート、生成する動画を定義します。
- `projects/<id>/patterns/`: トラックのノート情報を置きます。
- `src/index.tsx`: すべてのRemotion構成を登録するRootです。
- `src/compositions/`: 横型の`InstrumentScore`と縦型の`Visualizer`、汎用テンプレートを置いています。
- `src/visualizers/`: 縦型ビジュアライザーの描画構成を置いています。
- `scripts/render.mjs`: 設定JSONを読み込み、準備、静止画、動画の生成を行う共通入口です。
- `lib/`: 音声解析、ノート変換、トラック準備、各構成の準備処理を置いています。
- `out/`: 音声、静止画、動画などの生成物です。Gitでは管理しません。

## 音源プロジェクトを生成する

プロジェクトJSONには、完成したミックス音声、音源ごとのステム、ノート情報、生成対象を記載します。音声とステムは設定JSONからの相対パスで指定します。

```powershell
cd tools/music-video
npm ci
node scripts/render.mjs prepare projects/chrome-afterglow/project.json
node scripts/render.mjs render projects/chrome-afterglow/project.json
```

`render`はプロジェクトJSONの`renders`に定義した動画をすべて生成します。特定の出力だけを生成する場合は、生成対象のIDを指定します。

```powershell
node scripts/render.mjs render projects/chrome-afterglow/project.json horizontal-with-drums
node scripts/render.mjs render projects/chrome-afterglow/project.json vertical
node scripts/render.mjs still projects/chrome-afterglow/project.json vertical resonance 12
```

横型は`InstrumentScore`、縦型は`Visualizer`が描画します。音声解析とトラック準備は共通ですが、各構成が必要とするsceneの形はそれぞれの準備処理で作ります。

## 汎用テンプレートを使う

音声を`public/`の下に置き、props JSONを指定して標準コマンドを実行します。

```powershell
npm run studio -- --props=path/to/props.json
npm run still -- out/my-track/poster.png --frame=450 --props=path/to/props.json
npm run render -- out/my-track/video.mp4 --props=path/to/props.json --codec=h264 --crf=20 --pixel-format=yuv420p --audio-bitrate=256k
```

## 制作例

[Chrome Afterglow](projects/chrome-afterglow/README.md)には、1つのプロジェクトJSONから横型と縦型を生成する設定例を置いています。
