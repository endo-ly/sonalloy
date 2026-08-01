# Sonalloy

Sonalloyは、音声素材と音響合成をLayerとして組み合わせ、演奏可能なInstrumentへ変換する音源エンジンです。JSONのDefinitionをCompileし、OscillatorとSampleを同じVoiceで合成して、Stereo WAVをOffline Renderします。

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
```

## 必要なツール

- Rust stable（`rustup`で導入）
- CMake 3.14以上
- Windows: Visual Studio C++ Build Tools
- Linux: `g++`または`clang++`、`git`

初回のNative BuildではCMakeがDaisySP V1.0.0の固定Commitを取得するため、Network接続が必要です。

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
| [`docs/creating-an-instrument.md`](docs/creating-an-instrument.md) | 音源の作り方（ガイド） |
| [`docs/architecture.md`](docs/architecture.md) | 静的構造：Crate・依存方向・Native境界 |
| [`docs/runtime-processing.md`](docs/runtime-processing.md) | 実行時仕様：Process Contract・Lifecycle・Error規則 |
| [`docs/cli.md`](docs/cli.md) | CLI仕様：Command・Option・Exit Code |
| [`docs/instrument-definition.md`](docs/instrument-definition.md) | Definitionのデータ仕様・制約・Compile |
| [`docs/testing-and-sound-review.md`](docs/testing-and-sound-review.md) | 検証とReviewの手順 |
| [`docs/CONCEPT.md`](docs/CONCEPT.md) | 要件定義・基本設計 |
| [`docs/plan/plan-mvp.md`](docs/plan/plan-mvp.md) | 詳細設計・実装計画 |
