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
| `generator` | `oscillator`（`sine` / `saw` / `square` / `triangle` / `pulse`）、`noise`（`white` / `pink` / `brown`）、`wavetable`、または`sample` |
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
| Parameter Target | `layer.<layer_id>.(gain\|pan\|tuning)`、`layer.<layer_id>.generator.(pulse_width\|sync_ratio\|waveshape\|wavetable_position\|unison_detune\|unison_spread\|noise_correlation)`、`layer.<layer_id>.generator.operator.<1-4>.<parameter>`、`layer.<layer_id>.processor.<processor_id>.<parameter>`、`voice.processor.<processor_id>.<parameter>`、`global.processor.<processor_id>.<parameter>` |
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

Generator ParameterはLayer Gain / Pan / Tuningの後、Layer Processorの前にParameter Catalogへ追加されます。Sample GeneratorにはGenerator Dynamic Parameterはありません。

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
            "type": "one_shot",
            "start_seconds": 0.0,
            "end_seconds": null
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
- StereoのWAVは左右の平均を取ってMonoへ変換します
- 再生時のSample Rateと違う場合は、RubatoでSample Rateを変換します
- 元のSample Rate、Channel数、Bit Depth、Frame数はCompiled Sampleに保持します
- 同一Assetを参照するZoneはCompile時にPrepared Sampleを共有します
- Assetの欠落・Hash不一致・Decode失敗はそのZoneだけを無効化し、ほかのZoneとLayerのCompileを継続します

**Sample Zone**

- `id`はSample Generator内で一意なComponent IDです
- `root_note`は0〜127、`key_min` / `key_max`は0〜127、`velocity_min` / `velocity_max`は1〜127です。各範囲は`min <= max`で記述します
- `key_min` / `key_max`と`velocity_min` / `velocity_max`で発音範囲を指定します
- 重なるZoneは同じ`round_robin_group`と完全一致するKey / Velocity範囲を持つ必要があります
- Velocity Layerは重ならないVelocity範囲で記述します。範囲のGapでは発音しません
- 同じRound Robin Groupの選択はDefinition順で、Instrument単位のCounterによりA/B/A/Bと進みます

**Playback Region**

- `one_shot`は`start_seconds`から`end_seconds`（`null`はAsset終端）までを一度だけ再生します
- `forward_loop`はRegion内の`loop_start_seconds`から`loop_end_seconds`へ到達するとLoop Startへ戻ります
- RegionとLoopはCompile時にEngine Sample RateのFrameへ変換され、最低2 Frameが必要です
- Explicit Sliceは同じAssetと異なるOne-shot Regionを持つ複数Zoneで表現します

Sampleの再生の動き（Cursor、再生速度、補間、終端の扱い）は、`docs/runtime-processing.md`の「Sample Runtime」を参照してください。

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
| Filter Cutoff | Sample Rateの上限へ制限 |
| Parameter Catalog | LayerとProcessorの連続Parameterへ安定ID、範囲、Scale、Smoothingを割り当て |
| Modulation | SourceをDense Tableへ、RouteをTarget別の範囲へ変換 |

**ErrorとWarning**

- Errorが1つでもあれば、`CompiledInstrument`を返しません
- Warningだけなら、Warning付きの`CompiledInstrument`を返して処理を続けます
- Zone AssetのSHA-256省略はWarningです（Assetを読み込めたZoneは有効のまま）
- Assetの欠落・Hash不一致・読み込み失敗のあるSample Zoneは無効にしてWarningを残し、ほかの有効なZoneやLayerがあれば処理を続けます
- Parameter ID、Source ID、Source設定、Route Target、AmountのErrorはCompile前にまとめて返します
