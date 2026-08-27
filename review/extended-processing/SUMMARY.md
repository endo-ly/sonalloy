# Extended Processing Summary

## 自動検証

`scripts/generate_package.py`は次を生成・検証します。

- 全9 DefinitionのSchema 4 Validate結果
- 全9 DefinitionのCompile後Inspect結果
- 48,000 Hz、Block Size 257の試聴用WAVとAnalysis JSON。LadderはCutoff LFO、Formantは3秒のNoteでVowel Position MSEGの`0 → 1 → 0`、Frequency Shifterは長いModal Bellと`-420 → 0 → +420 Hz`のParameter Change、Convolutionは適正な出力レベルのModal impulse、Gateは左右独立のNoiseを使う
- FormantはNote Offを144,000 frameに置き、WAVの活動区間は160,029 frameまで継続します。Frequency Shifterは活動区間が80,554 frameまで続き、Convolutionはピーク`-19.19 dBFS`、`over_full_scale=false`です
- Full HybridのParameter Change Event、Trace、Analysis
- Mono / Stereo IRの決定的生成結果とSHA-256参照
- Layer Alignment LatencyとReported Latencyを含むInspect結果
- LFO、MSEG、Envelope、Macro、Transport Phaseから新Processorへの6 Route

Processor RuntimeのFinite、Reset、任意Block分割、Latency、Tempo変換はCoreのUnit Testでも確認します。複数Sample RateとBlock Sizeの再測定条件はREADMEと生成Scriptに固定しています。

## リソース概算

| Asset / Runtime | 内容 | 概算メモリ |
|---|---|---:|
| `body-short.wav` | 0.18秒、Mono、34 partitions | Prepared IR 139,264 bytes / Stereo Runtime 303,104 bytes |
| `room-medium.wav` | 1.00秒、Stereo、188 partitions | Prepared IR 1,540,096 bytes / Stereo Runtime 1,564,672 bytes |
| Delay 1個 | 48,000 Hz、16秒上限、Stereo | 6,144,032 bytes |
| Delay 4個 | 96,000 Hz、16秒上限、Stereo | 49,152,128 bytes |

ConvolutionのPrepared IRは256-frame partitionと512-point FFTの複素float32配列、Runtimeは左右各自の周波数領域履歴、FFT作業領域、再利用scratchを基準に算出しています。Delayの上限はProcessorごとに16秒で、DefinitionのGlobal Delayは最大4個です。

## 試聴記録

| 対象 | 確認内容 | 結果 |
|---|---|---|
| Ladder Filter | Resonance Sweep、Driveの倍音、高Resonanceの安定性 | 未確認 |
| Formant Processor | 母音Profileの連続変化、Shift、Mix時のLevel | 未確認 |
| Frequency Shifter | Inharmonicな移動、0 Hz付近のClick、Latency | 未確認 |
| Convolution | Bodyの色付け、Dry / Wet整合、Tail | 未確認 |
| Gate | Hysteresis、Hold、Release、Stereo image | 未確認 |
| Transient Shaper | Attack boost、Sustain cut、Clipping | 未確認 |
| Delay | Beat同期、Ping-Pong、Multi-Tap、Tempo変更 | 未確認 |
| Full Hybrid | Layer / Voice / Globalの一体感、総合Tail | 未確認 |

試聴結果を記録するまで、音楽的な品質判定は保留とします。
