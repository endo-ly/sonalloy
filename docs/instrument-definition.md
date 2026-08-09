# 音源定義（Instrument Definition）

## 本書の範囲

本書ではInstrument Definitionファイルの**データ形式**を説明します。JSONの書き方、各項目の制約、Compile時の変換とError / Warningの扱いです。

| 本書で扱わない内容 | 参照先 |
|---|---|
| 実行時の動き（Voice・ADSR・Sampleの再生） | `docs/runtime-processing.md` |
| CLIの使い方・Option | `docs/cli.md` |

Instrument Definitionは、手で編集して保存・管理するJSONファイルの正本です。Audio処理はDefinitionを直接使わず、`compile_instrument`で変換した`CompiledInstrument`の値だけを使います。

## 全体の例

```json
{
  "schema_version": 1,
  "metadata": {
    "name": "Basic Poly Synth",
    "author": null,
    "description": "A headless oscillator instrument"
  },
  "performance": {
    "polyphony": 16,
    "voice_stealing": "quietest_releasing_then_oldest"
  },
  "layers": [
    {
      "id": "body",
      "enabled": true,
      "trigger": {
        "event": "note_on",
        "key_min": 0,
        "key_max": 127,
        "velocity_min": 1,
        "velocity_max": 127
      },
      "gain_db": -14.0,
      "pan": 0.0,
      "tuning_cents": 0.0,
      "envelope": {
        "attack_seconds": 0.005,
        "decay_seconds": 0.18,
        "sustain_level": 0.65,
        "release_seconds": 0.3
      },
      "generator": {
        "oscillator": {
          "waveform": {
            "type": "saw"
          },
          "phase_reset": true,
          "phase": 0.0
        }
      },
      "processors": []
    }
  ],
  "voice_processors": [
    {
      "type": "filter",
      "id": "tone",
      "cutoff_hz": 12000.0,
      "resonance": 0.12
    }
  ],
  "global_processors": [],
  "modulation": {
    "sources": [],
    "routes": [
      {
        "source": "velocity",
        "target": "layer.body.gain",
        "amount": 0.08,
        "curve": "linear"
      },
      {
        "source": "velocity",
        "target": "voice.processor.tone.cutoff",
        "amount": 0.08,
        "curve": "linear"
      }
    ]
  }
}
```

## 各項目の制約

| 項目 | 制約 |
|---|---|
| `schema_version` | 1のみ |
| `layers` | 1個以上。複数のLayerは書かれた順に同じVoiceへMixされます。`enabled`が`false`のLayerはCompile対象外 |
| `generator` | `oscillator`（`sine` / `saw` / `square` / `triangle` / `pulse`）、`noise`（`white` / `pink` / `brown`）、`additive`、`formant`、`wavetable`、`sample`、`granular`、`wave_sequence`、または`operator_modulation` |
| `processors` | Layerごとの直列Processor配列。書かれた順にGeneratorとLayer Mixの間で適用 |
| `voice_processors` | Voice Mix後に適用する直列Processor配列 |
| `global_processors` | Voice Sum後にInstrument全体へ適用する直列Processor配列 |
| `polyphony` | 1〜64 |
| `gain_db` | -60〜12 dB |
| `pan` | -1〜1 |
| `tuning_cents` | -1200〜1200 |
| Key / Velocity | 0〜127。最小値は最大値以下 |
| ADSR | Attack / Decay / Releaseは0〜30秒、Sustainは0〜1 |
| Filter | `cutoff_hz`は20〜20000Hz、`resonance`は0〜1。CutoffがSample Rateの上限を超える場合はWarningを出して`min(20000, Sample Rate × 0.45)`に制限します |
| Drive | `amount`、`mix`ともに0〜1 |
| Delay | `time_seconds`は0.001〜2秒、`feedback`は0〜0.95、`mix`は0〜1。Globalのみ |
| Reverb | `pre_delay_seconds`は0〜0.2秒、`decay`は0〜0.98、`damping`、`width`、`mix`は0〜1。Globalのみ |
| Processor ID | 各Chain内で一意。小文字で始まり、小文字・数字・`_`を使用。`.`は使用しません |
| Layer / Source ID | 小文字で始まり、小文字・数字・`_`を使用。`.`は使用しません |
| Modulation Amount | -1〜1。TargetのNative範囲に対する割合 |
| LFO | Rate 0.01〜40Hz、Phase 0以上1未満 |
| Modulation Envelope | 各時間0〜30秒、Sustain 0〜1 |
| Parameter Target | `layer.<layer_id>.(gain\|pan\|tuning)`、`layer.<layer_id>.generator.(pulse_width\|sync_ratio\|waveshape\|wavetable_position\|unison_detune\|unison_spread\|noise_correlation\|granular_position\|grain_size\|grain_density\|grain_pitch\|grain_randomness\|grain_pan_spread\|additive_morph\|additive_spectrum_tilt\|additive_inharmonicity\|formant_vowel_position\|formant_shift\|formant_throat\|formant_spectral_tilt)`、`layer.<layer_id>.generator.operator.<1-4>.<parameter>`、`layer.<layer_id>.processor.<processor_id>.<parameter>`、`voice.processor.<processor_id>.<parameter>`、`global.processor.<processor_id>.<parameter>` |
| 未知のField | JSON Parse Errorとして扱います |
| 保存しないもの | Runtime状態、DaisySP Handle、Decode済みBuffer、Layer / Voice / Global Processor状態、Scratch Buffer |

Validation Errorには`layers[0].envelope.attack_seconds`のようなField Pathが付きます。

## Generator

### Oscillator

`waveform`はTagged Objectです。文字列だけのWaveformは受け付けません。

```json
{
  "generator": {
    "oscillator": {
      "waveform": {
        "type": "pulse",
        "pulse_width": 0.35
      },
      "phase_reset": true,
      "phase": 0.0
    }
  }
}
```

`type`は`sine`、`saw`、`square`、`triangle`、`pulse`です。`pulse`だけが`pulse_width`を持ち、値域は0.05〜0.95です。`phase_reset`はNote Onごとの初期PhaseへのResetを、`phase`は0〜1の初期Phaseを表します。

Square、Triangle、PulseはBand-limited Native Oscillatorを使用します。Pulse Widthは`layer.<layer_id>.generator.pulse_width`として5msでSmoothingされ、既存のLFO、Envelope、External ControlなどからModulationできます。

### Complex Oscillator

基本Oscillatorへ`hard_sync`、`waveshaping`、`unison`を追加できます。これらの設定はStatic Fieldであり、存在する設定だけがDynamic Parameter Catalogへ登録されます。

```json
{
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
        "stereo_spread": 0.85,
        "phase_spread": 0.0
      }
    }
  }
}
```

| Field | Range | Dynamic | Scale | Smoothing |
|---|---:|---:|---|---:|
| `hard_sync.ratio` | 1〜16 | 可 | Log2 | 5ms |
| `waveshaping.amount` | 0〜1 | 可 | Linear | 5ms |
| `unison.voices` | 2〜8 | 不可 | — | — |
| `unison.detune_cents` | 0〜100 | 可 | Linear | 10ms |
| `unison.stereo_spread` | 0〜1 | 可 | Linear | 10ms |
| `unison.phase_spread` | 0〜1 | 不可 | — | — |

Hard SyncはSineでは使用できません。Saw、Square、Triangle、PulseはDaisySPのVariable Shape Oscillatorを使い、Master Frequency、Slave Frequency（Master × Ratio）、Pulse WidthをSample単位で更新します。Hard Syncは任意の開始Phaseを設定できないため、`phase`は0だけを指定できます。Hard SyncのEffective Frequency上限はBackendの安全範囲に制限されます。Hard SyncとUnisonを組み合わせる場合、`phase_spread`は0だけを指定できます。

UnisonのDetune DistributionとPan Distributionは`-1`から`1`の対称係数で、Phase Distributionは`phase_spread × index / voices`です。各Voiceは`1 / sqrt(voices)`で正規化し、2 Voice以上ではStereo GeneratorとしてLayerへ渡します。WaveshapingはUnison MixとStereo Placementの直後、Layer Processorの前に適用されます。`amount = 0`は入力を変更しません。

Dynamic Parameterは次のIDで既存のLFO、Envelope、Mod Wheel、Parameter Changeから制御できます。

- `layer.<layer_id>.generator.sync_ratio`
- `layer.<layer_id>.generator.waveshape`
- `layer.<layer_id>.generator.unison_detune`
- `layer.<layer_id>.generator.unison_spread`

Phase Distortion、Oscillator Feedback、WavefoldはOptional Fieldです。Phase DistortionとOscillator FeedbackはSineだけで使用でき、Hard Syncとは併用できません。Wavefoldは全Waveformで使用できます。

```json
{
  "generator": {
    "oscillator": {
      "waveform": { "type": "sine" },
      "phase_reset": true,
      "phase": 0.0,
      "hard_sync": null,
      "waveshaping": { "amount": 0.15 },
      "phase_distortion": { "amount": 0.65 },
      "wavefold": { "amount": 0.35 },
      "feedback": { "amount": 0.3 },
      "unison": null
    }
  }
}
```

| Field | Range | Dynamic | Meaning |
|---|---:|---:|---|
| `phase_distortion.amount` | 0〜1 | Yes | SineのRead Phaseを連続的に変形する量 |
| `wavefold.amount` | 0〜1 | Yes | DaisySP WavefolderのDriveとDry/Wetへ変換する量 |
| `feedback.amount` | 0〜1 | Yes | 直前Sampleの出力をPhaseへ戻す量 |

Canonical Parameter IDは`layer.<layer_id>.generator.phase_distortion`、`layer.<layer_id>.generator.wavefold`、`layer.<layer_id>.generator.oscillator_feedback`です。いずれも5msでSmoothingされます。WavefoldのAmountは内部で`drive = 1 + amount × 7`、`mix = amount`へ変換され、DaisySPのOffsetは0に固定されます。

信号順は`Phase-domain生成 → Unison Mix → Existing Waveshaping → Wavefolder → DC Blocker`です。Wavefoldだけを使用する場合は既存Oscillator Backendを維持し、WavefoldをUnison MixとExisting Waveshapingの後へ適用します。Phase Distortion、Oscillator Feedback、Wavefoldのいずれかが有効な場合はGenerator末尾へDC Blockerを置きます。

### Noise

```json
{
  "generator": {
    "noise": {
      "color": "pink",
      "seed": 812347,
      "stereo_correlation": 0.65
    }
  }
}
```

`color`は`white`、`pink`、`brown`です。`seed`、Layer ID、Note ID、Stream種別から決定的なNoise Streamを生成します。`stereo_correlation`は0〜1で、0は左右独立、1は左右同一のStreamです。このParameterは`layer.<layer_id>.generator.noise_correlation`として10msでSmoothingされます。Noise Generatorは常にStereoです。

### Additive

Additiveは、LayerのNote Frequencyを基準にした1〜64個のSine Partialを加算します。Integer RatioによるHarmonic、Fractional RatioによるInharmonic Spectrum、PartialごとのInitial PhaseとOptional ADSRを同じPartial Slotへ定義できます。

```json
{
  "generator": {
    "additive": {
      "phase_reset": true,
      "morph": 0.0,
      "spectrum_tilt_db_per_octave": -3.0,
      "inharmonicity": 0.0,
      "partials": [
        {
          "id": "fundamental",
          "ratio": 1.0,
          "amplitude_a": 1.0,
          "amplitude_b": 0.7,
          "phase": 0.0
        },
        {
          "id": "second",
          "ratio": 2.0,
          "amplitude_a": 0.45,
          "amplitude_b": 0.8,
          "phase": 0.0,
          "envelope": {
            "attack_seconds": 0.01,
            "decay_seconds": 0.8,
            "sustain_level": 0.2,
            "release_seconds": 0.3
          }
        },
        {
          "id": "inharmonic",
          "ratio": 2.73,
          "amplitude_a": 0.15,
          "amplitude_b": 0.5,
          "phase": 0.25
        }
      ]
    }
  }
}
```

| Field | Range / Unit | Dynamic | Meaning |
|---|---:|---:|---|
| `phase_reset` | Boolean | No | Note OnでPartial Phaseを初期値へ戻すか。Instrument Resetでは常に戻ります |
| `morph` | 0〜1 | Yes | Spectrum AからSpectrum BへのAmplitude補間 |
| `spectrum_tilt_db_per_octave` | -24〜12 dB / octave | Yes | Ratio 1を基準にした高次Partialの傾き |
| `inharmonicity` | 0〜1 | Yes | Ratioが高いPartialほど周波数をStretchする量 |
| `partials` | 1〜64個 | No | Definition順の固定Partial Slot |
| `partials[].id` | 空でない一意な文字列 | No | InspectとDiagnosticでPartialを識別する名前。Parameter IDには展開しません |
| `partials[].ratio` | 0.125〜64 | No | Note Frequencyに対する倍率。整数に限定しません |
| `partials[].amplitude_a` / `amplitude_b` | 0〜1 | No | Morphの両端で使うAmplitude |
| `partials[].phase` | 0〜1 cycle | No | PartialのInitial Phase |
| `partials[].envelope` | Optional ADSR | No | Partialだけに適用するAmplitude Envelope |

`morph`はAmplitudeだけを`A + (B - A) × morph`で補間します。Ratio、Phase、Partial ID、Envelope DefinitionはMorph中に変わりません。Spectrum Tiltは`log2(ratio)`をOctave数としてGainへ変換し、Inharmonicityは`ratio × sqrt((1 + B × ratio²) / (1 + B))`で高次RatioをStretchします。Ratio 1.0のFundamentalは常に1.0です。

多数Partialの加算はEnergy Normalizationを使います。`Σ gain²`の平方根が1を超える場合だけ全Partial Gainを割り、Partial EnvelopeのActive数はNormalizationへ使用しません。高域PartialはFrequency Clampを行わず、Sample Rateの0.40〜0.45倍でLinear Fadeします。Additiveの出力はMonoです。

Dynamic Parameterは次のCanonical IDで制御できます。

- `layer.<layer_id>.generator.additive_morph`（Normalized、0〜1、Linear、10ms）
- `layer.<layer_id>.generator.additive_spectrum_tilt`（Decibels Per Octave、-24〜12、Linear、10ms）
- `layer.<layer_id>.generator.additive_inharmonicity`（Normalized、0〜1、Linear、10ms）

基準となるDefinitionは[`examples/instruments/additive-generator-reference.json`](../examples/instruments/additive-generator-reference.json)です。

### Formant

Formantは、Note Frequencyの整数倍Harmonic Partialへ5本のGaussian-likeなFormant Bandを適用します。External Audioを必要とせず、Profileの組み合わせでVowel-likeなSpectrumを作ります。出力はMonoです。

```json
{
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
            { "frequency_hz": 800.0, "bandwidth_hz": 80.0, "gain_db": 0.0 },
            { "frequency_hz": 1150.0, "bandwidth_hz": 90.0, "gain_db": -5.0 },
            { "frequency_hz": 2900.0, "bandwidth_hz": 120.0, "gain_db": -12.0 },
            { "frequency_hz": 3900.0, "bandwidth_hz": 130.0, "gain_db": -18.0 },
            { "frequency_hz": 4950.0, "bandwidth_hz": 140.0, "gain_db": -24.0 }
          ]
        }
      ]
    }
  }
}
```

| Field | Range / Unit | Dynamic | Meaning |
|---|---:|---:|---|
| `phase_reset` | Boolean | No | Note OnでHarmonic PartialのPhaseを初期値へ戻すか |
| `partial_count` | 1〜64 | No | 生成するHarmonic Partial数 |
| `vowel_position` | 0〜1、Normalized | Yes | Profile配列の先頭から末尾までの位置 |
| `formant_shift_cents` | -2400〜2400 cents | Yes | Formant中心とBandwidthへ同じRatioで適用する周波数移動 |
| `throat` | 0〜1、Normalized | Yes | Bandwidth倍率。0で0.5倍、0.5で1倍、1で2倍 |
| `spectral_tilt_db_per_octave` | -24〜12 dB / octave | Yes | Harmonic Ratioに対するSpectral Tilt |
| `profiles` | 1〜8件 | No | Morphに使うDefinition順のProfile |
| `profiles[].id` | 空でない一意な文字列 | No | InspectとDiagnosticでProfileを識別する名前 |
| `profiles[].formants` | 5件固定 | No | 周波数昇順のFormant Band |
| `frequency_hz` | 100〜12000 Hz | No | Gaussian Peakの中心周波数 |
| `bandwidth_hz` | 20〜5000 Hz | No | GaussianのFWHM |
| `gain_db` | -60〜12 dB | No | Formant Bandの相対強度 |

Profile間のFrequencyとBandwidthはGeometric Interpolation、GainはdB Linear Interpolationです。`formant_shift_cents`はCenterとBandwidthへ同じ`2^(cents / 1200)`を掛け、NoteのFundamental Frequencyは変更しません。`throat`のBandwidth倍率は`2^(2 × (throat - 0.5))`です。各PartialのGainは5 Bandの`db_to_linear(gain_db) × exp(-0.5 × ((frequency - center) / (bandwidth / 2.35482))²)`を加算し、Spectral TiltとAlias Fadeを乗算します。最後に固定Partial全体のEnergyを基準としてTarget GainをNormalizeします。

高域PartialはFrequency Clampを行わず、Sample Rateの0.40〜0.45倍でLinear Fadeします。PhaseはGainが0の間も進み続けます。Gaussian計算は約1msのSpectral Control Tickで行い、Tick間はPartial Bankの固定Rampを使います。

Dynamic Parameterは次のCanonical IDで制御できます。

- `layer.<layer_id>.generator.formant_vowel_position`（Normalized、0〜1、Linear、10ms）
- `layer.<layer_id>.generator.formant_shift`（Cents、-2400〜2400、Linear、10ms）
- `layer.<layer_id>.generator.formant_throat`（Normalized、0〜1、Linear、10ms）
- `layer.<layer_id>.generator.formant_spectral_tilt`（Decibels Per Octave、-24〜12、Linear、10ms）

Profileが0件または9件以上、Partial数が0または65以上、Band数が5以外、ID重複、周波数非昇順、各Range違反はValidation Errorです。基準となるDefinitionは[`examples/instruments/formant-generator-reference.json`](../examples/instruments/formant-generator-reference.json)です。AdditiveとFormantを重ねる基準例は[`examples/instruments/harmonic-formant-hybrid-reference.json`](../examples/instruments/harmonic-formant-hybrid-reference.json)です。

### Harmonic / Formant Hybrid

Formant、Additive、Sample、NoiseはLayerへ同時に記述できます。Layerの記述順にGenerator出力を加算し、各LayerのEnvelopeとProcessor、Voice Processor、Global Processorを順番に適用します。Formantの4 ParameterはHybridでも同じCanonical IDを使うため、LFO、Modulation Envelope、Velocity、Mod Wheel、Aftertouch、Parameter Changeから制御できます。

動作する構成例は[`harmonic-formant-hybrid-reference.json`](../examples/instruments/harmonic-formant-hybrid-reference.json)です。このDefinitionは、FormantとAdditiveを持続成分、SampleをAttack、NoiseをAirとして配置し、Layer Filter / Drive、Voice Filter / Drive、Global Delay / Reverbを組み合わせています。Processor ChainとRouteは`instrument inspect --json`で配置、順序、Parameter ID、Source、Targetを確認できます。

### Wavetable

Wavetableは、WAVのMono Sample列またはStereo Sample列を、明示した`frame_length`ごとの連続した周期Frameとして読み込みます。Stereo AssetはCompile時にMonoへ平均Downmixされます。

```json
{
  "generator": {
    "wavetable": {
      "asset": {
        "path": "../../testdata/assets/digital-motion.wav",
        "sha256": "<SHA-256>"
      },
      "frame_length": 2048,
      "position": 0.25,
      "phase_reset": true,
      "phase": 0.0,
      "unison": {
        "voices": 5,
        "detune_cents": 14.0,
        "stereo_spread": 0.75,
        "phase_spread": 0.5
      }
    }
  }
}
```

| Field | Range | Dynamic | Meaning |
|---|---:|---:|---|
| `asset` | — | No | MonoまたはStereoのWAV Asset |
| `frame_length` | 64〜4096、2の冪 | No | 一周期FrameのSample数 |
| `position` | 0〜1 | Yes | 最初のFrameから最後のFrameまでの位置 |
| `phase_reset` | Boolean | No | Note Onで初期Phaseへ戻すか |
| `phase` | 0〜1 | No | Initial Phase |
| `unison` | 既存Unison範囲 | 一部Yes | Wavetable全体のUnison設定 |

Asset全体のSample数は`frame_length`で割り切れ、Frame数が1〜256である必要があります。FrameはAssetの先頭から順に分割され、Frame間はLinear、Table内はFour-point Cubicで補間されます。Source Sample RateはWavetableの時間軸やPitchへ使わず、Wavetable AssetをResampleしません。

Compile時には各FrameへFFTを適用し、`frame_length / 2`から`1`までのHarmonic上限を持つBand Tableを作成します。DCは保持し、Bandごとの自動Normalizeは行いません。RuntimeはComponent Frequencyに応じてBandを選び、隣接BandをLog2領域でCrossfadeします。WavetableはUnison 1ではMono、2 Voice以上ではStereoです。

Dynamic Parameterは次のCanonical IDを持ちます。

- `layer.<layer_id>.generator.wavetable_position`（0〜1、10ms Smoothing）
- `layer.<layer_id>.generator.unison_detune`（Unison指定時、0〜100 cents、10ms Smoothing）
- `layer.<layer_id>.generator.unison_spread`（Unison指定時、0〜1、10ms Smoothing）

Assetの欠落・Hash不一致・Decode失敗ではWavetable Layerだけを発音候補から除外し、ほかの有効LayerはCompileとRenderを継続します。レイアウト不正や全Frame無音はWavetableを準備できない診断になります。

### Granular

Granularは一つのPrepared AudioをCompile時にRegionへ変換し、Noteごとに固定Poolから複数のGrainを生成するGeneratorです。Sample GeneratorのPlayback Modeではなく、独立したGeneratorとしてLayerへ配置します。Mono AssetもGrainごとにConstant-powerでStereo配置するため、出力はStereoです。

```json
{
  "generator": {
    "granular": {
      "asset": {
        "path": "../../testdata/assets/metal-hit.wav",
        "sha256": "<SHA-256>"
      },
      "root_note": 60,
      "region": {
        "start_seconds": 0.05,
        "end_seconds": 0.9
      },
      "position": 0.5,
      "grain_size": 0.08,
      "density": 24.0,
      "pitch": 0.0,
      "randomness": 0.35,
      "pan_spread": 0.75,
      "seed": 8128
    }
  }
}
```

| Field | Range / Unit | Dynamic | Meaning |
|---|---:|---:|---|
| `asset` | — | No | MonoまたはStereoのWAV Asset。Sampleと同じPrepared Audioを共有 |
| `root_note` | 0〜127 | No | Sourceの基準MIDI Note |
| `region.start_seconds` / `end_seconds` | 0以上の秒 | No | Grain Positionが参照するPrepared Region。End省略時はAsset終端 |
| `position` | 0〜1 | Yes | Region内の基本Source Position。0はRegion Start、1はGrain長を考慮したRegion End側 |
| `grain_size` | 0.005〜0.5秒 | Yes | Hann Windowを適用するGrain長 |
| `density` | 1〜100 grains/sec | Yes | Grain Schedulerの生成密度 |
| `pitch` | -2400〜2400 cents | Yes | Note / Layer Tuningへ加算するGrain Pitch |
| `randomness` | 0〜1 | Yes | Deterministic RandomでRegion内へ分散する量 |
| `pan_spread` | 0〜1 | Yes | GrainごとのStereo配置幅 |
| `seed` | uint64 | No | Grain Position / Panの決定的Seed |

WindowはHann固定で、Window種別をDefinitionへ公開しません。GrainはCompile時に確保した64 Slotの固定Poolを再利用し、最大Densityと最大Grain SizeでもPoolを超えない範囲で動作します。PositionがRegion外へ回り込むRandom結果は循環し、実際のGrain長とPitchを考慮してRegion外をReadしません。

Canonical Parameter IDは次の形式です。

- `layer.<layer_id>.generator.granular_position`（Normalized、0〜1、5ms）
- `layer.<layer_id>.generator.grain_size`（Seconds、0.005〜0.5、Log2、10ms）
- `layer.<layer_id>.generator.grain_density`（Per Second、1〜100、Log2、10ms）
- `layer.<layer_id>.generator.grain_pitch`（Cents、-2400〜2400、5ms）
- `layer.<layer_id>.generator.grain_randomness`（Normalized、0〜1、10ms）
- `layer.<layer_id>.generator.grain_pan_spread`（Normalized、0〜1、10ms）

Assetの欠落・Hash不一致・Decode失敗ではGranular Layerを発音候補から除外し、ほかの有効LayerはCompileとRenderを継続します。RegionがPrepared Frameへ変換できない場合は`INVALID_GRAIN_REGION`、Parameter範囲違反は`INVALID_GRAIN_PARAMETER`を返します。

### Wave Sequence

Wave Sequenceは複数のAudio Assetを一つのGeneratorで時間順に再生します。StepはCompile後もDefinition順の不変配列として保持され、Assetを共有します。

```json
{
  "generator": {
    "wave_sequence": {
      "root_note": 60,
      "direction": "ping_pong",
      "loop": true,
      "crossfade": 0.25,
      "steps": [
        {
          "id": "attack",
          "asset": {
            "path": "../../testdata/assets/metal-hit.wav",
            "sha256": "<SHA-256>"
          },
          "region": {
            "start_seconds": 0.0,
            "end_seconds": 0.08
          },
          "duration": {
            "mode": "seconds",
            "value": 0.18
          },
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
          "region": {
            "start_seconds": 0.08,
            "end_seconds": 0.16
          },
          "duration": {
            "mode": "beats",
            "value": 0.5
          },
          "playback": "one_shot",
          "playback_direction": "reverse",
          "gain_db": -6.0,
          "pitch_cents": 300.0
        }
      ]
    }
  }
}
```

| Field | Range / Unit | Dynamic | Meaning |
|---|---:|---:|---|
| `root_note` | 0〜127 | No | Step Assetの基準MIDI Note |
| `direction` | `forward` / `reverse` / `ping_pong` | No | Stepを選択する順序 |
| `loop` | Boolean | No | 終端後にSequenceを繰り返すか |
| `crossfade` | 0〜0.5 | No | 隣接Stepを重ねる割合。Constant-powerで混合 |
| `steps` | 1〜128個 | No | 時間順のStep配列 |
| `steps[].id` | Component ID | No | Sequence内で一意なStep ID |
| `steps[].asset` | — | No | MonoまたはStereoのWAV Asset |
| `steps[].region` | 0以上の秒 | No | Assetから読む`[start, end)` Region |
| `steps[].duration` | `seconds`または`beats`、正の値 | No | Stepを保持する時間 |
| `steps[].playback` | `one_shot` / `loop` | No | Step内でAssetを一度だけ読むか繰り返すか |
| `steps[].playback_direction` | `forward` / `reverse` | No | Step AssetのRead方向 |
| `steps[].gain_db` | -60〜12 dB | No | Step固有のGain |
| `steps[].pitch_cents` | -2400〜2400 cents | No | Root Noteへ加算するStep Pitch |

`direction`はStepを選ぶ順序、`playback_direction`は選択されたAssetを読む方向です。`ping_pong`は終端Stepを重複再生せず、`A B C D C B A`の順で進みます。`one_shot`のSourceがDurationより先に終わった場合はStep終端まで無音を保持し、`loop`はRegionを繰り返します。

`duration.mode`が`seconds`の場合はTempoに依存せず、`beats`の場合はProcess Tempoの変更後から進行速度を変えます。途中のTempo変更でStepを先頭へ戻しません。Missing Assetや不正RegionのStepはCompile配列から削除せず、指定Durationを持つ無音Stepとして残します。全Stepが利用できない場合はLayerだけを発音候補から除外します。

Wave Sequence固有のDynamic Parameterは定義しません。Step構造、Duration、Direction、Crossfade、Pitch、GainはCompile時に確定します。

### Operator Modulation

Operator Modulationは4つのSine Operatorを固定Topologyで接続するGeneratorです。Definitionでは利用者向けのOperator番号を1〜4で記述し、Compile後は固定配列へ変換します。接続Algorithmは`stack_4`、`stack_3_plus_carrier`、`two_stacks`、`fork_to_carrier`、`two_modulators_plus_carrier`、`three_modulators`、`shared_modulator`、`parallel`です。任意の接続GraphやOperator間Cycleは指定できません。

```json
{
  "generator": {
    "operator_modulation": {
      "mode": "phase",
      "algorithm": "stack_4",
      "operators": [
        {
          "ratio": 1.0,
          "detune_cents": 0.0,
          "level": 0.9,
          "modulation_amount": 0.0,
          "feedback": 0.0,
          "phase": 0.0,
          "envelope": {
            "attack_seconds": 0.0,
            "decay_seconds": 0.1,
            "sustain_level": 1.0,
            "release_seconds": 0.1
          }
        },
        {
          "ratio": 2.0,
          "detune_cents": 0.0,
          "level": 0.0,
          "modulation_amount": 2.5,
          "feedback": 0.0,
          "phase": 0.0,
          "envelope": {
            "attack_seconds": 0.0,
            "decay_seconds": 0.08,
            "sustain_level": 1.0,
            "release_seconds": 0.08
          }
        },
        {
          "ratio": 3.0,
          "detune_cents": 0.0,
          "level": 0.0,
          "modulation_amount": 1.5,
          "feedback": 0.0,
          "phase": 0.0,
          "envelope": {
            "attack_seconds": 0.0,
            "decay_seconds": 0.06,
            "sustain_level": 1.0,
            "release_seconds": 0.06
          }
        },
        {
          "ratio": 5.0,
          "detune_cents": 0.0,
          "level": 0.0,
          "modulation_amount": 2.0,
          "feedback": 0.25,
          "phase": 0.0,
          "envelope": {
            "attack_seconds": 0.0,
            "decay_seconds": 0.04,
            "sustain_level": 1.0,
            "release_seconds": 0.04
          }
        }
      ],
      "phase_reset": true,
      "unison": null
    }
  }
}
```

`mode`は`phase`、`frequency`、`amplitude`、`ring`のいずれかです。PhaseはModulator出力へAmountの0.5を掛けてCycle単位のRead Phaseへ加え、Frequencyは`base_frequency × (1 + modulation + feedback)`で瞬時周波数を作ります。AmplitudeはIncomingごとの`1 + output × depth`を乗算し、最終値を0〜4へ制限します。RingはCarrierと`Carrier × Modulator`をDepthでCrossfadeします。

各Operatorの`ratio`は0.25〜32、`detune_cents`は-100〜100、`level`と`phase`は0〜1です。`modulation_amount`はPhase / Frequencyで0〜8のIndex、Amplitude / Ringで0〜1のNormalizedです。`feedback`は0〜1で、直前Sampleの自身の出力だけを`tanh`でBoundしてPhaseまたはFrequencyへ加えます。Amplitude / Ringでは0だけを許可します。

Operator EnvelopeはLayer Envelopeとは別に全Operatorへ適用され、Note Onで開始、Note OffでReleaseへ移行します。Carrierの`level`だけが最終出力へ寄与し、Carrier以外の`level`と出力先を持たない`modulation_amount`は0でなければなりません。Unisonは最大4 Voiceで、Envelopeを共有しながらComponentごとにPhaseとFeedback Stateを持ちます。Unison 1はMono、2以上はStereoです。

Dynamic ParameterはTopologyとModeに応じて次を公開します。

- `layer.<layer_id>.generator.operator.<1-4>.ratio`（Ratio、Log2、0.25〜32、5ms）
- `layer.<layer_id>.generator.operator.<1-4>.detune`（Cents、Linear、-100〜100、5ms）
- `layer.<layer_id>.generator.operator.<1-4>.level`（Carrierのみ、Normalized、0〜1、5ms）
- `layer.<layer_id>.generator.operator.<1-4>.modulation_amount`（接続元のみ、Mode依存、5ms）
- `layer.<layer_id>.generator.operator.<1-4>.feedback`（Phase / Frequencyのみ、0〜1、5ms）
- `layer.<layer_id>.generator.unison_detune` / `unison_spread`（Unison指定時）

OperatorのEffective Frequency上限はPhase / Frequencyで`Sample Rate × 0.24`、Amplitude / Ringで`Sample Rate × 0.45`です。詳細なTopologyとSignal順序は[`docs/runtime-processing.md`](runtime-processing.md)を参照してください。

Generator ParameterはLayer Gain / Pan / Tuningの後、Layer Processorの前にParameter Catalogへ追加されます。Sample、Wave Sequenceの構造はStaticで、Additiveの3 Parameter、Formantの4 Parameter、Granularの6 ParameterはDynamicです。

## Processor Chain

Processorは配列の順序で直列に適用されます。Processorの種類と配置は固定されており、LayerとVoiceではFilterとDrive、GlobalではFilter、Drive、Delay、Reverbを指定できます。DelayとReverbをLayerまたはVoiceへ置くとValidation Errorになります。

```json
{
  "processors": [
    {
      "type": "filter",
      "id": "attack_tone",
      "cutoff_hz": 9000.0,
      "resonance": 0.1
    },
    {
      "type": "drive",
      "id": "attack_drive",
      "amount": 0.25,
      "mix": 0.4
    }
  ],
  "voice_processors": [],
  "global_processors": [
    {
      "type": "delay",
      "id": "echo",
      "time_seconds": 0.28,
      "feedback": 0.3,
      "mix": 0.15
    },
    {
      "type": "reverb",
      "id": "space",
      "pre_delay_seconds": 0.012,
      "decay": 0.6,
      "damping": 0.35,
      "width": 1.0,
      "mix": 0.2
    }
  ]
}
```

Filterの`cutoff_hz`と`resonance`、Driveの`amount`と`mix`、Delayの`feedback`と`mix`、Reverbの`decay`、`damping`、`width`、`mix`がDynamic Parameterです。Delayの`time_seconds`とReverbの`pre_delay_seconds`、Processorの種類・ID・配置・順序はCompile時に固定されます。

Canonical Parameter IDは次の形式です。

- `layer.<layer_id>.processor.<processor_id>.<parameter>`
- `voice.processor.<processor_id>.<parameter>`
- `global.processor.<processor_id>.<parameter>`

Parameter Catalogは、各Layerの基本Parameter、Generator Parameter、Layer Processor、Voice Processor、Global Processorの順に並びます。Disabled LayerのCatalog項目もDefinitionの順序を維持します。

## Sample Layer

Sampleを使うLayerの最小構成です。

```json
{
  "id": "attack",
  "enabled": true,
  "trigger": {
    "event": "note_on",
    "key_min": 0,
    "key_max": 127,
    "velocity_min": 1,
    "velocity_max": 127
  },
  "gain_db": -18.0,
  "pan": 0.0,
  "tuning_cents": 0.0,
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
          "asset": {
            "path": "../../testdata/assets/metal-hit.wav",
            "sha256": "ecebbaa000ad97f19d659b4c7b42313ae47889b54191b85e6da0e8471979635c"
          },
          "root_note": 60,
          "key_min": 0,
          "key_max": 127,
          "velocity_min": 1,
          "velocity_max": 127,
          "round_robin_group": null,
          "playback": {
            "region": {
              "start_seconds": 0.0,
              "end_seconds": null
            },
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

**Assetの読み込み（Compile時）**

- Asset PathはDefinitionがあるフォルダを基準に解決します
- SHA-256を照合してから、SymphoniaでWAVを読み込みます
- Sample GeneratorのStereo WAVは左右を保持したPlanar Prepared Audioへ変換します。Mono WAVはMonoのまま保持します
- 再生時のSample Rateと違う場合は、RubatoでSample Rateを変換します
- ResampleはChannelごとに同じ設定で行い、左右のFrame数を一致させます
- 元のSample Rate、Channel数、Bit Depth、Frame数はPrepared AudioのSource Metadataに保持します
- 同一Assetを参照するZoneはCompile時にPrepared Audioを共有します
- Assetの欠落・Hash不一致・Decode失敗はそのZoneだけを無効化し、ほかのZoneとLayerのCompileを継続します

**Sample Zone**

- `id`はSample Generator内で一意なComponent IDです
- `root_note`は0〜127、`key_min` / `key_max`は0〜127、`velocity_min` / `velocity_max`は1〜127です。各範囲は`min <= max`で記述します
- `key_min` / `key_max`と`velocity_min` / `velocity_max`で発音範囲を指定します
- 重なるZoneは同じ`round_robin_group`と完全一致するKey / Velocity範囲を持つ必要があります
- Velocity Layerは重ならないVelocity範囲で記述します。範囲のGapでは発音しません
- 同じRound Robin Groupの選択はDefinition順で、Instrument単位のCounterによりA/B/A/Bと進みます

**Layer Trigger Event**

- `event`は`note_on`または`note_off`です。通常のLayerは`note_on`を指定します
- `note_on` LayerはNote Onで発音を開始します
- `note_off` LayerはNote OnでArmedになり、Audioを生成せずにNote IDを保持します。対応するNote Offで独立したEnvelopeのAttackから発音を開始します
- Voice Stealingは演奏上のNote Offではないため、Armed Layerを発音しません

**Playback Region**

- `region`は`start_seconds`と`end_seconds`を持ち、`[start, end)`として扱います。`end_seconds: null`はAsset終端です
- `direction`は`forward`または`reverse`です。ReverseはPrepared Audioを複製せずCursor方向だけを反転します
- `loop: null`はOne-shotです。Loopを指定する場合はRegion内の`start_seconds`、`end_seconds`、`crossfade_seconds`を記述します
- `crossfade_seconds: 0`は通常Loop、0より大きい場合はConstant-power Crossfade Loopです。CrossfadeはLoop長の半分以下でなければなりません
- `time`は`resample`、`fixed_stretch`、`tempo_sync`のいずれかです。省略できません
- `resample`はPitch変更に合わせてDurationも変えます。`fixed_stretch`は`ratio`（0.5〜2.0）でDurationだけを変え、`tempo_sync`は`source_bpm`（0より大きい値）とProcess TempoからDuration比を決めます
- `fixed_stretch`と`tempo_sync`はReverseと併用できません。RatioはCompile時にも検証され、範囲外の値をClampしません
- RegionとLoopはCompile時にEngine Sample RateのFrameへ変換され、RegionとLoopには最低2 Frameが必要です
- Explicit Sliceは同じAssetと異なるOne-shot Regionを持つ複数Zoneで表現します

```json
"playback": {
  "region": { "start_seconds": 0.0, "end_seconds": null },
  "direction": "forward",
  "loop": null,
  "time": { "mode": "fixed_stretch", "ratio": 1.5 }
}
```

Tempoに追従するZoneは次の形式です。

```json
"time": { "mode": "tempo_sync", "source_bpm": 120.0 }
```

`source_bpm / process_tempo_bpm`が0.5〜2.0の範囲外になる場合はProcess Errorになります。Tempo SyncはTempo Mapの境界でProcess Blockを分割して適用します。

Sampleの再生の動き（Cursor、再生速度、補間、終端の扱い）は、`docs/runtime-processing.md`の「Sampleの再生」を参照してください。

## Modulation

`modulation`は省略可能です。`sources`はVoiceごとのSource定義、`routes`はSourceから連続Parameterへの接続です。Routeは書かれた順に同じTargetへ加算され、最後にTarget範囲へClampされます。

組み込みSourceは次のとおりです。

| Source ID | 範囲 | 動作 |
|---|---|---|
| `velocity` | 0〜1 | Note OnのVelocity |
| `key_tracking` | -1〜1 | MIDI Note 0を-1、127を+1へ変換 |
| `pitch_bend` | -1〜1 | 共有External Control |
| `mod_wheel` | 0〜1 | 共有External Control |
| `aftertouch` | 0〜1 | 共有External Control |

Definitionで追加できるSourceは`lfo`、`envelope`、`random`です。LFOはBipolar、EnvelopeはNote Lifecycle、RandomはSeedとNote IDから決まるVoice単位の値です。

```json
{
  "modulation": {
    "sources": [
      {
        "type": "lfo",
        "id": "vibrato",
        "waveform": "sine",
        "rate_hz": 5.0,
        "phase": 0.0
      },
      {
        "type": "envelope",
        "id": "filter_env",
        "attack_seconds": 0.01,
        "decay_seconds": 0.2,
        "sustain_level": 0.3,
        "release_seconds": 0.25
      },
      {
        "type": "random",
        "id": "random_pan",
        "seed": 42
      }
    ],
    "routes": [
      {
        "source": "vibrato",
        "target": "layer.body.tuning",
        "amount": 0.02,
        "curve": "linear"
      },
      {
        "source": "filter_env",
        "target": "voice.processor.tone.cutoff",
        "amount": 0.2,
        "curve": "smooth_step"
      },
      {
        "source": "random_pan",
        "target": "layer.body.pan",
        "amount": 1.0,
        "curve": "linear"
      }
    ]
  }
}
```

ParameterのBase値はNormalized EventからDescriptorを通してNative値へ戻されます。GainはdB、Panは-1〜1、Tuningはcent、CutoffとRatioはLog2、IndexとResonanceはLinearで評価します。音声処理中に文字列IDやJSONを扱わないため、Parameter IDとRoute解決はCompile前に完了します。

## Compile時の変換

Compileで一度だけ計算します。

| 変換 | 内容 |
|---|---|
| dB → Gain | `gain_db`を線形のGainへ |
| cent → 音程比 | `tuning_cents`を再生速度の比へ |
| ADSRの秒 → Frame数 | Sample Rateに依存するFrame数へ |
| Granular Regionの秒 → Frame数 | Prepared Audio内の固定Regionへ |
| Filter Cutoff | Sample Rateの上限へ制限 |
| Parameter Catalog | LayerとProcessorの連続Parameterへ安定ID、範囲、Scale、Smoothingを割り当て |
| Modulation | SourceをDense Tableへ、RouteをTarget別の範囲へ変換 |

**ErrorとWarning**

- Errorが1つでもあれば、`CompiledInstrument`を返しません
- Warningだけなら、Warning付きの`CompiledInstrument`を返して処理を続けます
- Zone AssetのSHA-256省略はWarningです（Assetを読み込めたZoneは有効のまま）
- Assetの欠落・Hash不一致・読み込み失敗のあるSample Zoneは無効にしてWarningを残し、ほかの有効なZoneやLayerがあれば処理を続けます
- Parameter ID、Source ID、Source設定、Route Target、AmountのErrorはCompile前にまとめて返します
