# Review Tooling

このディレクトリには、音声Reviewを同じ入力条件で再生成するための補助Scriptだけを置きます。RuntimeやCLIの実行経路からは参照されません。

| Script | 責務 |
|---|---|
| `generate_midi_fixtures.py` | Basic Poly Synth、Metallic Hybrid、Expressive Hybrid Leadの固定MIDI入力を生成する |
| `generate_basic_poly_synth_package.py` | Basic Poly SynthのDefinition、MIDI、WAVをReview Packageへコピー・生成する |
| `generate_basic_poly_synth_metrics.py` | Basic Poly SynthのWAV MetricsとBlock Size比較を生成する |
| `generate_metallic_hybrid_inputs.py` | Metallic Hybridの決定論的AssetとMIDI入力を生成する |
| `generate_metallic_hybrid_package.py` | Metallic HybridのDefinition、MIDI、Asset、WAV、MetricsをReview Packageへまとめる |
| `generate_dynamic_parameters_package.py` | Dynamic ParameterのEvent、MIDI、Reference Instrument、Source／Target別WAV、MetricsをReview Packageへまとめる |
| `generate_processor_chain_package.py` | Processor ChainのDefinition、Event、MIDI、Asset、WAV、MetricsをReview Packageへまとめる |
| `generate_basic_generators_package.py` | Basic GeneratorのDefinition、Event、WAV、MetricsをReview Packageへまとめる |
| `generate_granular_package.py` | Granular GeneratorのDefinition、Event、Asset、WAV、Inspect、MetricsをReview Packageへまとめる |
| `generate_wave_sequence_package.py` | Wave SequenceのDefinition、Event、Asset、WAV、Inspect、Metrics、HybridをReview Packageへまとめる |
| `additive_review.py` | Harmonic / Formant Review PackageへAdditive GeneratorのPartial、Spectrum A / B、Morph、Tilt、Inharmonicity、Partial Envelope、PolyphonyのDefinition、Event、WAV、Inspect、Metricsを提供する |
| `generate_harmonic_formant_package.py` | Additive / Formant GeneratorとHarmonic / Formant HybridのDefinition、Event、MIDI、Layer / Voice / Global Processor、Modulation、WAV、Inspect、Release Performance、Metrics、既存Reference回帰を一つのReview Packageへまとめる |
| `generate_complex_oscillator_package.py` | Digital Synthesis Packageの生成中に取り込むComplex OscillatorのDefinition、Event、WAV、Metrics、性能計測を生成する内部Script |
| `common.py` | Review Package生成で共有するCLI実行、入力出力、Render、WAV測定補助を定義する |
| `generate_essential_synthesis_sampling_package.py` | Sample Zone、Velocity Layer、Round Robin、Forward / Reverse Playback、Loop / Crossfade、Release Trigger、Slice、HybridのDefinition、Event、Synthetic Asset、WAV、MetricsをReview Packageへまとめる |
| `generate_digital_synthesis_package.py` | Wavetable、4 Operator Modulation、Complex Oscillator、Digital HybridのAsset、Definition、Event、WAV、Inspect、Metricsを一つのReview Packageへまとめる |
| `generate_spectral_reference_assets.py` | Spectral Reference Instrumentが使用する決定論的Stereo sourceとLatency impulse fixtureを生成する |
| `generate_spectral_resynthesis_package.py` | Spectral Generator、Spectral Hybrid、既存Generator回帰、MIDI、Processor、Modulation、Block Size、Sample Rate、Fresh Runtime、Release PerformanceのDefinition、Event、Asset、WAV、Inspect、MetricsをReview Packageへまとめる |
| `manifest.py` | Basic Poly Synthの固定Render条件と共通Render処理を定義する |
| `measure_wav.py` | WAVのMetadata、Finite性、Peak、RMS、DC、周波数、境界差分を測定する |

これらは本体機能ではありませんが、Review資料を同じ条件で再生成し、測定方法を追跡できるようにするためRepositoryへ含めます。生成済みWAVやMetricsと一緒に変更履歴へ残す対象です。

## 再生成

```bash
python scripts/review/generate_basic_poly_synth_package.py
python scripts/review/generate_metallic_hybrid_package.py
python scripts/review/generate_dynamic_parameters_package.py
python scripts/review/generate_processor_chain_package.py
python scripts/review/generate_basic_generators_package.py
python scripts/review/generate_granular_package.py
python scripts/review/generate_wave_sequence_package.py
python scripts/review/generate_harmonic_formant_package.py
python scripts/review/generate_essential_synthesis_sampling_package.py
py -3 scripts/review/generate_digital_synthesis_package.py
python3 scripts/review/generate_spectral_reference_assets.py
python3 scripts/review/generate_spectral_resynthesis_package.py
```

生成先は`review-output/basic-poly-synth/`、`review-output/metallic-hybrid/`、`review-output/dynamic-parameters/`、`review-output/processor-chain/`、`review-output/basic-generators/`、`review-output/granular-generator/`、`review-output/wave-sequence/`、`review-output/harmonic-formant-synthesis/`、`review-output/essential-synthesis-sampling/`、`review-output/digital-synthesis/`、`review-output/spectral-resynthesis/`です。Digital Synthesis PackageにはComplex Oscillatorの成果物も含まれ、Package内のDefinitionは同梱された入力を参照するため、コピー後の内容だけでも再確認できます。Wave Sequence Packageも同梱Assetを参照し、SequenceとHybridの検証を単独で再現できます。Harmonic / Formant PackageはAdditive、Formant、HybridのDefinition、Event、MIDI、Asset、Release Performance Fixtureを同梱し、各Generator、Processor、Block Size、Sample Rate、Fresh Runtime、既存Reference回帰を同じMetricsへ記録します。Spectral Resynthesis PackageはStereo A/B source、決定論的Latency impulse、Spectral Hybrid、全Spectral Parameter、既存Generator回帰、MIDI、Identity / Freeze / Blur / Shift / Pitch / Morph / LatencyのMetrics、Release Performance Metricsを同梱します。Performance測定の音声はPackageへ保存しません。

Metallic Hybridの生成時は、`instrument inspect --json`のSample Layer状態、許容されたAsset Warning、DefinitionとSource AssetのSHA-256一致、Sample-only出力の非無音性、Hybrid MixとOscillator-onlyの差分も自動検査します。検査に失敗した場合はMetricsやReview資料を更新せず終了します。
