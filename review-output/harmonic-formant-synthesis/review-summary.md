# Harmonic / Formant Synthesis Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Performance Render：Release Build、48,000 Hz、257 frames、2 seconds

## 入力と再生成

Additive、Formant、HybridのDefinitionは`definitions/`、Eventは`events/`、MIDIは`midi/`、Assetは`assets/`、通常のWAVは`audio/technical/`、性能計測のWAVは`audio/performance/`へ保存しています。`additive-inspect.json`、`inspect.json`、`hybrid-inspect.json`には各Generatorと4 LayerのCLI Inspect結果を保存しています。

```bash
python scripts/review/generate_harmonic_formant_package.py
```

## 音声一覧

### Additive

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

### Formant

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

### Hybrid and regression

| WAV | 目的 |
|---|---|
| `24-harmonic-formant-hybrid.wav` | Formant and Additive Hybrid |
| `25-harmonic-formant-hybrid-midi.wav` | Hybrid MIDI Phrase |
| `26-harmonic-formant-hybrid-controls.wav` | Hybrid Parameter and External Control |
| `27-existing-processor-chain.wav` | Existing Processor Chain Regression |
| `28-existing-digital-hybrid.wav` | Existing Digital Hybrid Regression |

## 性能計測

`metrics.json`の`performance`には、Additiveの1 / 16 / 32 / 64 Partial × 1 / 4 / 8 / 16 Voice、Formantの32 / 64 Partial × 1 / 5 / 8 Profile、64 Partial × 5 Profile × 16 Voiceを記録しています。各Caseは`audio_duration_seconds`、`elapsed_seconds`、`realtime_ratio`（`elapsed_seconds / audio_duration_seconds`）、`work_units`、有限値、Peak、RMSを持ちます。Partial、Voice、Profileごとの相対Realtime比も同じJSONへ保存しています。絶対的な合格閾値は設けず、計算量に対する増加傾向と16 Voice × 64 Partialの実測値を確認します。Timingは実行環境に依存します。

## 機械検査

`metrics.json`はAdditiveのSine Table、Partial、Spectrum差分、Formant Profile / Band / Parameter、Hybrid Layer / Processor / Route、全WAVのFinite性、Peak、RMS、DC、Stereo、Parameter差分、High-frequency Energy、Sample Rate別値、Block Size比較、Fresh Runtime再現性、既存Reference回帰、Release Performanceを記録します。WAVは正規化せず、Metricsと試聴で同じ生出力を使用します。

## 人間の確認

- [ ] Harmonic Organで基音と整数倍Partialが明確に聞こえ、Clickがない
- [ ] Inharmonic BellでInteger Harmonicとの差と金属的な質感が聞き取れる
- [ ] Spectrum Morphが連続し、中間値で音量が急落・急増しない
- [ ] Partial Envelope終了時に残りPartialのGainが段差変化しない
- [ ] Additive High-noteで高域Partialが主音として折り返さず、自然に薄くなる
- [ ] Additive Polyphonyで音量、Pitch、Reset、Voice Stealingが安定している
- [ ] A / I / U / E / Oの共鳴位置を聞き分けられる
- [ ] Vowel Morphが連続し、Profile境界にClickやZipper Noiseがない
- [ ] Formant Shiftで基音のPitchを保ったままVocal Characterが変化する
- [ ] ThroatでResonanceの幅が変化し、端点で急増しない
- [ ] Spectral Tiltで明るさが連続して変化する
- [ ] Formant High-noteで高次Aliasが主音として支配的にならない
- [ ] Vowel Position LFOの動きが連続している
- [ ] Noise TextureがFormantの共鳴を隠さず、Hybridの一体感がある
- [ ] Layer Filter / DriveとVoice Filter / Driveの作用範囲が分かれ、Delay / ReverbのTailが自然である
- [ ] MIDI PhraseとParameter / External Control Eventが音色の動きへ連続して反映される
- [ ] Polyphony、Voice Stealing、Reset後の発音が安定している
