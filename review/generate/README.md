# Review Tooling

このディレクトリには、音声Reviewを同じ入力条件で再生成するための補助Scriptだけを置きます。RuntimeやCLIの実行経路からは参照されません。`testdata/`のFixture（MIDI、Asset）の生成Scriptは[`testdata/generate/`](../testdata/generate/README.md)へ置きます。

| Script | 責務 |
|---|---|
| `generate_basic_poly_synth_package.py` | Basic Poly SynthのDefinition、MIDI、WAVをReview Packageへコピー・生成する |
| `generate_basic_poly_synth_metrics.py` | Basic Poly SynthのWAV MetricsとBlock Size比較を生成する |
| `generate_metallic_hybrid_package.py` | Metallic HybridのDefinition、MIDI、Asset、WAV、MetricsをReview Packageへまとめる |
| `generate_dynamic_parameters_package.py` | Dynamic ParameterのEvent、MIDI、Reference Instrument、Source／Target別WAV、MetricsをReview Packageへまとめる |
| `generate_processor_chain_package.py` | Processor ChainのDefinition、Event、MIDI、Asset、WAV、MetricsをReview Packageへまとめる |
| `generate_processor_expansion.py` | Filter Mode、EQ、Resonator、Bitcrusher、Modulation FX、Dynamics、Full Chain 3のDefinition、Event、Asset、WAV、Inspect、MetricsをReview Packageへまとめる |
| `generate_basic_generators_package.py` | Basic GeneratorのDefinition、Event、WAV、MetricsをReview Packageへまとめる |
| `generate_granular_package.py` | Granular GeneratorのDefinition、Event、Asset、WAV、Inspect、MetricsをReview Packageへまとめる |
| `generate_wave_sequence_package.py` | Wave SequenceのDefinition、Event、Asset、WAV、Inspect、Metrics、HybridをReview Packageへまとめる |
| `additive_review.py` | Harmonic / Formant Review PackageへAdditive GeneratorのPartial、Spectrum A / B、Morph、Tilt、Inharmonicity、Partial Envelope、PolyphonyのDefinition、Event、WAV、Inspect、Metricsを提供する |
| `generate_harmonic_formant_package.py` | Additive / Formant GeneratorとHarmonic / Formant HybridのDefinition、Event、MIDI、Layer / Voice / Global Processor、Modulation、WAV、Inspect、Release Performance、Metrics、既存Reference回帰を一つのReview Packageへまとめる |
| `generate_complex_oscillator_package.py` | Digital Synthesis Packageの生成中に取り込むComplex OscillatorのDefinition、Event、WAV、Metrics、性能計測を生成する内部Script |
| `common.py` | Review Package生成で共有するCLI実行、入力出力、Render、WAV測定補助を定義する |
| `generate_essential_synthesis_sampling_package.py` | Sample Zone、Velocity Layer、Round Robin、Forward / Reverse Playback、Loop / Crossfade、Release Trigger、Slice、HybridのDefinition、Event、Synthetic Asset、WAV、MetricsをReview Packageへまとめる |
| `generate_digital_synthesis_package.py` | Wavetable、4 Operator Modulation、Complex Oscillator、Digital HybridのAsset、Definition、Event、WAV、Inspect、Metricsを一つのReview Packageへまとめる |
| `generate_spectral_resynthesis_package.py` | Spectral Generator、Spectral Hybrid、既存Generator回帰、MIDI、Processor、Modulation、Block Size、Sample Rate、Fresh Runtime、Release PerformanceのDefinition、Event、Asset、WAV、Inspect、MetricsをReview Packageへまとめる |
| `generate_physical_modal_package.py` | Physical String、Modal、String + Modal HybridのDefinition、Validate / Inspect、Parameter Change、WAV、Block Size、Sample Rate、Reset、Repeat、48 / 96 kHzのVoice数別Performance MatrixをReview Packageへまとめる |
| `generate_performance_modulation_package.py` | Mono Portamento、MSEG、Step、Sample & Hold、Smooth Random、Macro、Vector、Tempo / Meter変更のDefinition、Pattern / Events、製品CLIのValidate / Inspect / Analysis / Trace、WAV、MetricsをReview Packageへまとめる |
| `../extended-processing/scripts/generate_package.py` | Schema 5のLadder Filter、Formant Processor、Frequency Shifter、Convolution、Gate、Transient Shaper、Tempo Delay、Multi-Tap DelayのDefinition、IR、Inspect、Trace、WAV、Analysisを生成する |
| `../external-audio-cross-synthesis/scripts/generate_package.py` | External Audioの決定論的Asset、Envelope Follower、Sidechain、Vocoder、Envelope Transfer、Spectral MorphのDefinition、Event、WAV、Analysis、Trace、Metricsを生成する |
| `manifest.py` | Basic Poly Synthの固定Render条件と共通Render処理を定義する |
| `measure_wav.py` | WAVのMetadata、Finite性、Peak、RMS、DC、周波数、境界差分を測定する |

これらは本体機能ではありませんが、Review資料を同じ条件で再生成し、測定方法を追跡できるようにするためRepositoryへ含めます。生成済みWAVやMetricsと一緒に変更履歴へ残す対象です。共通のRenderではCLIの`--analyze --json`を実行して製品のAnalysis Reportを取得し、Metricsへ保存します。`measure_wav.py`はそのReportの代替計算を増やさず、Review固有の比較（Package間の差分、Block境界、回帰判定）だけを担当します。

## 再生成

```bash
python review/generate/generate_basic_poly_synth_package.py
python review/generate/generate_metallic_hybrid_package.py
python review/generate/generate_dynamic_parameters_package.py
python review/generate/generate_processor_chain_package.py
python3 review/generate/generate_processor_expansion.py
python review/generate/generate_basic_generators_package.py
python review/generate/generate_granular_package.py
python review/generate/generate_wave_sequence_package.py
python review/generate/generate_harmonic_formant_package.py
python review/generate/generate_essential_synthesis_sampling_package.py
py -3 review/generate/generate_digital_synthesis_package.py
python3 review/generate/generate_spectral_resynthesis_package.py
python3 review/generate/generate_physical_modal_package.py
python3 review/generate/generate_performance_modulation_package.py
python3 review/external-audio-cross-synthesis/scripts/generate_package.py
```

生成先は`review/basic-poly-synth/`、`review/metallic-hybrid/`、`review/dynamic-parameters/`、`review/processor-chain/`、`review/processor-expansion/`、`review/basic-generators/`、`review/granular-generator/`、`review/wave-sequence/`、`review/harmonic-formant-synthesis/`、`review/essential-synthesis-sampling/`、`review/digital-synthesis/`、`review/spectral-resynthesis/`、`review/physical-modal/`、`review/performance-modulation/`、`review/external-audio-cross-synthesis/`です。Digital Synthesis PackageにはComplex Oscillatorの成果物も含まれ、Package内のDefinitionは同梱された入力を参照するため、コピー後の内容だけでも再確認できます。Wave Sequence Packageも同梱Assetを参照し、SequenceとHybridの検証を単独で再現できます。Harmonic / Formant PackageはAdditive、Formant、HybridのDefinition、Event、MIDI、Asset、Release Performance Fixtureを同梱し、各Generator、Processor、Block Size、Sample Rate、Fresh Runtime、既存Reference回帰を同じMetricsへ記録します。Spectral Resynthesis PackageはStereo A/B source、決定論的Latency impulse、Spectral Hybrid、全Spectral Parameter、既存Generator回帰、MIDI、Identity / Freeze / Blur / Shift / Pitch / Morph / LatencyのMetrics、Release Performance Metricsを同梱します。Processor Expansion PackageはProcessor固有の比較とFull Chain 3音色を同梱します。Physical / Modal PackageはTechnical Definition、3つのMusical Definition、CLI Validate / Inspect、Parameter Change、Trace Final Value、Block Size、Sample Rate、Reset、Repeat、48 / 96 kHzにおけるPhysical 1 / 8 / 16 / 32 Voice、Modal 12 / 24 Mode × 1 / 8 / 16 VoiceのPerformanceを同じMetricsへ記録します。Performance / Modulation Packageは、Mono Portamento、MSEG、Step、Sample & Hold、Smooth Random、Macro、Vector、Tempo / Meter変更について、製品CLIのValidate / Inspect / Analysis / TraceとWAVのBlock Size比較を同じ条件で記録します。External Audio / Cross Synthesis Packageは、決定論的なMono / Stereo入力、Envelope Follower、External Sidechain、Vocoder、Envelope Transfer、Spectral Morphを同じ条件で記録します。Performance測定の音声はPackageへ保存しません。

Metallic Hybridの生成時は、`instrument inspect --json`のSample Layer状態、許容されたAsset Warning、DefinitionとSource AssetのSHA-256一致、Sample-only出力の非無音性、Hybrid MixとOscillator-onlyの差分も自動検査します。検査に失敗した場合はMetricsやReview資料を更新せず終了します。
