# Chrome Afterglow

ミックス音声、9本のステム、演奏ノートを使うプロジェクトの設定例です。`project.json`に音源と生成対象をまとめ、横型2種類と縦型4種類を同じ入口から生成します。

音声とステムは次の場所に用意します。音声ファイルは生成物なのでGitでは管理しません。

```text
tools/music-video/out/audio/chrome-afterglow/
├─ chrome-afterglow-balanced.wav
└─ stems/
   ├─ steel.wav
   ├─ chrome.wav
   ├─ glass.wav
   ├─ vowel.wav
   ├─ bloom.wav
   ├─ kick.wav
   ├─ clap.wav
   ├─ hat.wav
   └─ open.wav
```

## 生成する

リポジトリのルートで音声とステムを用意したあと、`tools/music-video`から実行します。

```powershell
cd tools/music-video
npm ci
node scripts/render.mjs prepare projects/chrome-afterglow/project.json
node scripts/render.mjs render projects/chrome-afterglow/project.json
```

出力先は`out/demo-video/`と`out/visualizers/chrome-afterglow/`です。横型の静止画は次のように生成できます。

```powershell
node scripts/render.mjs still projects/chrome-afterglow/project.json horizontal-with-drums 24
```

縦型は構成名を指定します。

```powershell
node scripts/render.mjs still projects/chrome-afterglow/project.json vertical resonance 12
```

別の音源を追加する場合は、`projects/<id>/project.json`にミックス、ステム、ノート、生成対象を記載し、同じコマンドを実行します。
