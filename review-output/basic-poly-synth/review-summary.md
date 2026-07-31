# Basic Poly Synth Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- 基準Block Size：257 frames
- Output：Stereo、32-bit float WAV
- Source implementation commit：dc53482ccd4d85198814711d25691e7c2afda789
- Backend：DaisySP V1.0.0 (`a0494a3adb67f549e18dfd71a35fa656f65b38b6`)
- Platform：Windows
- Rust：1.97.0
- CMake：3.31.6

## 入力

Definitionは`definitions/`、MIDI Fileは`midi/`へ保存しています。基準音色は`basic-poly-synth.json`、Sine比較は`basic-poly-synth-sine.json`、Voice Stealing比較は`basic-poly-synth-poly4.json`です。

再生成例：

```bash
sonalloy instrument validate examples/instruments/basic-poly-synth.json
sonalloy render midi examples/instruments/basic-poly-synth.json testdata/midi/basic-poly-synth-phrase.mid \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output review-output/basic-poly-synth/audio/07-musical-phrase.wav
```

Review Package全体は次で再生成できます。

```bash
python scripts/review/generate_basic_poly_synth_package.py
```

このScriptはMIDI、基準WAV、比較用WAVを生成し、Block Size 64 / 257 / 1024で同じ入力を再RenderしてMetricsへ比較結果を保存します。

## 音声一覧

| WAV | 目的 |
|---|---|
| `01-sine-reference.wav` | C3、A4、C6の音程と不要Noiseの基準 |
| `02-saw-registers.wav` | C2〜C6のSaw高音域、Filterを開いた状態 |
| `02-saw-registers-filter-closed.wav` | 同じMIDIをFilterを閉じた状態で再生する比較音源 |
| `03-attack-release.wav` | 短い/長いGate、短いAttack、短いRelease |
| `03-attack-release-slow-attack.wav` | 同じMIDIを遅いAttack、長いReleaseで再生する比較音源 |
| `04-repeated-notes.wav` | C4の同音連打、Phase Reset、Click |
| `05-polyphony-and-stealing.wav` | Polyphony 4で8音を順番に重ね、Release中VoiceをStealする比較 |
| `06-filter-and-velocity.wav` | Velocity 32/64/96/127のGainとCutoff |
| `07-musical-phrase.wav` | 和音と単音を含む4小節相当のPhrase |

## 機械検査

`metrics.json`は`scripts/review/generate_basic_poly_synth_metrics.py`と`scripts/review/measure_wav.py`から生成しました。全WAVについて次を確認しています。

- Sample Rate、Channel数、Frame数が想定どおり
- 全SampleがFinite
- Peak、RMS、DC、推定基本周波数を測定済み
- 隣接Frame差分、大きな不連続候補数、候補Frame位置を測定済み
- `02-saw-registers.wav`には参考用Spectrumピークを収録
- Block Size 64 / 257 / 1024で同じ入力を実際に再Renderし、基準WAVとの差分を比較
- 自動Limiterや自動Normalizeで問題を隠していない

## 現在の制約

- 有効Oscillator Layerは一つに限定しています。
- Sample、Noise、Sustain Pedal、Pitch Bend、Aftertouch、Realtime Device、Pluginは対象外です。
- 未対応のMIDI EventはWarningを出して無視します。
- Cutoffの連続Parameter変更は公開していません。Voice開始時のVelocity Responseだけを適用します。
- Metricsと自動Testは、音の魅力・自然さ・Alias感・演奏感を判定しません。

## 試聴時の注意

隣接Frame差分を0.25以上とする機械的な候補検出は、Saw波形の通常の急峻な変化や複数Voiceの重なりでも発生し得る値です。クリックの有無は判定していないため、該当WAVのVoice Stealing、Note境界、Phrase冒頭を重点的に試聴してください。Block Size比較は`metrics.json`の`block_size_comparisons`に記録します。

## 人間の確認欄

次の項目を試聴して記録してください。

- [x] SawのC6付近に明確な耳障りさがない
- [x] Note On / Note Off境界にClickがない
- [x] Attack / Releaseが自然である
- [x] 同音連打が不自然でない
- [x] Voice Stealingが目立ちすぎない
- [x] Filterを開いた/閉じた比較で変化が確認できる
- [x] Filterの変化が滑らかである
- [x] Velocity Responseが自然である
- [x] Bass / Lead / Pluckの素材として使いたい

### 人間の回答

- 判定：承認
- 修正指示：なし
- 確認者：利用者
- 確認日：2026-08-01
