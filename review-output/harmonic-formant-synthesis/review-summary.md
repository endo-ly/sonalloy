# Harmonic Formant Synthesis Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV

## 入力

Definitionは`definitions/`、Eventは`events/`、MIDIは`midi/`、Assetは`assets/`、WAVは`audio/technical/`へ保存しています。`inspect.json`にはFormant ProfileとParameter Descriptor、`hybrid-inspect.json`には4 LayerとProcessor / Modulation統合を保存しています。

再生成：

```bash
python scripts/review/generate_harmonic_formant_package.py
```

## 音声一覧

| WAV | 目的 |
|---|---|
| `12-vowel-a.wav` | Vowel A |
| `13-vowel-i.wav` | Vowel I |
| `14-vowel-u.wav` | Vowel U |
| `15-vowel-e.wav` | Vowel E |
| `16-vowel-o.wav` | Vowel O |
| `17-vowel-morph.wav` | Vowel Position Morph |
| `18-formant-shift-sweep.wav` | Formant Shift |
| `19-throat-sweep.wav` | Throat |
| `20-formant-tilt-sweep.wav` | Spectral Tilt |
| `21-vowel-position-lfo.wav` | Vowel Position LFO |
| `22-high-note-formant.wav` | High-note Alias Fade |
| `23-formant-noise-texture.wav` | Formant and Noise Texture |
| `24-harmonic-formant-hybrid.wav` | Formant and Additive Hybrid |
| `25-harmonic-formant-hybrid-midi.wav` | Hybrid MIDI Phrase |
| `26-harmonic-formant-hybrid-controls.wav` | Hybrid Parameter and External Control |
| `27-existing-processor-chain.wav` | Existing Processor Chain Regression |
| `28-existing-digital-hybrid.wav` | Existing Digital Hybrid Regression |

## 機械検査

`metrics.json`はProfile Count、Partial Count、4つのFormant Parameter、Parameter ID、Hybrid Layer / Processor / Route、Finite性、Peak、RMS、DC、Stereo、Profile差分、Parameter差分、Hybrid Control差分、High-frequency Energy、Sample Rate別値、Block Size比較、Hybrid Block Size比較、Fresh Runtime再現性、既存Reference回帰を記録します。WAVは正規化せず、Metricsと試聴で同じ生出力を使用します。

## 人間の確認

- [ ] A / I / U / E / Oの共鳴位置を聞き分けられる
- [ ] Vowel Morphが連続し、Profile境界にClickやZipper Noiseがない
- [ ] Formant Shiftで基音のPitchを保ったままVocal Characterが変化する
- [ ] ThroatでResonanceの幅が変化し、端点で急増しない
- [ ] Spectral Tiltで明るさが連続して変化する
- [ ] High-noteで高次Aliasが主音として支配的にならない
- [ ] Vowel Position LFOの動きが連続している
- [ ] Noise TextureがFormantの共鳴を隠さず、Hybridの一体感がある
- [ ] Layer Filter / DriveとVoice Filter / Driveの作用範囲が分かれ、Delay / ReverbのTailが自然である
- [ ] MIDI PhraseとParameter / External Control Eventが音色の動きへ連続して反映される
- [ ] Polyphony、Voice Stealing、Reset後の発音が安定している
