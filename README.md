# Sonalloy

Sonalloyは、音声素材と音響合成をLayerとして組み合わせ、演奏可能なInstrumentへ変換する音源エンジンです。JSONのDefinitionをCompileし、Basic / Complex Oscillator、Wavetable、4 Operator Modulation、Stereo Sample、Granular Generatorを同じVoiceで合成して、Stereo WAVをOffline Renderします。SampleはRegion、Reverse、Loop、Constant-power Crossfade Loop、Release Trigger、Fixed Stretch、Tempo Syncを備え、GranularはPosition、Grain Size、Density、Pitch、Randomness、Pan Spreadを備えます。Complex OscillatorはPhase Distortion、Wavefold、Oscillator Feedbackを含みます。

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
| [`docs/creating-an-instrument.md`](docs/creating-an-instrument.md) | 音源の作り方（ガイド） |
| [`docs/architecture.md`](docs/architecture.md) | 静的構造：Crate・依存方向・Native境界 |
| [`docs/runtime-processing.md`](docs/runtime-processing.md) | 実行時仕様：Process Contract・Lifecycle・Error規則 |
| [`docs/cli.md`](docs/cli.md) | CLI仕様：Command・Option・Exit Code |
| [`docs/instrument-definition.md`](docs/instrument-definition.md) | Definitionのデータ仕様・制約・Compile |
| [`docs/testing-and-sound-review.md`](docs/testing-and-sound-review.md) | 検証とReviewの手順 |
| [`docs/CONCEPT.md`](docs/CONCEPT.md) | 要件定義・基本設計 |
| [`docs/plan/plan-dynamic-parameters.md`](docs/plan/plan-dynamic-parameters.md) | Dynamic Parameter / Modulationの詳細設計 |
| [`docs/plan/plan-mvp.md`](docs/plan/plan-mvp.md) | 詳細設計・実装計画 |
