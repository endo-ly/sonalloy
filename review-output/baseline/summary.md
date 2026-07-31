# Render Review Artifact

## Render条件

```text
sonalloy dev render-sine \
  --frequency 440 \
  --duration 1.0 \
  --sample-rate 48000 \
  --block-size 257 \
  --output review-output/baseline/audio/sine.wav
```

BackendはDaisySP V1.0.0（`a0494a3adb67f549e18dfd71a35fa656f65b38b6`）です。

## 機械的確認

- Stereo、48,000 Frame、48 kHz
- 全SampleがFinite
- Peak `1.0`
- RMS `0.7071069782706861`
- DC `-2.025576089105622e-07`
- Left Channelの推定周波数 `440.0 Hz`
- Block Size `64 / 257 / 1024`で同等出力をTest済み
- Reset後に初期波形を再現

Metricsは`scripts/review/measure_wav.py`でWAVから再生成しています。

## 判定

Offline Render経路、Process Contract、Native FFI、Stereo WAV出力、Diagnostics、CLI Smokeを自動確認済みです。
