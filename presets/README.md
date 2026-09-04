# Presets

`presets-defs.md`の代表60音源に対応する音源定義と試聴WAV。1音源 = 1 Directoryで、`definition.json`と試聴用WAVを置く。

| ファイル | 内容 |
|---|---|
| `definition.json` | 音源定義 |
| `note-<key>.wav` | カテゴリの代表単音（Velocity 100）。ベースは`note-c2.wav`（C2）、リードは`note-c4.wav`（C4）。Attack / Sustain / Releaseの素性を確認する |
| `phrase.wav` | Velocity差付きの試聴フレーズ。発音分離と音色の一貫性を確認する |

`assets/`はSample、Wavetableなど、定義から参照するWAV Assetの配置先である。

## 定義の検証

```bash
sonalloy instrument validate <preset>/definition.json
sonalloy instrument inspect <preset>/definition.json --json
```

## 試聴WAVの再生成

代表音のMIDI NoteとEvent列は、音源のカテゴリごとに使い分ける。

| カテゴリ | 代表音 | Event列 |
|---|---|---|
| ベース | C2（MIDI Note 36） | `bassline-events.json` |
| リード | C4（MIDI Note 60） | `leadline-events.json` |
| パッド | C4（MIDI Note 60） | `padline-events.json` |
| キー／コード | C4（MIDI Note 60） | `padline-events.json` |

```bash
sonalloy render note <preset>/definition.json \
  --note <代表音> --velocity 100 --gate 0.5 --tail 0.5 \
  --sample-rate 48000 --block-size 257 \
  --output <preset>/note-<key>.wav

sonalloy render events <preset>/definition.json <Event列> \
  --duration-frames 115200 --tail 0.6 \
  --sample-rate 48000 --block-size 257 \
  --output <preset>/phrase.wav
```

`bassline-events.json`と`leadline-events.json`、`padline-events.json`は120 BPMの4/4を基準にしたEvent列で、`phrase.wav`のNote配置に使う。キー／コードカテゴリの`phrase.wav`もパッドと同じコード進行を使う。`render events`の絶対Frame位置はSample Rate 48000 Hzを前提とする。パッドの`phrase.wav`はコード進行のため、`--duration-frames 230400 --tail 2.5`で余韻まで含めて再生成する。
