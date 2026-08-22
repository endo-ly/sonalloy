# Audition Pattern

Audition Patternは、1つのSonalloy Instrumentを音楽的な時間軸で試奏するためのJSON形式です。Sample Rateに依存しないTickを正本にするため、Chord、Bass Phrase、Arpeggio、Drum Pattern、Velocity差、Sustain、Pitch Bend、Mod Wheel、Aftertouchを同じ形式で保存できます。

Patternは曲全体や複数InstrumentのArrangementを表しません。複数Track、Clip、Recording、Mixer、Song Timelineが必要な場合はRiffraなどのHost / DAWで扱います。

## 基本Workflow

```bash
sonalloy pattern init groove.json
sonalloy pattern validate groove.json
sonalloy pattern inspect groove.json

# Audio Deviceから試聴する。MIDI Keyboardは不要
sonalloy audition pattern instrument.json groove.json --loop

# WAVとして確認する
sonalloy render pattern instrument.json groove.json --output groove.wav

# MIDIへ持ち出す
sonalloy pattern export-midi groove.json --output groove.mid
```

MIDI FileをPatternへ変換する場合は次を使います。

```bash
sonalloy pattern import-midi phrase.mid --output phrase.json
sonalloy audition midi instrument.json phrase.mid
```

詳細なCLI Optionは[`docs/cli.md`](cli.md)、音源定義のParameter IDは[`docs/instrument-definition.md`](instrument-definition.md)を参照してください。

## JSON Schema

PatternのSchema VersionはInstrument Definitionとは独立しています。現在受け付けるのは`schema_version: 1`だけです。すべてのObjectとEventは未知Fieldを拒否します。

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
| `schema_version` | `1`。Instrument DefinitionのVersionとは別です |
| `name` | 任意の表示名。演奏結果には影響しません |
| `ticks_per_beat` | 1〜32767。既定値は480です |
| `length_ticks` | 1周の長さ。0より大きい値です |
| `tempo_changes` | Tick 0から始まるTempo Timeline |
| `time_signature_changes` | Tick 0から始まる拍子情報 |
| `events` | NoteとPerformance Controlの宣言的な列 |

`tempo_changes`と`time_signature_changes`は1件以上必要で、TickはStrict Ascending、Tick 0以外のChangeは`length_ticks`未満でなければなりません。TempoはFiniteかつ正、拍子の分母は1〜128のPower of Two、分子は1以上です。

## Event

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

`tick`は`length_ticks`未満、`duration_ticks`は0より大きく、Note EndはPattern終端を超えないようにします。Noteは`0..=127`、Velocityは`1..=127`です。Patternには少なくとも1つのNoteが必要です。

Compile時に各NoteはNote OnとNote Offへ展開されます。Pattern利用者がNote IDを指定する必要はありません。

### Performance Control

```json
{ "type": "sustain_pedal", "tick": 960, "down": true }
{ "type": "pitch_bend", "tick": 960, "value": -0.5 }
{ "type": "mod_wheel", "tick": 960, "value": 0.75 }
{ "type": "aftertouch", "tick": 960, "value": 0.4 }
```

ControlのTickは`0..=length_ticks`です。Pitch Bendは`-1..=1`、Mod WheelとAftertouchは`0..=1`で、値はFiniteでなければなりません。終端のControlはLoop境界で状態を明示的に戻すために使えます。

### Parameter Change

```json
{
  "type": "parameter_change",
  "tick": 960,
  "parameter": "voice.processor.tone.cutoff",
  "native_value": 4000.0
}
```

Pattern単体の`pattern validate`では、Parameter IDが空でないことと`native_value`がFiniteであることを検証します。Parameter IDの存在とNative RangeはInstrumentに依存するため、`render pattern`または`audition pattern`のCompile時にParameter Catalogから解決します。未知のIDは`PARAMETER_NOT_FOUND`、Range外の値は`VALUE_OUT_OF_RANGE`です。

## Musical TimeとCompile

Patternの正本時間軸はTickです。

```text
tick + tempo + ticks_per_beat + sample_rate
        ↓
absolute frame
        ↓
ProcessBlock内のsample_offset
```

Tempo Changeを区間ごとに積算したFractional Frame Positionを最後にNearestへ丸めます。途中の区間ごとに整数へ丸めないため、長いPatternでも丸め誤差を累積させません。Tempo Changeが同じFrameへ丸められるSample RateではCompile Errorです。

Compile結果は既存の`ScheduledEvent`と`TempoMap`になります。Offline RenderとScheduled Realtime Auditionは同じCompile結果を使い、Time SignatureはCore Eventへ変換せずPatternとMIDIのMetadataとして保持します。

同じFrameのEventは、元Tick、既存`ProcessEventKind::priority()`、Pattern内のSource Indexで順序を決めます。基本的なPriorityは次のとおりです。

```text
Sustain Pedal → Note Off → Parameter Change → Pitch Bend
→ Mod Wheel → Aftertouch → Note On
```

異なるTickが丸めで同じFrameになっても、元Tickの順序を保ちます。Pattern Eventの配列順だけをTimingの根拠にはしません。

## Patternの長さとLoop

`length_ticks`をTickからFrameへ変換した値が1周の長さです。Note OffまたはControlが終端にあるPatternも記述できます。One-shot Renderでは終端Eventを処理するため、最後のEvent Frameの次までをMain Durationに含めます。

`audition pattern --loop`はAudio RuntimeをResetせず、Event Timelineだけを繰り返します。Release、Delay、ReverbなどのTailはLoop境界で切りません。Sustain、Pitch Bend、Mod Wheel、Aftertouch、Parameterの状態も自動的にNeutralへ戻さず、Patternに書かれたEventの結果を引き継ぎます。

Loop反復ごとのNote IDは、上位32bitに反復番号、下位32bitにPattern内のNote Serialを持つ一意値です。終端の旧IterationのNote Offは次IterationのNote Onより先に適用されます。

## MIDI Interchange

PatternとStandard MIDI Fileは、1 Instrumentの演奏情報を交換するために相互変換できます。

### Import

```bash
sonalloy pattern import-midi input.mid --output pattern.json
sonalloy pattern import-midi input.mid --channel 10 --output drums.json
```

MIDIのMetrical TimingをTickとして保持し、TempoとTime SignatureはGlobal Metadataとして取り込みます。MIDIにない場合は120 BPMと4/4を補います。Noteは同一Channel / NoteごとにFIFOでNote On / Note Offを対応付けます。Velocity 0のNote OnはNote Offです。

PatternはMIDI Channelを持たないため、Note Channelは1つだけ選びます。自動選択はNote Channelが1つの場合だけで、複数Channelは`--channel 1..16`を要求します。異なるTrackが同じChannelを使う場合は1つのPatternへ統合します。Unmatched Note OffはWarningとして無視し、Unmatched Note Onは正確なDurationを決められないためErrorです。同一Tickで長さ0になるNoteは取り込みません。

### Export

```bash
sonalloy pattern export-midi pattern.json --output pattern.mid
sonalloy pattern export-midi drums.json --channel 10 --output drums.mid
```

出力はPatternのTicks Per Beatを使ったSingle Track SMFです。Tempo、Time Signature、Note、Pitch Bend、CC1、CC64、Channel Aftertouch、End Of Trackを出力します。CC1とAftertouchは7-bitへNearest Quantizeし、Pitch Bendは`-8192..8191`へ変換します。

Sonalloy固有の`ParameterChange`はStandard MIDIで表現できません。1件でも含むPatternのExportは`MIDI_ERROR`で失敗し、CCやSysExへ黙って変換しません。

NoteのTick、Duration、Note Number、Velocity、Time Signature、SustainはMIDI Round Tripで維持されます。TempoはMIDIの整数Microseconds-per-Beatによる微小差が生じる場合があります。7-bit ControlとPitch BendはMIDIのResolution範囲内で量子化されます。

## Realtime Audition

```bash
sonalloy audition pattern instrument.json pattern.json \
  --audio-device <id> --sample-rate 48000 --buffer-size 256 --tail 1.0
sonalloy audition pattern instrument.json pattern.json --loop
sonalloy audition midi instrument.json phrase.mid --channel 2
```

`audition pattern`はAudio Deviceだけを使い、MIDI Input Deviceを必要としません。One-shotはPattern、Tail、Engine Latencyを再生して自動終了します。`--loop`はPatternだけに指定でき、MIDI AuditionをLoopしたい場合は先にPatternへImportします。

Audio Callbackの開始前にJSON Parse、MIDI Parse、Instrument Compile、Parameter Resolve、Tick変換、Event Sort、Tempo Map構築を完了します。CallbackではScheduled EventをBlock内のSample Offsetへ配置し、Tempo Change、Loop境界、One-shot終端を跨ぐProcessBlockを作りません。

## 責務境界

Audition Patternが扱うのは1つのInstrumentへ送る演奏条件です。ChordやDrum Kitの複数Noteも、1つのInstrumentを試奏する目的ならPatternに含められます。

次の機能はPatternの責務ではありません。

- 複数InstrumentのTrack、Clip、Arrangement、Song Timeline
- MIDI Recording、Audio Recording、Mixer、Master Bus
- Piano Roll、Step Sequencer、Undo / Redo Editor
- Quantize、Humanize、Transpose、Chord Generatorなどの作曲編集
- MPE、Polyphonic Aftertouch、Program Change、SysEx、External Sync

これらはHost / DAW側で管理し、必要なEventだけをSonalloyのProcess Contractへ渡します。
