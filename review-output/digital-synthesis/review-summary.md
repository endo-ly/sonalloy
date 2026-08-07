# Digital Synthesis Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Package範囲：Wavetable、4 Operator Modulation、Complex Oscillator、Digital Hybrid

Definitionは`definitions/`、Assetは`assets/`、Eventは`events/`、MIDI入力は`midi/`、WAVは`audio/technical/`へ保存しています。同じ生WAVをMetricsと人間の試聴に使用します。

再生成：

```bash
py -3 scripts/review/generate_digital_synthesis_package.py
```

## 音声一覧

| WAV | 目的 |
|---|---|
| `01-sine-single-frame.wav` | Sine single frame |
| `02-saw-single-frame-low.wav` | Saw low note |
| `03-saw-single-frame-high.wav` | Saw high note |
| `04-position-0.wav` | Wavetable position 0 |
| `05-position-05.wav` | Wavetable position 0.5 |
| `06-position-1.wav` | Wavetable position 1 |
| `07-position-sweep.wav` | Wavetable position sweep |
| `08-position-lfo.wav` | Wavetable position LFO |
| `09-unison-5-stereo.wav` | Wavetable unison 5 stereo |
| `10-band-boundary-sweep.wav` | Wavetable band boundary |
| `11-operator-pm-stack4-bell.wav` | PM Stack 4 bell |
| `12-operator-fm-stack4-bass.wav` | FM Stack 4 bass |
| `13-operator-am-two-stacks.wav` | AM two stacks |
| `14-operator-ring-two-stacks.wav` | Ring two stacks |
| `15-operator-algorithm-stack4.wav` | Stack 4 topology |
| `16-operator-algorithm-two-stacks.wav` | Two stacks topology |
| `17-operator-algorithm-shared.wav` | Shared modulator topology |
| `18-operator-ratio-sweep.wav` | Operator ratio sweep |
| `19-operator-modulation-amount-sweep.wav` | Operator modulation amount sweep |
| `20-operator-feedback-sweep.wav` | Operator feedback sweep |
| `21-operator-envelope-bell.wav` | Operator envelope |
| `22-operator-unison-4.wav` | Operator unison 4 |
| `23-operator-polyphony-stealing.wav` | Operator polyphony and voice stealing |
| `24-phase-distortion-025.wav` | Phase distortion 0.25 |
| `25-phase-distortion-075.wav` | Phase distortion 0.75 |
| `26-phase-distortion-sweep.wav` | Phase distortion sweep |
| `27-feedback-03.wav` | Oscillator feedback 0.3 |
| `28-feedback-08.wav` | Oscillator feedback 0.8 |
| `29-feedback-sweep.wav` | Oscillator feedback sweep |
| `30-wavefold-025.wav` | Wavefold 0.25 |
| `31-wavefold-075.wav` | Wavefold 0.75 |
| `32-wavefold-sweep.wav` | Wavefold sweep |
| `33-waveshaping-wavefold.wav` | Waveshaping and wavefold |
| `34-hard-sync-wavefold.wav` | Hard sync and wavefold |
| `35-unison-wavefold.wav` | Unison and wavefold |
| `36-wavetable-motion-bass.wav` | Wavetable motion bass |
| `37-four-operator-fm-bell.wav` | Four-operator FM bell |
| `38-phase-distortion-lead.wav` | Phase-distortion lead |
| `39-digital-hybrid-lead.wav` | Digital hybrid lead |
| `40-digital-hybrid-phrase.wav` | Digital hybrid phrase |

Regression WAVは`audio/technical/regression-*.wav`、`audio/technical/sample-rate-*.wav`です。Metricsは`metrics.json`に保存しています。

## 自動確認

- 全40件のWAVがFiniteで、Metricsを再生成済み
- 基準周波数が成立する単音RenderのSpectrum、Spectral Centroid、Harmonic / Non-harmonic Energy参考値をMetricsに記録
- Wavetable / Operator / ComplexのDefinition ValidateとInspect JSONを確認済み
- WavetableのFrame、Position、Band、Missing Asset診断を確認済み
- Wavetable / Operator / ComplexのParameter Sweep境界差分を確認済み
- OperatorのPM / FM / AM / Ring、8 topology、Unison、Reset、Allocation 0を確認済み
- Operatorの1 / 8 / 16 Voice × Unison 1 / 4のCLI性能値を記録済み
- ComplexのPhase Distortion、Feedback、Wavefold、Hard Sync / Unison組合せを確認済み
- Block Size、Sample Rate、Fresh Runtime、Reset、ネイティブ有限値境界を自動検査済み
- Digital Hybrid ReferenceをWavetable + Operator + Sampleの3レイヤーでValidate・Render済み
- Digital Hybrid Phraseを`render events`と`render midi`でRenderし、MIDI出力の有限値を確認済み

## 人間の確認

| 確認項目 | 判定 |
|---|---|
| Wavetable frame / positionの音色差 | 未確認 |
| Wavetable position sweepとLFOの滑らかさ | 未確認 |
| Wavetable band切替と高音域Alias | 未確認 |
| Wavetable unisonのBeat・Stereo幅・Mono互換性 | 未確認 |
| Wavetable motion bassの音色成立 | 未確認 |
| PM / FMの差とRatio Sweepの連続性 | 未確認 |
| AM / Ringの差 | 未確認 |
| Operator topologyの音色差 | 未確認 |
| Operator envelope・feedback・indexの連続性 | 未確認 |
| Operator unison・polyphony・releaseの成立 | 未確認 |
| Phase Distortionの音色範囲とSweepの連続性 | 未確認 |
| Oscillator Feedbackの粗さと安定性 | 未確認 |
| WavefoldのFold感とAmount 0からの連続性 | 未確認 |
| Waveshaping + Wavefoldの役割差 | 未確認 |
| Hard Sync + WavefoldのAliasと実用性 | 未確認 |
| Unison + WavefoldのBeat・Stereo幅・Level | 未確認 |
| FM Bellの倍音変化・減衰・音色成立 | 未確認 |
| Phase Distortion Leadの音色成立 | 未確認 |
| Digital Hybrid Leadの音色成立 | 未確認 |
| Digital Hybrid Phraseのレイヤー一体感 | 未確認 |

判定は同じ再生環境・音量で確認後に記録します。Metricsは音質の承認を代替しません。
