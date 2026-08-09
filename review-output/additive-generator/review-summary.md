# Additive Generator Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV

## 入力

Definitionは`definitions/`、Eventは`events/`、WAVは`audio/technical/`へ保存しています。`inspect.json`にはPartial構造とParameter Descriptorを保存しています。

再生成：

```bash
python scripts/review/generate_additive_package.py
```

## 音声一覧

| WAV | 目的 |
|---|---|
| `01-additive-fundamental.wav` | Single Fundamental |
| `02-harmonic-organ.wav` | Harmonic Organ |
| `03-inharmonic-bell.wav` | Fractional Ratio and Inharmonicity |
| `04-spectrum-a.wav` / `05-spectrum-b.wav` | Spectrum A / B |
| `06-spectrum-morph-sweep.wav` | Spectrum Morph |
| `07-spectrum-tilt-sweep.wav` | Spectrum Tilt |
| `08-inharmonicity-sweep.wav` | Global Inharmonicity |
| `09-partial-envelope-bell.wav` | Partial Envelope |
| `10-high-note-alias-check.wav` | High-note Alias Fade |
| `11-additive-polyphony.wav` | 16-note Polyphony |

## 機械検査

`metrics.json`はSine TableのLength / Guard / Lookup最大絶対誤差、Finite性、Peak、RMS、DC、隣接Frame差分、単音Spectrum、Spectrum A / B差分、Inharmonicity差分、高周波Energy、Sample Rate別値、Block Size比較、Fresh Render再現性を記録します。WAVは正規化せず、Metricsと試聴で同じ生出力を使用します。

## 人間の確認

- Harmonic Organで基音と整数倍Partialが明確に聞こえ、BzzzやClickがない
- Inharmonic BellでInteger Harmonicとの差と金属的な質感が聞き取れる
- Spectrum Morphが連続し、中間値で音量が急落・急増しない
- Partial Envelope終了時に残りPartialのGainが段差変化しない
- High-note Aliasで高域Partialが主音として折り返さず、自然に薄くなる
- Polyphonyで音量、Pitch、Reset、Voice Stealingが安定している
