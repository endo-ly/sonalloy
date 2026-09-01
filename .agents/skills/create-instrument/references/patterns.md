# Pattern仕様（Audition Pattern）

Audition Pattern（以下、Pattern）は、1つのSonalloy音源を試奏するための演奏データ（JSON）です。Sample Rateに依存しないTickを正本の時間軸として扱うため、NoteやChord、フレーズ、ドラム、Pitch Bendのような演奏操作、Parameter Changeを同じ形式で記述・保存できます。

Patternが扱うのは1つの音源への演奏条件だけです。複数InstrumentのTrackやArrangement、録音、ミキサーといった楽曲全体の構成は対象外で、Host / DAW側で管理します。雛形は`sonalloy pattern init`で生成できます。

## JSONの構造

Schema VersionはInstrument Definitionとは独立して管理しており、現在受け付けるのは`1`のみです。定義されていないFieldはErrorになります。

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

| Field | 内容 |
|---|---|
| `name` | 表示名。演奏結果には影響しない |
| `ticks_per_beat` | 1拍あたりのTick数。1〜32767、Defaultは480 |
| `length_ticks` | 1ループの長さ。0より大きい整数 |
| `tempo_changes` | Tempoの推移。Tick 0から始める |
| `time_signature_changes` | 拍子の変更。Tick 0から始める |
| `events` | Noteと演奏操作の配列 |

検証規則：

- `tempo_changes`と`time_signature_changes`は、それぞれ1件以上必要です
- 各変更の`tick`は重複しない昇順で、Tick 0以外は`length_ticks`未満にします
- Tempoは正の有限値、拍子の分母は1〜128の2の冪、分子は1以上にします

## Eventの種類

書けるEventはNote、演奏操作、Parameter Changeの3分類です。Polyphonic Aftertouch、Program Change、SysExといったMIDI機能は扱いません。

### Note

```json
{
  "type": "note",
  "tick": 0,
  "duration_ticks": 480,
  "note": 60,
  "velocity": 100
}
```

- `tick`は`length_ticks`未満、`duration_ticks`は0より大きくし、Noteの終端がPatternの終端を超えないようにします
- `note`は0〜127、`velocity`は1〜127です
- Patternには少なくとも1つのNoteが必要です
- Compile時に各NoteはNote On / Note Offへ展開されるため、利用者がNote IDを管理する必要はありません

### 演奏操作

Sustain Pedal、Pitch Bend、Mod Wheel、AftertouchをTick位置で切り替えます。

```json
[
  { "type": "sustain_pedal", "tick": 960, "down": true },
  { "type": "pitch_bend", "tick": 960, "value": -0.5 },
  { "type": "mod_wheel", "tick": 960, "value": 0.75 },
  { "type": "aftertouch", "tick": 960, "value": 0.4 }
]
```

- `tick`は`0`以上`length_ticks`以下で指定できます。終端に置いたEventは、ループ境界で状態を明示的に戻すために使えます
- `value`は有限の数で、Pitch Bendは-1〜1、Mod WheelとAftertouchは0〜1です

### Parameter Change

```json
{
  "type": "parameter_change",
  "tick": 960,
  "parameter": "voice.processor.tone.cutoff",
  "native_value": 4000.0
}
```

- `parameter`は空でないこと、`native_value`は有限の数であることを検証します
- 音源に存在するIDかどうかと値の範囲は、音源を指定する`render pattern`または`audition pattern`のCompile時にCatalogへ照合します。存在しないIDは`PARAMETER_NOT_FOUND`、範囲外の値は`VALUE_OUT_OF_RANGE`になります

MacroとVector Axisも通常のParameterとして指定できます。

```json
[
  {
    "type": "parameter_change",
    "tick": 960,
    "parameter": "macro.motion",
    "native_value": 0.75
  },
  {
    "type": "parameter_change",
    "tick": 1440,
    "parameter": "vector.character.x",
    "native_value": 0.25
  }
]
```

Monophonic、Legato、Portamentoの意味はPatternではなくInstrument Definitionが所有します。同じPatternでも、Polyphonic InstrumentとMonophonic InstrumentではNoteの重なり方が変わります。

## 時間の扱い

PatternはTickのまま保持され、Compile時に処理Frameへ変換されます。小数のFrame位置をTempo変更の区間ごとに積算し、最後に最も近い整数Frameへ丸めます。途中で整数へ丸めないため、長いPatternでも丸め誤差が累積しません。異なるTickが同じFrameへ丸め込まれる場合はCompile Errorです。

同じFrameに複数のEventが重なった場合は、元のTick、Eventの種類、Pattern内での定義順に基づいて処理します。Tempoと拍子から作られたMusical Time Mapは、CoreのBeat / Bar PositionとTempo同期Sourceへ同じ値を渡します。

## ループ

- **長さ**: `length_ticks`をFrameへ変換した値が1ループの長さです。終端にNote Offや演奏操作を置くこともできます。単発再生では終端のEventを処理するため、最後のEventの次までを再生時間に含めます
- **反復方法**: `--loop`では音源RuntimeをResetせず、Eventの時間軸だけを繰り返します。Release音やReverbなどの残響がループ境界で途切れることはなく、Sustain PedalやPitch Bendの状態もPatternの記述どおり引き継がれます
- **Note ID**: ループごとのNote IDは、周回番号とPattern内の通し番号から作る一意な値です。ループ境界では、前の周回のNote Offを次の周回のNote Onより先に処理します

## MIDIファイルとの相互変換

Standard MIDI File（SMF）とPatternは、1つの音源の演奏情報として相互に変換できます。

**インポート（`pattern import-midi`）**

- MIDIの拍位置をTickとして維持し、Tempoと拍子を全体のMetadataとして取り込みます。記録がない場合は120 BPMと4/4拍子を補います
- Noteは同じChannelとNote番号ごとに、開始した順番でNote OnとNote Offを対応付けます。Velocity 0のNote OnはNote Offとして扱います
- PatternにはChannelの概念がないため、取り込めるNote Channelは1つだけです。複数Channelがある場合は`--channel 1..16`で選び、同じChannelの複数Trackは1つのPatternへ統合します
- 対応するNote OnがないNote OffはWarningを出して無視し、Note OffがないNote OnはErrorになります。長さ0のNoteは取り込みません

**エクスポート（`pattern export-midi`）**

- `ticks_per_beat`を引き継いだSingle TrackのSMFとして出力します。Tempo、拍子、Note、Pitch Bend、CC1、CC64、Channel Aftertouch、End Of Trackを含みます
- CC1とAftertouchはMIDIの7-bit値へ丸め、Pitch Bendは-8192〜8191へ変換します
- Sonalloy固有のParameter ChangeはStandard MIDIで表現できないため、1件でも含むPatternは`MIDI_ERROR`で失敗し、CCやSysExへ黙って変換することはありません
- 同じ音程のNoteが時間的に重なる場合も、MIDIのNote OffにNote IDがないため`MIDI_ERROR`で出力を中止します（Pattern自体は重なりを許可します）
- 往復変換では、Noteの位置・長さ・音程・Velocity、拍子、Sustainが保たれます。TempoはMIDIのマイクロ秒/拍という整数制約によりわずかに差が出ることがあり、CC1・Aftertouch・Pitch BendはMIDIの分解能へ丸められます
