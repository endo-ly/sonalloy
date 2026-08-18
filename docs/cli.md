# CLI

Sonalloy CLI（バイナリ名`sonalloy`）は、音源定義（JSON）を読み込み、検証・コンパイルし、リアルタイム演奏またはWAVレンダリングを行います。

この文書では、各コマンドを「**音源を作る → 検証する → 音を鳴らす**」の順に説明します。実行時の挙動（Voice、ADSR、Sample再生など）は`docs/runtime-processing.md`、音源定義のJSON形式は`docs/instrument-definition.md`を参照してください。

## コマンドの全体像

| コマンド | 役割 |
|---|---|
| `instrument init` | 音源定義のひな形を生成する |
| `instrument validate` | 音源定義を検証する |
| `instrument inspect` | コンパイル後の実行値を表示する |
| `render note` | 1音をレンダリングする |
| `render events` | Event Sequenceをレンダリングする |
| `render midi` | MIDI Fileをレンダリングする |
| `device list` | Audio OutputとMIDI Inputを列挙する |
| `play` | MIDI InputからAudio Outputへリアルタイム演奏する |
| `dev render-sine` | 動作確認用のSineをレンダリングする |

## 音源定義を作る

### `instrument init` — ひな形の生成

最小のOscillator音源（Saw波形、Polyphony 16）を生成します。ここから編集を始めるための土台です。

```bash
sonalloy instrument init <path>
```

## 音源定義を検証する

### `instrument validate` — 検証

音源定義が正しくコンパイルできるかを確認します。JSONの構文、Fieldの制約、Assetの準備まで検証します（WAVは生成しません）。成功すると`valid <path>`と表示されます。失敗時は、どのFieldに問題があるか（例：`layers[0].envelope.attack_seconds`）を示すので、その場所を直します。

```bash
sonalloy instrument validate <definition>
sonalloy instrument validate <definition> --json   # 結果を機械可読で出力
```

### `instrument inspect` — コンパイル結果の確認

音源定義をコンパイルした結果（最終的なGain、Pan、ADSR、Parameter、Modulation、Processorなど）を表示します。書いた定義が意図どおりに解釈されたかを確認するために使います。

```bash
sonalloy instrument inspect <definition>
sonalloy instrument inspect <definition> --json
```

主な確認項目：

| 項目 | 内容 |
|---|---|
| Performance | 同時発音数、Voice Stealingの方式、報告Latency |
| Layer | 発音条件、Generator、Gain、Pan、Tuning、ADSR |
| Generator | 各Generatorの構成値（波形、Asset、Parameter、Algorithmなど）。Physical StringはExciterとLoop Parameter、ModalはExciter・Mode Count・共鳴Parameter・実効周波数上限を表示 |
| Parameter | Parameter ID、Owner、Native Unit、Native範囲、Default、Scale、Smoothing、Modulation Unit、最大Depth |
| Modulation | Sourceの範囲・Polarity、RouteのDepth、Curve、Static Effect、DefaultからのReachable Range、Clamp可能性 |
| Processor | Layer / Voice / Globalの各Processor Chain |
| Warning | コンパイル時の警告（Asset欠落など） |

`--json`は、Generatorごとの構造をFieldとして返します。Parameter IDは`layer.<layer_id>.generator.<name>`形式（Operator Modulationだけ`operator.<1-4>.<parameter>`）。各GeneratorがどのFieldを返すかは、実際に`--json`を実行して確認してください。`parameters[].modulation`はTargetに許可されたUnitと最大絶対Depthを、`routes[].effect`はSource Endpointが作るAdditive DeltaまたはLog2 Factorを返します。

例（抜粋）：

```json
{
  "id": "layer.body.tuning",
  "unit": "cents",
  "min": -1200.0,
  "max": 1200.0,
  "default": 0.0,
  "scale": "linear",
  "modulation": { "unit": "cents", "max_abs_depth": 2400.0 },
  "modulated_range_from_default": {
    "unclamped_min": -20.0,
    "unclamped_max": 20.0,
    "effective_min": -20.0,
    "effective_max": 20.0,
    "may_clamp": false
  }
}
```

`modulated_range_from_default`は、各Sourceが宣言されたEndpointへ独立に到達できると仮定した決定的なBoundです。特定の演奏で実際に通る値の予測ではありません。

## リアルタイム演奏

### `device list` — Deviceの列挙

Audio OutputとMIDI Inputを列挙します。Audio Inputは対象外です。IDは表示順やIndexではなく、CPAL / Midirが返すOpaque Stringをそのまま指定します。

```bash
sonalloy device list
sonalloy device list --json
```

JSONでは次のFieldを返します。`SupportedBufferSize::Unknown`のときは`buffer_size: null`です。

| Field | 内容 |
|---|---|
| `audio_outputs[].id` | CPALのAudio Device ID |
| `audio_outputs[].default` | OS Default Outputかどうか |
| `audio_outputs[].default_config` | Device DefaultのSample Rate、Channel、Sample Format、Buffer範囲 |
| `midi_inputs[].id` | MidirのMIDI Port ID |

### `play` — MIDIからAudioへ演奏

```bash
sonalloy play <definition>
sonalloy play <definition> --midi-device <id>
sonalloy play <definition> --audio-device <id> --sample-rate 48000 --buffer-size 256 --tempo 120
```

| Option | Default | 内容 |
|---|---:|---|
| `--audio-device <id>` | OS Default | CPAL Stable Device ID。指定IDが存在しない場合はError |
| `--midi-device <id>` | 条件付き自動選択 | Midir Stable Port ID。0件はError、1件は自動選択、2件以上はID必須 |
| `--sample-rate <hz>` | Device Default | Deviceが対応するRateだけを選択し、そのRateでCompile |
| `--buffer-size <frames>` | 256 | CPALへ要求するFrame数。0やDeviceの対応範囲外はError |
| `--tempo <bpm>` | 120 | `ProcessContext.tempo_bpm`へ渡す一定Tempo |

起動時にDefinition名、Audio / MIDI Device名とOpaque ID、Sample Rate、Device Channel、Sample Format、要求Buffer、Engine Latency、Tempoを表示します。Backendから実際のCallback Frame数を取得できない場合があるため、Host Callbackは要求値と異なることがあります。`play`は長時間実行Commandで、標準入力のEnterで停止します。

AudioはCoreのPlanar `f32` StereoをDevice Sample Formatへ変換します。2chより多いDeviceではch 0 / 1へLeft / Rightを出力し、残りを無音にします。Mono Device、PCM以外のFormat、Unsupported Bufferは起動Errorです。Audio CallbackではHeap Allocation、Log、Blocking Lockを行いません。

DeviceがRealtime Schedulingを拒否した場合はWarningを表示してSessionを継続します。XrunはCounterとして終了時に表示し、Device Error、Process Error、Queue Overflowは無音化してSessionを終了します。

MIDIのNote、Pitch Bend、CC1、Channel Aftertouch、CC64は、Offline経路と同じCore Eventへ変換されます。CC64はDown中にNote OffのReleaseを保留し、UpでReleaseを開始します。Realtime DeniedはWarning、XrunはCounter、Device / Process / Queue Faultは無音化してSessionを終了します。

`play`にはStreaming JSON Protocolを設けません。Deviceの機械可読な確認には`device list --json`を使用します。

## 音を鳴らす

3つの`render`コマンドは、いずれもWAVを生成します。確認したい内容に合わせて使い分けます。

| コマンド | 向いている用途 |
|---|---|
| `render note` | 1音の鳴り方（Attack、Sustain、Release）を手軽に確かめる |
| `render events` | 演奏中のParameter変化（Filter Cutoff、Pitch Bendなど）を正確な位置で再現する |
| `render midi` | MIDI Fileのフレーズを鳴らす |

### `render note` — 1音のレンダリング

1つのNote OnとNote Offをレンダリングします。音色の素性を手軽に確かめるのに使います。

```bash
sonalloy render note <definition> \
  --note 60 --velocity 100 --gate 0.5 --tail 0.5 \
  --tempo 120 \
  --sample-rate 48000 --block-size 257 --output out/note.wav
```

| Option | Default | 内容 |
|---|---|---|
| `--note` | 60 | MIDI Note番号（0〜127） |
| `--velocity` | 100 | 強さ（1〜127） |
| `--gate` | 0.5 | Note OnからNote Offまでの秒数 |
| `--tail` | 0.5 | Note Off後の余韻の秒数 |
| `--tempo` | 120 | Tempo Sync Sampleの基準BPM |
| `--sample-rate` | 48000 | 出力Sample Rate |
| `--block-size` | 257 | 処理の最大Block Size |
| `--output` | — | 出力先（必須） |
| `--analyze` | Off | 補正後WAVのLevel、DC、Activity、Continuity、Stereo、Spectrumを計算 |
| `--trace <id>` | なし | 選択したDynamic ParameterをTrace（複数指定可） |
| `--trace-every-frames <N>` | 480（Trace指定時） | 定期Trace間隔。1以上。Traceなしでは指定不可 |
| `--json` | Off | 結果を機械可読で出力 |

### `render events` — Event Sequenceのレンダリング

MIDI Fileを使わずに、Event（Note、Parameter Change、Pitch Bendなど）を**正確なFrame位置で制御**しながらレンダリングします。パラメータ変化の滑らかさや、特定のタイミングでの変化を検証する時に使います。

```bash
sonalloy render events <definition> <events.json> \
  --duration-frames 192000 --tail 1.0 \
  --sample-rate 48000 --block-size 257 --output out/events.wav
```

**Event Fileの書き方**

Event Fileは、Eventの並びをJSONで書いたものです。各Eventは、**再生開始位置からの経過Frame数**（`absolute_frame`）と、Eventの種類（`type`）を持ちます。

```json
{
  "events": [
    { "absolute_frame": 0,     "type": "parameter_change", "parameter": "voice.processor.tone.cutoff", "native_value": 3500.0 },
    { "absolute_frame": 0,     "type": "note_on",          "note_id": 1, "note": 60, "velocity": 100 },
    { "absolute_frame": 24000, "type": "mod_wheel",        "value": 1.0 },
    { "absolute_frame": 48000, "type": "note_off",         "note_id": 1 }
  ]
}
```

書けるEventの種類：

| `type` | 渡す値 | 働き |
|---|---|---|
| `note_on` / `note_off` | `note_id`、`note`、`velocity` | 音を鳴らす / 止める。`note_id`でOnとOffを対応付ける |
| `sustain_pedal` | `down`（bool） | Pedal Down中はNote Off後のReleaseを保留し、Pedal UpでReleaseを開始する |
| `parameter_change` | `parameter`、`native_value` | Parameter CatalogのNative Unit値を送る（CutoffはHz、TuningはCents、GainはdB） |
| `pitch_bend` | `value` | -1〜1 |
| `mod_wheel` | `value` | 0〜1 |
| `aftertouch` | `value` | 0〜1 |

読み込み時の処理：

- Eventを**時系列へ正しく処理するため**、`absolute_frame`の昇順へ整列します。同じFrameでは、決まった優先順位（Sustain Pedal → Note Off → Parameter Change → Pitch Bend → Mod Wheel → Aftertouch → Note On）で処理します
- 次のいずれかがあると、安全のためWAVを生成しません：`--duration-frames`を超えるFrameのEvent、音源定義に存在しないParameter ID、Native範囲外の値。旧`normalized` Fieldは受け付けません

| Option | Default | 内容 |
|---|---|---|
| `--duration-frames` | — | レンダリング長（Frame、必須）。Eventの`absolute_frame`はこの値未満にします |
| `--tail` | 1.0 | レンダリング後の余韻の秒数 |
| `--tempo` | 120 | Tempo Sync Sampleの基準BPM |
| `--sample-rate` | 48000 | 出力Sample Rate |
| `--block-size` | 257 | 処理の最大Block Size |
| `--output` | — | 出力先（必須） |
| `--analyze` | Off | 補正後WAVの解析を追加 |
| `--trace <id>` | なし | 選択したDynamic ParameterをTrace（複数指定可） |
| `--trace-every-frames <N>` | 480（Trace指定時） | 定期Trace間隔 |
| `--reset-check` | Off | 同じPrepared RuntimeをResetして同じEvent列を再実行し、差分をReportへ追加 |
| `--json` | Off | 結果を機械可読で出力 |

### Render diagnostics — AnalysisとTrace

`render note`、`render events`、`render midi`は、`--analyze`で補正後の出力WAVを決定的に解析し、`--trace`で選択したParameterの実行中の値をJSON成功Reportへ追加します。`--json`を付けない場合も、短いSummaryを標準出力へ表示します。

Analysisの主なFieldは次のとおりです。

| Field | 意味 |
|---|---|
| `level` | 全ChannelのPeak、RMS、dBFS、Crest Factor、`over_full_scale`（Peak > 1） |
| `dc` | Channelごとの算術平均 |
| `activity` | -80 dBFSを超えた最初・Peak・最後のFrame。無音なら`null` |
| `continuity` | 隣接Frame最大差分、差分が0.25を超えた件数、先頭最大16箇所 |
| `stereo.correlation` | Zero-mean Pearson相関。分母が0なら`null` |
| `spectrum` | Hann窓STFTのCentroid、最大8局所Peak、指定NoteのReference周波数とHarmonic比 |

ZeroのdBFS、無音のActivity、短すぎる音声のSpectrum指標、一定信号のStereo相関は`null`で表し、NaNやInfinityはJSONへ出しません。`render note`だけは指定MIDI Noteから`440 × 2^((note - 69) / 12)`をReference周波数として使い、`events`と`midi`はFundamentalを推測しません。

Trace対象は既存CatalogのDynamic Parameter IDだけです。Traceは次を含みます。

- frame 0のBaseline、`N` FrameごとのPeriodic Point、Event処理後のPoint、最終Frame。重複Frameは1点にまとめます
- `base`、Routeごとの`raw` / `shaped` Source、Definitionの`depth`、Domain Contribution、Clamp前の`before_clamp`、Clamp後の`final`、`clamped`
- Voice所属TargetではVoice Index、Note ID / Number、Velocity、State。Global Targetでは`voice: null`
- Layer TargetはそのLayerがActiveなVoiceだけを報告し、Inactive Layerの架空値は出しません

Trace FrameはLatency補正後のWAVと同じPublic Timelineです。`--trace`は繰り返し指定でき、重複IDは最初の指定順を保ってDeduplicateされます。未知のID、0以下の間隔、Traceなしの`--trace-every-frames`は入力Errorです。観測数には100,000件の上限があります。

例：

```bash
sonalloy render note presets/basic.json --analyze \
  --trace layer.body.tuning --trace voice.processor.tone.cutoff \
  --trace-every-frames 480 --json --output out/note.wav
```

`trace.parameters[].observations[]`の`final`が、全Route加算とTarget Clamp後の実効Native値です。Traceを有効にしても、通常Renderと同じRuntimeを使い、出力Audioは既存のBlock分割許容範囲内で一致します。

`render events --reset-check`は`trace`と併用できません。成功Reportの`reset_comparison`には、同じPrepared Runtimeを`reset`した前後のStereo Audioについて、`max_abs_difference`、`rms_difference`、差分Sample数を記録します。全Stateが初期化されていれば、これらは0になります。

### `render midi` — MIDI Fileのレンダリング

Standard MIDI Fileを読み込んでレンダリングします。演奏フレーズを鳴らす時に使います。

```bash
sonalloy render midi <definition> <midi-file> \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/phrase.wav
```

MIDIを読み込むと、CLIは次の変換を行います：

- MIDIのTick・Tempo・Channel・Noteを、**再生開始位置からのFrame数**へ変換します。Tempo変更があると、その位置で処理Blockを分けて、切り替わりの前後で正確な長さを保ちます
- Note OnのVelocity 0はNote Offとして扱います
- CC64はSustain Pedalへ変換します。64以上をDown、63以下をUpとします
- CC1はMod Wheel、Pitch Bendは-1〜1、Channel Aftertouchは0〜1へ変換します
- 同じ時刻でNote OnとNote Offが重なると、長さ0のNoteとして両方を無視します

対応しないMIDI機能（Polyphonic Aftertouch、CC1以外のController、Program Change）は無視してWarningを出します。複数ChannelのNoteを1つの音源へ当てた場合や、ChannelごとにControl値が違う場合もWarningを出します。Note Eventを1つも含まないMIDI Fileは、Errorとして受け付けません。

| Option | Default | 内容 |
|---|---|---|
| `--tail` | 1.0 | 最後のNote Off後の余韻の秒数 |
| `--sample-rate` | 48000 | 出力Sample Rate |
| `--block-size` | 257 | 処理の最大Block Size |
| `--output` | — | 出力先（必須） |
| `--analyze` | Off | 補正後WAVの解析を追加 |
| `--trace <id>` | なし | 選択したDynamic ParameterをTrace（複数指定可） |
| `--trace-every-frames <N>` | 480（Trace指定時） | 定期Trace間隔 |
| `--json` | Off | 結果を機械可読で出力 |

## 動作確認

### `dev render-sine` — 処理経路の確認

処理契約、Native FFI、WAV出力までの全経路を通す、単音のSineレンダリングです。ビルド後の動作確認に使います。

```bash
sonalloy dev render-sine \
  --frequency 440 --duration 1.0 \
  --sample-rate 48000 --block-size 257 --output out/sine.wav
```

| Option | Default | 内容 |
|---|---|---|
| `--frequency` | 440 | 周波数（Hz） |
| `--duration` | — | レンダリング長（秒、必須） |
| `--sample-rate` | 48000 | 出力Sample Rate |
| `--block-size` | 257 | 処理の最大Block Size |
| `--tail` | 0 | レンダリング後の余韻の秒数 |
| `--output` | — | 出力先（必須） |
| `--json` | Off | 結果を機械可読で出力 |

## 出力とエラー

### 出力WAV

すべての`render`コマンドは、32-bit float・2 Channel・指定Sample RateのStereo WAVを出力します。出力先の親Directoryは事前に作成してください。

Time Stretchを含む音源では、CLIが内部で報告Latency分を追加レンダリングし、先頭の無音部分を除去して、**演奏タイムラインのFrame 0**からWAVを始めます。成功時のJSONには`reported_latency_frames`が含まれます。

### Exit Code

| Code | 意味 |
|---:|---|
| `0` | 成功 |
| `1` | 音源定義 / コンパイルエラー |
| `2` | CLI入力 / レンダリングリクエストエラー |
| `3` | Core処理 / レンダリングエラー |
| `4` | WAV出力エラー |

`--json`を付けると、入力エラーを次の形で返します：

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

### 診断Code

検証・コンパイル・レンダリングで発生する主な診断Codeです。

**音源定義・Event**

`SCHEMA_UNSUPPORTED`、`JSON_INVALID`、`REQUIRED_FIELD_MISSING`、`ID_DUPLICATED`、`VALUE_OUT_OF_RANGE`、`LAYER_RANGE_INVALID`、`PARAMETER_ID_INVALID`、`PARAMETER_NOT_FOUND`、`SOURCE_ID_INVALID`、`SOURCE_ID_DUPLICATED`、`SOURCE_NOT_FOUND`、`SOURCE_VALUE_INVALID`、`ROUTE_DEPTH_INVALID`、`ROUTE_DEPTH_UNIT_INVALID`、`ROUTE_TARGET_INVALID`、`FILTER_CUTOFF_CLAMPED`、`TRACE_LIMIT_EXCEEDED`、`EVENT_ORDER_INVALID`、`DSP_ERROR`、`MIDI_ERROR`、`AUDIO_DEVICE_ERROR`

**Asset**

`ASSET_NOT_FOUND`、`ASSET_HASH_MISMATCH`、`ASSET_DECODE_FAILED`、`ASSET_RESAMPLED`、`ASSET_DOWNMIXED`、`ASSET_HASH_MISSING`、`ASSET_ABSOLUTE_PATH`

**Generator別**

| Generator | Code |
|---|---|
| Wavetable | `WAVETABLE_LAYOUT_INVALID`、`WAVETABLE_PREPARATION_FAILED`、`WAVETABLE_SILENT_FRAME`、`WAVETABLE_DC_OFFSET`、`GENERATOR_RESOURCE_LIMIT_EXCEEDED` |
| Spectral | `SPECTRAL_PREPARATION_FAILED`、`GENERATOR_RESOURCE_LIMIT_EXCEEDED` |
| Wave Sequence | `INVALID_SEQUENCE`、`INVALID_STEP_DURATION` |
| Operator Modulation | `VALUE_OUT_OF_RANGE`、`DEFINITION_ERROR`（Carrier Level / 非Carrier Level / 未接続Amount / AM・Ring Feedback）、`GENERATOR_RESOURCE_LIMIT_EXCEEDED` |

Assetの欠落・Decode失敗はWarningとして扱われ、ほかの有効なLayerがあればレンダリングを続けます。
