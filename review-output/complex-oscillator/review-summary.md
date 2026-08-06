# Complex Oscillator Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Backend：DaisySP V1.0.0 (a0494a3adb67f549e18dfd71a35fa656f65b38b6)

## 入力

Definitionはdefinitions/、Eventはevents/、WAVはaudio/technical/へ保存しています。同じWAVをMetricsと人間の試聴に使用します。inspect.jsonにはBackend、Dynamic Parameter、Unison構成、Effective Frequency上限を保存しています。

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

## 機械検査

metrics.jsonは全WAVのFinite性、Peak、RMS、DC、隣接Frame差分、固定長Spectrum、左右差、Stereo Correlation、Sample Rate別値、Block Size比較、新規Runtime間の再現性比較、Basic Saw / Hard Sync / Waveshaping / Processor ChainをPolyphony 1 / 8 / 16、Unison 1 / 4 / 8で実際に同時発音させたCLI Render時間とピークWorking Setを記録します。性能値にはCLI起動・Definition Compile・WAV出力を含むため参考値として扱い、Runtime単体のリアルタイム性能とは分けて扱います。WAVは正規化せず、Metricsと試聴で同じ生出力を使用します。聴感比較時の音量は再生側で調整してください。

## 人間の確認欄

- [x] Ratio 2とRatio 6で倍音構成の差が明確である
- [x] Hard Sync Sweepが滑らかで、意図しないPitch Jumpがない
- [x] 高音域Hard Syncで耳障りなAliasや破綻が使用不能な水準にない
- [x] Waveshaping Amount 0.5で有用な倍音変化がある
- [x] Waveshaping SweepにClickやBlock境界の不連続がない
- [x] Unison 3 / 5 / 8でBeatとStereo幅が自然である
- [x] UnisonをMono再生しても過度な位相キャンセルがない
- [x] Unison 8で濁りやLevel Explosionがない
- [x] Hard Sync + Unisonが音色として使用可能である
- [x] Full Essential Synth PatchがBass / Lead / Pad用途で破綻しない

### 人間の回答

- 判定：承認
- 修正指示：なし
- 確認者：利用者
- 確認日：2026-08-06
