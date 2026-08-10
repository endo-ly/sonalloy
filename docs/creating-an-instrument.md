# 音源（Instrument）の作り方

このガイドは、Sonalloyで自分の音源を作ってWAVを出すまでの道筋を説明します。ひな形の生成から始め、パラメータの意味を理解しながら、試聴して、必要ならAdditiveのPartial設計、FormantのVowel Profile設計、自作WAVのWavetable、Spectral、Sample、Granular、Wave Sequenceへの組み込みまで進みます。

> **本書の範囲**：音源作成の操作手順（人間向けガイド）です。仕様の詳細は本書に書かず、各仕様文書へ委ねます。
>
> | 本書に書かないこと | 参照先 |
> |---|---|
> | DefinitionのJSON仕様・制約・Range | `docs/instrument-definition.md` |
> | 実行時の挙動（Voice・ADSR・Sample再生） | `docs/runtime-processing.md` |
> | CLIの全Option・Exit Codeの定義 | `docs/cli.md` |
> | Agentが音源作成を実行する手順 | `.agents/skills/create-instrument/` |

## 全体の流れ

```text
ひな形の生成 → 音色の編集 → 検証 → Additive / Formant / Sample / Wavetable / Spectral追加 → Granular追加 → Wave Sequence追加 → 試聴 → 仕上げ
   Step 1       Step 2     Step 3       Step 4                    Step 5       Step 6             Step 7   Step 8
```

| Step | 内容 | 使うコマンド |
|---|---|---|
| 1 | ひな形の生成（新規の場合のみ） | `instrument init` |
| 2 | 音色の編集（Layer、ADSR、Processorなど） | エディタでJSONを編集 |
| 3 | 検証 | `instrument validate` / `instrument inspect` |
| 4 | AdditiveまたはFormantを追加 | JSON編集 |
| 5 | 自作WAVをSample、Wavetable、またはSpectralとして組み込み | SHA-256計算 → JSON編集 |
| 6 | 自作WAVをGranularとして組み込み | SHA-256計算 → JSON編集 |
| 7 | 複数AssetをWave Sequenceとして組み込み | SHA-256計算 → JSON編集 |
| 8 | 試聴 | `render note` / `render midi` |
| 9 | 仕上げ（名前・説明・関連docsへの反映） | — |

## Step 1. ひな形を生成する

次のコマンドで、Saw Oscillatorの最小Definitionが生成されます。倍音を直接設計する場合は、生成後に`generator`をAdditiveまたはFormantへ置き換えるか、[`examples/instruments/additive-generator-reference.json`](../examples/instruments/additive-generator-reference.json)または[`examples/instruments/formant-generator-reference.json`](../examples/instruments/formant-generator-reference.json)を複製します。

```bash
sonalloy instrument init my-instrument.json
```

生成されるJSONは、Polyphony 16、ADSR `0.005 / 0.18 / 0.65 / 0.3`、Gain `-14 dB`、Voice ProcessorのFilter `12000 Hz / 0.12`を持つBasic Poly Synth型です。これを土台に音色を編集していきます。既存音源（`examples/instruments/`）のコピーから始めても構いません。

## Step 2. 音色を編集する

### Layerとは何か

音源は1つ以上の**Layer**で構成されます。Layerは「1つの音の発生源」です。

```text
Note On
  │
  ▼
Layer 1（Oscillator）→ Layer Processor → ADSR → Layer Gain → Pan ─┐
                                                  ├→ Voice Processor → Global Processor → ステレオ出力
Layer 2（Sample）    → Layer Processor → ADSR → Layer Gain → Pan ─┘
```

- Layerごとに**独立したADSR**と**Gain / Pan / Tuning**を持ちます。
- Layer同士は**同じVoice内でMix**されるため、Sampleのアタック＋Oscillatorの余韻のように、別々の音が一つの音色として聞こえます。
- 発音条件は`trigger`で制御します（`event`で`note_on` / `note_off`、`key_min / key_max`で鍵盤の範囲、`velocity_min / velocity_max`で打鍵の強さの範囲）。`note_off` LayerはNote OnでArmedになり、対応するNote Offで発音します。

### ADSRで音の輪郭を作る

ADSRは音の音量変化（エンベロープ）を形作る4つの区間です。

```text
Level
  ▲
  │        ┌──── sustain ────┐
  │       ╱                 ╲
  │      ╱                  ╲
  │     ╱                   ╲
  │    ╱                    ╲
  └───┴──────────────────────┴───────▶ Time
    attack  decay        release
```

| パラメータ | 役割 | 目安 |
|---|---|---|
| `attack_seconds` | 押してから最大音量まで達する時間 | 0（瞬発）〜数秒（うねり） |
| `decay_seconds` | 最大音量からSustainレベルへ下がる時間 | 0.05〜0.3が一般的 |
| `sustain_level` | 押している間の音量（0〜1） | 0だと短い音、1だと伸びる音 |
| `release_seconds` | 離してから消えるまでの時間 | 0だとバツンと切れる |

### Generatorを選ぶ

OscillatorのWaveformはTagged Objectで指定します。Pulseは`pulse_width`を持ち、Square / Triangle / PulseはSine / Sawと同じLayerへ配置できます。

```json
"generator": {
  "oscillator": {
    "waveform": { "type": "pulse", "pulse_width": 0.35 },
    "phase_reset": true,
    "phase": 0.0
  }
}
```

NoiseはColor、Seed、Stereo Correlationを指定します。ColorはWhite / Pink / Brownから選び、Correlation 0は左右独立、1は左右同一です。

```json
"generator": {
  "noise": {
    "color": "pink",
    "seed": 812347,
    "stereo_correlation": 0.65
  }
}
```

Pulse Widthは既存LFOなどのModulation Targetへ接続できます。

```json
{
  "source": "pwm_lfo",
  "target": "layer.main.generator.pulse_width",
  "amount": 0.35,
  "curve": "linear"
}
```

### Additiveで倍音を設計する

Additiveは、Note Frequencyに対する1〜64個のPartialを直接記述するGeneratorです。まずは基音を1つ置き、整数Ratioを追加してHarmonic Toneを作ります。`2.73`のようなFractional Ratioを混ぜるとInharmonic BellやMetallic Textureになります。

```json
"generator": {
  "additive": {
    "phase_reset": true,
    "morph": 0.0,
    "spectrum_tilt_db_per_octave": -3.0,
    "inharmonicity": 0.0,
    "partials": [
      { "id": "fundamental", "ratio": 1.0, "amplitude_a": 1.0, "amplitude_b": 0.7, "phase": 0.0 },
      { "id": "second", "ratio": 2.0, "amplitude_a": 0.45, "amplitude_b": 0.8, "phase": 0.0 },
      { "id": "metal", "ratio": 2.73, "amplitude_a": 0.15, "amplitude_b": 0.5, "phase": 0.25 }
    ]
  }
}
```

`amplitude_a`と`amplitude_b`は`morph = 0`と`morph = 1`のSpectrumです。Morph中にRatioやPhaseは変わらないため、Partialの増減による不連続を避けられます。`spectrum_tilt_db_per_octave`は高次Partialの明るさ、`inharmonicity`は高次RatioのStretchを制御します。各Partialには既存ADSRの`envelope`を任意で指定できます。Layer EnvelopeはPartial Sumの後に適用されます。

次のParameterをLFO、Modulation Envelope、Mod Wheel、Aftertouch、またはEventから制御できます。

- `layer.<layer_id>.generator.additive_morph`
- `layer.<layer_id>.generator.additive_spectrum_tilt`
- `layer.<layer_id>.generator.additive_inharmonicity`

`instrument inspect --json`でPartial Count、Ratio、Amplitude、Phase、Envelopeの有無と3つのParameter Descriptorを確認します。完全無音のSpectrum、空のPartial配列、重複ID、65個以上のPartialはValidation Errorです。実際の8 Partial構成は[`additive-generator-reference.json`](../examples/instruments/additive-generator-reference.json)にあります。

### FormantでVowel Spectrumを設計する

Formantは基音の整数倍Partialへ、母音の共鳴を表す5本のBandを適用するGeneratorです。`profiles`へ1〜8個のVowel ProfileをDefinition順に記述し、各Profileは`formants`へ周波数の昇順に5本のBandを持ちます。Vowel Positionは隣接Profileを補間し、Frequency / BandwidthはGeometric、GainはdB Linearで変化します。

```json
"generator": {
  "formant": {
    "phase_reset": true,
    "partial_count": 48,
    "vowel_position": 0.0,
    "formant_shift_cents": 0.0,
    "throat": 0.5,
    "spectral_tilt_db_per_octave": -6.0,
    "profiles": [
      {
        "id": "a",
        "formants": [
          { "frequency_hz": 800.0, "bandwidth_hz": 80.0, "gain_db": 6.0 },
          { "frequency_hz": 1150.0, "bandwidth_hz": 90.0, "gain_db": 3.0 },
          { "frequency_hz": 2900.0, "bandwidth_hz": 120.0, "gain_db": -3.0 },
          { "frequency_hz": 3900.0, "bandwidth_hz": 150.0, "gain_db": -12.0 },
          { "frequency_hz": 4950.0, "bandwidth_hz": 200.0, "gain_db": -18.0 }
        ]
      },
      {
        "id": "i",
        "formants": [
          { "frequency_hz": 300.0, "bandwidth_hz": 60.0, "gain_db": 3.0 },
          { "frequency_hz": 2500.0, "bandwidth_hz": 100.0, "gain_db": 6.0 },
          { "frequency_hz": 3200.0, "bandwidth_hz": 120.0, "gain_db": -6.0 },
          { "frequency_hz": 4300.0, "bandwidth_hz": 150.0, "gain_db": -12.0 },
          { "frequency_hz": 5500.0, "bandwidth_hz": 200.0, "gain_db": -18.0 }
        ]
      }
    ]
  }
}
```

`partial_count`は1〜64、Profileは1〜8個、Bandの周波数は100〜12000 Hz、Bandwidthは20〜5000 Hz、Gainは-60〜12 dBです。`formant_shift_cents`はFormantの中心周波数とBandwidthを移動し、基音のPitchは変えません。`throat`はBandwidthを0.5〜2倍、`spectral_tilt_db_per_octave`はPartialの高域傾斜を制御します。Formant固有のEnvelopeは持たず、Layer EnvelopeをPartial Sumへ適用します。

次の4つをModulation TargetまたはParameter Changeから制御できます。

- `layer.<layer_id>.generator.formant_vowel_position`（0〜1）
- `layer.<layer_id>.generator.formant_shift`（-2400〜2400 cents）
- `layer.<layer_id>.generator.formant_throat`（0〜1）
- `layer.<layer_id>.generator.formant_spectral_tilt`（-24〜12 dB/octave）

`instrument inspect --json`でProfile Count、5本のBand、4つのParameter Descriptor、Output Modeを確認します。実際の5 Profile構成は[`formant-generator-reference.json`](../examples/instruments/formant-generator-reference.json)にあり、Additiveと重ねる例は[`harmonic-formant-hybrid-reference.json`](../examples/instruments/harmonic-formant-hybrid-reference.json)にあります。

### Harmonic / Formant Hybridを作る

FormantへAdditiveを重ねるとVocal-likeな共鳴へ基音と倍音の芯を加えられます。SampleをAttack、NoiseをAirとして追加し、Layer Filter / Drive、Voice Filter / Drive、Global Delay / Reverbを順に配置すると、Transient、持続成分、空気感、空間を一つのInstrumentで管理できます。動作する4 Layer構成は[`harmonic-formant-hybrid-reference.json`](../examples/instruments/harmonic-formant-hybrid-reference.json)です。

HybridではFormantの4 ParameterへLFOやModulation Envelopeを接続し、Mod WheelやAftertouchでFormant ShiftとGlobal Effect Mixを操作できます。最初に各LayerのGainとEnvelopeを単独で確認し、その後に`instrument inspect --json`でProcessor ChainとRouteを確認します。

```bash
sonalloy instrument validate examples/instruments/harmonic-formant-hybrid-reference.json
sonalloy instrument inspect examples/instruments/harmonic-formant-hybrid-reference.json --json
sonalloy render midi examples/instruments/harmonic-formant-hybrid-reference.json \
  testdata/midi/basic-poly-synth-phrase.mid \
  --sample-rate 48000 --block-size 257 --tail 0.5 \
  --output out/harmonic-formant-hybrid/midi.wav
```

Hard Sync、Waveshaping、UnisonはOscillator Definitionへ追加します。Hard SyncはSineでは使用できず、開始`phase`とHard Sync併用時の`phase_spread`は0にします。

```json
"generator": {
  "oscillator": {
    "waveform": { "type": "saw" },
    "phase_reset": true,
    "phase": 0.0,
    "hard_sync": { "ratio": 3.0 },
    "waveshaping": { "amount": 0.25 },
    "unison": {
      "voices": 5,
      "detune_cents": 18.0,
      "stereo_spread": 0.8,
      "phase_spread": 0.0
    }
  }
}
```

`sync_ratio`、`waveshape`、`unison_detune`、`unison_spread`は既存のLFO、Envelope、Mod Wheel、Parameter Changeから制御できます。値域と信号順序は[`docs/instrument-definition.md`](instrument-definition.md)を参照してください。

### Phase Distortion、Wavefold、Feedbackを使う

SineへPhase DistortionとOne-sample Feedbackを追加すると、Phaseの時間変化と倍音の粗さを作れます。WavefoldはSine以外の既存Waveformにも追加できます。

```json
"generator": {
  "oscillator": {
    "waveform": { "type": "sine" },
    "phase_reset": true,
    "phase": 0.0,
    "hard_sync": null,
    "waveshaping": { "amount": 0.1 },
    "phase_distortion": { "amount": 0.55 },
    "wavefold": { "amount": 0.25 },
    "feedback": { "amount": 0.3 },
    "unison": null
  }
}
```

Phase DistortionとFeedbackはSineだけで使用でき、Hard Syncとは併用できません。WavefoldのParameter IDは`layer.<layer_id>.generator.wavefold`、FeedbackのParameter IDは`layer.<layer_id>.generator.oscillator_feedback`です。`instrument inspect --json`で`phase_domain` Backend、Signal Order、DC Blocker、Parameter IDを確認してから、Parameter ChangeやModulationでAmountを動かします。

### そのほかのパラメータ

| パラメータ | 意味 | 注意 |
|---|---|---|
| `gain_db` | Layerの音量（-60〜12 dB） | Sampleを複数重ねる場合は重なり分を下げる |
| `pan` | 左右位置（-1 = 左、0 = 中央、1 = 右） | Constant-powerで自然に定位する |
| `tuning_cents` | 半音の100分の1単位の音程調整 | -1200〜1200 |
| `processors` | LayerのGenerator後に直列適用するFilterまたはDrive | 配列順に適用 |
| `voice_processors` | 全Layer Mix後に直列適用するFilterまたはDrive | `cutoff_hz`、`resonance`、`amount`、`mix` |
| `global_processors` | Voice Sum後にInstrument全体へ適用するFilter、Drive、Delay、Reverb | Delay/ReverbのTailを保持 |
| `modulation` | Velocity、LFO、Envelope、RandomなどのSourceをTargetへ接続 | `routes`でLayer、ProcessorのDynamic Parameterへ反映 |

打鍵の強さや発音中の変化を設定する場合は、`modulation.sources`へSourceを定義し、`modulation.routes`でTargetへ接続します。VelocityとKey Trackingは組み込みSourceなので、Source定義なしで参照できます。詳細なID、Range、Curveは[`docs/instrument-definition.md`](instrument-definition.md)を参照してください。

```json
"modulation": {
  "routes": [
    { "source": "velocity", "target": "layer.main.gain", "amount": 0.08, "curve": "linear" },
    { "source": "lfo", "target": "voice.processor.tone.cutoff", "amount": 0.18, "curve": "linear" }
  ],
  "sources": [
    { "id": "lfo", "type": "lfo", "waveform": "sine", "rate_hz": 0.5, "phase": 0.0 }
  ]
}
```

### Wavetableを使う

Wavetableは、周期波形をFrame単位で連結したWAVを用意して、`frame_length`を明示します。AssetのSample RateはPitchへ使われないため、同じ周期Frame列を異なるCompile Sample Rateで利用できます。

```json
"generator": {
  "wavetable": {
    "asset": {
      "path": "../../testdata/assets/digital-motion.wav",
      "sha256": "<計算した値>"
    },
    "frame_length": 2048,
    "position": 0.0,
    "phase_reset": true,
    "phase": 0.0
  }
}
```

`frame_length`は64〜4096の2の冪で、WAV全体のSample数が割り切れる値を選びます。Position 0、0.5、1はそれぞれ最初、中間、最後のFrame側の音色になります。`position`は`layer.<layer_id>.generator.wavetable_position`としてLFO、Mod Wheel、Parameter Changeから制御できます。

```json
{
  "source": "motion_lfo",
  "target": "layer.main.generator.wavetable_position",
  "amount": 1.0,
  "curve": "linear"
}
```

`instrument validate`でFrame Layout、Asset Hash、Frame Warningを確認し、`instrument inspect --json`でPrepared状態、Band、Position Parameter ID、Effective Frequency上限を確認します。Assetが欠落した場合はそのLayerだけが発音候補から外れるため、ほかのLayerの確認を続けられます。

### Spectral / Resynthesisを使う

録音や生成したWAVをSpectrum経由で元に近く再構成する場合は、`spectral` Generatorへ`asset_a`を指定します。`asset_a`のMono / Stereo Channel、Source Metadata、時間軸を保ったままCompile時にSTFTを準備します。A/B Morphを使う場合は、同じChannel数のWAVを`asset_b`へ指定します。

```json
"generator": {
  "spectral": {
    "asset_a": {
      "path": "../../testdata/assets/metal-hit.wav",
      "sha256": "<計算した値>"
    },
    "asset_b": null,
    "root_note": 60,
    "fft_size": 2048,
    "position": 0.0,
    "freeze": 0.0,
    "blur_seconds": 0.0,
    "shift_hz": 0.0,
    "morph": 0.0,
    "phase_reset": true
  }
}
```

`fft_size`は1024、2048、4096から選びます。Hop SizeはFFT Sizeの4分の1、Reported Latencyは`fft_size - hop_size`です。`position`はSourceの開始位置へNatural Scanを加え、`freeze`はScan速度を下げて1でFrameを固定します。`blur_seconds`は時間方向Magnitude Smoothing、`morph`はA/Bの正規化タイムライン上のMorph、MIDI NoteとLayer TuningはRoot NoteからのPitch比として適用され、Source Durationは変わりません。`shift_hz`は各Spectral成分をHz単位で移動します。`asset_b`を指定した場合だけMorph ParameterがCatalogへ追加され、Bの準備失敗時はAだけへフォールバックせずLayerが無効になります。Identity Resynthesisを確認するときは、`position`、`freeze`、`blur_seconds`、`shift_hz`、`morph`を0にして、元WAVとRender結果をLatency後で比較します。

```bash
sonalloy instrument validate my-instrument.json --json
sonalloy instrument inspect my-instrument.json --json
sonalloy render note my-instrument.json \
  --note 60 --gate 0.5 --tail 0.5 --sample-rate 48000 \
  --block-size 257 --output out/my-instrument/spectral.wav
```

`inspect --json`ではAsset A/BのPrepared状態、Source Channel、Spectral Frame数、Prepared Bytes、FFT / Hop / Bin数、Latency、5つのParameter IDを確認します。Asset Aまたは指定Bが見つからない、Hashが一致しない、Decodeできない場合はSpectral Layerだけが無効になります。A/BのChannel数が異なる場合はCompile Errorです。

### Operator Modulationを使う

4つのSine OperatorでBell、Bass、AM、Ringの音色を作れます。最初は既存の[`examples/instruments/operator-modulation-reference.json`](../examples/instruments/operator-modulation-reference.json)を複製し、`algorithm`、Ratio、Envelope、Modulation Amountを調整します。

```json
"generator": {
  "operator_modulation": {
    "mode": "frequency",
    "algorithm": "stack_4",
    "operators": [
      { "ratio": 1.0, "detune_cents": 0.0, "level": 0.9, "modulation_amount": 0.0, "feedback": 0.0, "phase": 0.0, "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.2, "sustain_level": 1.0, "release_seconds": 0.1 } },
      { "ratio": 2.0, "detune_cents": 0.0, "level": 0.0, "modulation_amount": 2.5, "feedback": 0.0, "phase": 0.0, "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.1, "sustain_level": 1.0, "release_seconds": 0.1 } },
      { "ratio": 3.0, "detune_cents": 0.0, "level": 0.0, "modulation_amount": 1.5, "feedback": 0.0, "phase": 0.0, "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.08, "sustain_level": 1.0, "release_seconds": 0.08 } },
      { "ratio": 5.0, "detune_cents": 0.0, "level": 0.0, "modulation_amount": 2.0, "feedback": 0.25, "phase": 0.0, "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.05, "sustain_level": 1.0, "release_seconds": 0.05 } }
    ],
    "phase_reset": true,
    "unison": null
  }
}
```

Carrier Operatorだけに`level`を設定し、接続元Operatorの`modulation_amount`を設定します。`stack_4`では4→3→2→1の順に信号が進むため、Operator 1がCarrier、Operator 4が最上流です。`phase`はPhase Modulation、`frequency`はFrequency Modulation、`amplitude`はUnipolar AM、`ring`はCarrierとProductのCrossfadeです。AMとRingではFeedbackを0にします。

`instrument inspect --json`でMode、Algorithm、Evaluation Order、Carrier、各OperatorのParameter IDを確認し、RatioやIndexをParameter Changeで動かす場合はEventのTargetに`layer.<layer_id>.generator.operator.<1-4>.<parameter>`を指定します。Offline Renderと試聴の対象は[`docs/testing-and-sound-review.md`](testing-and-sound-review.md)のDigital Synthesis Packageにまとめています。

### Digital Hybridを作る

異なるGeneratorを同じVoiceへ重ねる場合は、Wavetableを持続音、Operator Modulationを倍音の芯、Sampleを短いアタックとして役割分担させると調整しやすくなります。動作する3レイヤーの基準例は[`examples/instruments/digital-hybrid-reference.json`](../examples/instruments/digital-hybrid-reference.json)です。Wavetable AssetとSample AssetのHashを保持したまま複製し、各LayerのGainとEnvelopeを先に調整します。

```bash
sonalloy instrument validate examples/instruments/digital-hybrid-reference.json
sonalloy instrument inspect examples/instruments/digital-hybrid-reference.json --json
sonalloy render note examples/instruments/digital-hybrid-reference.json --note 60 --output digital-hybrid.wav --json
```

Layerごとの発音、全体のMix、Phrase中のWavetable Position変更を分けて確認し、最終的な音声確認対象は[`docs/testing-and-sound-review.md`](testing-and-sound-review.md)のDigital Synthesis Packageへ集約します。

## Step 3. 検証する

編集したら、必ず検証します。

```bash
sonalloy instrument validate my-instrument.json
sonalloy instrument inspect my-instrument.json
```

- `validate`はJSON Parse、Validation、Compileまで実行し、問題がなければ`valid`と表示されます。
- `inspect`は実行値を人間が読める形で表示します（`--json`で機械可読）。Gain・Pan・Tuning・Envelopeに加え、Parameter、Source、Routeが意図どおりにCompileされたかをここで確認します。
- Errorには`layers[0].envelope.attack_seconds`のようなField Pathが付くので、そのまま該当箇所を修正できます。

発音中のParameter変更を確認する場合は、Event Sequence JSONを用意して次のようにRenderします。

```bash
sonalloy render events my-instrument.json events.json --duration-frames 96000 --output out/my-instrument/events.wav
```

Event SequenceではNote Eventと同じ絶対Frame位置にParameter Change、Pitch Bend、Mod Wheel、Aftertouchを記述できます。`render midi`ではMIDI Pitch Bend、CC1、Channel Aftertouchも同じRuntime Eventへ変換されます。

## Step 4. 自作WAVをSampleとして使う

録音や生成したWAVを、Sample Layerの音源として組み込めます。Sample LayerではMonoとStereoのChannel構成を保持したまま再生します。

**1. WAVを準備する**

PCM 16/24 bitまたはFloat 32のWAVです。MonoとStereoのどちらも使用でき、Stereoは左右のChannelを保持して再生します。`testdata/assets/`へ置くのが慣例です。

**2. SHA-256を計算する**

```powershell
# Windows
Get-FileHash -Algorithm SHA256 testdata\assets\my-sample.wav
```

```bash
# Linux
sha256sum testdata/assets/my-sample.wav
```

**3. Layerの`generator`へ`sample`を記述する**

```json
{
  "id": "attack",
  "enabled": true,
  "trigger": {
    "event": "note_on",
    "key_min": 0, "key_max": 127,
    "velocity_min": 1, "velocity_max": 127
  },
  "gain_db": -18.0,
  "pan": 0.0,
  "envelope": {
    "attack_seconds": 0.0,
    "decay_seconds": 0.08,
    "sustain_level": 0.0,
    "release_seconds": 0.1
  },
  "generator": {
    "sample": {
      "interpolation": "cubic",
      "zones": [
        {
          "id": "main",
          "asset": { "path": "../../testdata/assets/my-sample.wav", "sha256": "<計算した値>" },
          "root_note": 60,
          "key_min": 0,
          "key_max": 127,
          "velocity_min": 1,
          "velocity_max": 127,
          "round_robin_group": null,
          "playback": {
            "region": { "start_seconds": 0.0, "end_seconds": null },
            "direction": "forward",
            "loop": null,
            "time": { "mode": "resample" }
          }
        }
      ]
    }
  }
}
```

| パラメータ | 意味 |
|---|---|
| `zones[].asset.path` | DefinitionのあるDirectoryを基準にした相対Path（または絶対Path） |
| `zones[].asset.sha256` | 起動時の検証用ハッシュ。省略するとWarningが出ます |
| `zones[].root_note` | このZoneのSampleが基準とする音程（MIDI Note番号。0〜127、60 = C4） |
| `zones[].key_min` / `key_max` | Zoneが受け付けるMIDI Note範囲（0〜127、min <= max） |
| `zones[].velocity_min` / `velocity_max` | Zoneが受け付けるVelocity範囲（1〜127、min <= max） |
| `zones[].round_robin_group` | 同一条件のZoneをDefinition順に選択するGroup。不要なら`null` |
| `zones[].playback` | Region、`forward` / `reverse`、Loop / Constant-power Crossfade、Time Mode |
| `interpolation` | `cubic`（4点補間） |

SampleのPath違いやハッシュ不一致の場合は**そのZoneだけが無効化され**、ほかのZoneやLayerでRenderは継続します。SHA-256を省略した場合はWarningだけが付きます。

`direction: "reverse"`はPrepared Audioの複製を作らず、Cursorを逆方向へ進めます。LoopはRegion内に置き、`crossfade_seconds`を0より大きくするとLoop終端と開始をConstant-powerでBlendします。Release Sampleを作る場合はLayerの`trigger.event`を`note_off`にし、Note OnでArmedにしてからNote Offで発音します。

`playback.time`は必須です。通常のSampleは`{"mode": "resample"}`、Pitchを維持してDurationを2倍にする場合は`{"mode": "fixed_stretch", "ratio": 2.0}`、Source BPM 120のLoopをProcess Tempoへ追従させる場合は`{"mode": "tempo_sync", "source_bpm": 120.0}`を指定します。Fixed StretchとTempo SyncのRatioは0.5〜2.0で、Reverseとは併用できません。

## Step 5. Granularとして使う

録音、Vocal、Field RecordingなどをGrainへ分割して再構成する場合は、Sampleと同じAssetを`granular` Generatorへ指定します。`region`はPrepared Audio内のSource範囲で、`position`はそのRegion内の0〜1です。

```json
{
  "id": "texture",
  "enabled": true,
  "trigger": {
    "event": "note_on",
    "key_min": 0, "key_max": 127,
    "velocity_min": 1, "velocity_max": 127
  },
  "gain_db": -12.0,
  "pan": 0.0,
  "tuning_cents": 0.0,
  "envelope": {
    "attack_seconds": 0.02,
    "decay_seconds": 0.1,
    "sustain_level": 1.0,
    "release_seconds": 0.4
  },
  "generator": {
    "granular": {
      "asset": { "path": "../../testdata/assets/my-sample.wav", "sha256": "<計算した値>" },
      "root_note": 60,
      "region": { "start_seconds": 0.0, "end_seconds": null },
      "position": 0.5,
      "grain_size": 0.08,
      "density": 24.0,
      "pitch": 0.0,
      "randomness": 0.35,
      "pan_spread": 0.75,
      "seed": 8128
    }
  },
  "processors": []
}
```

| Parameter | Range / Unit | 使い方 |
|---|---:|---|
| `position` | 0〜1 | Region内の読出位置。0はStart、1はGrain長を考慮したEnd側。LFOやMod WheelでScrub、固定値でFreeze |
| `grain_size` | 0.005〜0.5秒 | Hann Windowを適用するGrain長 |
| `density` | 1〜100 grains/sec | 1秒あたりのGrain数 |
| `pitch` | -2400〜2400 cents | NoteのPitchとLayer Tuningへ加算 |
| `randomness` | 0〜1 | Positionの決定的な分散幅 |
| `pan_spread` | 0〜1 | GrainごとのStereo配置幅 |

`instrument inspect --json`でPrepared状態、Region Frame、6つのParameter ID、Source Channel、Seed、Grain Pool Limitを確認します。GranularはMono AssetでもStereo Generatorとして動作します。Note OffではGrainを破棄せずLayer EnvelopeがReleaseへ進み、Voice StealingまたはReset時だけPoolを初期化します。

## Step 6. Wave Sequenceとして使う

複数のAudio Assetを時間順に切り替える場合は、`wave_sequence` GeneratorへStepを記述します。Sequenceの`direction`はStepの選択順、Stepの`playback_direction`はAssetのRead方向です。Stepは`seconds`または`beats`でDurationを指定し、`playback`を`one_shot`または`loop`から選びます。

```json
"generator": {
  "wave_sequence": {
    "root_note": 60,
    "direction": "forward",
    "loop": true,
    "crossfade": 0.25,
    "steps": [
      {
        "id": "attack",
        "asset": {
          "path": "../../testdata/assets/metal-hit.wav",
          "sha256": "<SHA-256>"
        },
        "region": { "start_seconds": 0.0, "end_seconds": 0.08 },
        "duration": { "mode": "seconds", "value": 0.18 },
        "playback": "loop",
        "playback_direction": "forward",
        "gain_db": -3.0,
        "pitch_cents": 0.0
      },
      {
        "id": "body",
        "asset": {
          "path": "../../testdata/assets/metal-hit.wav",
          "sha256": "<SHA-256>"
        },
        "region": { "start_seconds": 0.08, "end_seconds": 0.16 },
        "duration": { "mode": "beats", "value": 0.5 },
        "playback": "one_shot",
        "playback_direction": "reverse",
        "gain_db": -6.0,
        "pitch_cents": 300.0
      }
    ]
  }
}
```

Step数は1〜128、`crossfade`は0〜0.5です。`ping_pong`は終端を重複させずに往復し、Missing AssetのStepもDurationを保持した無音として後続StepのTimingを変えません。`instrument inspect --json`でStep Count、Direction、Loop、Crossfade、Region Frame、Duration、Playback、Availability、Pitch、Gainを確認します。動作する4 Stepの例は[`examples/instruments/wave-sequence-reference.json`](../examples/instruments/wave-sequence-reference.json)です。

```bash
sonalloy instrument validate examples/instruments/wave-sequence-reference.json
sonalloy instrument inspect examples/instruments/wave-sequence-reference.json --json
sonalloy render note examples/instruments/wave-sequence-reference.json \
  --note 60 --tempo 120 --gate 1.5 --tail 0.2 \
  --sample-rate 48000 --block-size 257 --output out/wave-sequence.wav
```

## Step 7. 音を出す

**単音の確認**（音色の素性を確かめます）：

```bash
sonalloy render note my-instrument.json \
  --note 60 --velocity 100 --gate 0.5 --tail 0.5 \
  --tempo 120 \
  --sample-rate 48000 --block-size 257 --output out/my-instrument/note.wav
```

**フレーズの確認**（演奏感を確かめます）：

```bash
sonalloy render midi my-instrument.json \
  testdata/midi/basic-poly-synth-phrase.mid \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/my-instrument/phrase.wav
```

| Option | 意味 | 既定値 |
|---|---|---|
| `--note` | MIDI Note番号 | `60` |
| `--velocity` | 打鍵の強さ | `100` |
| `--gate` | Note OnからNote Offまでの時間（秒） | `0.5` |
| `--tail` | 最後のNote Off後の追加時間（秒） | note: `0.5` / midi: `1.0` |
| `--tempo` | Process Tempo（BPM）。Tempo Sync Sampleへ適用 | `120` |
| `--sample-rate` | Sample Rate（Hz） | `48000` |
| `--block-size` | Process最大Block Size（Frame） | `257` |
| `--output` | Stereo WAV出力先（必須） | — |

出力は**32-bit float・2 Channel**のStereo WAVです。Time Stretchを含む場合はReported LatencyがInspectと成功JSONへ表示され、CLIが前置きLatencyを除去してMusical TimelineのFrame 0からWAVを生成します。親Directoryは事前に作成してください。既存のMIDIがなければ、`scripts/review/generate_midi_fixtures.py`で固定のテスト用MIDIを生成できます。

## Step 8. 仕上げる

- `metadata.name`と`metadata.description`を実際の音色に合わせます。
- 音源作成の一連の流れをAgentに実行させる場合は、`.agents/skills/create-instrument/`の手順が利用できます。

## 困ったときは

| Exit Code | 意味 | 対処 |
|---:|---|---|
| `0` | 成功 | — |
| `1` | Definition / Compile Error | `--json`を付けて再実行し、ErrorのField Pathを修正する |
| `2` | CLI入力またはRender Request Error | Option値（Sample Rate、Block Size、Tailなど）を確認する |
| `3` | Core Process / Render Error | `--json`の`DSP_ERROR`を確認する |
| `4` | WAV出力 Error | 出力先Directoryの存在と書き込み権限を確認する |

- **Warningが出た**：意図しないLayerの無効化（Sample欠落など）ではないかを`instrument inspect`で確認します。
- **音が鳴らない**：`enabled: true`、`trigger`の範囲に発音するNote / Velocityが含まれているかを確認します。
- **Sampleが無視された**：Asset PathとSHA-256の一致、WAV形式（PCM 16/24、Float 32）を確認します。

## 関連文書

| 文書 | 内容 |
|---|---|
| `docs/instrument-definition.md` | DefinitionのJSON仕様（全Fieldの単位・Range） |
| `docs/runtime-processing.md` | 実行時の挙動（Voice、ADSR、Sample再生） |
| `docs/cli.md` | CLIの全Command・Option・Exit Code |
| `docs/architecture.md` | システムの静的構造 |
| `.agents/skills/create-instrument/` | Agent向けの実行手順 |
