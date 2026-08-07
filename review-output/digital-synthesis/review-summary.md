# Wavetable Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Wavetable Asset：PCM16、Frame Length 256、Frame Count 4

Definitionは`definitions/`、Assetは`assets/`、Eventは`events/`、WAVは`audio/technical/`へ保存しています。同じWAVをMetricsと人間の試聴に使用します。`inspect.json`にはWavetable Motion BassのCompiled表示を保存しています。

再生成：

```bash
python scripts/review/generate_digital_synthesis_package.py
```

## 音声一覧

| WAV | 目的 |
|---|---|
| `01-sine-single-frame.wav` | Sine Single Frame |
| `02-saw-single-frame-low.wav` | Saw Single Frame Low Note |
| `03-saw-single-frame-high.wav` | Saw Single Frame High Note |
| `04-position-0.wav` | Position 0 |
| `05-position-05.wav` | Position 0.5 |
| `06-position-1.wav` | Position 1 |
| `07-position-sweep.wav` | Parameter Position Sweep |
| `08-position-lfo.wav` | LFO to Position |
| `09-unison-5-stereo.wav` | Unison 5 Stereo |
| `10-band-boundary-sweep.wav` | High Register Band Selection |
| `11-mod-wheel-position.wav` | Mod Wheel to Position |
| `12-motion-bass.wav` | Wavetable Motion Bass |
| `13-missing-asset-fallback.wav` | Missing Wavetable Asset with Oscillator Layer |

Regression WAVは`regression-block-*.wav`、`regression-fresh-*.wav`、`sample-rate-*.wav`です。Metricsは`metrics.json`に保存しています。

## 自動確認

- Definition Validate：成功
- CLI Inspect JSON：成功
- Wavetable Layout Error診断：`layout-error.json`で確認済み
- Missing Asset Layer除外：`missing-asset-inspect.json`で確認済み
- 全WAVのFinite：成功
- Position 0 / 0.5 / 1の出力差：生成済み
- Block Size比較：許容差以内
- Sample Rate比較：生成済み
- Fresh Render比較：一致
- Reset：Core Integration Testで確認済み
- Missing Asset時のOscillator Layer継続：生成済み
- Prepared Wavetable Byte数：`metrics.json`へ記録済み

## 人間の確認

| 確認項目 | 対象 | 判定 |
|---|---|---|
| Frameごとの音色差 | `04-position-0.wav` / `05-position-05.wav` / `06-position-1.wav` | 未確認 |
| Position Sweepの滑らかさ | `07-position-sweep.wav` / `08-position-lfo.wav` / `11-mod-wheel-position.wav` | 未確認 |
| Band切替の不連続 | `10-band-boundary-sweep.wav` | 未確認 |
| 高音域Alias | `03-saw-single-frame-high.wav` / `10-band-boundary-sweep.wav` | 未確認 |
| 低音域の倍音保持 | `02-saw-single-frame-low.wav` / `12-motion-bass.wav` | 未確認 |
| UnisonのBeatとStereo幅 | `09-unison-5-stereo.wav` | 未確認 |
| Mono再生時のLevel | `09-unison-5-stereo.wav` | 未確認 |
| Missing Asset時の継続 | `13-missing-asset-fallback.wav` | 未確認 |
| 音色としての成立 | `12-motion-bass.wav` | 未確認 |

人間の確認では同じ再生環境・音量を使い、結果と指摘をこの表へ記録します。Metricsは音質の承認を代替しません。
