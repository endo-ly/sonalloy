# Sonalloy

Sonalloyは、音声素材と音響合成をLayerとして組み合わせ、演奏可能なInstrumentへ変換する音源エンジンです。CoreはFrontendやAudio Deviceから独立したRust APIを持ち、CLIはそのAPIを使ってOffline Renderを実行します。

## 処理経路

```text
sonalloy instrument Definition
  → Parse / Validate / Compile
  → Asset Decode / Resample
  → Offline Renderer
  → Process Contract
  → Safe Rust DSP Wrapper
  → Internal C ABI
  → DaisySP Oscillator / Prepared Sample / Voice Filter
  → Stereo WAV
```

Compile時にAssetのSHA-256検証、WAV Decode、StereoからMonoへのDownmix、必要なSample Rate変換を完了します。Process中はJSON解析、File I/O、Asset Decode、Resample、Hash計算、Native Heap Allocationを行いません。RuntimeはDaisySPのSine/SawとOne-shot Sampleを複数Layerとして同じVoiceへ処理し、ADSR、Velocity Response、Pan、左右独立Voice FilterをStereoへ適用します。

## Basic Poly Synth

```bash
cargo run -p sonalloy-cli -- instrument validate \
  examples/instruments/basic-poly-synth.json

cargo run -p sonalloy-cli -- render midi \
  examples/instruments/basic-poly-synth.json \
  testdata/midi/basic-poly-synth-phrase.mid \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/basic-poly-synth.wav
```

JSON Definitionから一つのSine/Saw Oscillator LayerをCompileし、Polyphonic Voice、ADSR、Velocity Response、Constant-power Pan、Voice Low-pass Filter、Sample Accurate Note Eventを経由してStereo WAVを生成します。

## Metallic Hybrid

Sample AttackとOscillator Bodyを一つのVoiceへ組み合わせるDefinitionです。

```bash
cargo run -p sonalloy-cli -- instrument validate \
  examples/instruments/metallic-hybrid.json --json

cargo run -p sonalloy-cli -- instrument inspect \
  examples/instruments/metallic-hybrid.json --json

cargo run -p sonalloy-cli -- render midi \
  examples/instruments/metallic-hybrid.json \
  testdata/midi/metallic-hybrid-phrase.mid \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/metallic-hybrid.wav
```

同じ入力から音声とMetricsを再生成する場合は、`python scripts/review/generate_metallic_hybrid_package.py`を実行します。レビュー用の音声、Definition、MIDI、Metricsは`review-output/metallic-hybrid/`に保存されます。

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
| `testdata/assets` | WAV Sample FixtureとSHA-256の基準 |
| `testdata/midi` | MIDI入力Fixture |
| `review-output/basic-poly-synth` | 試聴用WAV、Metrics、確認資料 |
| `review-output/metallic-hybrid` | Sample / HybridのWAV、Metrics、確認資料 |
| `scripts/review` | 再現可能なレビュー用Fixture、Render、Metricsの生成 |
| `testdata/expected` | 自動Testの期待Metrics |
| `docs/architecture.md` | 依存方向と所有権 |
| `docs/runtime-processing.md` | LifecycleとBuffer Contract |
| `docs/cli.md` | CLI仕様 |
| `docs/instrument-definition.md` | Definition、Validation、Compile仕様 |
| `docs/testing-and-sound-review.md` | TestとReview Artifact仕様 |
| `docs/completion-report.md` | 完了範囲、検証結果、試聴結果 |

詳細な製品要件は [`docs/CONCEPT.md`](docs/CONCEPT.md)を参照してください。
