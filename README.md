# Sonalloy

**AIファーストのシンセサイザー。** 音源をJSONで定義し、CLIで検証・演奏・レンダリングします。設計も検証も演奏も、すべてテキストだけで完結するため、AIエージェントが音源を「書いて、鳴らして、直す」ところまで一人で行えます。

Sonalloyの音源は、つまみを並べたパネルではなく、**JSONで記述する1つのデータ**です。

- **テキストだけで完結**: 波形、エフェクト、モジュレーション、演奏設定まで、すべて1つのJSONに定義します。GUIは不要です
- **CLIだけで回る改善ループ**: `validate`はエラーの箇所をField Path付きで返し、`inspect`はコンパイル後の実行値を機械可読で出し、`render`はWAVを生成します。AIは「書く → 検証する → 直す」を自分だけで繰り返せます
- **エージェント向けの手順書を同梱**: `.agents/skills/create-instrument/`に手順書と仕様リファレンスを同梱しています。Claude Codeなどのcoding agentに読ませれば、音源設計の手順をまるごと任せられます

## 作れる音

12種類のGeneratorをLayerとして重ね、エフェクトとモジュレーションで仕上げるハイブリッド音源です。

| 分類 | 音源・機能 |
|---|---|
| 波形系 | Oscillator（Hard Sync / Wavefold / Unison）、Operator Modulation（FM / PM / AM / Ring）、Wavetable |
| 加算・声系 | Additive（倍音設計）、Formant（母音共鳴） |
| 物理モデル系 | Physical String（撥弦・硬質振動）、Modal（棒・板・ベルの共鳴） |
| サンプリング系 | Sample（鍵盤マッピング再生）、Granular、Wave Sequence |
| スペクトル系 | Spectral（STFT再構成とA/B Morph） |
| エフェクト | Filter、Ladder Filter、Drive、EQ、Resonator、Chorus、Flanger、Phaser、Delay、Reverb、Convolution、Compressor、Limiter など |
| モジュレーション | LFO、Envelope、MSEG、Step、Sample & Hold、Smooth Random、Velocity、Macro、2-Way / 4-Way Vector |
| 演奏表現 | 最大64声のPolyphonic、Monophonic（Legato / Portamento）、Sustain、Pitch Bend / Aftertouch |
| 外部Audio | Vocoder、Envelope Transfer、Spectral MorphとのCross Synthesis |

## クイックスタート

インストールは1行です（Linux x86_64 / arm64、macOS arm64、Windows x86_64はGit Bashから実行）。

```bash
curl -fsSL https://raw.githubusercontent.com/endo-ly/sonalloy/main/scripts/install.sh | bash
```

`~/.local/bin`（Windowsは`C:\Users\<ユーザー名>\.local\bin`）へインストールされるため、PATHに追加してください。アップデートは`sonalloy update`、アンインストールはバイナリの削除だけです。

3コマンドで、最初の音源が鳴ります。

```bash
sonalloy instrument init my-synth.json      # 音源定義のひな形を生成
sonalloy instrument validate my-synth.json  # 定義を検証
sonalloy render note my-synth.json --output my-synth.wav  # 1音をレンダリング
```

生成された`my-synth.wav`を再生して確認したら、あとはJSONを編集して「検証 → レンダリング → 試聴」を繰り返すだけです。

**AIエージェントと作る場合**、コマンドを覚える必要はありません。coding agentに「暖かいパッド音源を作って」と頼めば、同梱の手順書に沿って定義の作成から検証、試聴用WAVの生成までを行います。

## コマンド

| したいこと | コマンド |
|---|---|
| 音源定義のひな形を生成する | `sonalloy instrument init <path>` |
| 定義を検証する | `sonalloy instrument validate <definition>` |
| コンパイル後の実行値を確認する | `sonalloy instrument inspect <definition>` |
| 1音 / Event列 / MIDI / Patternをレンダリングする | `sonalloy render note` / `events` / `midi` / `pattern` |
| 演奏パターンを試聴する（MIDI Keyboard不要） | `sonalloy pattern init` → `sonalloy audition pattern` |
| MIDI Keyboardで演奏する | `sonalloy device list` → `sonalloy play` |
| インストールしたCLIをアップデートする | `sonalloy update` |

全コマンドのOptionと出力の詳細は[CLIリファレンス](.agents/skills/create-instrument/references/cli.md)を参照してください。

## アプリケーションへの組込み

`sonalloy-capi`と公開ヘッダ`sonalloy.h`により、C / C++ Applicationから同じCompile・Process・Runtime Lifecycleを利用できます。詳細は[docs/c-api.md](docs/c-api.md)を参照してください。

## ドキュメント

音源の作成・検証・演奏など、**Sonalloyを使うための資料**は`.agents/skills/create-instrument/`配下に置きます（agentはここだけで作業を完結できる）。設計・実行時仕様など、**Sonalloyを作るための資料**は`docs/`配下に置きます。

| 文書 | 内容 |
|---|---|
| [`.agents/skills/create-instrument/SKILL.md`](.agents/skills/create-instrument/SKILL.md) | 音源の作り方（手順書）。CLI・音源定義・Patternの仕様リファレンス（`references/`）を同梱 |
| [`docs/runtime-processing.md`](docs/runtime-processing.md) | 実行時の動作：Block処理・Noteの一生・実行上の約束事 |
| [`docs/c-api.md`](docs/c-api.md) | 公開C ABI：Handle所有権・Compile・Process・Runtime Update |
| [`docs/architecture.md`](docs/architecture.md) | 静的構造：Crate・依存方向・Native境界 |
| [`docs/CONCEPT.md`](docs/CONCEPT.md) | 要件定義・基本設計 |

## 開発

ソースからビルドする場合は、次の環境が必要です。

- Rust stable（`rustup`で導入）
- CMake 3.15以上
- Windows: Visual Studio C++ Build Tools
- Linux: `g++`または`clang++`、`git`、`pkg-config`、`libasound2-dev`
- macOS: Xcode Command Line Tools

初回ビルドではCMakeがDaisySP V1.0.0の固定Commitを取得するため、ネットワーク接続が必要です。Signalsmith StretchとSignalsmith Linearは固定Revisionをリポジトリへ同梱しているため、これらのビルドではネットワーク接続を必要としません。

```bash
cargo build --workspace --release
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

| 項目 | 内容 |
|---|---|
| 言語 | Rust（Edition 2024） |
| 音源定義 | JSON（テキストだけで完結する定義形式） |
| DSP | DaisySP、Signalsmith Stretch / Linear |
| Realtime I/O | CPAL、Midir、Crossbeam Queue（CLIのみ） |

## ロードマップ

Sonalloyの先は、**AIと人間の共同作業**です。AIがJSONで設計と検証のループを回し、人間は耳とGUIで方向性を決める。

- **CLAP / VST3対応**: DAWの音源として使える
- **音源機能の整備**: 細部まで仕上げる
- **音作りのGUI**: 人間が耳と手で確かめられる編集画面
- **AIとの音作り体験**: 人間とAIで音を反復編集する
