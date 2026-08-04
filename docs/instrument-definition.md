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
| `generator` | `oscillator`（`sine` / `saw` / `square` / `triangle` / `pulse`）、`noise`（`white` / `pink` / `brown`）、または`sample` |
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
| Parameter Target | `layer.<layer_id>.(gain\|pan\|tuning)`、`layer.<layer_id>.generator.(pulse_width\|noise_correlation)`、`layer.<layer_id>.processor.<processor_id>.<parameter>`、`voice.processor.<processor_id>.<parameter>`、`global.processor.<processor_id>.<parameter>` |
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
      "asset": {
        "path": "../../testdata/assets/metal-hit.wav",
        "sha256": "ecebbaa000ad97f19d659b4c7b42313ae47889b54191b85e6da0e8471979635c"
      },
      "root_note": 60,
      "playback_mode": "one_shot",
      "interpolation": "cubic"
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

ParameterのBase値はNormalized EventからDescriptorを通してNative値へ戻されます。GainはdB、Panは-1〜1、Tuningはcent、CutoffはLog2、ResonanceはLinearで評価します。音声処理中に文字列IDやJSONを扱わないため、Parameter IDとRoute解決はCompile前に完了します。

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
- AssetのSHA-256省略はWarningです（Layerは有効のまま）
- Assetの欠落・Hash不一致・読み込み失敗のあるSample Layerは無効にしてWarningを残し、ほかの有効なLayerがあれば処理を続けます
- Parameter ID、Source ID、Source設定、Route Target、AmountのErrorはCompile前にまとめて返します
