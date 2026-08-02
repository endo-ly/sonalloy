# Review Tooling

このディレクトリには、音声Reviewを同じ入力条件で再生成するための補助Scriptだけを置きます。RuntimeやCLIの実行経路からは参照されません。

| Script | 責務 |
|---|---|
| `generate_midi_fixtures.py` | Basic Poly Synth、Metallic Hybrid、Expressive Hybrid Leadの固定MIDI入力を生成する |
| `generate_basic_poly_synth_package.py` | Basic Poly SynthのDefinition、MIDI、WAVをReview Packageへコピー・生成する |
| `generate_basic_poly_synth_metrics.py` | Basic Poly SynthのWAV MetricsとBlock Size比較を生成する |
| `generate_metallic_hybrid_inputs.py` | Metallic Hybridの決定論的AssetとMIDI入力を生成する |
| `generate_metallic_hybrid_package.py` | Metallic HybridのDefinition、MIDI、Asset、WAV、MetricsをReview Packageへまとめる |
| `generate_dynamic_parameters_package.py` | Dynamic ParameterのEvent、MIDI、Reference Instrument、WAV、MetricsをReview Packageへまとめる |
| `manifest.py` | Basic Poly Synthの固定Render条件と共通Render処理を定義する |
| `measure_wav.py` | WAVのMetadata、Finite性、Peak、RMS、DC、周波数、境界差分を測定する |

これらは本体機能ではありませんが、Review資料を同じ条件で再生成し、測定方法を追跡できるようにするためRepositoryへ含めます。生成済みWAVやMetricsと一緒に変更履歴へ残す対象です。

## 再生成

```bash
python scripts/review/generate_basic_poly_synth_package.py
python scripts/review/generate_metallic_hybrid_package.py
python scripts/review/generate_dynamic_parameters_package.py
```

生成先は`review-output/basic-poly-synth/`、`review-output/metallic-hybrid/`、`review-output/dynamic-parameters/`です。Package内のDefinitionは同梱された入力を参照するため、コピー後の内容だけでも再確認できます。

Metallic Hybridの生成時は、`instrument inspect --json`のSample Layer状態、許容されたAsset Warning、DefinitionとSource AssetのSHA-256一致、Sample-only出力の非無音性、Hybrid MixとOscillator-onlyの差分も自動検査します。検査に失敗した場合はMetricsやReview資料を更新せず終了します。
