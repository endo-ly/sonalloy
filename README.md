# Sonalloy

Sonalloyは、JSONで書いた音源定義を読み込んで、オフラインで音源WAVを生成する音源エンジンです。複数の音の層（Layer）を重ね、エフェクトとモジュレーションを組み合わせて、1つの音源を作ります。

## 特長

- **AIファースト設計**: 音源はJSONで定義し、CLIで検証・確認する。テキストだけで完結するため、AIが音源を設計・生成・検査できる
- **多彩な合成方式を組み合わせる**: 次の方式をLayerとして重ね、1つの音源を作る
  - 波形オシレータ（基本波形、FM、ハードシンク、ウェーブテーブル）
  - 加算合成・フォルマント（倍音設計、母音共鳴）
  - サンプリング（サンプル再生、グラニュラー、ウェーブシーケンス）
  - スペクトル再構成（周波数分解した音の再構成）
- **エフェクトとモジュレーション**:
  - エフェクト: Filter、Drive、Delay、Reverb
  - モジュレーション: Velocity、LFO、Envelope等でパラメータを動かす
- **決定的なレンダリング**: 同じ入力から常に同じWAVを生成。単音、イベントシーケンス、MIDIファイルに対応

## クイックスタート

```bash
cargo build --workspace

# 動作確認：Sine WAVを生成する
cargo run -p sonalloy-cli -- dev render-sine \
  --frequency 440 --duration 1.0 --sample-rate 48000 \
  --block-size 257 --output out/sine.wav

# 既存音源からWAVを生成する
cargo run -p sonalloy-cli -- render midi \
  examples/instruments/basic-poly-synth.json \
  testdata/midi/basic-poly-synth-phrase.mid \
  --output out/basic-poly-synth.wav

# 発音中のParameter / Control Eventを再現する
cargo run -p sonalloy-cli -- render events \
  examples/instruments/expressive-hybrid-lead.json \
  testdata/events/expressive-hybrid-lead.json \
  --duration-frames 96000 --output out/expressive-lead.wav
```

## 必要なツール

- Rust stable（`rustup`で導入）
- CMake 3.14以上
- Windows: Visual Studio C++ Build Tools
- Linux: `g++`または`clang++`、`git`

初回のNative BuildではCMakeがDaisySP V1.0.0の固定Commitを取得するため、Network接続が必要です。Signalsmith StretchとSignalsmith Linearは固定RevisionをRepositoryへ同梱しているため、これらのBuildにはNetwork接続を必要としません。

## BuildとTest

```bash
cargo build --workspace
cargo build --workspace --release
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## ドキュメント

| 文書 | 内容 |
|---|---|
| [`.agents/skills/create-instrument/SKILL.md`](.agents/skills/create-instrument/SKILL.md) | 音源の作り方（手順書） |
| [`docs/architecture.md`](docs/architecture.md) | 静的構造：Crate・依存方向・Native境界 |
| [`docs/runtime-processing.md`](docs/runtime-processing.md) | 実行時仕様：Process Contract・Lifecycle・Error規則 |
| [`docs/cli.md`](docs/cli.md) | CLI仕様：Command・Option・Exit Code |
| [`docs/instrument-definition.md`](docs/instrument-definition.md) | Definitionのデータ仕様・制約・Compile |
| [`docs/testing-and-sound-review.md`](docs/testing-and-sound-review.md) | 検証とReviewの手順 |
| [`docs/CONCEPT.md`](docs/CONCEPT.md) | 要件定義・基本設計 |
| [`docs/plan/plan-dynamic-parameters.md`](docs/plan/plan-dynamic-parameters.md) | Dynamic Parameter / Modulationの詳細設計 |
| [`docs/plan/plan-mvp.md`](docs/plan/plan-mvp.md) | 詳細設計・実装計画 |
