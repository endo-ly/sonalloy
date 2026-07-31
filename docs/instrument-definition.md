# Instrument Definition

Instrument Definitionは、編集・保存・差分管理するJSONの正本です。Audio処理はDefinitionを直接参照せず、`compile_instrument`で`CompiledInstrument`へ変換された値だけを使用します。

## 完全なDefinition

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
          "waveform": "saw",
          "phase_reset": true
        }
      }
    }
  ],
  "voice_filter": {
    "cutoff_hz": 12000.0,
    "resonance": 0.12
  },
  "velocity_response": {
    "layer_gain_amount": 0.7,
    "filter_cutoff_octaves": 1.5
  }
}
```

## Definitionの制約

- `schema_version`は`1`だけを受け付ける。
- `layers`は配列で保存し、少なくとも1個のLayerを含める。有効Layerは複数指定でき、Definitionの順序で同じVoiceへMixする。`enabled: false`のLayerはCompile対象から除外する。
- Generatorは`oscillator`または`sample`を指定する。OscillatorのWaveformは`sine`または`saw`に限る。
- SampleはWAVの相対または絶対Path、任意のSHA-256、MIDI Root Note、`one_shot`、`cubic`を持つ。
- `polyphony`は1〜64、`gain_db`は-60〜12 dB、`pan`は-1〜1、`tuning_cents`は-1200〜1200 cent。
- Keyは0〜127、Velocityは1〜127で、各Rangeのminはmax以下にする。
- ADSRの時間は0〜30秒、Sustainは0〜1にする。0秒Segmentは次のStateへ直ちに遷移する。
- Voice FilterのCutoffは20〜20000 Hz、Resonanceは0〜1にする。CutoffがProcess Sample Rateの上限を超えた場合だけCompile時にWarningを出して`min(20000, sample_rate × 0.45)`へ制限する。
- 未知Fieldは無視せず、JSON Parse Errorとして扱う。
- Runtime状態、DaisySP Handle、Decode済みBuffer、Filter State、Scratch Bufferは保存しない。

## Sample Generator

Sample Layerの最小構成は次のとおりです。

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

Asset PathはDefinitionのあるDirectoryを基準に解決します。Compile時にSHA-256を検証してからSymphoniaでWAVをDecodeし、StereoはMonoへ平均Downmixします。Process Sample Rateと異なる場合はCompile時にRubatoでResampleします。SourceのSample Rate、Channel数、Bit Depth、Frame数はCompiled Sampleへ保持します。

SHA-256を省略した場合はWarningを出します。Assetが見つからない、Hashが一致しない、DecodeまたはResampleに失敗した場合、そのSample Layerは無効化してWarningを保持し、ほかの有効LayerがあればCompileとRenderを継続します。

SampleはNote OnでCursorを0から開始し、`root_note`との差とLayer Tuningから再生速度を決めます。4点Cubic Interpolationで読み出し、`one_shot`の末尾でLayerをIdleにします。Note OffはSampleのCursorを停止せず、Layer EnvelopeをReleaseへ遷移させます。

## Metallic Hybrid Definition

`examples/instruments/metallic-hybrid.json`は、全Key / 全VelocityをTriggerするSample Attack LayerとSine Body Layerを一つのVoiceへ定義した完全なReference Definitionです。Attackは短いDecayと低いSustainでTransientを担当し、Bodyは長いADSRで音程の芯と余韻を担当します。`voice_filter`と`velocity_response`はLayer Mix後のVoice処理へ適用します。

## Compile後の変換

Compile時に次を一度だけ計算します。

- dB → Linear Gain
- cent → Tuning Ratio
- ADSR秒 → Sample Rate依存のFrame数
- Voice Filter CutoffのProcess Sample Rate上限
- Velocity Responseの実行値

Compile Errorが一つでもある場合は`CompiledInstrument`を返しません。Warningだけの場合は、Warningを保持した`CompiledInstrument`を返し、Renderを継続できます。

## CLIでの確認

```bash
sonalloy instrument validate examples/instruments/basic-poly-synth.json
sonalloy instrument inspect examples/instruments/basic-poly-synth.json
```

Validation Errorには`layers[0].envelope.attack_seconds`のようなField Pathが付きます。Definitionの基準DirectoryはCompilerへ渡しますが、Definition自身には保持しません。
