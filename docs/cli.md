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
| Performance | Polyphony、Voice Stealing方式、Reported Latency（Frame） |
| Layer | Trigger Event（`note_on` / `note_off`）、Key / Velocity範囲、Generator、Gain、Pan、Tuning、Envelope |
| Oscillator | Waveform、Phase Reset、Phase、Backend、Output Mode、Effective Frequency上限、Pulse Width（Pulseのみ）、Hard Sync、Waveshaping、Phase Distortion、Wavefold、Oscillator Feedback、DC Blocker、Signal Order、Unison |
| Noise | Color、Seed、Stereo Correlation Parameter、Output Mode |
| Additive | Output Mode、Partial Count / 最大Partial Count、Phase Reset、Morph、Spectrum Tilt、Inharmonicity、各PartialのID / Ratio / Amplitude A / B / Phase / Envelope有無 |
| Formant | Output Mode、Partial Count / 最大Partial Count、Phase Reset、Profile Count、Vowel Position、Formant Shift、Throat、Spectral Tilt、各ProfileのIDと5本のFrequency / Bandwidth / Gain |
| Wavetable | Asset Path、SHA指定有無、Prepared状態、Source Channel / Frame Count、Frame Length / Count、Band Count / Max Harmonic、Position、Parameter ID、Phase、Unison、Output Mode、Effective Frequency上限 |
| Spectral | Output Mode、Asset A/B Path、SHA指定有無、Asset A/B Prepared状態、A/B各Source / Prepared Sample Rate、Channel / Frame Count、Spectral Frame Count、Prepared Bytes、FFT / Hop / Bin数、Root Note、Latency、Position / Freeze / Blur / Shift / Morph、各Parameter ID、Phase Reset |
| Granular | Asset Path、SHA指定有無、Prepared状態、Source Channel / Prepared Frame、Region、Root Note、Position、Grain Size、Density、Pitch、Randomness、Pan Spread、各Parameter ID、Seed、Grain Pool Limit、Output Mode |
| Wave Sequence | Output Mode、Step Count、Enabled Step Count、Direction、Loop、Crossfade、各StepのID / Asset / Region / Duration Type / Duration / Playback / Playback Direction / Gain / Pitch / Availability、Source Channel / Prepared Frame |
| Operator Modulation | Mode、Algorithm、Evaluation Order、Incoming Mask、Carrier Operator、4 OperatorのRatio / Detune / Level / Modulation Amount / Feedback / Envelope / Parameter ID、Phase Reset、Unison、Output Mode、Effective Frequency上限 |
| Sample | Zone Count、Enabled / Disabled Count、Prepared Asset共有数、Zone ID、Key / Velocity範囲、Root Note、Round Robin Group、Playback Region、Direction、Loop / Crossfade Frame、Time Mode、Duration Ratio / Source BPM、Asset Metadata、Output Mode |
| Parameter | Canonical ID、Owner、Unit、Range、Default、Scale、Smoothing |
| Modulation | Source ID、Source種類、Scope、Target、Amount、Curve |
| Processor | Layer、Voice、Globalの配置、Chain順、ID、Static Field、Dynamic Parameter |
| Warning | Compile時の警告一覧 |

```bash
sonalloy instrument inspect <definition>
sonalloy instrument inspect <definition> --json
```

`inspect`のParameter IDはCompiled Catalogから取得したCanonical IDです。RouteのSource ID、Target ID、設定値もCompiled Instrumentの内容をそのまま表示します。Sample Zoneは`direction`、Region Frame、Loop Frame、`crossfade_frames`、Time Mode、Duration Ratio / Source BPM、Source Channel数、Prepared Frame数を表示します。Wave SequenceはMissing Stepを含む全StepをDefinition順で表示し、`enabled`、Region Frame、Duration、Step Playback、Asset Playback Direction、Gain、Pitch、Source Metadataを確認できます。Instrument全体の`reported_latency_frames`も表示します。Layer Triggerは`event`を表示し、`note_off` LayerがRelease Triggerとして構成されていることを確認できます。
ProcessorはChainごとに`placement`、`chain_index`、`id`、`kind`、Static Field、Parameter Descriptorを表示します。FilterはParameterのDefaultとSample Rateに応じたDSP適用上限をStatic Fieldへ表示します。DelayのTimeとReverbのPre-delayはStatic Fieldです。

Operator ModulationのJSON Inspectでは、Operator番号を1始まりで表示し、固定Topologyを`evaluation_order`、`incoming_masks`、`carrier_operators`として表示します。`level`、`modulation_amount`、`feedback`はTopologyとModeで使用されないOperatorでは`null`になります。OperatorのParameter IDは`layer.<layer_id>.generator.operator.<1-4>.<parameter>`形式です。

Complex OscillatorのJSON Inspectでは、`backend`が`phase_domain`になる条件、`phase_distortion_parameter`、`wavefold_parameter`、`oscillator_feedback_parameter`、`dc_blocker`、`signal_order`、`combination_constraints`を表示します。Wavefoldだけを指定した場合は既存Oscillator Backendを維持します。

AdditiveのJSON Inspectでは、固定Partial数と最大値、初期Morph / Spectrum Tilt / Inharmonicity、Definition順のPartial ID、Ratio、Amplitude A / B、Initial Phase、Optional Envelopeの有無を表示します。Dynamic ParameterのCanonical IDは、`layer.<layer_id>.generator.additive_morph`、`layer.<layer_id>.generator.additive_spectrum_tilt`、`layer.<layer_id>.generator.additive_inharmonicity`です。

FormantのJSON Inspectでは、固定Partial数と最大値、Phase Reset、Profile数、初期Vowel Position / Formant Shift / Throat / Spectral Tilt、Profile順のID、5本のFormant Bandを表示します。Dynamic ParameterのCanonical IDは、`layer.<layer_id>.generator.formant_vowel_position`、`layer.<layer_id>.generator.formant_shift`、`layer.<layer_id>.generator.formant_throat`、`layer.<layer_id>.generator.formant_spectral_tilt`です。

SpectralのJSON Inspectでは、`asset_a`と指定された`asset_b`の準備状態、各AssetのSource / Prepared Sample Rate、Source Metadata、Prepared Spectral Frame数、Prepared Bytes、FFT / Hop / Bin数、Reported Latency、Position / Freeze / Blur / Shift / MorphのParameter値とCanonical IDを表示します。Aまたは指定Bが準備できない場合も、ほかの有効Layerを含むCompile結果を確認できます。

Harmonic / Formant HybridのJSON Inspectでは、各LayerのGeneratorとProcessor、Voice / Global Processor Chain、Modulation Source / Routeを同じReportで表示します。[`harmonic-formant-hybrid-reference.json`](../examples/instruments/harmonic-formant-hybrid-reference.json)を使うと、Formant、Additive、Sample、Noise、Filter、Drive、Delay、Reverb、MIDI制御Targetを一つの構造として確認できます。

Spectralの単体とHybridは[`spectral-generator-reference.json`](../examples/instruments/spectral-generator-reference.json)と[`spectral-hybrid-reference.json`](../examples/instruments/spectral-hybrid-reference.json)で確認できます。単体例ではStereo A/B Prepared状態、FFT 2048、Hop 512、Bin数、Reported Latency、5つのSpectral Parameterを確認し、Hybrid例ではSpectral、Additive、Sample、Noise、Layer / Voice / Global Processor、Modulation Routeを同じReportで確認します。

## `render` Command

### `render note`

Coreへ1つのNote On / Note Offを渡してRenderします。単音の確認用です。

```bash
sonalloy render note examples/instruments/basic-poly-synth.json \
  --note 60 --velocity 100 --gate 0.5 --tail 0.5 \
  --tempo 120 \
  --sample-rate 48000 --block-size 257 --output out/note.wav
```

| Option | 必須 | Default | 内容 |
|---|---:|---:|---|
| `--note <0-127>` | No | `60` | MIDI Note番号 |
| `--velocity <1-127>` | No | `100` | MIDI Velocity |
| `--gate <seconds>` | No | `0.5` | Note OnからNote Offまでの時間。有限かつ0以上 |
| `--tail <seconds>` | No | `0.5` | Note Off後の追加Frame。有限かつ0以上 |
| `--tempo <bpm>` | No | `120` | Process Tempo。有限かつ0より大きい値。Tempo Sync SampleのDuration比に使う |
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
| `--tempo <bpm>` | No | `120` | Process Tempo。Tempo Sync SampleのDuration比に使う |
| `--sample-rate <Hz>` | No | `48000` | 正の整数 |
| `--block-size <frames>` | No | `257` | Process最大Block Size |
| `--output <path>` | Yes | — | Stereo WAV出力先 |
| `--json` | No | Off | ResultまたはDiagnosticをJSONで出力 |

### `render midi`

Standard MIDI FileをAbsolute Frameの`ScheduledEvent`とTempo Mapへ変換してRenderします。

- Tick、Tempo、Channel、NoteはCLI側でAbsolute Frameへ変換する
- MIDI Tempo EventはTempo Mapとして保持し、Tempo変更FrameでCoreのProcess Blockを分割する
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

出力は32-bit float、2 Channel、指定Sample RateのWAVです。Time Stretchを含む場合、CLIはReported Latency分を内部Renderへ追加し、前置き分を除去してMusical TimelineのFrame 0からWAVを生成します。成功JSONには`reported_latency_frames`を含みます。親Directoryは事前に作成してください。

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

**Wavetable**

- `WAVETABLE_LAYOUT_INVALID`
- `WAVETABLE_PREPARATION_FAILED`
- `WAVETABLE_SILENT_FRAME`
- `WAVETABLE_DC_OFFSET`
- `GENERATOR_RESOURCE_LIMIT_EXCEEDED`

**Spectral**

- `SPECTRAL_PREPARATION_FAILED`
- `GENERATOR_RESOURCE_LIMIT_EXCEEDED`

**Wave Sequence**

- `INVALID_SEQUENCE`
- `INVALID_STEP_DURATION`

**Operator Modulation**

- `VALUE_OUT_OF_RANGE`（Operator数、Ratio、Detune、Level、Amount、Phase、Feedback、Unison範囲）
- `DEFINITION_ERROR`（Carrier Level、非Carrier Level、未接続Amount、AM / Ring Feedback）
- `GENERATOR_RESOURCE_LIMIT_EXCEEDED`（Unison Voice数）

AssetのMissingやDecode失敗はWarningとして表示され、ほかの有効LayerがあればRenderは継続します。

## Spectral Resynthesisの確認例

```bash
sonalloy instrument validate examples/instruments/spectral-generator-reference.json --json
sonalloy instrument inspect examples/instruments/spectral-generator-reference.json --json
sonalloy render midi examples/instruments/spectral-hybrid-reference.json \
  testdata/midi/basic-poly-synth-phrase.mid --sample-rate 48000 --block-size 257 \
  --tail 0.2 --output out/spectral-hybrid-reference/midi.wav --json
```

Review用の全条件（Parameter Change、Block Size、Sample Rate、Fresh Runtime、16 Voice、Voice Stealing、既存Generator回帰）は`python3 scripts/review/generate_spectral_resynthesis_package.py`で`review-output/spectral-resynthesis/`へ生成します。Performance測定の音声は保存せず、`metrics.json`へ測定値だけを記録します。

## Sampleを含むInstrumentの確認例

SampleとOscillatorを組み合わせたInstrumentは次のように確認できます。

```bash
sonalloy instrument inspect examples/instruments/metallic-hybrid.json --json
sonalloy render midi examples/instruments/metallic-hybrid.json \
  testdata/midi/metallic-hybrid-phrase.mid --sample-rate 48000 --block-size 257 \
  --tail 1.0 --output out/metallic-hybrid.wav
```
