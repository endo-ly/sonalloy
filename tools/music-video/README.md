# 音楽動画ツール

このディレクトリは、音声と演奏データからRemotionの動画を生成するツールです。RemotionのRootを一つにまとめ、設定JSONを使う動画の生成入口を一つにしています。汎用テンプレートは標準コマンドから、横型の制作例と縦型のビジュアライザーは共通の生成コマンドから扱います。

## 配置

- `src/index.tsx`: すべてのRemotion構成を登録する唯一のRootです。
- `src/compositions/`: 汎用テンプレート、横型の`InstrumentScore`、縦型の`Visualizer`を置いています。
- `src/visualizers/`: 音声解析と演奏データを使う4種類の描画構成を置いています。
- `scripts/render.mjs`: 設定JSONを読み込み、準備、プレビュー、静止画、動画の生成を行う共通の入口です。
- `lib/`: 音声解析、演奏パターンの時刻変換、ステム解析、Remotion実行、比較ページ生成を置いています。
- `examples/chrome-afterglow/`: 曲固有の音声生成、設定、パターン、横型レイアウトの準備を置いています。
- `out/`: 書き出した静止画と動画です。生成物なのでGitでは管理しません。

## 汎用テンプレートを使う

依存関係をインストールし、音声とprops JSONを用意します。

```powershell
cd tools/music-video
npm ci
```

音声は`public/`の下に置きます。`audio`には`public/`からの相対パスを指定します。

```json
{
  "audio": "my-track/music.wav",
  "eyebrow": "SONALLOY",
  "title": "My Track",
  "subtitle": "K-pop / EDM demo",
  "accent": "#6de7ff",
  "background": "#090a12",
  "durationInSeconds": 30
}
```

`MusicVideo`の確認と書き出しには、次の標準コマンドを使います。

```powershell
npm run studio -- --props=path/to/props.json
npm run still -- out/my-track/poster.png --frame=450 --props=path/to/props.json
npm run render -- out/my-track/video.mp4 --props=path/to/props.json --codec=h264 --crf=20 --pixel-format=yuv420p --audio-bitrate=256k
```

## 解析データを使う動画

ミックス音声、音源ごとのステム、演奏パターンを使って、音と画面の動きを同期させます。次の4種類は同じ解析済みデータを使い、見せ方だけを切り替えます。

| 名前 | 特徴 |
| --- | --- |
| `resonance` | 周波数で形が変わる立体リングと、演奏中のノートを表示します。 |
| `score-machine` | 音程を横位置、音の長さをノートの長さにした縦スクロール楽譜です。 |
| `impact-grid` | 音源ごとの図形を並べ、発音と音量に合わせて動かします。 |
| `phase-garden` | ステムの波形を軌跡として描き、キックから波紋を広げます。 |

設定JSONの`audio`には完成したミックス、各`tracks`の`audio`には同じ時間軸のステムを指定します。`pattern`または`notes`で演奏情報を指定し、`startSeconds`、`durationSeconds`、`fadeOutSeconds`で切り出し範囲を決めます。設定JSONからの相対パスを使います。

```powershell
node scripts/render.mjs prepare examples/chrome-afterglow/config/visualizers.json
node scripts/render.mjs still examples/chrome-afterglow/config/visualizers.json all 12
node scripts/render.mjs render examples/chrome-afterglow/config/visualizers.json all
node scripts/render.mjs studio examples/chrome-afterglow/config/visualizers.json resonance
```

出力は`out/visualizers/<id>/`に保存されます。`render`では各構成の動画と比較用の`index.html`を生成します。設定を別の曲へ使う場合は、`id`、`title`、`audio`、`tracks`を変更します。`notes`は次の配列形式です。

```json
[{ "start": 0.5, "duration": 0.25, "pitch": 60, "velocity": 100 }]
```

## 制作例

[Chrome Afterglow](examples/chrome-afterglow/README.md)には、音声生成、横型動画の設定、パターン、再生成手順をまとめています。横型動画も同じ`scripts/render.mjs`から生成します。
