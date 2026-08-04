# Basic Generator Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Backend：DaisySP V1.0.0 (`a0494a3adb67f549e18dfd71a35fa656f65b38b6`)

## 入力

Definitionは`definitions/`、Eventは`events/`、WAVは`audio/technical/`へ保存しています。同じWAVをMetricsと人間の試聴に使用します。`inspect.json`にはBasic GeneratorのCompiled表示を保存しています。

再生成：

```bash
python scripts/review/generate_basic_generators_package.py
```

## 音声一覧

| WAV | 目的 |
|---|---|
| `01-sine-reference.wav` | Existing Sine Baseline |
| `02-saw-reference.wav` | Existing Saw Baseline |
| `03-square.wav` | Band-limited Square |
| `04-triangle.wav` | Band-limited Triangle |
| `05-pulse-width-025.wav` | Pulse Width 0.25 |
| `06-pulse-width-075.wav` | Pulse Width 0.75 |
| `07-white-noise.wav` | White Noise |
| `08-pink-noise.wav` | Pink Noise |
| `09-brown-noise.wav` | Brown Noise |
| `10-pink-correlation-1.wav` | Correlation 1 |
| `11-pink-correlation-0.wav` | Correlation 0 |
| `12-pwm-lfo.wav` | Existing LFOによるPulse Width Modulation |
| `13-noise-correlation-ramp.wav` | Noise Correlation Parameter Change |
| `14-high-register-square.wav` | High-register aliasing |

## 機械検査

`metrics.json`は全WAVのFinite性、Peak、RMS、DC、隣接Frame差分、固定長Spectrum、左右差、Stereo Correlation、Sample Rate別値、Block Size比較、Reset比較を記録します。WAVは正規化せず、Metricsと試聴で同じ生出力を使用します。聴感比較時の音量は再生側で調整してください。

## 人間の確認欄

- [ ] Square / Triangle / Pulseの音色差が明確である
- [ ] 高音域で耳障りなAliasが強すぎない
- [ ] Pulse Width 0.25 / 0.75の差が明確である
- [ ] PWMにClickやBlock境界の不連続がない
- [ ] White / Pink / Brownの差が明確である
- [ ] Brownが低域へ過度に偏らず、DC感が強すぎない
- [ ] Pinkに不自然な周期性がない
- [ ] Correlation 0 / 1でStereo幅の差が明確である
- [ ] Reset後にNoiseの冒頭が不自然に変化しない

### 人間の回答

- 判定：
- 修正指示：
- 確認者：
- 確認日：
