# Audition Pattern（演奏パターン）

Audition Pattern（以下、演奏パターン）は、1つのSonalloy Instrument（音源）を音楽的な時間軸に沿って試奏するためのJSON形式です。サンプリングレートに依存しないTickを正本の時間軸として扱うため、コード、ベースフレーズ、アルペジオ、ドラムパターン、ベロシティの変化、サステイン、ピッチベンド、モジュレーションホイール、アフタータッチなどを同じ形式で記述・保存できます。

演奏パターンは、楽曲全体や複数Instrumentの配置（アレンジメント）を表現するものではありません。複数トラック、クリップ、録音、ミキサー、楽曲タイムラインなどが必要な場合は、RiffraなどのHost / DAW側で管理します。

## 基本的な使い方

```bash
# パターンの作成・検証・確認
sonalloy pattern init groove.json
sonalloy pattern validate groove.json
sonalloy pattern inspect groove.json

# オーディオデバイスから直接試聴（MIDI キーボード不要）
sonalloy audition pattern instrument.json groove.json --loop

# WAV ファイルとしてレンダリング出力
sonalloy render pattern instrument.json groove.json --output groove.wav

# MIDI ファイルとしてエクスポート
sonalloy pattern export-midi groove.json --output groove.mid
```

MIDI ファイルを Pattern へ変換して利用する場合は、以下のコマンドを使用します。

```bash
sonalloy pattern import-midi phrase.mid --output phrase.json
sonalloy audition midi instrument.json phrase.mid
```

詳細な CLI オプションは [`docs/cli.md`](cli.md)、音源定義のパラメータ ID については [`docs/instrument-definition.md`](instrument-definition.md) を参照してください。

## JSONの構造

演奏パターンのスキーマバージョンは、Instrument Definitionとは独立して管理されています。現在受け付けるのは `schema_version: 1` のみです。すべてのオブジェクトとイベントで、定義されていないフィールドはエラーになります。

```json
{
  "schema_version": 1,
  "name": "basic groove",
  "ticks_per_beat": 480,
  "length_ticks": 3840,
  "tempo_changes": [
    { "tick": 0, "bpm": 120.0 }
  ],
  "time_signature_changes": [
    { "tick": 0, "numerator": 4, "denominator": 4 }
  ],
  "events": [
    {
      "type": "note",
      "tick": 0,
      "duration_ticks": 120,
      "note": 36,
      "velocity": 110
    },
    {
      "type": "pitch_bend",
      "tick": 960,
      "value": 0.25
    }
  ]
}
```

| フィールド | 説明 |
| --- | --- |
| `schema_version` | `1`。Instrument Definitionのバージョンとは独立しています。 |
| `name` | 任意の表示名。演奏結果には影響しません。 |
| `ticks_per_beat` | 1拍あたりのTick数。1〜32767、既定値は480です。 |
| `length_ticks` | 1ループの長さ。0より大きい整数です。 |
| `tempo_changes` | Tick 0から始まるテンポの推移です。 |
| `time_signature_changes` | Tick 0から始まる拍子の変更です。 |
| `events` | ノートと演奏コントロールの配列です。 |

検証ルールは次のとおりです。

- `tempo_changes` と `time_signature_changes` は、どちらも1件以上必要です。
- 各変更の `tick` は重複しない昇順で、Tick 0以外の変更は `length_ticks` 未満でなければなりません。
- テンポは有限の正の数、拍子の分母は1〜128の2の累乗（1、2、4、8…）、分子は1以上でなければなりません。

## イベントの種類

### ノート

```json
{
  "type": "note",
  "tick": 0,
  "duration_ticks": 480,
  "note": 60,
  "velocity": 100
}
```

- `tick` は `length_ticks` 未満、`duration_ticks` は0より大きくし、ノートの終端がパターンの終端を超えないようにします。
- `note` は `0..=127`、`velocity` は `1..=127` の範囲です。
- 演奏パターンには、少なくとも1つのノートが必要です。
- コンパイル時に各ノートはNote On / Note Offへ展開されるため、利用者がノートIDを管理する必要はありません。

### 演奏コントロール

```json
[
  { "type": "sustain_pedal", "tick": 960, "down": true },
  { "type": "pitch_bend", "tick": 960, "value": -0.5 },
  { "type": "mod_wheel", "tick": 960, "value": 0.75 },
  { "type": "aftertouch", "tick": 960, "value": 0.4 }
]
```

- コントロールの `tick` は `0..=length_ticks` の範囲で指定できます。終端に置いたイベントは、ループ境界で状態を明示的に戻すために使えます。
- `value` は有限の数で指定します。ピッチベンドは `-1.0..=1.0`、モジュレーションホイールとアフタータッチは `0.0..=1.0` の範囲です。

### 音源パラメータの変更

```json
{
  "type": "parameter_change",
  "tick": 960,
  "parameter": "voice.processor.tone.cutoff",
  "native_value": 4000.0
}
```

- `pattern validate` では、パラメータIDが空でないことと、`native_value` が有限の数であることを検証します。
- パラメータIDの存在と値の範囲はInstrumentに依存するため、`render pattern` または `audition pattern` のコンパイル時にパラメータカタログと照合します。
- Instrumentに存在しないIDは `PARAMETER_NOT_FOUND`、範囲外の値は `VALUE_OUT_OF_RANGE` エラーになります。

## 時間の扱いとコンパイル

演奏パターンはTickを正本の時間軸として保持し、次の流れで処理フレームへ変換されます。

```text
Tick + テンポ + ticks_per_beat + サンプリングレート
        ↓
絶対フレーム
        ↓
ProcessBlock内のsample_offset
```

- 小数のフレーム位置をテンポ変更の区間ごとに積算し、最後に最も近い整数フレームへ丸めます。途中で整数へ丸めないため、長い演奏パターンでも丸め誤差が累積しません。複数のテンポ変更が同じフレームに丸め込まれる場合はコンパイルエラーになります。
- 同じフレームに複数のイベントが重なった場合は、元のTick、イベントの種類、パターン内での定義順に基づいて処理します。

```text
サステインペダル → Note Off → パラメータ変更 → ピッチベンド → モジュレーションホイール → アフタータッチ → Note On
```

異なる Tick に配置されたイベントが丸め処理によって同一フレームになった場合でも、元の Tick の前後関係が保持されます。

## 長さとループ

- **長さ:** `length_ticks` をフレームへ変換した値が、1ループの長さになります。終端にNote Offやコントロールを置くこともできます。単発再生では終端イベントを処理するため、最後のイベントフレームの次までをメインの再生時間に含めます。
- **ループ:** `audition pattern --loop` はオーディオランタイムをリセットせず、イベントの時間軸だけを繰り返します。リリース音、ディレイ、リバーブなどの残響がループ境界で途切れることはありません。サステインやピッチベンドなどの状態も自動的には初期化されず、パターンの記述に従って引き継がれます。
- **ノートID:** ループごとのノートIDは、周回番号とパターン内のノート通し番号から作る一意な値です。ループ境界では、前の周回のNote Offを次の周回のNote Onより先に処理します。

## MIDIファイルとのやり取り

Standard MIDI File（SMF）と演奏パターンは、1つのInstrumentの演奏情報を相互に変換できます。

### インポート

```bash
sonalloy pattern import-midi input.mid --output pattern.json
sonalloy pattern import-midi input.mid --channel 10 --output drums.json
```

- MIDIの拍位置をTickとして維持し、テンポと拍子を全体のメタデータとして取り込みます。MIDIに記録がない場合は、120 BPMと4/4拍子を補います。
- ノートは同じチャンネルとノート番号ごとに、開始した順番でNote OnとNote Offを対応付けます。ベロシティ0のNote OnはNote Offとして扱います。
- 演奏パターンにはチャンネルの概念がないため、取り込めるノートチャンネルは1つだけです。複数チャンネルが含まれる場合は `--channel 1..16` を指定します。複数トラックに同じチャンネルのノートが分散している場合は、1つのパターンへ統合します。
- 対応するNote OnがないNote Offは警告を出して無視します。Note OffがないNote Onは正確な発音長を決められないためエラーにし、長さが0のノートは取り込みません。

### エクスポート

```bash
sonalloy pattern export-midi pattern.json --output pattern.mid
sonalloy pattern export-midi drums.json --channel 10 --output drums.mid
```

- `ticks_per_beat` を引き継いだ1トラックのSMFとして出力します。テンポ、拍子、ノート、ピッチベンド、CC1、CC64、チャンネルアフタータッチ、End Of Trackを出力します。
- CC1とアフタータッチはMIDIの7-bit値へ丸め、ピッチベンドは `-8192..8191` の範囲へ変換します。
- **制約:** Sonalloy固有の `ParameterChange` はStandard MIDIで表現できません。1件でも含むパターンのエクスポートは `MIDI_ERROR` で失敗し、CCやSysExへ黙って変換することもありません。
- **制約:** 同じ音程のノートが時間的に重なるパターンは、Standard MIDIのNote OffにノートIDがないためエクスポートできません。演奏パターン自体では重複を許可しますが、`MIDI_ERROR` を返して出力を中止します。
- **往復変換:** MIDIで表現できるパターンでは、ノートの位置・長さ・音程・ベロシティ、拍子、サステインが保たれます。テンポはMIDIの整数マイクロ秒/拍という制約により、わずかな差が生じる場合があります。CC1、アフタータッチ、ピッチベンドはMIDIの分解能に合わせて丸められます。

## リアルタイム試聴

```bash
sonalloy audition pattern instrument.json pattern.json \
  --audio-device <id> --sample-rate 48000 --buffer-size 256 --tail 1.0
sonalloy audition pattern instrument.json pattern.json --loop
sonalloy audition midi instrument.json phrase.mid --channel 2
```

- `audition pattern` はオーディオデバイスだけを使うため、MIDI入力デバイスは不要です。
- 単発再生では、パターン本体、残響、エンジンのレイテンシー分を再生したあと自動終了します。
- オーディオコールバックが始まる前に、JSON/MIDIの解析、Instrumentのコンパイル、パラメータ解決、Tick変換、イベントの整列、テンポマップ構築を完了します。コールバック内では、スケジュール済みイベントをブロック内の `sample_offset` へ配置するだけです。テンポ変更、ループ境界、終了位置をまたぐ `ProcessBlock` は生成しません。

## 対象範囲

演奏パターンが扱うのは、1つのInstrumentへ送る演奏条件です。単一の音源を試奏する目的であれば、コードやドラムキットのような複数ノートの同時発音も含められます。

以下は演奏パターンの対象外であり、DAWやHost側で管理します。

- 複数インストゥルメントにまたがるトラック、クリップ、アレンジメント、楽曲タイムライン
- オーディオおよび MIDI の録音、ミキサー、マスターバス機能
- ピアノロール、ステップシーケンサー、Undo / Redo などの GUI エディタ機能
- クオンタイズ、ヒューマナイズ、トランスポーズ、コードジェネレーターなどの作曲支援
- MPE、ポリフォニックアフタータッチ、プログラムチェンジ、SysEx、外部同期信号（External Sync）

これらは上位のホスト環境で処理し、最終的に必要なイベントのみを Sonalloy のプロセスインターフェースへ渡す設計となっています。
