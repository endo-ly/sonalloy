# CLI

Sonalloy CLI（バイナリ名`sonalloy`）は、音源定義（JSON）を読み込み、検証・コンパイルし、リアルタイム演奏またはWAVレンダリングを行います。本書は全コマンドのOption・出力・診断の正本です。

## コマンドの全体像

| コマンド | 役割 |
|---|---|
| `instrument init` | 音源定義のひな形を生成する |
| `instrument validate` | 音源定義を検証する |
| `instrument inspect` | コンパイル後の実行値を表示する |
| `score init / validate / inspect` | Score YAMLを生成・検証・展開する |
| `score import-midi / export-midi` | ScoreとMIDIを相互変換する |
| `render score` | 楽曲・試奏をWAV、Stem、MP3へ出力する |
| `audition score` | Scoreの1楽器をAudio Deviceで試聴する |
| `render note` | 1音をレンダリングする |
| `render events` | Event Sequenceをレンダリングする |
| `render midi` | MIDI Fileをレンダリングする |
| `audition midi` | MIDI Fileを1 Channel選択して試聴する |
| `device list` | Audio Input / OutputとMIDI Inputを列挙する |
| `play` | MIDI InputからAudio Outputへリアルタイム演奏する。外部Audio定義ではAudio Inputも接続する |
| `update` | インストール済みバイナリを最新Releaseへ自己アップデートする |
| `dev render-sine` | 動作確認用のSineをレンダリングする |

## 音源定義を作る

### `instrument init` — ひな形の生成

最小のOscillator音源（Saw波形、同時発音数16）を生成します。

```bash
sonalloy instrument init <path>
```

## 音源定義を検証する

### `instrument validate` — 検証

音源定義が正しくコンパイルできるかを確認します。JSONの構文、Fieldの制約、Assetの準備まで検証します（WAVは生成しません）。成功すると`valid <path>`と表示されます。失敗時は、どのFieldに問題があるか（例：`layers[0].envelope.attack_seconds`）を示すので、その場所を直します。

```bash
sonalloy instrument validate <definition>
sonalloy instrument validate a.json b.json
sonalloy instrument validate instruments/
sonalloy instrument validate "instruments/*/definition.json" --json
```

ファイルを複数指定でき、Directoryでは配下の`definition.json`を再帰的に収集します。Globは一致するファイルをCLI側で解決し、重複を除いた安定した順番で検証します。1件の失敗後も残りを検証し、通常表示ではファイルごとの結果と全体件数、JSONでは`files[]`の`path`、`status`、`diagnostics`、`valid`、`invalid`を返します。

### `instrument report` — 複数音源の音響比較

指定したNote / Velocityを各音源で1回ずつ鳴らし、WAVを書き出さずに`AudioAnalysis`を比較します。入力の指定方法は`instrument validate`と共通です。既定のGateは0.5秒、Tailは0.5秒で、外部Audioを使う音源には`--audio-input silence`を指定できます。

```bash
sonalloy instrument report instruments/ --note 60 --velocity 127 --json
sonalloy instrument report "instruments/*/definition.json" --gate 1.0 --tail 1.0
```

JSONの`files[]`は音源ごとの`path`、`status`、`analysis`（Peak / RMS / 7帯域を含む）、`diagnostics`を返します。1件でも失敗すると終了コード1になります。

### `instrument inspect` — コンパイル結果の確認

音源定義をコンパイルした結果（最終的なGain、Pan、ADSR、Parameter、Modulation、Processorなど）を表示します。書いた定義が意図どおりに解釈されたかを確認するために使います。

```bash
sonalloy instrument inspect <definition>
sonalloy instrument inspect <definition> --json
```

主な確認項目：

| 項目 | 内容 |
|---|---|
| Performance | `mode`、Voice Count、PolyphonicのVoice StealingとChoke Group、MonophonicのLegato / Portamento、Layer Alignment Latency、報告Latency |
| Layer | 発音条件、Generator、Gain、Pan、Tuning、ADSR |
| Generator | 各Generatorの構成値（波形、Asset、Parameter、Algorithmなど）。Physical StringはExciterとLoop Parameter、ModalはExciter・Mode Count・共鳴Parameter・実効周波数上限を表示 |
| Parameter | Parameter ID、Owner、Native Unit、Native範囲、Default、Scale、Smoothing、Modulation Unit、最大Depth |
| Modulation | Sourceの範囲とPolarity、RouteごとのDepthとCurve、Static Effect、Default値からModulationで到達しうる範囲とClampの有無 |
| Macro / Vector | MacroのParameter ID・Default・Route、VectorのAxis ID・所属Layer・初期値 |
| Processor | Layer / Voice / Globalの各Processor Chain、固定Latency、DelayのTempo Unit / Feedback Mode / Tap数、ConvolutionのIR準備情報、外部Detector / Cross Synthesisの入力整列 |
| External Audio | `channels`、要求Input Channel数、使用するProcessorと入力整列Frame |
| Warning | コンパイル時の警告 |

`--json`は、Generatorごとの構造をFieldとして返します。返るFieldはGeneratorの種類ごとに異なります。`parameters[].modulation`はTargetに許可されたUnitと最大絶対Depthを、`routes[].effect`はSource Endpointが作るAdditive DeltaまたはLog2 Factorを返します。`sources[]`にはScope、RateとRate Unit、MSEG / Step / Randomの構造が含まれ、`macros[]`と`vectors[]`には外部から操作するIDを含めます。

例（抜粋）：

```json
{
  "id": "layer.body.tuning",
  "unit": "cents",
  "min": -1200.0,
  "max": 1200.0,
  "default": 0.0,
  "scale": "linear_unbounded",
  "modulation": { "unit": "cents", "max_abs_depth": 3.4028235e38 },
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

## Scoreの編集・検証・出力

楽曲・試奏・プレビューは[Score YAML](score_spec.md)を使います。正式な文法と演奏規則はこの仕様を参照し、このリファレンスではCLIの実行方法・Option・出力を説明します。音源のJSONとアセットはScoreから参照します。

```bash
sonalloy score init phrase.score.yaml
sonalloy score validate phrase.score.yaml --json
sonalloy score inspect phrase.score.yaml --json
sonalloy render score phrase.score.yaml --output phrase.wav --stems-dir stems --json
sonalloy score export-midi phrase.score.yaml --output phrase.mid --json
sonalloy score import-midi phrase.mid --instrument instrument.json --output imported.score.yaml --json
```

`score init` は組み込みLEADをPreset IDで参照する1小節のScoreを生成します。生成時にプリセットの配置先を解決しないため、別の環境へ移しても同じIDを使えます。生成先が存在する場合は失敗します。`validate`、`inspect`、`render score`ではPreset IDを実行時に解決します。プリセットの探索順と、見つからない場合の診断は[Score仕様](score_spec.md)を参照してください。`validate` は構文・参照・音楽・音源能力・配線を検査し、`inspect` はセクション順、和声、共通時間軸、楽器別イベント、元の度数と入力位置を表示します。

| 共通Option | 意味 |
|---|---|
| `--preset-root DIR` | `preset:` の探索先。未指定時は環境変数、Scoreの親階層にある `presets/` の順に解決 |
| `--bind ID=DEFINITION` | `external: true` の音源を束縛。複数回指定可。固定音源の差替えや重複IDはエラー |
| `--json` | 成功Reportと構造化DiagnosticをJSONで出力 |

`inspect --preview-json` は1楽器・1テンポ・1拍子のNote列をブラウザ用の `tempoBpm` / `ticksPerBeat` / `timeSignature` / `lengthTicks` / `notes` として返します。配布スクリプトは `--sonalloy-bin <絶対パス>` で指定したCLIからこのReportを取得します。

`inspect --summary` はイベント明細を省いた要約を返します。曲全体（セクション数・楽器数・総音符数・演奏時間・PPQと総Tick数）、セクション別（出現順・小節数・開始終了Tick・主音・スケール・テンポ・拍子・区間内開始音符数）、楽器別（音符数・音域・Velocity範囲・制御イベント数）の集計を含み、同名セクションの再登場は別の出現として数えます。解決時に出た `diagnostics` も詳細出力と同じ形で含み、`--json` なしの場合はwarningを標準エラーへ表示します。`--json` と併用でき、`--preview-json` とは併用できません。省略時は従来どおりの詳細出力です。

MIDI入力の `--channel 1..16` はNote Channelを選び、`--tonic` は度数表記の主音（既定C）を指定します。テンポ、拍子、非グリッド位置、Note、Sustain、Bend、Wheel、Aftertouchを保持します。出力はConductorと楽器ごとのTrackを持つType 1 MIDIです。楽器順に16チャンネルごとにMIDI Portを分け、最大128ポートを使用します。成功Reportは`status`、`output`、`diagnostics`を返し、通常表示では出力先とwarningを表示します。省略情報と同音重複の扱いは[Score仕様](score_spec.md#113-midi出力)を参照してください。MIDI入出力は既存の出力先を上書きしません。

### `render score` — 楽曲・試奏の音声出力

すべての楽器を共通の時間軸でレンダリングし、楽器Gainを適用したStereo Mixを生成します。外部Audioは依存元から準備し、追加GainのLaneは独立に鳴らして元の楽器ID単位で合算します。

| Option | 意味 |
|---|---|
| `--output WAV` | 完成Stereo WAV。必須 |
| `--sample-rate HZ` / `--block-size FRAMES` | 全楽器共通の処理条件。既定48000 / 257 |
| `--tail SEC` | 曲終端後の余韻。既定1秒 |
| `--stems-dir DIR` | 楽器Gain適用後・全体Fade/Master適用前の楽器別WAV |
| `--premaster-output WAV` | Gain・全体Fade適用後、Master前のMix |
| `--mp3-output PATH` | 完成音声からMP3を生成し測定。FFmpeg必須 |
| `--analyze` | Premasterと完成WAVの音響・LUFS・True Peak・LRA解析。FFmpeg必須 |
| `--section NAME` / `--occurrence N` | 指定Sectionの1出現を選択。Occurrenceは`form`内の1始まり。名前が一意ならOccurrenceを省略可 |
| `--bars START-END` | 曲頭からの通し小節範囲（両端を含む）。`--section`とは排他的 |
| `--part ID` / `--mute ID` | Mixへ含める / 除外するPart。複数回指定可。依存元PartはAudio Input用に内部処理される |
| `--preroll-bars N` | 選択出力の前から処理する小節数。出力は指定区間から始まる |
| `--apply-master` | 範囲・Part・Muteを指定した場合もScore Masterを適用 |
| `--trace [ID:]PARAMETER` | 指定楽器、または該当する全楽器のParameterを観測。複数回指定可 |
| `--trace-every-frames N` | Trace間隔。正の整数、既定480 |

MasterはScoreの `meta.master` を使い、LUFS差0.5以内・True Peak上限以下のWAVを確定します。LRAは測定値との差と警告を返します。PremasterはMaster失敗時にも残り、完成WAVは後続の解析やMP3出力を含む処理が成功してから確定します。出力先の衝突はエラーです。

JSON Reportには `sample_rate`、`frames`、`output`、楽器別の `parts` を含みます。指定した処理に応じて `mix_analysis`、`output_analysis`、`mix_loudness`、`output_loudness`、`master`、`mp3_measurement`、各Partの `traces` を返します。`--analyze`では、マスタリング前Mixを元のScore時間軸でSectionごとに集計した`sections[]`も返します。各要素は`section`、`occurrence`、Score先頭からの`start_frame` / `end_frame`と`start_seconds` / `end_seconds`、Integrated LUFS、True Peak、7帯域の`bands`、Part別のRMS / Peakを持ちます。無音や測定できない値は`null`です。Part処理に失敗した場合の診断には、`part`、`section`、`occurrence`、`time_seconds`、`block_start_frame`、`tail`を含みます。時刻とFrameはLatency補正とPrerollを反映した元のScore時間軸上の位置です。

## リアルタイム演奏

### `device list` — Deviceの列挙

Audio Input / OutputとMIDI Inputを列挙します。外部Audioを使う音源では、表示されたAudio InputのIDを`--audio-input-device`へ指定できます。IDには、表示順やIndexではなくCPAL / Midirが返す文字列をそのまま指定します。

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
| `audio_inputs[].id` | CPALのAudio Device ID |
| `audio_inputs[].default` | OS Default Inputかどうか |
| `audio_inputs[].default_config` | Device DefaultのSample Rate、Channel、Sample Format、Buffer範囲 |
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
| `--audio-input-device <id>` | OS Default | 外部Audioを使う定義で選ぶCPAL Input Device ID。Sample RateとChannel数が定義の要求に一致するDeviceだけを選択 |
| `--midi-device <id>` | 条件付き自動選択 | Midir Stable Port ID。0件はError、1件は自動選択、2件以上はID必須 |
| `--sample-rate <hz>` | Device Default | Deviceが対応するRateだけを選択し、そのRateでCompile |
| `--buffer-size <frames>` | 256 | CPALへ要求するFrame数。0やDeviceの対応範囲外はError |
| `--tempo <bpm>` | 120 | `ProcessContext.tempo_bpm`へ渡す一定Tempo |
| `--time-signature <n/d>` | 4/4 | `ProcessContext.time_signature`へ渡す一定拍子。分母は1〜128の2の冪 |
| `--macro-cc <id=cc>` | なし | Macro ParameterをMIDI CCへ割り当てる。複数指定可。CC1 / CC64は標準Controlとして予約 |

`play`は標準入力のEnterで停止する長時間実行コマンドで、次の情報を表示します。

- 起動時: Definition名、Audio / MIDI Device名とID、Sample Rate、Channel数、Sample Format、要求Buffer Size、Engine Latency、Tempo、Time Signature、Macro CC Mapping
- 終了時: 観測した最小 / 最大Frame数とCallback回数（Host Callbackの実Frame数は要求値と異なることがあるため）

音声とエラーの扱いは次のとおりです。

- Mono Device、PCM以外のFormat、対応範囲外のBuffer Sizeは起動Errorになります
- Realtime Schedulingの拒否はWarningを表示して継続し、Xrunは回数を終了時に表示します
- Audio InputのFrame不足は無音Frameで補って継続し、Device / MIDI / Processの致命的なErrorでは出力を無音化してSessionを終了します

MIDIのNote、Pitch Bend、CC1、Channel Aftertouch、CC64（Sustain Pedal）は、Offline経路と同じCore Eventへ変換されます。`--macro-cc`で割り当てたCCはMacroの`parameter_change`へ変換されます。1つのCCを複数Macroへ割り当てたり、予約済みCCを割り当てたりする指定は起動時に拒否します。

### `audition score` — 1楽器のリアルタイム試聴

```bash
sonalloy audition score phrase.score.yaml --loop
sonalloy audition score song.score.yaml --part lead --section verse --occurrence 2
```

複数楽器のScoreでは `--part` が必須です。`--section` は1つの出現を選び、同名Sectionが複数回ある場合は`--occurrence`（1始まり）で特定します。テンポ・拍子・継続Note・演奏操作・Automationを区間先頭へ揃えます。`sections.<name>.part_gains`も5msの線形Gain遷移として反映し、`--loop`では各周回で同じ時間列を使い、音源RuntimeはResetしません。選択結果が複数の独立Laneを必要とする場合はエラーになります。

`--audio-device`、`--audio-input-device`、`--sample-rate`、`--buffer-size` は `play` と共通です。`--tail` は1回再生の余韻、`--loop` はEnterまでの繰返しを指定します。Loopでは音源状態を保持し、Note IDと固定Latencyを補正します。Scoreの音声配線を含む楽曲確認には `render score` を使います。

### `audition midi` — MIDI FileのRealtime試聴

```bash
sonalloy audition midi <definition> <midi-file>
sonalloy audition midi <definition> <midi-file> --channel 2
```

MIDI FileをTickベースで読み込み、1つのChannelを内部演奏イベントへ変換してから、`audition score`と同じ仕組みで再生します。複数Note Channelを含む場合は`--channel 1..16`が必要です。MIDI Input Deviceは使いません。外部Audioを使う定義では`--audio-input-device <id>`でCPAL Inputを選べます。Loop再生が必要なときは、`score import-midi <midi> --instrument <definition> --output <score.yaml>`でScoreへ変換してから`audition score --loop`を使います。

## 音を鳴らす

`render`コマンドはいずれもWAVを生成します。確認したい内容に合わせて使い分けます。

外部Audioを使う定義では、`render note/events/midi`に`--audio-input <wav>`または`--audio-input silence`を指定します。`silence`は要求Channel数の短い無音Busとして扱われ、終端後も無音が続きます。WAVはCompile時のSample Rateへ準備され、Monoは左右へ複製、Stereoは左右を保持します。入力の終端後は無音を渡します。必須入力の省略は`AUDIO_INPUT_REQUIRED`とMessage `external audio input is required; specify --audio-input <WAV> or --audio-input silence`を返します。外部Audioを使わない定義への入力指定はErrorです。Scoreの入力接続は`instruments.<id>.audio_input`で指定します。

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
| `--tempo` | 120 | Tempo Sync SampleとTempo同期Sourceの基準BPM |
| `--sample-rate` | 48000 | 出力Sample Rate |
| `--block-size` | 257 | 処理の最大Block Size |
| `--audio-input <WAV|silence>` | なし | Definitionへ接続するMono / Stereoの外部Audio WAV、または無音Bus |
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
| `parameter_ramp` | `duration_frames`、`parameter`、`from_value`、`to_value` | Native Unitの両端を指定し、開始Frameから指定長で補間する |
| `pitch_bend` | `value` | -1〜1 |
| `mod_wheel` | `value` | 0〜1 |
| `aftertouch` | `value` | 0〜1 |

読み込み時の処理：

- Eventは`absolute_frame`の昇順で記述します。同時刻はEventの種類と定義順に従います
- 次のいずれかはErrorになり、WAVを生成しません：`--duration-frames`を超えるFrameのEvent、音源定義に存在しないParameter ID、Native範囲外の値

Rampの`duration_frames`は正の整数で、`absolute_frame + duration_frames`は`--duration-frames`以下かつ処理Frame数として表現できる必要があります。両端は有限値です。補間と次の変更による置き換えは[Scoreの演奏制御](score_spec.md#10-演奏イベントマクロ優先規則)と共通です。

`render note`と`render events`は指定Tempoの4/4から始まります。`render midi`はTempo Meta EventとTime Signature Meta Eventを`MusicalTimeMap`へ変換し、Time Signatureがない場合は4/4を使います。`render score`はScoreの共通テンポ・拍子マップを使います。Tempo / Meterの変更位置でProcess Blockを分割し、`beat_position`と`bar_position`をProcessContextへ渡します。

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

`render note`、`render events`、`render midi`、`render score`は、`--analyze`で補正後の出力WAVを決定的に解析し、`--trace`で選択したParameterの実行中の値をJSON成功Reportへ追加します。`--json`を付けない場合も、短いSummaryを標準出力へ表示します。

Analysisの主なFieldは次のとおりです。

| Field | 意味 |
|---|---|
| `level` | 全ChannelのPeak、RMS、dBFS、Crest Factor、`over_full_scale`（Peak > 1） |
| `dc` | Channelごとの算術平均 |
| `activity` | -80 dBFSを超えた最初・Peak・最後のFrame。無音なら`null` |
| `continuity` | 隣接Frame最大差分、差分が0.25を超えた件数、先頭最大16箇所 |
| `stereo.correlation` | Zero-mean Pearson相関。分母が0なら`null` |
| `spectrum` | Hann窓STFTのCentroid、最大8局所Peak、指定NoteのReference周波数とHarmonic比 |
| `spectrum.bands` | 周波数帯ごとの`lower_hz` / `upper_hz`、`mean_square`、`power_dbfs`、`energy_ratio` |
| `loudness` | Integrated LUFS（`integrated_lufs`）、True Peak（`true_peak_db`、dBTP）、LRA（`loudness_range_lu`、LU）、`short_term` |

帯域Energyは既存のMono Spectrumと同じ窓から計算し、20–60、60–120、120–250、250–500、500–2000、2000–6000、6000–20000 Hzの帯域へFFT Binを一度ずつ集計します。`mean_square`は窓のPowerを補正した平均二乗値、`power_dbfs`はそのPowerのdBFS、`energy_ratio`は全正周波数Powerに対する比率です。上端はNyquistへ切り詰め、利用できるBinがない帯域は`null`を返します。

LoudnessはCLIのFFmpegによるオフライン計測です。`--analyze`にはFFmpegが必要です。単独Renderでは`analysis.loudness`、Scoreでは`mix_loudness` / `output_loudness`に出力します。`short_term_window_seconds`と`short_term_interval_seconds`はいずれも3です。`short_term[]`の`time_seconds`はRender開始からの秒数で、その時刻を終端とする3秒窓の`short_term_lufs`を3秒間隔で返します。3秒未満の音声には完全な窓がなく、配列は空でLRAは`null`になります。無音のLUFSやTrue Peakも`null`です。

測定できない項目（無音時のLevel / Activityなど）は`null`になり、NaNとInfinityはJSONへ出力しません。`render note`だけは指定MIDI Noteの標準音高をReference周波数として使い、`events`と`midi`はFundamentalを推測しません。

Trace対象は既存CatalogのDynamic Parameter IDだけです。`trace.parameters[].observations[]`には各時点のTarget値（Base、Routeごとの寄与、Clamp前後の値。`final`が最終値）が記録され、Voice所属Parameterなら所属Voiceの情報も付きます。Layer TuningをTraceした場合、Portamento中はRouteとは別の`portamento_offset_cents`と、実際にGeneratorへ渡る値を示す`effective_value`が記録されます。MacroはInstrument単位のため、Voice情報なしで一度だけ観測され、Layer Targetは発音中のVoiceだけを報告します。

Traceの時刻はLatency補正後の出力WAVと同じTimelineです。Renderと同じMusical Time Mapを通るため、Tempo / Meter変更位置では処理Blockが分かれます。`--trace`は繰り返し指定でき、観測総数には100,000件の上限があります。

例：

```bash
sonalloy render note presets/basic.json --analyze \
  --trace layer.body.tuning --trace voice.processor.tone.cutoff \
  --trace-every-frames 480 --json --output out/note.wav
```

`--reset-check`は同じRuntimeをResetして同じEvent列を再実行し、前後のAudio差分をReportへ記録します。すべてのStateが初期化されていれば差分は0になります。`trace`とは併用できません。

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
| `--frequency` | 440 | 周波数（Hz）。有限・非負で、選択したSample Rateの半分以下（Nyquist周波数） |
| `--duration` | — | レンダリング長（秒、必須） |
| `--sample-rate` | 48000 | 出力Sample Rate |
| `--block-size` | 257 | 処理の最大Block Size |
| `--tail` | 0 | レンダリング後の余韻の秒数 |
| `--output` | — | 出力先（必須） |
| `--json` | Off | 結果を機械可読で出力 |

## アップデート

### `update` — 自己アップデート

GitHubの最新Releaseを取得して、実行中のバイナリを差し替えます。アーカイブは`SHA256SUMS.txt`で検証され、旧バイナリは`.sonalloy.old`として同じDirectoryに残ります。`~/.local/bin`へのユーザーローカル インストールが対象で、それ以外の場所から実行した場合はErrorになります。

```bash
sonalloy update
```

## 出力とエラー

### 出力WAV

すべての`render`コマンドは、32-bit float・2 Channel・指定Sample RateのStereo WAVを出力します。出力先の親Directoryは事前に作成してください。

固定Algorithmic Latencyを持つ音源では、CLIが内部で報告Latency分を追加レンダリングし、先頭の無音部分を除去して、**演奏タイムラインのFrame 0**からWAVを始めます。Latencyの値は[Processor仕様](processors.md)、合計は成功時のJSONの`reported_latency_frames`に含まれます。

### Exit Code

| Code | 意味 | 対処 |
|---:|---|---|
| `0` | 成功 | — |
| `1` | 音源定義 / コンパイルエラー | `--json`でDiagnosticsを取得し、Field Path付きのErrorを修正する |
| `2` | CLI入力 / レンダリングリクエストエラー | Option値（Sample Rate、Block Size、Tail、Frequency）を確認する |
| `3` | Core処理 / レンダリングエラー | `--json`の`DSP_ERROR`等のDiagnosticsを確認する |
| `4` | WAV出力エラー | 出力先Directoryの存在と書き込み権限を確認する |

Clapによるコマンドラインの構文・値エラーは標準エラーへ表示され、Exit Code `2`で終了します。これらのエラーは`--json`の対象外です。コマンドの実行開始後に発生したエラーは、そのコマンドが`--json`に対応していれば、次の形式で標準出力へ返します。例えば、`render note`でParameter CatalogにないIDを`--trace`へ指定した場合です：

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
| 外部Audio入力 | `AUDIO_INPUT_REQUIRED` |
| 実行と書き出し | `PROCESS_ERROR`、`DSP_ERROR`、`RENDER_ERROR`、`WAV_OUTPUT_ERROR`、`MIDI_ERROR` |
| Realtime I/O | `AUDIO_DEVICE_ERROR` |

Generator固有の診断Codeは次のとおりです。

| Generator | Code |
|---|---|
| Wavetable | `WAVETABLE_LAYOUT_INVALID`、`WAVETABLE_PREPARATION_FAILED`、`GENERATOR_RESOURCE_LIMIT_EXCEEDED` |
| Spectral | `SPECTRAL_PREPARATION_FAILED`、`GENERATOR_RESOURCE_LIMIT_EXCEEDED` |
| Granular | `INVALID_GRAIN_REGION`、`INVALID_GRAIN_PARAMETER` |
| Sample | `UNSUPPORTED_PLAYBACK_COMBINATION`、`INVALID_STRETCH_RATIO`、`INVALID_SOURCE_TEMPO`、`STRETCH_BACKEND_FAILURE` |
| Wave Sequence | `INVALID_SEQUENCE`、`INVALID_STEP_DURATION` |
| Operator Modulation | `VALUE_OUT_OF_RANGE`、`DEFINITION_ERROR`（Carrier Level / 非Carrier Level / 未接続Amount / AM・Ring Feedback）、`GENERATOR_RESOURCE_LIMIT_EXCEEDED` |

有効なLayerやProcessorの必須Assetが欠落・Decode失敗した場合はCompile Errorです。
