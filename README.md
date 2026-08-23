# Sonalloy

Sonalloyは、JSONで書いた音源定義からリアルタイム演奏とオフラインレンダリングを行うハイブリッド音源エンジンです。複数の音の層（Layer）を重ね、エフェクトとモジュレーションを組み合わせて、1つの音源を作ります。

## 特長

- **AIファースト設計**: 音源はJSONで定義し、CLIで検証・確認する。テキストだけで完結するため、AIが音源を設計・生成・検査できる
- **多彩な合成方式を組み合わせる**: 次の方式をLayerとして重ね、1つの音源を作る
  - 波形オシレータ（基本波形、FM、ハードシンク、ウェーブテーブル）
  - 加算合成・フォルマント（倍音設計、母音共鳴）
  - Physical String・Modal（撥弦、硬質振動、棒・板・ベル・金属的な共鳴）
  - サンプリング（サンプル再生、グラニュラー、ウェーブシーケンス）
  - スペクトル再構成（周波数分解した音の再構成）
- **エフェクトとモジュレーション**:
  - エフェクト: Filter、Drive、EQ、Resonator、Bitcrusher、Chorus、Flanger、Phaser、Delay、Reverb、Compressor、Limiter
  - モジュレーション: Velocity、LFO、Envelope等でパラメータを動かす
- **演奏と検証を同じCoreで実行**: `device list`でAudio / MIDI Deviceを確認し、`play`でMIDI演奏、PatternをMIDI Keyboardなしで試聴し、単音・Event Sequence・MIDI Fileをオフラインで再現できる

## インストール

対応プラットフォームは Linux x86_64 / arm64、macOS arm64、Windows x86_64です（Windows は Git Bash から実行します）。次のコマンドで、最新リリースのバイナリが `~/.local/bin`（Windows では `C:\Users\<ユーザー名>\.local\bin`）へインストールされます。

```bash
curl -fsSL https://raw.githubusercontent.com/endo-ly/sonalloy/main/scripts/install.sh | bash
```

`sonalloy` をどこからでも使えるようにするため、インストール先をPATHへ追加してください。

| 環境 | PATHへの追加方法 |
|---|---|
| Linux / macOS | 次の行を `~/.bashrc`（または `~/.zshrc`）へ追加: `export PATH="$HOME/.local/bin:$PATH"` |
| Windows | 「環境変数」設定の「Path」へ `C:\Users\<ユーザー名>\.local\bin` を追加 |

アップデートは同じコマンドを再実行します。アンインストールする場合は、バイナリ（`sonalloy` / Windowsでは`sonalloy.exe`）を削除してください。

## クイックスタート

3つのコマンドで、最初の音源を作って鳴らします。

```bash
# 音源定義のひな形を生成する（Saw波形・同時発音数16）
sonalloy instrument init my-synth.json

# 定義を検証する
sonalloy instrument validate my-synth.json

# 1音をレンダリングしてWAVを生成する
sonalloy render note my-synth.json --output my-synth.wav
```

生成された `my-synth.wav` を再生して音を確認します。以降は、`my-synth.json` を編集して「検証 → Inspect → レンダリング（必要ならAnalysis / Trace） → 試聴」を繰り返して音を作り込みます。定義ファイルの書き方は[音源定義](docs/instrument-definition.md)、コマンドの詳細は[CLI](docs/cli.md)、手順全体は[音源の作り方](.agents/skills/create-instrument/SKILL.md)を参照してください。

MIDI Keyboardがない場合は、1つのInstrumentを試奏するPatternを作成してAudio Deviceへ直接送れます。

```bash
sonalloy pattern init phrase.json
sonalloy pattern validate phrase.json
sonalloy audition pattern my-synth.json phrase.json --loop
sonalloy render pattern my-synth.json phrase.json --output phrase.wav
sonalloy pattern export-midi phrase.json --output phrase.mid
```

PatternのSchemaとMIDI Interchangeは[Audition Pattern](docs/pattern.md)を参照してください。複数InstrumentのTrackやArrangementはHost / DAWの責務です。

MIDI Keyboardで演奏する場合は、Deviceを確認してから次を実行します。

```bash
sonalloy device list
sonalloy play my-synth.json --midi-device <id>
```

## 使い方

| コマンド | 役割 |
|---|---|
| `sonalloy instrument init <path>` | 音源定義のひな形を生成する |
| `sonalloy instrument validate <definition>` | 定義を検証する（JSON構文・制約・Assetの準備） |
| `sonalloy instrument inspect <definition>` | コンパイル後の実行値を表示する |
| `sonalloy pattern init <path>` | 1 Instrument用の試奏Patternを生成する |
| `sonalloy pattern validate <pattern>` | Patternの構造を検証する |
| `sonalloy pattern inspect <pattern>` | Patternの音楽的な長さとEvent概要を表示する |
| `sonalloy pattern import-midi <midi> --output <pattern>` | MIDIの1 ChannelをPatternへ変換する |
| `sonalloy pattern export-midi <pattern> --output <midi>` | PatternをSingle Track MIDIへ変換する |
| `sonalloy render note <definition> --output <wav>` | 1音をレンダリングする |
| `sonalloy render events <definition> <events.json> --output <wav>` | Event Sequenceをレンダリングする（Pitch BendやParameter変更をFrame単位で制御） |
| `sonalloy render midi <definition> <midi-file> --output <wav>` | MIDI Fileをレンダリングする |
| `sonalloy render pattern <definition> <pattern> --output <wav>` | Musical-time Patternをレンダリングする |
| `sonalloy audition pattern <definition> <pattern>` | PatternをAudio Deviceで試聴する（MIDI Keyboard不要） |
| `sonalloy audition midi <definition> <midi-file>` | MIDI Fileを1 Channel選択してAudio Deviceで試聴する |
| `sonalloy device list [--json]` | Audio OutputとMIDI Inputを列挙する |
| `sonalloy play <definition>` | MIDI InputからAudio Outputへリアルタイム演奏する |

`render`コマンドは32-bit float・StereoのWAVを出力します。`play`はDeviceのNative Sample Formatへ変換して出力します。各コマンドのオプションは[CLI](docs/cli.md)を参照してください。

## 技術スタック

| 項目 | 内容 |
|---|---|
| 言語 | Rust（Edition 2024） |
| 音源定義 | JSON（テキストだけで完結する定義形式） |
| DSP | DaisySP、Signalsmith Stretch / Linear |
| Realtime I/O | CPAL、Midir、Crossbeam Queue（CLIのみ） |

## 開発

ソースからビルドする場合は、次の環境が必要です。

- Rust stable（`rustup`で導入）
- CMake 3.14以上
- Windows: Visual Studio C++ Build Tools
- Linux: `g++`または`clang++`、`git`、`pkg-config`、`libasound2-dev`
- macOS: Xcode Command Line Tools

初回のビルドではCMakeがDaisySP V1.0.0の固定Commitを取得するため、ネットワーク接続が必要です。Signalsmith StretchとSignalsmith Linearは固定Revisionをリポジトリへ同梱しているため、これらのビルドではネットワーク接続を必要としません。

```bash
cargo build --workspace --release
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## ドキュメント

| 文書 | 内容 |
|---|---|
| [`docs/cli.md`](docs/cli.md) | CLI仕様：Command・Option・Exit Code |
| [`docs/instrument-definition.md`](docs/instrument-definition.md) | 音源定義のデータ仕様・制約・Compile |
| [`docs/pattern.md`](docs/pattern.md) | 1 Instrument用Audition PatternとMIDI Interchange |
| [`.agents/skills/create-instrument/SKILL.md`](.agents/skills/create-instrument/SKILL.md) | 音源の作り方（手順書） |
| [`docs/runtime-processing.md`](docs/runtime-processing.md) | 実行時仕様：Process Contract・Lifecycle・Error規則 |
| [`docs/testing-and-sound-review.md`](docs/testing-and-sound-review.md) | 検証とReviewの手順 |
| [`docs/architecture.md`](docs/architecture.md) | 静的構造：Crate・依存方向・Native境界 |
| [`docs/release.md`](docs/release.md) | リリース手順 |
| [`docs/CONCEPT.md`](docs/CONCEPT.md) | 要件定義・基本設計 |
| [`docs/plan/plan-dynamic-parameters.md`](docs/plan/plan-dynamic-parameters.md) | Dynamic Parameter / Modulationの詳細設計 |
| [`docs/plan/plan-mvp.md`](docs/plan/plan-mvp.md) | 詳細設計・実装計画 |
