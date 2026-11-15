# CLI詳細仕様

引数、Option、既定値、表示内容は各コマンドの `--help` が正本です。本書は `--help` に出るコマンド一覧とFieldの名前では分からない、JSON Reportの構造、診断Code、イベントファイルの形式、演奏データの変換規則を扱います。

## JSON・診断の共通契約

`--json`は成功Reportを標準出力へ出し、WarningはReport内の`diagnostics`へ含めます。Warning付き成功の終了コードは0です。コマンド実行開始後の失敗は、次の構造のJSON診断を返します。

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

- Clapによる引数構文・値のエラーは標準エラーへ表示され、終了コード2です。JSON診断には含まれません
- Definition JSONのParse Errorは、特定できるときDefinition上の位置を`path`へ返します（例: `global_processors[4].detector`の`REQUIRED_FIELD_MISSING`）

## 出力契約

### preset list

一覧はビルド時にCLIへ埋め込まれ、作業ディレクトリに依存しません。検索はID、Score参照値、名前、説明、カテゴリ、タグへの大文字小文字を区別しない部分一致で、該当なしは`count: 0`です。`id`は管理用ID、`preset`はScoreから参照する値です。

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

### instrument validate / report / inspect

- `validate`: 入力を順に検証し、`files[]`（`path`・`status`・`diagnostics`）と`valid`/`invalid`件数を返します。1件でも無効なら終了コード1。検証範囲はJSON構文・制約・Asset準備までで、レンダリングは行いません。
- `report`: 各Definitionから同じ条件（Note、Velocity、Gate、Tail、Sample Rate）で1音を生成し、`analysis`（Peak、RMS、7帯域Energy Ratio）を比較します。WAVは保存しません。`--loudness`でIntegrated LUFSとTrue Peakを追加します。
- `inspect --json`: Compile後の実行構成を返します。`parameters[]`はParameter ID・単位・範囲・既定値、`routes[].effect`はRouteの寄与量、`sources[]`はSource構造を示します。`modulated_range_from_default`は各Sourceが単独で最大Depthへ達する場合の理論範囲であり、演奏値の予測には使わず、実行中の値はTraceで確認します。

### sample analyze / index / search

| コマンド | 主な出力 |
|---|---|
| `sample analyze` | `total`、`succeeded`、`failed`、各入力の`path`・`analysis`・`diagnostics`を持つ`files[]` |
| `sample index` | `index`・`source_root`の絶対パス、`total_files`、`added`・`updated`・`unchanged`・`removed`・`analyzed`・`failed`、`diagnostics` |
| `sample search` | `total_matches`（ページング前の一致件数）、`returned`、`evaluated`、`unevaluated`（判定できなかった件数）、`complete`（`unevaluated == 0`）、各WAVの`path`・`source_index`・`analysis_status`（`analyzed`/`unanalyzed`/`stale`/`failed`）・`analysis`を持つ`results[]` |

`analysis`の内容は[音響解析](#音響解析)を参照してください。

### score validate / inspect

- `validate --json`: `status`、セクション数、楽器数、`diagnostics`を返します。
- `inspect --json`: 展開済みScore（Section、Instrument、Lane、PartごとのEvent、時間情報、診断）を返します。各音の`sources[].spelling`は、旋律の音符表記・和音のコード表記を保持します。文法と演奏意味論は[Score仕様](score_spec.md)が正本です。
- `inspect --summary`: イベント明細を省いた集計（`--json`と併用可）。同じ名前のSectionは出現別に集計します。

| 集計対象 | 内容 |
|---|---|
| Score全体 | セクション数、楽器数、総音符数、演奏時間、PPQ、総Tick数 |
| Section | Form内の出現順、小節数、開始・終了Tick、主音、スケール、テンポ、拍子、区間内で始まる音符数 |
| Instrument | 音符数、音域、Velocity範囲、制御イベント数 |

- `inspect --preview-json`: ブラウザ試聴用に`tempoBpm`、`ticksPerBeat`、`timeSignature`、`lengthTicks`と、`tick`、`durationTicks`、`note`、`velocity`を持つ`notes`を返します。使えるScoreの制約は[Score仕様](score_spec.md)に集約しています。`--json`・`--summary`とは併用できません。

### render events のイベントファイル

NoteやParameter変更を絶対Frame位置で処理する、`events`配列のJSON形式です。

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
| `parameter_ramp` | `parameter`、`from_value`、`to_value`、`duration_frames` | 指定値をFrame数にわたり補間 |
| `pitch_bend` | `value`（-1..=1） | Pitch Bendを送る |
| `mod_wheel`、`aftertouch` | `value`（0..=1） | Control値を送る |

規則は次のとおりです。

- Eventは`absolute_frame`の昇順に記述します。同じFrameのEventは、Event種別と入力順に従って処理します。
- `parameter`はCatalogに存在し、値は有限でNative範囲内である必要があります。
- Rampの長さは正の整数で、終端が`--duration-frames`以下である必要があります。
- `--tail`中のFrameのEventは受け付けません。

### 音響解析

`--analyze`はLatency補正後の出力を解析します。主要Fieldは次のとおりです。

| Field | 内容 |
|---|---|
| `level` | 全ChannelのPeak、RMS、dBFS、Crest Factor、Peakが1を超えたかを示す`over_full_scale` |
| `dc` | Channelごとの算術平均 |
| `activity` | -80 dBFSを超えた最初・Peak・最後のFrame。無音は`null` |
| `continuity` | 隣接Frameの最大差分、0.25を超えた差分数、先頭最大16箇所 |
| `stereo.correlation` | Zero-mean Pearson相関。分母が0なら`null` |
| `spectrum` | Hann窓STFTのCentroid、最大8局所Peak、条件に応じた基音・倍音情報 |
| `spectrum.bands` | 7帯域（20–60、60–120、120–250、250–500、500–2000、2000–6000、6000–20000 Hz。上限はNyquistへ切り詰め、使用できるFFT Binがない帯域は`null`）ごとの上下限、平均二乗値、dBFS、全正周波数Powerに対する比率 |
| `loudness` | Integrated LUFS、True Peak（dBTP）、LRA（LU）、Short-term LUFS |

- 測定できない値（無音など）は`null`で、NaNやInfinityはJSONへ出力しません。
- Short-term LUFSは3秒窓を3秒間隔で測定し、`time_seconds`は各窓の終端時刻です。3秒未満の音声では配列が空になり、LRAは`null`です。
- 基音参照は`render note`だけが決めます。指定Noteの標準周波数を使い、EventsとMIDIでは基音を推定しません。

### Trace・Reset Check・Score Render Report

- `--trace`: Parameter CatalogのDynamic Parameter IDを観測します。`trace.parameters[].observations[]`には観測時点のBase値、Routeごとの寄与、Clamp前後の値、最終値を含みます。Trace時刻はLatency補正後のWAVと同じTimelineを沿い、合計観測数は100,000件までです。
- `render events --reset-check`: 同じPrepared RuntimeをResetしてEvent列を再実行し、2回分の音声差分をReportへ加えます。RuntimeのStateが初期化されると差分は0になります。`--trace`とは併用できません。
- `render score --json`: `sample_rate`、`frames`、`output`、楽器別の`parts`を返します。`--analyze`ではPart全体とSectionごとのMix解析（Part全体の範囲は出力Stemと同じ、Score全体のFadeとMasterの前）を加えます。Master設定時は、目標LUFS差0.5以内とTrue Peak上限を満たす音声を確定し、LRAとの差とWarningをReportします。

## MIDI変換の規則

`render midi`はStandard MIDI Fileを演奏Timelineへ変換します。TempoがないFileは120 BPM、拍子がないFileは4/4です。

- Note OnのVelocity 0はNote Offとして扱います。
- CC64はSustain Pedal（64以上がDown、63以下がUp）、CC1はMod Wheel、Pitch Bendは-1..=1、Channel Aftertouchは0..=1へ変換します。
- 同じ時刻に重なったNote On / Note Offは長さ0として無視します。
- Polyphonic Aftertouch、CC1以外のController、Program Changeなどは無視してWarningを出します。
- Noteを含まないFileはErrorです。複数ChannelにNoteがある場合やChannel間でControl値が異なる場合もWarningを出します。

`audition midi`と`score import-midi`ではNote Channelを1..=16で選びます。省略時はNote Channelが1つなら自動選択し、0件または複数件ではErrorです。Scoreとの変換で保持・省略される情報と、`score export-midi`の変換規則・Warningは[Score仕様](score_spec.md)に集約しています。

## Realtime演奏

`play`はDefinitionをMIDI InputからAudio Outputへ演奏し、標準入力のEnterで停止します。MIDI Inputが0件ならError、1件なら自動選択、2件以上なら明示したIDが必要です。Mono Output、PCM以外のFormat、対応外Buffer Sizeは起動Errorです。Realtime Schedulingを拒否された場合はWarningを出して処理を続けますが、致命的なDevice、MIDI、Process Errorでは出力を無音化して終了します。

`--macro-cc ID=CC`で割り当てたCCはMacroのParameter Changeになります。CC1とCC64は予約されています。

`audition score`は1楽器のScoreを再生し、複数楽器ならPartを選択します。`--loop`は音源Stateを保持してTimelineを繰り返します。ScoreのAudio配線とMix全体を確認する場合はOffline Renderを使ってください。

## WAVと外部Audio

- Render系コマンドの出力は32-bit float、Stereo、指定Sample RateのWAVです。出力先の親Directoryは事前に用意します。
- 外部Audioを必要とするDefinitionへは、単独RenderでMono / Stereo WAVまたは`silence`を接続します。Scoreでは`instruments.<id>.audio_input`で接続します。

## Latency補償

固定Algorithmic Latencyを持つ音源では、報告Frame数分を余分にレンダリングして先頭を取り除き、WAVを演奏TimelineのFrame 0から始めます。合計Latencyは成功JSONの`reported_latency_frames`です。ProcessorごとのLatencyは[Processor仕様](processors.md)を参照してください。

## Exit Codeと診断Code

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
| Generator固有 | `GENERATOR_UNSUPPORTED`、`GENERATOR_RESOURCE_LIMIT_EXCEEDED`、`WAVETABLE_LAYOUT_INVALID`、`WAVETABLE_PREPARATION_FAILED`、`SPECTRAL_PREPARATION_FAILED`、`UNSUPPORTED_PLAYBACK_COMBINATION`、`INVALID_STRETCH_RATIO`、`INVALID_SOURCE_TEMPO`、`STRETCH_BACKEND_FAILURE`、`INVALID_GRAIN_REGION`、`INVALID_GRAIN_PARAMETER`、`INVALID_SEQUENCE`、`INVALID_STEP_DURATION` |
