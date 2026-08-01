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

## 各項目の制約

| 項目 | 制約 |
|---|---|
| `schema_version` | 1のみ |
| `layers` | 1個以上。複数のLayerは書かれた順に同じVoiceへMixされます。`enabled`が`false`のLayerはCompile対象外 |
| `generator` | `oscillator`（`sine` / `saw`）または`sample` |
| `polyphony` | 1〜64 |
| `gain_db` | -60〜12 dB |
| `pan` | -1〜1 |
| `tuning_cents` | -1200〜1200 |
| Key / Velocity | 0〜127。最小値は最大値以下 |
| ADSR | Attack / Decay / Releaseは0〜30秒、Sustainは0〜1 |
| Voice Filter | Cutoff 20〜20000Hz、Resonance 0〜1。CutoffがSample Rateの上限を超える場合はWarningを出して`min(20000, Sample Rate × 0.45)`に制限します |
| 未知のField | JSON Parse Errorとして扱います |
| 保存しないもの | Runtime状態、DaisySP Handle、Decode済みBuffer、Filter状態、Scratch Buffer |

Validation Errorには`layers[0].envelope.attack_seconds`のようなField Pathが付きます。

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

## Compile時の変換

Compileで一度だけ計算します。

| 変換 | 内容 |
|---|---|
| dB → Gain | `gain_db`を線形のGainへ |
| cent → 音程比 | `tuning_cents`を再生速度の比へ |
| ADSRの秒 → Frame数 | Sample Rateに依存するFrame数へ |
| Filter Cutoff | Sample Rateの上限へ制限 |
| Velocity Response | `layer_gain_amount`と`filter_cutoff_octaves`を実行時の値へ |

**ErrorとWarning**

- Errorが1つでもあれば、`CompiledInstrument`を返しません
- Warningだけなら、Warning付きの`CompiledInstrument`を返して処理を続けます
- AssetのSHA-256省略はWarningです（Layerは有効のまま）
- Assetの欠落・Hash不一致・読み込み失敗のあるSample Layerは無効にしてWarningを残し、ほかの有効なLayerがあれば処理を続けます
