# CLI詳細仕様

Sonalloy CLIは、音源定義とScoreを検証し、音声をレンダリングし、MIDIやAudio Deviceと接続します。引数、Option、既定値、制約は各コマンドの `--help` を正本とし、本書ではJSON Report、診断、演奏データの入出力契約を説明します。

## JSON出力と診断の共通契約

`--json`は成功時のReportを標準出力へ出し、通常表示のWarningはJSON時にはReport内の`diagnostics`へ含めます。コマンドの実行開始後に失敗した場合、対応コマンドは次の構造で診断を返します。

```json
{
  "status": "error",
  "exit_code": 2,
  "diagnostics": [
    {
      "code": "PARAMETER_NOT_FOUND",
      "severity": "error",
      "path": "--trace",
      "message": "trace parameter id is not present in the compiled catalog",
      "detail": "definitely.missing.parameter"
    }
  ]
}
```

Clapによる引数構文や値のエラーは標準エラーへ表示され、終了コード2で終了します。これはコマンド実行後のJSON診断には含まれません。Warningを持つ成功Reportは終了コード0です。

## Preset一覧

プリセット一覧はビルド時にCLIへ埋め込まれ、実行時の作業ディレクトリに依存しません。JSON Reportの形式は次のとおりです。

```json
{
  "status": "ok",
  "count": 1,
  "presets": [
    {
      "id": "BASS-001",
      "preset": "BASS/001-clean-sub-bass",
      "name": "Clean Sub Bass",
      "description": "...",
      "author": "...",
      "category": "Bass",
      "tags": ["Sub", "Clean"],
      "recommended_range": { "min_midi": 24, "max_midi": 60 }
    }
  ]
}
```

`id`は管理用ID、`preset`はScoreから参照する値です。説明と作者は元のメタデータがある場合に含まれます。検索はID、Score参照値、名前、説明、カテゴリ、タグに対する大文字小文字を区別しない部分一致で、結果はカテゴリと管理用IDの安定した順序を保ちます。該当なしは終了コード0で、`count: 0`と空の配列を返します。

## SampleのJSON Report

| コマンド | 主な出力 |
|---|---|
| `sample analyze` | `total`、`succeeded`、`failed`、各入力の`path`・`analysis`・`diagnostics`を持つ`files[]` |
| `sample index` | 絶対パスの`index`・`source_root`、`total_files`、`added`・`updated`・`unchanged`・`removed`・`analyzed`・`failed`、`diagnostics` |
| `sample search` | `total_matches`、`returned`、`evaluated`、`unevaluated`、`complete`、各WAVの`path`・`source_index`・`analysis_status`・`analysis`を持つ`results[]` |

`analysis`の内容は[音響解析](#analysis)を参照してください。`analysis_status`は`analyzed`、`unanalyzed`、`stale`、`failed`です。`total_matches`はページング前の一致件数、`unevaluated`は判定できなかった件数で、`complete`は`unevaluated == 0`を示します。

## Instrumentの検証・解析Report

### Definitionの一括検証

`instrument validate`はファイル、Directory、Globを受け取り、対象ごとの検証を続けます。JSONには各入力の`path`、`status`、`diagnostics`を持つ`files[]`と、全体の`valid` / `invalid`件数が含まれます。1件でも無効な入力があると終了コード1です。

検証はJSON構文、Definitionの制約、Assetの読込み・準備まで行います。WAVのレンダリングは行いません。

### 音響比較

`instrument report`は各Definitionから同じNote、Velocity、Gate、Tail、Sample Rateで1音をレンダリングして比較します。WAVは保存しません。JSONには測定条件と、Peak、RMS、7帯域のEnergy Ratioを含む`analysis`を返します。

`--loudness`を指定するとIntegrated LUFSとTrue Peak（dBTP）を追加します。`loudness.status`は`measured`、`silent`、`unmeasurable`のいずれかで、測定できない数値は`null`です。入力または解析が失敗した場合も残りのDefinitionを処理し、1件でも失敗すれば終了コード1になります。

### Compile後のDefinition

`instrument inspect --json`はCompile後に実行される構成を返します。ReportにはPerformance、Layer、Generator、Parameter、Modulation、Macro、Vector、Processor、External Audio、Warningが含まれます。Generator構造Fieldは種類ごとに異なります。

Sample Generatorでは`note_off_behavior`と`normalize`、各Zoneでは`pitch_tracking`、`gain_db`、`normalize_gain_db`、`effective_gain_db`を確認できます。

`parameters[]`はParameter ID、Owner、Native Unit、範囲、既定値、Scale、Smoothing、Modulation Unit、最大Depthを示します。`routes[].effect`はSource Endpointが作る加算量またはLog2倍率を、`sources[]`はScope、Rate、MSEG / Step / Randomの構造を示します。

`modulated_range_from_default`は、各Sourceが単独で最大Depthへ達する場合の理論範囲です。実際の演奏値の予測には使わず、実行中の値はTraceで確認します。

## Scoreの検証・展開Report

`score validate --json`は成功時に`status`、セクション数、楽器数、`diagnostics`を返します。Score YAMLの構文、参照、演奏イベント、音源能力、Audio配線を検証します。

通常のテキスト出力は曲名、演奏時間、Section構成、楽器ごとの音符数を示し、個々のEventは表示しません。`score inspect --json`は展開済みScoreを返し、Section、Instrument、Lane、PartごとのEvent、時間情報、診断を含みます。各音の`sources[].spelling`は旋律の音符表記を保持し、和音から生成した音では元のコード表記を保持します。Scoreの文法と演奏意味論は[Score仕様](score_spec.md)を参照してください。

`score inspect --summary`はイベント明細を省いた集計を返します。

| 集計対象 | 内容 |
|---|---|
| Score全体 | セクション数、楽器数、総音符数、演奏時間、PPQ、総Tick数 |
| Section | Form内の出現順、小節数、開始・終了Tick、主音、スケール、テンポ、拍子、区間内で始まる音符数 |
| Instrument | 音符数、音域、Velocity範囲、制御イベント数 |

同じ名前のSectionが複数回現れる場合は、それぞれ別の出現として集計します。Reportには詳細出力と同じ診断が含まれます。`--json`なしではWarningを標準エラーへ出します。`--summary`は`--json`と併用できます。

`score inspect --preview-json`はブラウザ試聴用に`tempoBpm`、`ticksPerBeat`、`timeSignature`、`lengthTicks`と、`tick`、`durationTicks`、`note`、`velocity`を持つ`notes`を返します。Scoreは1楽器、一定Tempo、一定拍子でNoteだけを含む必要があります。Noteは1〜32個、演奏時間は10秒以内、Tempoは30〜300 BPM、拍子の分子は32以下です。`--json`および`--summary`とは併用できません。

## Renderの出力・解析・Trace

### Event Sequence入力

`render events`はNoteやParameter変更を絶対Frame位置で処理します。JSONのトップレベルは`events`配列で、各要素に`absolute_frame`と`type`を指定します。

次の例は、Noteを開始し、再生中にLayer Gainを変更してからNoteを解放します。選択したDefinitionに`layer.body.gain`がある場合に使えます。

```json
{
  "events": [
    { "absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100 },
    { "absolute_frame": 12000, "type": "parameter_change", "parameter": "layer.body.gain", "native_value": -12.0 },
    { "absolute_frame": 24000, "type": "note_off", "note_id": 1 }
  ]
}
```

| `type` | 必須Field | 動作 |
|---|---|---|
| `note_on` | `note_id`、`note`（0..=127）、`velocity`（1..=127） | Noteを開始 |
| `note_off` | `note_id` | 対応Noteを解放 |
| `sustain_pedal` | `down`（Boolean） | Note Off後のReleaseをPedal Upまで保留 |
| `parameter_change` | `parameter`、`native_value` | CatalogにあるParameterへNative Unitの値を設定 |
| `parameter_ramp` | `parameter`、`from_value`、`to_value`、`duration_frames` | Native Unitで指定した値をFrame数にわたり補間 |
| `pitch_bend` | `value`（-1..=1） | Pitch Bendを送る |
| `mod_wheel`、`aftertouch` | `value`（0..=1） | Control値を送る |

Eventは`absolute_frame`の昇順で記述し、同じFrameのEventも指定できます。同時刻のEventはEvent種別と入力順に従って処理します。`parameter_change`や`parameter_ramp`の値は有限なNative値で、Parameter Catalogに存在しないIDやNative範囲外の値はErrorです。Rampの長さは正の整数で、終端が`--duration-frames`以下である必要があります。Frameはレンダリング開始からの位置を表し、`--tail`中のEventは受け付けません。

### Analysis

`render note / events / midi / score`は`--analyze`でLatency補正後の出力を解析します。主要Fieldは次のとおりです。

| Field | 内容 |
|---|---|
| `level` | 全ChannelのPeak、RMS、dBFS、Crest Factor、Peakが1を超えたかを示す`over_full_scale` |
| `dc` | Channelごとの算術平均 |
| `activity` | -80 dBFSを超えた最初・Peak・最後のFrame。無音は`null` |
| `continuity` | 隣接Frameの最大差分、0.25を超えた差分数、先頭最大16箇所 |
| `stereo.correlation` | Zero-mean Pearson相関。分母が0なら`null` |
| `spectrum` | Hann窓STFTのCentroid、最大8局所Peak、条件に応じた基音・倍音情報 |
| `spectrum.bands` | 7帯域ごとの上下限、平均二乗値、dBFS、全正周波数Powerに対する比率 |
| `loudness` | Integrated LUFS、True Peak（dBTP）、LRA（LU）、Short-term LUFS |

7帯域は20–60、60–120、120–250、250–500、500–2000、2000–6000、6000–20000 Hzです。上限はNyquist周波数へ切り詰めます。使用できるFFT Binがない帯域の値は`null`です。

単独RenderとScore MixのLUFS測定はFFmpegを使います。`short_term[]`は3秒窓を3秒間隔で測定し、`time_seconds`は各窓の終端時刻です。3秒未満の音声では配列が空になり、LRAは`null`です。無音など測定できない値は`null`で、NaNやInfinityはJSONへ出力しません。`render note`は指定Noteの標準周波数を参照し、EventsとMIDIでは基音を推定しません。

### TraceとReset Check

`--trace`はDefinitionのParameter CatalogにあるDynamic Parameter IDを観測します。`trace.parameters[].observations[]`には観測時点のBase値、Routeごとの寄与、Clamp前後の値、最終値を含みます。Voice所属ParameterにはVoice情報が付きます。MacroはInstrument単位で一度だけ観測され、Layer Targetは発音中のVoiceを報告します。

Layer TuningではPortamento中の`portamento_offset_cents`と、Generatorへ渡る`effective_value`を記録します。Trace時刻はLatency補正後のWAVと同じTimelineに沿い、合計観測数は100,000件までです。`--trace-every-frames`はTrace時の観測間隔です。

`render events --reset-check`は同じPrepared RuntimeをResetしてEvent列を再実行し、2回分の音声差分をReportへ加えます。RuntimeのStateが初期化されると差分は0になります。`--reset-check`と`--trace`は併用できません。

### Score Render Report

`render score --json`は`sample_rate`、`frames`、`output`、楽器別の`parts`を返します。指定した処理に応じてMix / Outputの解析と音量、Master、MP3測定、PartごとのTraceが加わります。

`--analyze`ではPart全体とSectionごとのMix、Part解析を返します。Part全体の解析範囲は出力Stemと同じで、Score全体のFadeとMasterの前の音声です。Section内PartはSection境界で区切ります。Section Mixには`integrated_lufs`、`true_peak_db`、`loudness_status`が含まれます。Master設定時は目標LUFS差0.5以内とTrue Peak上限を満たす音声を確定し、LRAとの差とWarningをReportします。

## MIDI・Realtimeの詳細仕様

### MIDI入力と変換

`render midi`はStandard MIDI FileのTick、Tempo、Meter、Noteを演奏Timelineへ変換します。Tempo変更とMeter変更の位置でProcess Blockを分割し、`beat_position`と`bar_position`を更新します。TempoがないFileでは120 BPM、拍子がないFileでは4/4を使います。

- Note OnのVelocity 0はNote Offとして扱います。
- CC64はSustain Pedal（64以上がDown、63以下がUp）、CC1はMod Wheelへ変換します。
- Pitch Bendは-1..=1、Channel Aftertouchは0..=1へ変換します。
- 同じ時刻に重なったNote On / Note Offは長さ0として無視します。
- Polyphonic Aftertouch、CC1以外のController、Program Changeなどは無視してWarningを出します。
- Noteを含まないFileはErrorです。複数ChannelにNoteがある場合やChannel間でControl値が異なる場合もWarningを出します。

`audition midi`と`score import-midi`ではNote Channelを1..=16で選びます。省略時はNote Channelが1つなら自動選択し、0件または複数件ではErrorです。Scoreとの変換で保持される情報と省略される情報は[Score仕様](score_spec.md)を参照してください。

`score export-midi`はType 1 MIDIを生成し、Conductor Trackと楽器ごとのTrackを持ちます。楽器順に16 ChannelごとにPortを分け、最大128 Portを使います。既存ファイルは上書きしません。Scoreからの詳細な変換規則、Warning、省略情報は[Score仕様](score_spec.md)に集約しています。

### Device Report

`device list --json`はAudio Input、Audio Output、MIDI Inputを返します。対応Buffer範囲を取得できないDeviceでは`buffer_size`が`null`です。

| Field | 内容 |
|---|---|
| `audio_outputs[].id`、`audio_inputs[].id` | CPALが返すDevice ID |
| `audio_outputs[].default`、`audio_inputs[].default` | OSの既定Deviceか |
| `audio_outputs[].default_config`、`audio_inputs[].default_config` | Sample Rate、Channel、Sample Format、Buffer範囲 |
| `midi_inputs[].id` | MidirのMIDI Port ID |

### Realtime演奏

`play`はDefinitionをMIDI InputからAudio Outputへ演奏し、標準入力のEnterで停止します。MIDI Inputが0件ならError、1件なら自動選択、2件以上なら明示したIDが必要です。Audio Inputを要求するDefinitionは、必要なChannel数とSample Rateに対応するInput Deviceを使います。Mono Output、PCM以外のFormat、対応外Buffer Sizeは起動Errorです。

起動時はDefinition、Device、Sample Rate、Channel数、Sample Format、Buffer Size、Engine Latency、Tempo、拍子、Macro CC Mappingを表示します。終了時はCallback Frame数の最小・最大と回数、Xrun、Audio Input underrun / overflowを表示します。Realtime Schedulingを拒否された場合はWarningを表示して処理を続けます。致命的なDevice、MIDI、Process Errorでは出力を無音化してSessionを終了します。

MIDI Note、Pitch Bend、CC1、Channel Aftertouch、CC64はCore Eventに変換されます。`--macro-cc ID=CC`で割り当てたCCはMacroのParameter Changeになります。CC1とCC64は予約され、同じCCやMacroを複数回割り当てられません。

`audition score`は1楽器のScoreを再生します。複数楽器ならPartを選択します。同名Sectionの出現を指定でき、選択結果が複数の独立Laneを必要とする場合はErrorです。`--loop`は音源Stateを保持してTimelineを繰り返します。ScoreのAudio配線とMix全体を確認する場合はOffline Renderを使います。

## WAV出力・Latency・Error

### WAVと外部Audio

Render系コマンドは、32-bit float、Stereo、指定Sample RateのWAVを出力します。出力先の親Directoryは事前に用意します。

外部Audioを必要とするDefinitionには、単独RenderでMono / Stereo WAVまたは`silence`を接続します。WAVはCompile時のSample Rateへ変換され、Monoは左右へ複製、StereoはChannelを保ちます。入力終端後は無音を渡します。必須入力がない場合は`AUDIO_INPUT_REQUIRED`、外部Audioを使わない定義へ入力を渡すとErrorです。Scoreでは`instruments.<id>.audio_input`で接続します。

### Latency補償

固定Algorithmic Latencyを持つ音源では、CLIが報告Frame数分だけ余分にレンダリングして先頭を取り除き、WAVを演奏TimelineのFrame 0から始めます。合計Latencyは成功JSONの`reported_latency_frames`に含まれます。ProcessorごとのLatencyは[Processor仕様](processors.md)を参照してください。

### Exit Code

| Code | 意味 |
|---:|---|
| 0 | 成功 |
| 1 | 音源定義、Score、Compileの検証Error |
| 2 | CLI入力、Render Request、Parameter指定のError |
| 3 | Core処理、DSP、RenderのError |
| 4 | WAVなど出力先のError |

### 診断Code

| 分類 | Code |
|---|---|
| DefinitionとValidation | `SCHEMA_UNSUPPORTED`、`JSON_INVALID`、`REQUIRED_FIELD_MISSING`、`ID_DUPLICATED`、`VALUE_OUT_OF_RANGE`、`LAYER_RANGE_INVALID`、`FILTER_CUTOFF_CLAMPED` |
| Parameter | `PARAMETER_ID_INVALID`、`PARAMETER_NOT_FOUND` |
| Modulation Source / Route | `SOURCE_ID_INVALID`、`SOURCE_ID_DUPLICATED`、`SOURCE_NOT_FOUND`、`SOURCE_VALUE_INVALID`、`ROUTE_DEPTH_INVALID`、`ROUTE_DEPTH_UNIT_INVALID`、`ROUTE_TARGET_INVALID` |
| Event File / Trace | `EVENT_ORDER_INVALID`、`TRACE_LIMIT_EXCEEDED` |
| Asset | `ASSET_NOT_FOUND`、`ASSET_HASH_MISMATCH`、`ASSET_DECODE_FAILED`、`ASSET_RESAMPLED`、`ASSET_DOWNMIXED`、`ASSET_HASH_MISSING`、`ASSET_ABSOLUTE_PATH` |
| 外部Audio | `AUDIO_INPUT_REQUIRED` |
| 実行と書き出し | `PROCESS_ERROR`、`DSP_ERROR`、`RENDER_ERROR`、`WAV_OUTPUT_ERROR`、`MIDI_ERROR` |
| Realtime I/O | `AUDIO_DEVICE_ERROR` |

Generator固有の診断Codeは`GENERATOR_UNSUPPORTED`、`GENERATOR_RESOURCE_LIMIT_EXCEEDED`、`WAVETABLE_LAYOUT_INVALID`、`WAVETABLE_PREPARATION_FAILED`、`SPECTRAL_PREPARATION_FAILED`、`UNSUPPORTED_PLAYBACK_COMBINATION`、`INVALID_STRETCH_RATIO`、`INVALID_SOURCE_TEMPO`、`STRETCH_BACKEND_FAILURE`、`INVALID_GRAIN_REGION`、`INVALID_GRAIN_PARAMETER`、`INVALID_SEQUENCE`、`INVALID_STEP_DURATION`です。Operator Modulationの設定Errorには`VALUE_OUT_OF_RANGE`または`DEFINITION_ERROR`が使われます。有効なLayerまたはProcessorが必要とするAssetの欠落・Decode失敗はCompile Errorです。
