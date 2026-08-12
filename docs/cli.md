# CLI

Sonalloy CLI（バイナリ名`sonalloy`）は、音源定義（JSON）を読み込み、検証・コンパイルし、WAVへレンダリングします。

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
| Generator | 各Generatorの構成値（波形、Asset、Parameter、Algorithmなど） |
| Parameter | Parameter ID、単位、範囲、初期値 |
| Modulation | SourceとTargetの接続 |
| Processor | Layer / Voice / Globalの各Processor Chain |
| Warning | コンパイル時の警告（Asset欠落など） |

`--json`は、Generatorごとの構造をFieldとして返します。Parameter IDは`layer.<layer_id>.generator.<name>`形式（Operator Modulationだけ`operator.<1-4>.<parameter>`）。各GeneratorがどのFieldを返すかは、実際に`--json`を実行して確認してください。

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
    { "absolute_frame": 0,     "type": "parameter_change", "parameter": "voice.processor.tone.cutoff", "normalized": 0.35 },
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
| `parameter_change` | `parameter`、`normalized` | Parameter IDへ0〜1の値を送る |
| `pitch_bend` | `value` | -1〜1 |
| `mod_wheel` | `value` | 0〜1 |
| `aftertouch` | `value` | 0〜1 |

読み込み時の処理：

- Eventを**時系列へ正しく処理するため**、`absolute_frame`の昇順へ整列します。同じFrameでは、決まった優先順位（Note Off → Parameter Change → Pitch Bend → Mod Wheel → Aftertouch → Note On）で処理します
- 次のいずれかがあると、安全のためWAVを生成しません：`--duration-frames`を超えるFrameのEvent、音源定義に存在しないParameter ID、範囲外の値

| Option | Default | 内容 |
|---|---|---|
| `--duration-frames` | — | レンダリング長（Frame、必須）。Eventの`absolute_frame`はこの値未満にします |
| `--tail` | 1.0 | レンダリング後の余韻の秒数 |
| `--tempo` | 120 | Tempo Sync Sampleの基準BPM |
| `--sample-rate` | 48000 | 出力Sample Rate |
| `--block-size` | 257 | 処理の最大Block Size |
| `--output` | — | 出力先（必須） |
| `--json` | Off | 結果を機械可読で出力 |

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
- CC1はMod Wheel、Pitch Bendは-1〜1、Channel Aftertouchは0〜1へ変換します
- 同じ時刻でNote OnとNote Offが重なると、長さ0のNoteとして両方を無視します

対応しないMIDI機能（Sustain Pedal、Polyphonic Aftertouch、CC1以外のController、Program Change）は無視してWarningを出します。複数ChannelのNoteを1つの音源へ当てた場合や、ChannelごとにControl値が違う場合もWarningを出します。Note Eventを1つも含まないMIDI Fileは、Errorとして受け付けません。

| Option | Default | 内容 |
|---|---|---|
| `--tail` | 1.0 | 最後のNote Off後の余韻の秒数 |
| `--sample-rate` | 48000 | 出力Sample Rate |
| `--block-size` | 257 | 処理の最大Block Size |
| `--output` | — | 出力先（必須） |
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

`SCHEMA_UNSUPPORTED`、`JSON_INVALID`、`REQUIRED_FIELD_MISSING`、`ID_DUPLICATED`、`VALUE_OUT_OF_RANGE`、`LAYER_RANGE_INVALID`、`PARAMETER_ID_INVALID`、`PARAMETER_NOT_FOUND`、`SOURCE_ID_INVALID`、`SOURCE_ID_DUPLICATED`、`SOURCE_NOT_FOUND`、`SOURCE_VALUE_INVALID`、`ROUTE_AMOUNT_INVALID`、`ROUTE_TARGET_INVALID`、`FILTER_CUTOFF_CLAMPED`、`EVENT_ORDER_INVALID`、`DSP_ERROR`

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
