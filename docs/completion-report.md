# Sonalloy Core 完了報告

## 完了内容

- JSON DefinitionのParse、Validation、Compileを実装し、検証済みのRuntime構造へ変換します。
- DaisySPのSine / PolyBLEP SawとPrepared Sampleを同一Voice上のLayerとして処理します。
- WAV AssetのSHA-256検証、PCM16 / PCM24 / Float32 Decode、Mono Downmix、必要なSample Rate変換をCompile時に実行します。
- Polyphonic Voice、ADSR、Velocity Response、Constant-power Pan、Voice Filter、Note Event、Voice Stealingを実装します。
- CLIからDefinitionのValidate / Inspect、Note Render、MIDI Render、JSON診断、Stereo Float WAV出力を利用できます。
- Basic Poly SynthとMetallic Hybridの再現可能な試聴資料、Metrics、Definition、MIDI、Asset、生成Scriptを保存します。

## 参照Instrument

| Instrument | Definition | 試聴資料 |
|---|---|---|
| Basic Poly Synth | `examples/instruments/basic-poly-synth.json` | `review-output/basic-poly-synth/` |
| Metallic Hybrid | `examples/instruments/metallic-hybrid.json` | `review-output/metallic-hybrid/` |

音声ごとの作成意図と期待結果は、それぞれの`review-summary.md`と`docs/testing-and-sound-review.md`に記録しています。

## 自動検証

| 環境 | 結果 |
|---|---|
| Windows: `cargo fmt --all -- --check` | Pass |
| Windows: `cargo test --workspace` | Pass、105 tests |
| Windows: `cargo clippy --workspace --all-targets -- -D warnings` | Pass |
| Windows: `cargo build --workspace --release` | Pass |
| Ubuntu 22.04 WSL: `cargo test --workspace` | Pass、105 tests |
| Ubuntu 22.04 WSL: `cargo clippy --workspace --all-targets -- -D warnings` | Pass |
| Ubuntu 22.04 WSL: `cargo build --workspace --release` | Pass |
| Basic Poly Synth / Metallic Hybrid review package生成 | Pass |

Review MetricsではWAV Metadata、Finite性、Peak / RMS / DC、推定基本周波数、隣接Frame差分、複数Block Sizeでの再現性を確認しています。音質の最終判断は試聴結果に記録しています。

## 試聴結果

- Basic Poly Synth：利用者確認済み、承認、修正指示なし。
- Metallic Hybrid：利用者確認済み、全確認項目を承認。
- `02-sample-decoded-root.wav`と`03-sample-pitch-range.wav`の音量が小さく聞こえることは、Sample Layer Gain、One-shot後の無音、音量補正なしという設計結果です。
- `06-hybrid-mix.wav`はSample AttackとOscillator Bodyを一つのNote Onから開始するため、一音のInstrumentとして聞こえることが設計結果です。

## 対象外

Noise Layer、複数Sample Zone、Round-robin、Loop Sample、連続Parameter Modulation、Effect、Realtime Audio Device、Plugin Hostは対象外です。

## 再現コマンド

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
python scripts/review/generate_basic_poly_synth_package.py
python scripts/review/generate_metallic_hybrid_package.py
```

レビュー用Scriptの責務とコミット対象の扱いは`scripts/review/README.md`に記録しています。レビュー資料のMetricsは生成Scriptから再生成し、音声と入力Fixtureは同じPackage内に保持します。

## 主要Revision

- 実装：`aa025a81ba88ce80d70974053400b7710b8aca26`
- レビュー資料：`e3cd9adb78a08b5720b6d5ce01f25c6ba2897acb`
- 完了日：2026-08-01
