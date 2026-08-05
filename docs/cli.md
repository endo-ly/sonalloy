# CLI

## 本書の範囲

本書はSonalloy CLIの**インターフェース**を定義します。Command、Option、Exit Code、Diagnosticsの出力形式です。

| 本書に書かないこと | 参照先 |
|---|---|
| CLIの所有責務・Crate境界 | `docs/architecture.md` |
| 実行時の挙動・Error規則 | `docs/runtime-processing.md` |
| DefinitionのJSON形式・制約 | `docs/instrument-definition.md` |

Binary名は`sonalloy`です。CLIはDefinitionの読込・表示・Compileを行い、CoreのRendererを呼び出して生成されたAudioをWAVへ保存します。

## コマンド一覧

| コマンド | 役割 |
|---|---|
| `instrument init` | 最小のOscillator Definitionを生成する |
| `instrument validate` | DefinitionをJSON Parse → Validation → Compileまで検証する |
| `instrument inspect` | Compile後の実行値を表示する |
| `render note` | 1つのNote On / Note OffをRenderしてWAVを生成する |
| `render events` | Absolute FrameのEvent SequenceをRenderしてWAVを生成する |
| `render midi` | Standard MIDI FileをRenderしてWAVを生成する |
| `dev render-sine` | 全処理経路を通すSine Render（動作確認用） |

## `instrument` Command

### `instrument init`

最小のOscillator Definition（Saw、Polyphony 16）を生成します。

```bash
sonalloy instrument init <path>
```

### `instrument validate`

DefinitionをJSON Parse → Validation → Compileの順に検証します。WAVは生成しません。

- 成功時：`valid <path>`とWarningを表示
- 失敗時：Diagnosticsを表示

```bash
sonalloy instrument validate <definition>
sonalloy instrument validate <definition> --json
```

### `instrument inspect`

Compile後の実行値を表示します。`--json`を付けると同じ内容を機械可読形式で返します。

| 表示項目 | 内容 |
|---|---|
| Metadata | 名前、作者、説明 |
| Performance | Polyphony、Voice Stealing方式 |
| Layer | Trigger（Key / Velocity範囲）、Generator、Gain、Pan、Tuning、Envelope |
| Oscillator | Waveform、Phase Reset、Phase、Backend、Output Mode、Effective Frequency上限、Pulse Width（Pulseのみ）、Hard Sync、Waveshaping、Unison |
| Noise | Color、Seed、Stereo Correlation Parameter、Output Mode |
| Sample | Asset Path、Root Note、Playback Mode、Interpolation、Source Metadata、Prepared Frame数、Output Mode |
| Parameter | Canonical ID、Owner、Unit、Range、Default、Scale、Smoothing |
| Modulation | Source ID、Source種類、Scope、Target、Amount、Curve |
| Processor | Layer、Voice、Globalの配置、Chain順、ID、Static Field、Dynamic Parameter |
| Warning | Compile時の警告一覧 |

```bash
sonalloy instrument inspect <definition>
sonalloy instrument inspect <definition> --json
```

`inspect`のParameter IDはCompiled Catalogから取得したCanonical IDです。RouteのSource ID、Target ID、設定値もCompiled Instrumentの内容をそのまま表示します。
ProcessorはChainごとに`placement`、`chain_index`、`id`、`kind`、Static Field、Parameter Descriptorを表示します。FilterはParameterのDefaultとSample Rateに応じたDSP適用上限をStatic Fieldへ表示します。DelayのTimeとReverbのPre-delayはStatic Fieldです。

## `render` Command

### `render note`

Coreへ1つのNote On / Note Offを渡してRenderします。単音の確認用です。

```bash
sonalloy render note examples/instruments/basic-poly-synth.json \
  --note 60 --velocity 100 --gate 0.5 --tail 0.5 \
  --sample-rate 48000 --block-size 257 --output out/note.wav
```

| Option | 必須 | Default | 内容 |
|---|---:|---:|---|
| `--note <0-127>` | No | `60` | MIDI Note番号 |
| `--velocity <1-127>` | No | `100` | MIDI Velocity |
| `--gate <seconds>` | No | `0.5` | Note OnからNote Offまでの時間。有限かつ0以上 |
| `--tail <seconds>` | No | `0.5` | Note Off後の追加Frame。有限かつ0以上 |
| `--sample-rate <Hz>` | No | `48000` | 正の整数。WAV HeaderとDSPへ同じ値を渡す |
| `--block-size <frames>` | No | `257` | Process最大Block Size |
| `--output <path>` | Yes | — | Stereo WAV出力先 |
| `--json` | No | Off | ResultまたはDiagnosticをJSONで出力 |

### `render events`

Absolute FrameのEvent Sequence JSONを読み込み、Parameter ChangeとExternal Controlを含む再現可能なRenderを行います。Parameter IDはRender開始前にHandleへ解決されます。

```bash
sonalloy render events \
  examples/instruments/basic-poly-synth.json \
  events.json \
  --sample-rate 48000 --block-size 257 \
  --duration-frames 192000 --tail 1.0 --output out/events.wav
```

Event Fileの例です。

```json
{
  "events": [
    {
      "absolute_frame": 0,
      "type": "parameter_change",
      "parameter": "voice.processor.tone.cutoff",
      "normalized": 0.35
    },
    {
      "absolute_frame": 0,
      "type": "note_on",
      "note_id": 1,
      "note": 60,
      "velocity": 100
    },
    {
      "absolute_frame": 24000,
      "type": "mod_wheel",
      "value": 1.0
    },
    {
      "absolute_frame": 48000,
      "type": "note_off",
      "note_id": 1
    }
  ]
}
```

対応する`type`は`note_on`、`note_off`、`parameter_change`、`pitch_bend`、`mod_wheel`、`aftertouch`です。入力EventはAbsolute Frame昇順で並べ、同一FrameではCoreのEvent Priorityへ従って安定Sortします。Duration外のEvent、未解決Parameter ID、範囲外の値が一つでもあればWAVを生成しません。

| Option | 必須 | Default | 内容 |
|---|---:|---:|---|
| `--duration-frames <frames>` | Yes | — | Main Render長。EventのAbsolute Frameはこの値未満 |
| `--tail <seconds>` | No | `1.0` | Main Render後の追加Frame |
| `--sample-rate <Hz>` | No | `48000` | 正の整数 |
| `--block-size <frames>` | No | `257` | Process最大Block Size |
| `--output <path>` | Yes | — | Stereo WAV出力先 |
| `--json` | No | Off | ResultまたはDiagnosticをJSONで出力 |

### `render midi`

Standard MIDI FileをAbsolute Frameの`ScheduledEvent`へ変換してRenderします。

- Tick、Tempo、Channel、NoteはCLI側でAbsolute Frameへ変換する
- Note OnのVelocity 0はNote Offとして扱う
- Note IDはChannel・Note Number・発音Serialから生成する
- CC1はMod Wheel、Pitch Bendは-1〜1、Channel Aftertouchは0〜1へ変換する
- 同一FrameのNote On / Note Offはゼロ長Noteとして両方を除外する
- Sustain Pedal、Polyphonic Aftertouch、CC1以外のController、Program Change等は無視し、Warningを返す
- 複数ChannelのNoteを一つのInstrumentへ統合した場合はWarningを返す
- Controlは、Active Noteへ適用されるInstrument Scope値が複数Channelから供給され、Channelごとの値と統合値が異なる場合にWarningを返す
- Note Eventを含まないMIDI FileはErrorとして拒否する
- Coreへ`midly`型は渡さない

```bash
sonalloy render midi \
  examples/instruments/basic-poly-synth.json \
  testdata/midi/basic-poly-synth-phrase.mid \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/basic-poly-synth.wav
```

| Option | 必須 | Default | 内容 |
|---|---:|---:|---|
| `--tail <seconds>` | No | `1.0` | 最後のNote Off後の追加Frame。有限かつ0以上 |
| `--sample-rate <Hz>` | No | `48000` | 正の整数。WAV HeaderとDSPへ同じ値を渡す |
| `--block-size <frames>` | No | `257` | Process最大Block Size |
| `--output <path>` | Yes | — | Stereo WAV出力先 |
| `--json` | No | Off | ResultまたはDiagnosticをJSONで出力 |

## `dev render-sine`

全処理経路（Process Contract、Native FFI、Stereo WAV出力）を通す動作確認用のSine Renderです。

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

## 診断Code

**Definition / Event**

- `SCHEMA_UNSUPPORTED`
- `JSON_INVALID`
- `REQUIRED_FIELD_MISSING`
- `ID_DUPLICATED`
- `VALUE_OUT_OF_RANGE`
- `LAYER_RANGE_INVALID`
- `PARAMETER_ID_INVALID`
- `PARAMETER_NOT_FOUND`
- `SOURCE_ID_INVALID`
- `SOURCE_ID_DUPLICATED`
- `SOURCE_NOT_FOUND`
- `SOURCE_VALUE_INVALID`
- `ROUTE_AMOUNT_INVALID`
- `ROUTE_TARGET_INVALID`
- `FILTER_CUTOFF_CLAMPED`
- `EVENT_ORDER_INVALID`
- `DSP_ERROR`

**Asset**

- `ASSET_NOT_FOUND`
- `ASSET_HASH_MISMATCH`
- `ASSET_DECODE_FAILED`
- `ASSET_RESAMPLED`
- `ASSET_DOWNMIXED`
- `ASSET_HASH_MISSING`
- `ASSET_ABSOLUTE_PATH`

AssetのMissingやDecode失敗はWarningとして表示され、ほかの有効LayerがあればRenderは継続します。

## Sampleを含むInstrumentの確認例

SampleとOscillatorを組み合わせたInstrumentは次のように確認できます。

```bash
sonalloy instrument inspect examples/instruments/metallic-hybrid.json --json
sonalloy render midi examples/instruments/metallic-hybrid.json \
  testdata/midi/metallic-hybrid-phrase.mid --sample-rate 48000 --block-size 257 \
  --tail 1.0 --output out/metallic-hybrid.wav
```
