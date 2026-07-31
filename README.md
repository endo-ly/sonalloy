# Sonalloy

Sonalloyは、音声素材と音響合成をLayerとして組み合わせ、演奏可能なInstrumentへ変換する音源エンジンです。CoreはFrontendやAudio Deviceから独立したRust APIを持ち、CLIはそのAPIを使ってOffline Renderを実行します。

## 処理経路

```text
sonalloy dev render-sine
  → Offline Renderer
  → Process Contract
  → Safe Rust DSP Wrapper
  → Internal C ABI
  → DaisySP Oscillator / Voice Filter
  → Stereo WAV
```

Process中はJSON解析、File I/O、Asset Decode、Native Heap Allocationを行いません。P1 RuntimeはDaisySPのSine/Saw、ADSR、Velocity Response、Pan、左右独立Voice FilterをStereoへ処理します。

## P1 Basic Poly Synth

```bash
cargo run -p sonalloy-cli -- instrument validate \
  examples/instruments/basic-poly-synth.json

cargo run -p sonalloy-cli -- render midi \
  examples/instruments/basic-poly-synth.json \
  testdata/midi/p1-review.mid \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/p1-basic-poly-synth.wav
```

P1はJSON Definitionから一つのSine/Saw Oscillator LayerをCompileし、Polyphonic Voice、ADSR、Velocity Response、Constant-power Pan、Voice Low-pass Filter、Sample Accurate Note Eventを経由してStereo WAVを生成します。

## 必要なツール

- Rust stable（`rustup`で導入）
- CMake 3.14以上
- Windows: Visual Studio C++ Build Tools
- Linux: `g++`または`clang++`、`git`

CMakeがDaisySP V1.0.0の固定Commitを取得するため、初回Native BuildにはNetwork接続が必要です。取得したDaisySPのSourceは使用するOscillatorだけを静的ライブラリへ組み込みます。

## BuildとTest

```bash
cargo build --workspace
cargo build --workspace --release
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Sine Render

```bash
cargo run -p sonalloy-cli -- dev render-sine \
  --frequency 440 \
  --duration 1.0 \
  --sample-rate 48000 \
  --block-size 257 \
  --output sine.wav
```

成功時はStereo、48,000 Frame、48 kHzのWAVが生成されます。`--json`を付けると成功結果と失敗診断をJSONで出力します。

## Repository構成

| Path | 責務 |
|---|---|
| `crates/sonalloy-core` | Process Contract、Runtime、Offline Renderer、Diagnostics |
| `crates/sonalloy-dsp-sys` | Internal C ABI、Safe Rust Wrapper、Native Build接続 |
| `crates/sonalloy-cli` | CLI引数、WAV出力、Exit Code、Diagnostics表示 |
| `native/daisysp-wrapper` | C++ Opaque HandleとDaisySP呼び出し |
| `testdata/definitions` | DefinitionのValid / Invalid Fixture |
| `testdata/midi` | P1 MIDI入力Fixture |
| `review-output/p1` | P1試聴用WAV、Metrics、確認資料 |
| `testdata/expected` | 自動Testの期待Metrics |
| `docs/architecture.md` | 依存方向と所有権 |
| `docs/runtime-processing.md` | LifecycleとBuffer Contract |
| `docs/cli.md` | CLI仕様 |
| `docs/instrument-definition.md` | Definition、Validation、Compile仕様 |
| `docs/testing-and-sound-review.md` | TestとReview Artifact仕様 |

詳細な製品要件は [`docs/CONCEPT.md`](docs/CONCEPT.md)を参照してください。
