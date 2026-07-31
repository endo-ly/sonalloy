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
- `layers`は配列で保存し、有効Layerをちょうど1個にする。
- Generatorは`oscillator`だけで、Waveformは`sine`または`saw`に限る。
- `polyphony`は1〜64、`gain_db`は-60〜12 dB、`pan`は-1〜1、`tuning_cents`は-1200〜1200 cent。
- Keyは0〜127、Velocityは1〜127で、各Rangeのminはmax以下にする。
- ADSRの時間は0〜30秒、Sustainは0〜1にする。0秒Segmentは次のStateへ直ちに遷移する。
- Voice FilterのCutoffは20〜20000 Hz、Resonanceは0〜1にする。CutoffがProcess Sample Rateの上限を超えた場合だけCompile時にWarningを出して`min(20000, sample_rate × 0.45)`へ制限する。
- 未知Fieldは無視せず、JSON Parse Errorとして扱う。
- Runtime状態、DaisySP Handle、Decode済みBuffer、Filter State、Scratch Bufferは保存しない。

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
