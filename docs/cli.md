# CLI

Binary名は`sonalloy`です。CLIはDefinitionの読込・表示・Compileを行い、CoreのRendererを呼び出して生成されたAudioをWAVへ保存します。

## Instrument Command

```bash
sonalloy instrument init <path>
sonalloy instrument validate <definition>
sonalloy instrument inspect <definition>
```

`init`は最小のP1 Definitionを生成します。`validate`はJSON Parse、Definition Validation、Compile Diagnosticsまでを行い、WAVは生成しません。`inspect`はMetadata、Polyphony、Layer Trigger、Generator、Phase Reset、P1では該当しないAsset状態、Gain、Pan、Tuning、Envelope、Voice Filter、Velocity Response、Warningを表示します。`--json`を付けると同じ構成を機械可読形式で返します。

## Render Command

### `render note`

```bash
sonalloy render note examples/instruments/basic-poly-synth.json \
  --note 60 --velocity 100 --gate 0.5 --tail 0.5 \
  --sample-rate 48000 --block-size 257 --output out/note.wav
```

`render note`はCoreへNote OnとNote Offを渡します。

### `render midi`

```bash
sonalloy render midi \
  examples/instruments/basic-poly-synth.json \
  testdata/midi/p1-review.mid \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/p1-basic-poly-synth.wav
```

MIDI FileのTick、Tempo、Channel、NoteをCLI側でAbsolute Frameの`ScheduledEvent`へ変換します。Note OnのVelocity 0はNote Offとして扱い、Note IDはChannel・Note Number・発音Serialから生成します。Sustain Pedal、Pitch Bend、Aftertouch、Program Change等のMVP外Eventは無視し、Warningを返します。Coreへ`midly`型は渡しません。

## `dev render-sine`

```bash
sonalloy dev render-sine \
  --frequency 440 \
  --duration 1.0 \
  --sample-rate 48000 \
  --block-size 257 \
  --output out/sine.wav
```

| Option | 必須 | Default | 内容 |
|---|---:|---:|---|
| `--frequency <Hz>` | No | `440` | Sine周波数。有限かつ0以上 |
| `--duration <seconds>` | Yes | — | Main Render時間。有限かつ0以上 |
| `--sample-rate <Hz>` | No | `48000` | 正の整数。WAV HeaderとDSPへ同じ値を渡す |
| `--block-size <frames>` | No | `257` | Process最大Block Size |
| `--tail <seconds>` | No | `0` | Main Render後の追加Frame |
| `--output <path>` | Yes | — | Stereo WAV出力先 |
| `--json` | No | Off | ResultまたはDiagnosticをJSONで出力 |

出力は32-bit float、2 Channel、指定Sample RateのWAVです。親Directoryは事前に作成してください。

## Exit Code

| Code | 意味 |
|---:|---|
| `0` | 成功 |
| `1` | Definition / Compile Error |
| `2` | CLI入力またはRender Request Error |
| `3` | Core Process / Render Error |
| `4` | WAV出力 Error |

入力不正はJSON時に次の形で返ります。

```json
{
  "status": "error",
  "exit_code": 2,
  "diagnostics": [
    {
      "code": "VALUE_OUT_OF_RANGE",
      "severity": "error",
      "path": null,
      "message": "block size must be greater than zero",
      "detail": null
    }
  ]
}
```

DefinitionやEventの主な診断Codeは`SCHEMA_UNSUPPORTED`、`JSON_INVALID`、`REQUIRED_FIELD_MISSING`、`ID_DUPLICATED`、`VALUE_OUT_OF_RANGE`、`LAYER_RANGE_INVALID`、`FILTER_CUTOFF_CLAMPED`、`EVENT_ORDER_INVALID`、`DSP_ERROR`です。P2のAsset処理に備えたAsset系Codeも共通Enumで予約しています。

## 責務境界

CLIはclapのArgument型、Terminal表示、Path、WAV Writer、Exit Codeを所有します。CoreへCLI型やhound型を渡しません。Native DSPを直接呼ばず、必ずCore Rendererを経由します。
