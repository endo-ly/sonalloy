# Complex Oscillator Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Backend：DaisySP V1.0.0 (a0494a3adb67f549e18dfd71a35fa656f65b38b6)

## 入力

Definitionはdefinitions/、Eventはevents/、WAVはaudio/technical/へ保存しています。同じWAVをMetricsと人間の試聴に使用します。inspect.jsonにはBackend、Dynamic Parameter、Unison構成、Effective Frequency上限を保存し、phase-inspect.jsonにはPhase Distortion、Wavefold、Feedbackを有効にしたInspect結果を保存しています。

再生成：

~~~bash
python scripts/review/generate_complex_oscillator_package.py
~~~

## 音声一覧

| WAV | 目的 |
|---|---|
| 13-hard-sync-ratio-2.wav | Hard Sync Ratio 2 |
| 14-hard-sync-ratio-6.wav | Hard Sync Ratio 6 |
| 15-hard-sync-sweep.wav | Hard Sync Ratio Sweep |
| 16-waveshaping-amount-05.wav | Waveshaping Amount 0.5 |
| 17-waveshaping-sweep.wav | Waveshaping Amount Sweep |
| 18-unison-3.wav | Unison 3 |
| 19-unison-5-stereo.wav | Unison 5 Stereo |
| 20-unison-8.wav | Unison 8 |
| 21-hard-sync-unison.wav | Hard Sync + Unison |
| 22-full-essential-synth-patch.wav | Full Essential Synth Patch |
| 24-phase-distortion-025.wav | Phase Distortion Amount 0.25 |
| 25-phase-distortion-075.wav | Phase Distortion Amount 0.75 |
| 26-phase-distortion-sweep.wav | Phase Distortion Amount Sweep |
| 27-feedback-03.wav | Oscillator Feedback Amount 0.3 |
| 28-feedback-08.wav | Oscillator Feedback Amount 0.8 |
| 29-feedback-sweep.wav | Oscillator Feedback Amount Sweep |
| 30-wavefold-025.wav | Wavefold Amount 0.25 |
| 31-wavefold-075.wav | Wavefold Amount 0.75 |
| 32-wavefold-sweep.wav | Wavefold Amount Sweep |
| 33-waveshaping-wavefold.wav | Existing Waveshaping + Wavefold |
| 34-hard-sync-wavefold.wav | Hard Sync + Wavefold |
| 35-unison-wavefold.wav | Unison + Wavefold |

## 機械検査

metrics.jsonは全WAVのFinite性、Peak、RMS、DC、隣接Frame差分、固定長Spectrum、左右差、Stereo Correlation、Sample Rate別値、Block Size比較、新規Runtime間の再現性比較、Basic Saw / Hard Sync / Waveshaping / Processor ChainをPolyphony 1 / 8 / 16、Unison 1 / 4 / 8で実際に同時発音させたCLI Render時間とピークWorking Setを記録します。性能値にはCLI起動・Definition Compile・WAV出力を含むため参考値として扱い、Runtime単体のリアルタイム性能とは分けて扱います。WAVは正規化せず、Metricsと試聴で同じ生出力を使用します。聴感比較時の音量は再生側で調整してください。

## 人間の確認欄

- [ ] Ratio 2とRatio 6で倍音構成の差が明確である
- [ ] Hard Sync Sweepが滑らかで、意図しないPitch Jumpがない
- [ ] 高音域Hard Syncで耳障りなAliasや破綻が使用不能な水準にない
- [ ] Waveshaping Amount 0.5で有用な倍音変化がある
- [ ] Waveshaping SweepにClickやBlock境界の不連続がない
- [ ] Unison 3 / 5 / 8でBeatとStereo幅が自然である
- [ ] UnisonをMono再生しても過度な位相キャンセルがない
- [ ] Unison 8で濁りやLevel Explosionがない
- [ ] Hard Sync + Unisonが音色として使用可能である
- [ ] Full Essential Synth PatchがBass / Lead / Pad用途で破綻しない
- [ ] Phase Distortion 0.25と0.75で音色範囲の差が明確である
- [ ] Phase Distortion SweepにClickやPitch Jumpがない
- [ ] Feedback 0.3と0.8で倍音の粗さが変化し、発散しない
- [ ] Feedback SweepがBlock境界で不連続にならない
- [ ] Wavefold 0.25と0.75でFold感が変化し、Amount 0がIdentityである
- [ ] Waveshaping + Wavefoldで役割の差を聞き分けられる
- [ ] Hard Sync + WavefoldがFiniteで、高音域のAliasが許容範囲に収まる
- [ ] Unison + WavefoldのBeat、Stereo幅、Levelが実用範囲にある
- [ ] Phase Distortion LeadがBass / Lead / Pad用途で成立する

### 人間の回答

- 判定：
- 修正指示：
- 確認者：
- 確認日：
