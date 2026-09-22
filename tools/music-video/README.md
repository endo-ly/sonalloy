# 音楽動画ツール

音声、ステム、ノート情報から音楽動画を生成します。音源ごとの設定は`projects/<id>/project.json`にまとめ、横型と縦型を同じコマンドから生成します。入力音声の生成方法には依存せず、動画ツールは用意された音声素材を読み込みます。

## 配置

- `projects/<id>/project.json`: 音源、トラック、ノート、生成する動画を定義します。
- `projects/<id>/patterns/`: トラックのノート情報を置きます。
- `src/index.tsx`: すべてのRemotion構成を登録するRootです。
- `src/compositions/`: 横型の`InstrumentScore`と縦型の`Visualizer`を置いています。
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

横型は`InstrumentScore`、縦型は`Visualizer`が描画します。音声解析、ノート変換、トラック準備は共通のPrepared Sceneへまとめ、各VisualizerはそのSceneだけを参照します。縦型Visualizerの内部座標系と出力は9:16です。

## Riffraから生成する場合

Riffraのプロジェクトファイルをこのツールで直接読み込まず、RiffraでMix、トラック別の音声、ノート情報を用意してから`project.json`へ渡します。

Riffra Desktopで対象プロジェクトを開くか、ヘッドレスHostを起動します。起動中のHostとプロジェクトは次で確認できます。

```powershell
# Desktopを使わない場合は、別のターミナルで起動したままにします
riffra --data-root <data-root> serve

riffra host list
riffra --attach project list
riffra --attach session inspect
```

Mixはアレンジ全体をレンダーし、トラック別の音声は`--track-id`を指定してトラックごとにレンダーします。

```powershell
riffra --attach render start --range entire-arrangement --normalize false
riffra --attach job wait --id <mix-job-id>

riffra --attach render start --track-id <track-id> --range entire-arrangement --normalize false
riffra --attach job wait --id <stem-job-id>
```

`session get`で取得したノート情報をトラックごとの`patterns/<track>.json`へ変換し、生成したWAVと一緒に次のように配置します。音声は生成物のためGitでは管理しません。

```powershell
riffra --attach session get > riffra-session.json
```

```text
tools/music-video/
├─ projects/<id>/
│  ├─ project.json
│  └─ patterns/<track>.json
└─ out/audio/<id>/
   ├─ mix.wav
   └─ stems/<track>.wav
```

`project.json`の`audio`、各トラックの`audio`と`notes`に配置先を記載したら、通常の手順で動画を生成します。

```powershell
cd tools/music-video
node scripts/render.mjs prepare projects/<id>/project.json
node scripts/render.mjs render projects/<id>/project.json
```

Riffraの左右チャンネル分離ではなく、`--track-id`で生成した音声をトラック別のSTEMとして使用します。

## 制作例

[Chrome Afterglow](projects/chrome-afterglow/README.md)には、1つのプロジェクトJSONから横型と縦型を生成する設定例を置いています。
