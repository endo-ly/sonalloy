# CLI

Sonalloy CLI（バイナリ名`sonalloy`）は、音源定義（JSON）を読み込み、検証・コンパイルし、リアルタイム演奏またはWAVレンダリングを行います。

この文書では、各コマンドを「**音源定義を作る → 検証する → Patternを用意する → 鳴らす（リアルタイム / オフライン）**」の順に説明します。実行時の挙動（Voice、ADSR、Sample再生など）は`docs/runtime-processing.md`、音源定義のJSON形式は`docs/instrument-definition.md`を参照してください。

## コマンドの全体像

| コマンド | 役割 |
|---|---|
| `instrument init` | 音源定義のひな形を生成する |
| `instrument validate` | 音源定義を検証する |
| `instrument inspect` | コンパイル後の実行値を表示する |
| `pattern init` | 1つのInstrumentを試奏するPatternを生成する |
| `pattern validate` | Patternの構造を検証する |
| `pattern inspect` | Patternの音楽的な長さとEvent概要を表示する |
| `pattern import-midi` | MIDIの1 ChannelをPatternへ変換する |
| `pattern export-midi` | PatternをSingle Track MIDIへ変換する |
| `render note` | 1音をレンダリングする |
| `render events` | Event Sequenceをレンダリングする |
| `render midi` | MIDI Fileをレンダリングする |
| `render pattern` | 演奏パターン（Pattern）をレンダリングする |
| `audition pattern` | PatternをAudio Deviceで試聴する |
| `audition midi` | MIDI Fileを1 Channel選択して試聴する |
| `device list` | Audio OutputとMIDI Inputを列挙する |
| `play` | MIDI InputからAudio Outputへリアルタイム演奏する |
| `dev render-sine` | 動作確認用のSineをレンダリングする |

## 音源定義を作る

### `instrument init` — ひな形の生成

最小のOscillator音源（Saw波形、同時発音数16）を生成します。ここから編集を始めるための土台です。

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
| Performance | `mode`、Voice Count、PolyphonicのVoice Stealing、MonophonicのLegato / Portamento、報告Latency |
| Layer | 発音条件、Generator、Gain、Pan、Tuning、ADSR |
| Generator | 各Generatorの構成値（波形、Asset、Parameter、Algorithmなど）。Physical StringはExciterとLoop Parameter、ModalはExciter・Mode Count・共鳴Parameter・実効周波数上限を表示 |
| Parameter | Parameter ID、Owner、Native Unit、Native範囲、Default、Scale、Smoothing、Modulation Unit、最大Depth |
| Modulation | Sourceの範囲とPolarity、RouteごとのDepthとCurve、Static Effect、Default値からModulationで到達しうる範囲とClampの有無 |
| Macro / Vector | MacroのParameter ID・Default・Route、VectorのAxis ID・所属Layer・初期値 |
| Processor | Layer / Voice / Globalの各Processor Chain |
| Warning | コンパイル時の警告（Asset欠落など） |

`--json`は、Generatorごとの構造をFieldとして返します。返るFieldはGeneratorの種類ごとに異なり、Parameter IDの形式は`docs/instrument-definition.md`を参照してください。`parameters[].modulation`はTargetに許可されたUnitと最大絶対Depthを、`routes[].effect`はSource Endpointが作るAdditive DeltaまたはLog2 Factorを返します。`sources[]`にはScope、RateとRate Unit、MSEG / Step / Randomの構造が含まれ、`macros[]`と`vectors[]`には外部から操作するIDを含めます。

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

`modulated_range_from_default`は、各Sourceが単独で最大Depthまで届いたと仮定して計算した理論上の範囲です。実際の演奏で通る値の予測ではありません。

## Audition Pattern

Audition Patternは、1つのInstrumentを試奏するための演奏パターン（JSON）です。NoteやChord、フレーズ、ドラム、演奏操作、Parameter ChangeをTickベースの時間軸で書けます。Schema、Validation、Loop、MIDI Interchangeの正本は[`docs/pattern.md`](pattern.md)です。

### `pattern init` — 試奏Patternの生成

```bash
sonalloy pattern init phrase.json
```

検証を通る1小節のPatternを生成します（120 BPM、4/4拍子、C4の四分音符1つ）。既存のPathは上書きしません。

### `pattern validate` — 構造検証

```bash
sonalloy pattern validate phrase.json
sonalloy pattern validate phrase.json --json
```

次を検証します：

- Schema Versionと未知のField
- Tick、Tempo、Time Signatureの整合
- NoteとControl値の範囲と有限数
- Noteが1つ以上あること

Instrument固有Parameterの存在と範囲は、Instrumentを指定する`render pattern`または`audition pattern`で解決します。

### `pattern inspect` — Pattern概要

```bash
sonalloy pattern inspect phrase.json
sonalloy pattern inspect phrase.json --json
```

表示項目：

- Name、Schema、Tick Resolution
- Length（Tempo Timelineから計算した、Sample Rateに依存しない音楽的な長さ）
- Tempo Change / Time Signature Changeの件数
- Note数とVelocity範囲
- Control数とParameter ID数

### `pattern import-midi` — MIDIからPatternへ変換

```bash
sonalloy pattern import-midi phrase.mid --output phrase.json
sonalloy pattern import-midi song.mid --channel 10 --output drums.json
```

Patternは1 Instrument用なので、Note Channelが複数あるMIDIは`--channel 1..16`で1つを選びます。Channelが1つだけの場合は自動選択します。Output Pathが存在する場合は失敗します。Tick対応付けやTempo・拍子の扱いなど変換の規則は[`docs/pattern.md`](pattern.md)を参照してください。

### `pattern export-midi` — PatternからMIDIへ変換

```bash
sonalloy pattern export-midi phrase.json --output phrase.mid
sonalloy pattern export-midi drums.json --channel 10 --output drums.mid
```

出力されるMIDIの内容と往復変換で保たれる情報は[`docs/pattern.md`](pattern.md)を参照してください。Sonalloy固有のParameter Changeを含むPatternは`MIDI_ERROR`で失敗し、Output Pathが存在する場合も上書きしません。

## リアルタイム演奏

### `device list` — Deviceの列挙

Audio OutputとMIDI Inputを列挙します。Audio Inputは対象外です。IDには、表示順やIndexではなくCPAL / Midirが返す文字列をそのまま指定します。

```bash
sonalloy device list
sonalloy device list --json
```

JSONでは次のFieldを返します。対応Buffer範囲が不明なDeviceでは`buffer_size`が`null`になります。

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
sonalloy play <definition> --audio-device <id> --sample-rate 48000 --buffer-size 256 \
  --tempo 120 --time-signature 4/4 --macro-cc motion=74
```

| Option | Default | 内容 |
|---|---:|---|
| `--audio-device <id>` | OS Default | CPAL Stable Device ID。指定IDが存在しない場合はError |
| `--midi-device <id>` | 条件付き自動選択 | Midir Stable Port ID。0件はError、1件は自動選択、2件以上はID必須 |
| `--sample-rate <hz>` | Device Default | Deviceが対応するRateだけを選択し、そのRateでCompile |
| `--buffer-size <frames>` | 256 | CPALへ要求するFrame数。0やDeviceの対応範囲外はError |
| `--tempo <bpm>` | 120 | `ProcessContext.tempo_bpm`へ渡す一定Tempo |
| `--time-signature <n/d>` | 4/4 | `ProcessContext.time_signature`へ渡す一定拍子。分母は1〜128の2の冪 |
| `--macro-cc <id=cc>` | なし | Macro ParameterをMIDI CCへ割り当てる。複数指定可。CC1 / CC64は標準Controlとして予約 |

`play`は標準入力のEnterで停止する長時間実行コマンドで、次の情報を表示します。

- 起動時: Definition名、Audio / MIDI Device名とID、Sample Rate、Channel数、Sample Format、要求Buffer Size、Engine Latency、Tempo、Time Signature、Macro CC Mapping
- 終了時: 観測した最小 / 最大Frame数とCallback回数（Host Callbackの実Frame数は要求値と異なることがあるため）

Deviceを機械可読で確認するときは`device list --json`を使います。

音声とエラーの扱いは次のとおりです。

- CoreのPlanar `f32` Stereo出力をDeviceのSample Formatへ変換します。2chより多いDeviceではch 0 / 1へLeft / Rightを出力し、残りを無音にします
- Mono Device、PCM以外のFormat、対応範囲外のBuffer Sizeは起動Errorになります
- Realtime Schedulingの拒否はWarningを表示して継続し、Xrunは回数を終了時に表示します
- Audio Device Error / MIDI Error / Process Error・Queue Overflowは、出力を無音化してから`AUDIO_DEVICE_ERROR` / `MIDI_ERROR` / `PROCESS_ERROR`としてSessionを終了します

MIDIのNote、Pitch Bend、CC1、Channel Aftertouch、CC64（Sustain Pedal）は、Offline経路と同じCore Eventへ変換されます。`--macro-cc`で割り当てたCCはMacroの`parameter_change`へ変換されます。1つのCCを複数Macroへ割り当てたり、予約済みCCを割り当てたりする指定は起動時に拒否します。

### `audition pattern` — PatternのRealtime試聴

```bash
sonalloy audition pattern <definition> <pattern>
sonalloy audition pattern <definition> <pattern> --loop
```

Audio Outputだけを使うため、MIDI Input DeviceやMIDI Keyboardは不要です。One-shotはPatternの終端、Tail、Engine Latencyを再生して自動終了します。`--loop`ではPatternのEvent Timelineだけを繰り返し、Instrument RuntimeをResetしません。Loop中は標準入力のEnterで停止します。

| Option | Default | 内容 |
|---|---:|---|
| `--audio-device <id>` | OS Default | CPAL Stable Device ID |
| `--sample-rate <hz>` | Device Default | PatternをCompileするSample Rate |
| `--buffer-size <frames>` | 256 | CPALへ要求するFrame数 |
| `--tail <seconds>` | 1.0 | One-shot終了時に追加するTail |
| `--loop` | Off | Patternを反復する。MIDI Auditionにはありません |

### `audition midi` — MIDI FileのRealtime試聴

```bash
sonalloy audition midi <definition> <midi-file>
sonalloy audition midi <definition> <midi-file> --channel 2
```

MIDI FileをTickベースで読み込み、1つのChannelをPatternへ変換してから、`audition pattern`と同じ仕組みで再生します。複数Note Channelを含む場合は`--channel 1..16`が必要です。MIDI Input Deviceは使いません。Loop再生が必要なときは、`pattern import-midi`でPatternへ変換してから`audition pattern --loop`を使います。

## 音を鳴らす

`render`コマンドはいずれもWAVを生成します。確認したい内容に合わせて使い分けます。

| コマンド | 向いている用途 |
|---|---|
| `render note` | 1音の鳴り方（Attack、Sustain、Release）を手軽に確かめる |
| `render events` | 演奏中のParameter変化（Filter Cutoff、Pitch Bendなど）を正確な位置で再現する |
| `render midi` | MIDI Fileのフレーズを鳴らす |
| `render pattern` | Tickベースの演奏パターンを鳴らす（Sample Rateに依存しない長さ） |

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
| `--tempo` | 120 | Tempo Sync SampleとTempo同期Sourceの基準BPM |
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

- Eventを時系列へ処理するため、`absolute_frame`の昇順へ整列します。同じFrameに複数Eventがある場合の適用順序は`docs/runtime-processing.md`を参照してください
- 次のいずれかはErrorになり、WAVを生成しません：`--duration-frames`を超えるFrameのEvent、音源定義に存在しないParameter ID、Native範囲外の値

`render note`と`render events`は指定Tempoの4/4から始まります。`render midi`はTempo Meta EventとTime Signature Meta Eventを`MusicalTimeMap`へ変換し、Time Signatureがない場合は4/4を使います。`render pattern`もPatternの`tempo_changes`と`time_signature_changes`から同じMapを作ります。Tempo / Meterの変更位置でProcess Blockを分割し、`beat_position`と`bar_position`をProcessContextへ渡します。

| Option | Default | 内容 |
|---|---|---|
| `--duration-frames` | — | レンダリング長（Frame、必須）。Eventの`absolute_frame`はこの値未満にします |
| `--tail` | 1.0 | レンダリング後の余韻の秒数 |
| `--tempo` | 120 | Tempo Sync SampleとTempo同期Sourceの基準BPM |
| `--sample-rate` | 48000 | 出力Sample Rate |
| `--block-size` | 257 | 処理の最大Block Size |
| `--output` | — | 出力先（必須） |
| `--analyze` | Off | 補正後WAVの解析を追加 |
| `--trace <id>` | なし | 選択したDynamic ParameterをTrace（複数指定可） |
| `--trace-every-frames <N>` | 480（Trace指定時） | 定期Trace間隔 |
| `--reset-check` | Off | 同じPrepared RuntimeをResetして同じEvent列を再実行し、差分をReportへ追加 |
| `--json` | Off | 結果を機械可読で出力 |

### AnalysisとTrace（`--analyze` / `--trace`）

`render note`、`render events`、`render midi`、`render pattern`は、`--analyze`で補正後の出力WAVを決定的に解析し、`--trace`で選択したParameterの実行中の値をJSON成功Reportへ追加します。`--json`を付けない場合も、短いSummaryを標準出力へ表示します。

Analysisの主なFieldは次のとおりです。

| Field | 意味 |
|---|---|
| `level` | 全ChannelのPeak、RMS、dBFS、Crest Factor、`over_full_scale`（Peak > 1） |
| `dc` | Channelごとの算術平均 |
| `activity` | -80 dBFSを超えた最初・Peak・最後のFrame。無音なら`null` |
| `continuity` | 隣接Frame最大差分、差分が0.25を超えた件数、先頭最大16箇所 |
| `stereo.correlation` | Zero-mean Pearson相関。分母が0なら`null` |
| `spectrum` | Hann窓STFTのCentroid、最大8局所Peak、指定NoteのReference周波数とHarmonic比 |

測定できない項目（無音時のLevel / Activityなど）は`null`になり、NaNとInfinityはJSONへ出力しません。`render note`だけは指定MIDI Noteの標準音高をReference周波数として使い、`events`と`midi`はFundamentalを推測しません。

Trace対象は既存CatalogのDynamic Parameter IDだけです。Reportには各時点のTarget値（Base、Routeごとの寄与、Clamp前後の値）が記録され、Voice所属Parameterなら所属Voiceの情報も付きます。Layer TuningをTraceした場合、Portamento中はRouteとは別の`portamento_offset_cents`と、Offsetを加えた実際の値を示す`effective_value`が記録されます。MacroはInstrument単位のため、Voice情報なしで一度だけ観測されます。Layer Targetは発音中のVoiceだけを報告します。

Traceの時刻はLatency補正後の出力WAVと同じTimelineです。Renderと同じMusical Time Mapを通るため、Tempo / Meter変更位置では処理Blockが分かれます。`--trace`は繰り返し指定でき、観測総数には100,000件の上限があります。

例：

```bash
sonalloy render note presets/basic.json --analyze \
  --trace layer.body.tuning --trace voice.processor.tone.cutoff \
  --trace-every-frames 480 --json --output out/note.wav
```

`trace.parameters[].observations[]`の`final`が、全Route加算とClamp後の値です。Portamento中のLayer Tuningだけは、その後段の音高Offsetを`portamento_offset_cents`へ分け、実際にGeneratorへ渡る値を`effective_value`へ記録します。

`--reset-check`は同じRuntimeをResetして同じEvent列を再実行し、前後のAudio差分をReportへ記録します。すべてのStateが初期化されていれば差分は0になります。`--reset-check`は`trace`と併用できません。

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

### `render pattern` — Audition Patternのレンダリング

Audition PatternをInstrumentへCompileし、ほかのrenderコマンドと同じようにWAVを出力します。

```bash
sonalloy render pattern <definition> <pattern> \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/pattern.wav
```

`--analyze`、`--trace`、`--trace-every-frames`、`--json`はほかのrenderコマンドと同じです。`--tail`はPatternの1周の長さに含めず、終端後の余韻として追加します。PatternのParameter ChangeはInstrument Compile後にParameter Catalogで解決されます。

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

検証・コンパイル・レンダリングで発生する主な診断Codeを分類ごとに示します。

| 分類 | Code |
|---|---|
| 定義と検証 | `SCHEMA_UNSUPPORTED`、`JSON_INVALID`、`REQUIRED_FIELD_MISSING`、`ID_DUPLICATED`、`VALUE_OUT_OF_RANGE`、`LAYER_RANGE_INVALID`、`FILTER_CUTOFF_CLAMPED` |
| Parameter | `PARAMETER_ID_INVALID`、`PARAMETER_NOT_FOUND` |
| Modulation Source / Route | `SOURCE_ID_INVALID`、`SOURCE_ID_DUPLICATED`、`SOURCE_NOT_FOUND`、`SOURCE_VALUE_INVALID`、`ROUTE_DEPTH_INVALID`、`ROUTE_DEPTH_UNIT_INVALID`、`ROUTE_TARGET_INVALID` |
| Event File | `EVENT_ORDER_INVALID` |
| Trace | `TRACE_LIMIT_EXCEEDED` |
| Asset | `ASSET_NOT_FOUND`、`ASSET_HASH_MISMATCH`、`ASSET_DECODE_FAILED`、`ASSET_RESAMPLED`、`ASSET_DOWNMIXED`、`ASSET_HASH_MISSING`、`ASSET_ABSOLUTE_PATH` |
| 実行時 | `PROCESS_ERROR`、`DSP_ERROR` |
| Realtime I/O | `MIDI_ERROR`、`AUDIO_DEVICE_ERROR` |

Generator固有の診断Codeは次のとおりです。

| Generator | Code |
|---|---|
| Wavetable | `WAVETABLE_LAYOUT_INVALID`、`WAVETABLE_PREPARATION_FAILED`、`WAVETABLE_SILENT_FRAME`、`WAVETABLE_DC_OFFSET`、`GENERATOR_RESOURCE_LIMIT_EXCEEDED` |
| Spectral | `SPECTRAL_PREPARATION_FAILED`、`GENERATOR_RESOURCE_LIMIT_EXCEEDED` |
| Granular | `INVALID_GRAIN_REGION`、`INVALID_GRAIN_PARAMETER` |
| Sample | `UNSUPPORTED_PLAYBACK_COMBINATION`、`INVALID_STRETCH_RATIO`、`INVALID_SOURCE_TEMPO`、`STRETCH_BACKEND_FAILURE` |
| Wave Sequence | `INVALID_SEQUENCE`、`INVALID_STEP_DURATION` |
| Operator Modulation | `VALUE_OUT_OF_RANGE`、`DEFINITION_ERROR`（Carrier Level / 非Carrier Level / 未接続Amount / AM・Ring Feedback）、`GENERATOR_RESOURCE_LIMIT_EXCEEDED` |

Assetの欠落・Decode失敗はWarningとして扱われ、ほかの有効なLayerがあればレンダリングを続けます。
