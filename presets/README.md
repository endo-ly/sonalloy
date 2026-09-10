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

代表音のMIDI Noteと演奏データ、Gate / Tailは、音源のカテゴリごとに使い分ける。

| カテゴリ | 代表音 | Gate / Tail | 演奏データ |
|---|---|---|---|
| ベース | C2（MIDI Note 36） | 0.5 / 0.5 | `bassline-events.json` |
| リード | C4（MIDI Note 60） | 0.5 / 0.5 | `leadline-events.json` |
| パッド | C4（MIDI Note 60） | 2.0 / 2.0 | `padline-events.json` |
| キー／コード | C4（MIDI Note 60） | 1.5 / 1.5 | `padline-events.json` |
| プラック／マレット | C4（MIDI Note 60） | 1.5 / 1.5 | `pluck-bell-mallet-pattern.json` |
| ベル | C4（MIDI Note 60） | 4.0 / 4.0 | `pluck-bell-mallet-pattern.json` |

```bash
sonalloy render note <preset>/definition.json \
  --note <MIDI Note番号> --velocity 100 --gate <Gate> --tail <Tail> \
  --sample-rate 48000 --block-size 257 \
  --output <preset>/note-<key>.wav

sonalloy render events <preset>/definition.json <Event列> \
  --duration-frames 115200 --tail 0.6 \
  --sample-rate 48000 --block-size 257 \
  --output <preset>/phrase.wav
```

`bassline-events.json`と`leadline-events.json`、`padline-events.json`は120 BPMの4/4を基準にしたEvent列で、`phrase.wav`のNote配置に使う。`render events`の絶対Frame位置はSample Rate 48000 Hzを前提とする。パッドとキー／コードの`phrase.wav`はコード進行のため、`--duration-frames 230400 --tail 2.5`で余韻まで含めて再生成する。

プラック／ベル／マレットの`phrase.wav`は、120 BPMの共通Patternで音色と減衰を比較する。同音の弱・中・強、8分音符のアルペジオ、4音の和音、長く保持する単音の順に演奏する。プラック／マレットは`--tail 2`、ベルは`--tail 4`で再生成する。

```bash
sonalloy render pattern <preset>/definition.json presets/pluck-bell-mallet-pattern.json \
  --sample-rate 48000 --block-size 257 --tail <余韻の秒数> \
  --output <preset>/phrase.wav
```

32〜37はすべて自然に減衰する音源で、Velocityが音量と明るさを変える。Pitch Bendは±2半音、Mod Wheelは明るさの調整に使える。外部WAV Assetは不要。
