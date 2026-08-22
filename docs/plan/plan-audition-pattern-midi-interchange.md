# Sonalloy Audition Pattern & MIDI Interchange 詳細設計・実装計画

- **対象Repository**：`endo-ly/sonalloy`
- **基準Commit**：`8c6fdb56589cff5846e942fc0309efd68436a659`
- **基準Main確認日時**：2026-08-22 JST
- **正本要件**：`docs/CONCEPT.md`
- **現行Instrument Definition Schema**：`schema_version = 2`
- **前提実装**：Realtime Performanceを含む現在の`main`
- **恒久名称**：`Audition Pattern & MIDI Interchange`
- **用途**：実装Agentへそのまま渡し、追加の設計判断を最小化した状態で実装を進めるための詳細計画
- **文書言語**：日本語。型名、API名、File Path、Command、MIDI / Audio固有名称だけ英語表記を使用する
- **成果物**：Markdownのみ

---

## 目次

1. この計画の位置づけ
2. 最新Mainの実装断面
3. 今回の到達点
4. SonalloyとRiffraの責務境界
5. 対象範囲
6. 今回扱わない範囲
7. 全体アーキテクチャ
8. Patternの所有場所と公開範囲
9. Pattern JSON Schema
10. Musical Time契約
11. Tempo契約
12. Time Signature契約
13. Note契約
14. Performance Control Event契約
15. Parameter Change契約
16. Pattern Validation
17. PatternからCore EventへのCompile
18. 同一Tick EventのCanonical順序
19. Note ID契約
20. Pattern長と境界Event
21. MIDI Parserの再構成
22. MIDI Import
23. MIDI Channel選択
24. MIDI Export
25. MIDI量子化とRound Trip
26. Offline Pattern Render
27. Realtime Audition
28. Realtime Scheduled Event Feed
29. Sample-accurate Event適用
30. Tempo ChangeとAudio Block分割
31. Loop Playback
32. Realtime SafetyとAllocation契約
33. Audio Device契約
34. LatencyとTail
35. CLI契約
36. Diagnostic / Error契約
37. 外部依存
38. File単位の変更計画
39. Pattern Unit Test
40. MIDI Unit Test
41. Scheduled Realtime Unit Test
42. CLI / Integration Test
43. Human Review
44. Documentation / Agent Skill
45. 実装順序
46. 完了条件
47. 将来へ残すもの
48. 実装Agent向け最終ルール

---

# 1. この計画の位置づけ

現在のSonalloyは、JSON Instrument DefinitionからInstrumentをCompileし、Note / Event Sequence / Standard MIDI FileをOffline Renderできる。またRealtime Performanceにより、物理MIDI Inputから届いたEventをAudio Callbackへ渡し、同一の`InstrumentRuntime`をRealtimeに継続駆動できる。

一方、Instrumentを設計・調整する利用者が物理MIDI Keyboardを持たない場合、現在のRealtime経路を使ってInstrumentを試奏する手段が不足している。既存の`render note`は単音確認には使えるが、Chord、Bass Phrase、Arpeggio、Velocity差、Sustain、Pitch Bend、Drum Patternなど、実際の演奏条件でInstrumentを繰り返し確認する用途には不足する。

また、既存の`render events`で使うEvent Sequence JSONは`absolute_frame`を直接指定する低レベル形式である。これはCoreへの入力としては適切だが、Sample Rateに依存し、人間やAIが音楽的なPatternを記述・再利用する正本には向かない。

今回追加する`Audition Pattern & MIDI Interchange`は、この不足を解消する。

本フェーズの中心は次の四点である。

1. **1つのSonalloy Instrumentを十分に試奏できるMusical-time Pattern形式を追加する**
2. **Patternを既存Core EventへCompileし、Offline RenderとRealtime Auditionの両方で同じ演奏意味を持たせる**
3. **PatternのMIDI表現可能部分をStandard MIDI Fileと相互変換し、Riffraや他DAWへ持ち出せるようにする**
4. **Patternを作曲・Arrangementモデルへ拡張せず、SonalloyとRiffraの責務境界を固定する**

本フェーズ完了時の状態を次の一文で固定する。

> **Sonalloyは、1つのInstrumentをNote、Chord、Phrase、Drum Pattern、Performance Control、Parameter Changeを含むPatternでOffline / Realtimeに試奏でき、MIDIで表現可能なPatternをStandard MIDI Fileとして入出力できる。**

## 1.1 本来の目的

今回の目的は「Sonalloyへ簡易DAWを作ること」ではない。

目的は、Instrumentを作る作業を次のループで完結させることである。

```text
Instrument Definitionを作る
        ↓
Patternで実際の演奏条件を作る
        ↓
Realtime Audition / Offline Render
        ↓
音色を判断する
        ↓
Instrument Definitionを修正する
        ↓
再Audition
```

さらに試奏で作ったNote / Controlデータを使い捨てにせず、Standard MIDIへExportしてRiffraや他DAWへ渡せる状態にする。

## 1.2 実装判断の優先順位

判断が衝突した場合は次の順序を使う。

1. `docs/CONCEPT.md`
2. 1 Instrumentの試奏という責務境界
3. 既存`ProcessEventKind` / `ScheduledEvent` / `ProcessBlock`契約
4. Offline / Realtimeで同じ演奏意味を持つこと
5. MIDIへ持ち出せる情報を意図せず失わないこと
6. Realtime Safety
7. AIが生成・編集しやすいこと
8. 実装の単純さと保守性
9. 将来のRiffra Integration

将来のPiano Roll、複数Track、Song Arrangement、Plugin Hostを理由に、今回使用しないEditor Framework、Timeline Framework、Transport Framework、新規Workspace Crate等を導入しない。

---

# 2. 最新Mainの実装断面

基準CommitはRealtime PerformanceをMainへ統合したMerge Commit `8c6fdb56589cff5846e942fc0309efd68436a659` とする。

## 2.1 Workspace

現在のWorkspaceは次の三Crateである。

```text
sonalloy-cli
    ↓
sonalloy-core
    ↓
sonalloy-dsp-sys
```

今回もWorkspace構成を維持する。

新しい`sonalloy-pattern`、`sonalloy-sequencer`、`sonalloy-midi` Crateは追加しない。

PatternはCLI FrontendがInstrumentを試奏するための文書形式・Adapterであり、DSP Runtimeそのものではない。Riffraが将来SonalloyをHostするときはRiffra自身のTimelineから共通Process Contractを直接駆動するため、Pattern Runtimeを共有Crateへ切り出す必要はない。

## 2.2 現行CLI

現在のTop-level Commandは次である。

```text
instrument
render
  ├─ note
  ├─ events
  └─ midi
device
play
dev
```

`play`はAudio Deviceと物理MIDI Inputを選択し、Live MIDI EventをAudio Callbackへ渡す。

## 2.3 現行Event Sequence

`render events`は次のような低レベル入力を使う。

```rust
struct EventSequenceEntry {
    absolute_frame: u64,
    event: EventSequenceKind,
}
```

扱うEventは現在次である。

```text
NoteOn
NoteOff
SustainPedal
ParameterChange
PitchBend
ModWheel
Aftertouch
```

この形式は削除しない。Frame精度を直接指定したい検証・低レベル用途として維持する。

Patternはこの形式の別名ではなく、Musical TimeからこのCore EventへCompileされる上位入力形式とする。

## 2.4 現行Core Process Contract

Coreには既に次が存在する。

```text
ScheduledEvent
ProcessEventKind
ProcessEvent
ProcessBlock
ProcessContext
ProcessSpec
InstrumentRuntime
TempoMap
```

`ProcessBlock`はBlock内`sample_offset`を持つEventを受け取れる。同一OffsetのEventは入力Slice順で処理する。

`ProcessEventKind::priority()`はFrontend Adapterが同一位置EventをCanonical化するための公開Helperである。

この契約をそのまま使う。

## 2.5 現行Offline MIDI Adapter

`crates/sonalloy-cli/src/midi.rs`はStandard MIDI Fileを読み、次を扱う。

```text
Note On / Note Off
Pitch Bend
CC1 Mod Wheel
Channel Aftertouch
CC64 Sustain Pedal
Tempo
```

現在はMIDI Tickを指定Sample RateのAbsolute Frameへ直接変換し、`MidiRender`として`ScheduledEvent`と`TempoMap`を返す。

また複数MIDI ChannelのNoteを1 InstrumentへMergeし、必要に応じてWarningを出す。

## 2.6 現行Realtime Adapter

`crates/sonalloy-cli/src/realtime/`は次を持つ。

```text
audio.rs
  AudioEngine
  Core Process
  Device PCM conversion
  Realtime status

device.rs
  Audio / MIDI device inventory and selection

midi.rs
  Live MIDI input
  Timestamp + sequence
  Bounded queue

mod.rs
  play session
```

Audio callbackは既存`InstrumentRuntime::process()`を使う。

今回、Pattern Realtime AuditionもこのAudio Output経路を再利用する。

---

# 3. 今回の到達点

## 3.1 利用者から見た完成像

最低限、次が成立する。

```bash
sonalloy pattern init groove.json
sonalloy pattern validate groove.json
sonalloy pattern inspect groove.json

sonalloy audition pattern drum-kit.json groove.json --loop

sonalloy render pattern drum-kit.json groove.json \
  --output groove.wav

sonalloy pattern export-midi groove.json \
  --output groove.mid

sonalloy pattern import-midi riffra-phrase.mid \
  --output phrase.json

sonalloy audition midi bass.json riffra-phrase.mid
```

MIDI Keyboardは不要である。

## 3.2 Drum Kitの利用例

1つのSonalloy Instrument内部でKey Rangeを使い、次のようなKitを構成できる。

```text
Note 36 → Kick
Note 38 → Snare
Note 42 → Closed Hi-Hat
Note 46 → Open Hi-Hat
```

Patternはその1 Instrumentへ複数Noteを送る。

```text
Kick   x-------x-------
Snare  ----x-------x---
Hat    x-x-x-x-x-x-x-x-
```

これは複数Instrument Compositionではなく、1つのDrum Kit Instrumentの試奏であるためSonalloyの責務に含める。

## 3.3 Riffraとの接続

PatternのMIDI表現可能部分は、次のように持ち出せる。

```text
Sonalloy Pattern
      ↓ export-midi
Standard MIDI File
      ↓
Riffra / 他DAW
```

逆も成立する。

```text
Riffra / 他DAW
      ↓ Standard MIDI File
Sonalloy pattern import-midi
      ↓
Sonalloy Pattern
      ↓
Instrument Audition
```

将来のRiffra Native IntegrationではMIDI Fileを中間にせず、Riffra Timelineから`ProcessEventKind`へ直接変換する。

---

# 4. SonalloyとRiffraの責務境界

境界は「MIDIを編集できるか」ではなく、**1 Instrumentの試奏か、複数要素を組み合わせた音楽制作か**で固定する。

## 4.1 Sonalloyの責務

Sonalloyは次を所有する。

```text
Instrument Definition
Instrument Compile
Instrument Runtime
Generator / Processor / Modulation
Voice Management
1 Instrument Audition Pattern
Pattern Offline Render
Pattern Realtime Audition
Pattern MIDI Import / Export
```

Pattern内では次を扱ってよい。

```text
Note
Chord
Phrase
Arpeggio
Drum Pattern
Velocity
Sustain
Pitch Bend
Mod Wheel
Aftertouch
Sonalloy Parameter Change
Tempo
Time Signature
Loop Playback
```

## 4.2 Riffraの責務

Sonalloyへ次を追加しない。

```text
Project
Track
複数Instrument
Clip
Arrangement
Song Timeline
Audio Track
Recording
Mixer
Master Bus
Verse / Chorus構造
一般的なPiano Roll GUI
Song単位の編集操作
```

複数Instrumentが必要になった時点でHost / DAW領域とする。

```text
Riffra
├─ Track: Drums → Sonalloy Instrument A
├─ Track: Bass  → Sonalloy Instrument B
└─ Track: Pad   → Sonalloy Instrument C
```

## 4.3 Patternの長さは境界にしない

Patternを4小節等へ人工的に制限しない。

長い演奏でも1 Instrumentの評価目的ならSonalloyで扱える。

責務境界は時間の長さではなく、1 InstrumentかCompositionかで判断する。

---

# 5. 対象範囲

今回含める。

### Pattern Format

- Musical Tick基準のJSON Schema
- `schema_version = 1`
- Tempo Change
- Time Signature Change
- Pattern Length
- Note + Duration
- Sustain
- Pitch Bend
- Mod Wheel
- Channel Aftertouch
- Sonalloy Parameter Change

### CLI

- `pattern init`
- `pattern validate`
- `pattern inspect`
- `pattern import-midi`
- `pattern export-midi`
- `render pattern`
- `audition pattern`
- `audition midi`

### Offline

- Pattern → `ScheduledEvent`
- Pattern → `TempoMap`
- WAV Render
- Existing Analyze / Traceへの接続

### Realtime

- Scheduled Pattern playback
- MIDI File scheduled playback
- Sample-accurate Event offset
- Tempo Change
- Loop
- Audio Device output

### MIDI

- Standard MIDI File Import
- Standard MIDI File Export
- Single Instrument Channel Selection
- Note / Velocity
- Tempo
- Time Signature
- Pitch Bend
- CC1
- CC64
- Channel Aftertouch

---

# 6. 今回扱わない範囲

以下は実装しない。

- Multi-track Pattern
- Pattern内の複数Instrument Reference
- Song / Project Format
- Clip
- Arrangement
- MIDI Recording
- Audio Recording
- Piano Roll GUI
- Step Sequencer GUI
- Undo / Redo Editor Framework
- Quantize Command
- Humanize Command
- Transpose Command
- Copy / Paste Command
- Pattern Library / Preset Marketplace
- Chord Generator
- Arpeggiator Generator
- AI Composerそのもの
- MPE
- Polyphonic Aftertouch
- Program ChangeによるInstrument切替
- MIDI SysEx
- MIDI Clock Input / Output
- External Sync
- Riffra Native Adapter
- CLAP / VST3 Adapter

MIDI Import / Exportはデータ交換であり、Sonalloyを一般MIDI Editorへ拡張する入口として扱わない。

---

# 7. 全体アーキテクチャ

```text
                    Pattern JSON
                         │
                         ▼
                  Pattern Parser
                         │
                         ▼
               Pattern Validation
                         │
             ┌───────────┴────────────┐
             │                        │
             ▼                        ▼
        MIDI Export              Pattern Compile
                                      │
                         ┌────────────┴────────────┐
                         │                         │
                         ▼                         ▼
                Vec<ScheduledEvent>           TempoMap
                         │                         │
              ┌──────────┴──────────┐              │
              │                     │              │
              ▼                     ▼              │
        Offline Renderer      Scheduled Realtime ──┘
              │                     │
              ▼                     ▼
             WAV               Audio Device
```

MIDI Importは次の経路を使う。

```text
Standard MIDI File
        │
        ▼
   MIDI Parser
        │
        ▼
Single Instrument Channel Selection
        │
        ▼
 Pattern Definition
        │
        ├─ write JSON
        │
        └─ direct Audition
```

CoreはPattern JSON、MIDI File、CPALを知らない。

---

# 8. Patternの所有場所と公開範囲

Patternの型・Parser・Validation・Compileは`sonalloy-cli`に置く。

```text
crates/sonalloy-cli/src/pattern.rs
```

理由：

- Core Process Contractには既に必要な表現がある
- Patternは1つのFrontend入力形式である
- Riffraは将来独自TimelineからCore Eventを直接作る
- CoreへComposition寄りの文書モデルを持ち込まない

ただしPattern JSON自体は利用者向けの公開File Formatとして`docs/`へ仕様を記載する。

つまり、

```text
Pattern Rust Type
  → CLI内部

Pattern JSON Schema / Meaning
  → Public contract
```

とする。

Riffraが将来Pattern JSON Importを実装することは妨げないが、今回Riffra側の実装は行わない。

新規型は最初から`pub`にしない。

原則：

```rust
pub(crate) struct PatternDefinition { ... }
```

で十分である。

---

# 9. Pattern JSON Schema

PatternはInstrument Definitionとは独立したSchema Versionを持つ。

```json
{
  "schema_version": 1,
  "name": "basic drum groove",
  "ticks_per_beat": 480,
  "length_ticks": 3840,
  "tempo_changes": [
    {
      "tick": 0,
      "bpm": 120.0
    }
  ],
  "time_signature_changes": [
    {
      "tick": 0,
      "numerator": 4,
      "denominator": 4
    }
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
      "type": "note",
      "tick": 480,
      "duration_ticks": 120,
      "note": 38,
      "velocity": 100
    }
  ]
}
```

## 9.1 Rust Type

概念形は次とする。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PatternDefinition {
    schema_version: u32,
    name: Option<String>,
    ticks_per_beat: u16,
    length_ticks: u64,
    tempo_changes: Vec<PatternTempoChange>,
    time_signature_changes: Vec<PatternTimeSignatureChange>,
    events: Vec<PatternEvent>,
}
```

各子Struct / Enumも`deny_unknown_fields`を使う。

AIが誤ったField名を書いたときに黙って無視しないためである。

## 9.2 Schema Version

今回のPatternは、

```text
schema_version = 1
```

のみ受理する。

Instrument Definitionの`schema_version = 2`とは別契約である。

Pattern Schema Version変更時も旧Version互換分岐を追加しない。Repository方針どおり新仕様へ更新する。

## 9.3 `name`

`name`は任意の表示用文字列とする。

Runtime / Event orderingへ影響しない。

## 9.4 `ticks_per_beat`

MIDI PPQ相当のMusical Resolutionである。

範囲：

```text
1..=32767
```

Standard MIDI FileのMetrical TimingへLosslessにExport可能な範囲へ固定する。

標準値は`480`とする。

## 9.5 `length_ticks`

Patternの1周の長さである。

```text
length_ticks > 0
```

Loop時はこの位置が次IterationのTick 0と一致する。

---

# 10. Musical Time契約

Pattern正本の時間軸はSample FrameではなくTickとする。

```text
Pattern
  tick
     ↓ tempo + ticks_per_beat + sample_rate
  absolute_frame
     ↓ block position
  sample_offset
```

## 10.1 Frameを正本にしない理由

FrameをPattern正本にすると次が起きる。

```text
48 kHz で1拍 = 24000 frame
44.1 kHzで1拍 = 22050 frame
```

同じPatternでもSample RateごとにJSONを書き換える必要が生じる。

Instrument試奏とMIDI InterchangeではMusical Tickの方が適切である。

## 10.2 Tick → Frameの丸め

TickからFrameへの変換は、Tempo Changeを順番に積算した`f64` Frame Positionを最後にround-to-nearestして`u64`へ変換する。

既存MIDI Adapterの考え方を共通化する。

個々のDeltaを整数Frameへ丸めてから累積してはいけない。長いPatternで丸め誤差が蓄積するためである。

## 10.3 共通Time Converter

PatternとMIDIでTick → Frame計算を別実装しない。

新規Private Moduleを追加する。

```text
crates/sonalloy-cli/src/musical_time.rs
```

責務は次だけとする。

```text
Tick resolution
Tempo timeline
Sample rate
         ↓
Absolute frame conversion
```

大きなTransport抽象化やDAW Timeline APIへ拡張しない。

---

# 11. Tempo契約

`tempo_changes`はPattern Musical Time上のTempo変化を持つ。

```rust
struct PatternTempoChange {
    tick: u64,
    bpm: f64,
}
```

## 11.1 Validation

- 1件以上必須
- 最初のChangeは`tick = 0`
- `tick`はStrict Ascending
- `tick < length_ticks`
- `bpm`はFinite
- `bpm > 0`

同一Tickへ複数Tempo Changeを置かない。

## 11.2 Core変換

Pattern Compile時に各Tempo TickをAbsolute Frameへ変換し、既存`TempoMap::new()`へ渡す。

`length_ticks`をFrameへ変換した結果が0 Frameになる場合はCompile Errorとする。Realtime Loop Periodとして0 Frameを許可しない。

```text
PatternTempoChange
      ↓
TempoChange { absolute_frame, tempo_bpm }
      ↓
TempoMap
```

複数Tempo TickがFrame丸めの結果同一Frameになる場合、そのPattern / Sample Rate組み合わせはErrorとする。

後勝ちで潰さない。

Realtimeでも同じFrame変換結果を使う。

---

# 12. Time Signature契約

Time SignatureはAudio生成そのものには使わないが、PatternのMusical StructureとMIDI Interchangeを保持するために持つ。

```rust
struct PatternTimeSignatureChange {
    tick: u64,
    numerator: u8,
    denominator: u8,
}
```

## 12.1 Validation

- 1件以上必須
- 最初は`tick = 0`
- TickはStrict Ascending
- `tick < length_ticks`
- `numerator >= 1`
- `denominator`はPower of Two
- `denominator`は`1..=128`

MIDI Time Signature Meta Eventへ表現可能な範囲に固定する。

## 12.2 Runtimeへの影響

Time Signature Changeは`ProcessEvent`へ変換しない。

用途は次に限定する。

- Pattern Inspect
- MIDI Import
- MIDI Export
- 将来Riffraへ持ち出すMusical Metadata

Time Signatureを理由にCoreへTransport Stateを追加しない。

---

# 13. Note契約

Patternでは利用者にNote On / Note OffとNote IDを直接管理させない。

```json
{
  "type": "note",
  "tick": 0,
  "duration_ticks": 480,
  "note": 60,
  "velocity": 100
}
```

## 13.1 Rust Variant

```rust
PatternEventKind::Note {
    duration_ticks: u64,
    note: u8,
    velocity: u8,
}
```

`tick`は共通Event fieldとして保持してよい。

## 13.2 Validation

- `tick < length_ticks`
- `duration_ticks > 0`
- `tick + duration_ticks`がOverflowしない
- `tick + duration_ticks <= length_ticks`
- 選択Sample RateでNote Start / Endが同一Frameへ丸められる場合はInstrument-aware Compile Error
- `note <= 127`
- `velocity`は`1..=127`

Note EndがPattern終端`length_ticks`と一致することは許可する。

## 13.3 Core Event展開

1つのNoteを次の2 Eventへ展開する。

```text
Note
├─ NoteOn  @ tick
└─ NoteOff @ tick + duration_ticks
```

これにより重複Note、Chord、Drum Patternも同じ仕組みで扱う。

---

# 14. Performance Control Event契約

Patternは現在Coreが受けるPerformance Controlをすべて表現する。

## 14.1 Sustain

```json
{
  "type": "sustain_pedal",
  "tick": 960,
  "down": true
}
```

## 14.2 Pitch Bend

```json
{
  "type": "pitch_bend",
  "tick": 960,
  "value": 0.5
}
```

Range：

```text
-1.0..=1.0
```

Finite必須。

## 14.3 Mod Wheel

```json
{
  "type": "mod_wheel",
  "tick": 960,
  "value": 0.75
}
```

Range：

```text
0.0..=1.0
```

## 14.4 Aftertouch

```json
{
  "type": "aftertouch",
  "tick": 960,
  "value": 0.75
}
```

Range：

```text
0.0..=1.0
```

## 14.5 Tick Range

Control Eventは、

```text
0 <= tick <= length_ticks
```

を許可する。

`tick = length_ticks`はLoop境界で状態を戻す用途に必要である。

例：

```text
Sustain Down @ tick 960
Sustain Up   @ length_ticks
```

---

# 15. Parameter Change契約

PatternはSonalloy Instrument固有Parameterを直接動かせる。

```json
{
  "type": "parameter_change",
  "tick": 960,
  "parameter": "layer.body.processor.filter.cutoff",
  "native_value": 4000.0
}
```

## 15.1 Pattern単体Validation

Pattern単体では次を検証する。

- `parameter`が空でない
- `native_value`がFinite
- `tick <= length_ticks`

Parameter IDの存在とRangeはInstrumentなしでは確定できない。

## 15.2 Instrument-aware Compile

`render pattern` / `audition pattern`時にInstrumentをCompileした後、既存Parameter CatalogからHandleを解決する。

```text
parameter string
      ↓
CompiledInstrument::parameter_handle
      ↓
ParameterHandle
      ↓
Descriptor.normalize(native_value)
      ↓
ProcessEventKind::ParameterChange
```

存在しないParameterは`PARAMETER_NOT_FOUND`。

Range外は`VALUE_OUT_OF_RANGE`。

## 15.3 MIDI Export

`ParameterChange`はStandard MIDIへ勝手にCCやSysExとして変換しない。

Patternに1件でも`ParameterChange`がある場合、`pattern export-midi`はErrorとする。

```text
Pattern contains Sonalloy-specific parameter changes that Standard MIDI cannot represent
```

`--ignore-unsupported`やCustom SysEx等のFallbackは今回追加しない。

---

# 16. Pattern Validation

Validationを二層に分ける。

## 16.1 Structural Validation

`pattern validate`が行う。

対象：

```text
Schema Version
Unknown Field
Ticks Per Beat
Length
Tempo
Time Signature
Event Tick
Note Duration
Note / Velocity
Control Range
Finite Value
最低1 Note
```

Parameter IDの存在は検証しない。

`pattern validate`のHelpとDocに、Instrument固有Parameterの解決は`render pattern` / `audition pattern`で行うことを明記する。

## 16.2 Instrument-aware Validation

Pattern Compile時に追加で行う。

```text
Parameter ID存在
Parameter Native Range
Frame conversion overflow
Tempo Change frame collision
Event frame conversion
Note ID capacity
```

## 16.3 空Pattern

`events`が空、またはNote Eventが0件のPatternはInvalidとする。

Control-only Patternは今回の「Instrument試奏」という目的に合わないため受理しない。

---

# 17. PatternからCore EventへのCompile

Pattern CompileはCLI側の純粋処理とする。

概念形：

```rust
struct CompiledPattern {
    events: Vec<ScheduledEvent>,
    tempo_map: TempoMap,
    length_frames: u64,
    one_shot_duration_frames: u64,
}
```

必要ならRealtime Loop用の追加Metadataをprivate fieldで持つ。

## 17.1 Compile入力

```text
PatternDefinition
CompiledInstrument
Sample Rate
```

## 17.2 Compile出力

- Canonical order済み`ScheduledEvent`
- Frame-based `TempoMap`
- 1周の正確な`length_frames`
- One-shot処理に必要なMain Duration

## 17.3 Core変更

原則`sonalloy-core`を変更しない。

既存の、

```text
ScheduledEvent
TempoMap
ProcessEventKind
ProcessBlock
InstrumentRuntime
```

で不足しない設計にする。

Pattern都合でCoreへ`Pattern`、`Bar`、`Beat`、`Track`を追加しない。

---

# 18. 同一Tick EventのCanonical順序

Pattern JSONの`events[]`入力順をTiming正本にしない。

Pattern Eventは`tick`で宣言的に位置を持ち、Compile時にCanonical化する。

## 18.1 Note展開後の順序

同一の**元Tick**から同一Frameへ変換されたEventでは既存`ProcessEventKind::priority()`を使う。

```text
SustainPedal
NoteOff
ParameterChange
PitchBend
ModWheel
Aftertouch
NoteOn
```

同じPriority同士は元Pattern EventのSource IndexでTie-breakする。

異なるTickがFrame丸めによって同一Absolute Frameへ潰れた場合は、`priority()`より先に元Tickの昇順を維持する。後のTickのNoteOff等が前のTickのNoteOnより先へ移動してはいけない。

Compile中の一時Eventは少なくとも次を保持し、最終的に、

```text
absolute_frame
original_tick
priority
source_index
```

の順でSortしてから`ScheduledEvent`へ落とす。

## 18.2 理由

PatternはOffline / Scheduled入力であり、Live MIDIのような実到着順序ではない。

したがって同一位置は既存Offline Event Sequence / MIDI Adapterと同じCanonical順にする。

## 18.3 MIDI Exportでも同じ意味を維持

MIDI Exportでは、ParameterChangeがないことを確認した後、Pattern Eventを一時的なCore Event相当へ展開し、同じ`priority()`で同一Tick順序を決める。

独自の第二優先順位表を作らない。

---

# 19. Note ID契約

Pattern利用者はNote IDを指定しない。

Compile時にNote EventのSource Index順で`note_serial`を割り当てる。

## 19.1 One-shot

Iteration 0では、

```text
NoteId = note_serial
```

に相当する一意値でよい。

0を避ける必要はない。Core契約上`u64`の一意性のみ必要である。

## 19.2 Loop

Loop中に同じNote IDを次Iterationで再利用しない。

Release Tail / Voice Stealing中の旧Voiceと新Voiceが混同する可能性があるためである。

次のEncodingを固定する。

```text
upper 32 bit = loop_iteration
lower 32 bit = note_serial
```

概念：

```rust
fn loop_note_id(iteration: u32, note_serial: u32) -> u64 {
    (u64::from(iteration) << 32) | u64::from(note_serial)
}
```

Pattern内Note数が`u32::MAX`を超える場合はCompile Error。

Loop Iterationが`u32::MAX`を超える場合はFatal Errorとして停止する。

現実的利用では到達しないが、Wrapさせない。

## 19.3 MIDI Live Note IDとは共有しない

Live MIDIのChannel / Note / Serial EncodingとPattern Note ID Encodingを統一する必要はない。

両者は別SessionのFrontend Assigned Identityである。

---

# 20. Pattern長と境界Event

`length_ticks`はLoop周期である。

Tick → Frame変換した値を、

```text
length_frames
```

とする。

## 20.1 Note Off at End

NoteがPattern終端まで続くことを許可する。

```text
NoteOn  @ tick 0
NoteOff @ length_ticks
```

そのためCompiled Eventは`absolute_frame == length_frames`を持ち得る。

## 20.2 One-shot Render Duration

既存Offline RendererはEventが`duration_frames`未満であることを要求する。

したがってOne-shot Main Durationは次とする。

```text
max(length_frames, last_event_frame + 1)
```

OverflowはError。

これによりPattern境界のNoteOff / SustainUpを正しく処理できる。

PatternのMusical Loop Period自体は`length_frames`のまま変えない。

## 20.3 Loop Boundary

Loop時、次のEventは同じAbsolute Audio Frameに存在し得る。

```text
前Iterationのtick = length_ticks
次Iterationのtick = 0
```

この境界では両方を1つのCanonical Event集合として扱う。

例えば、

```text
前Iteration NoteOff
次Iteration NoteOn
```

ならNoteOffを先に適用する。

Note IDのIteration対応は次で固定する。

- `0 <= tick < length_ticks`に由来するNoteOn / NoteOffはそのPattern IterationのIDを使う
- `tick = length_ticks`に由来するNoteOffは終了する側のPattern IterationのIDを使う
- 次Iterationの`tick = 0` NoteOnは次IterationのIDを使う

したがって境界で同時に存在する旧NoteOffと新NoteOnは異なるNote IDを持つ。

---

# 21. MIDI Parserの再構成

現在の`read_midi()`はParseとTick→Frame変換が一体化している。

Pattern Importを追加するため、MIDI File Parseを一度Musical Tick表現へ分離する。

## 21.1 新しい内部構造

概念形：

```rust
struct ParsedMidi {
    ticks_per_beat: u16,
    length_ticks: u64,
    events: Vec<RawMidiEvent>,
    tempo_changes: Vec<RawMidiTempoChange>,
    time_signature_changes: Vec<RawMidiTimeSignatureChange>,
    diagnostics: Vec<Diagnostic>,
}
```

`RawMidiEvent`はTick、Track Index、Event Index、Channel、Kindを持つ。

## 21.2 `parse_midi()`

責務：

- File read
- SMF parse
- Metrical Timing validation
- Delta Tick accumulation
- MIDI / Meta Event抽出
- Max Track End Tick計算
- Unsupported Event診断

ここではSample Rateを受け取らない。

## 21.3 既存`read_midi()`

既存CLI挙動を壊さないため、`read_midi(path, sample_rate)`はFacadeとして残してよい。

内部では、

```text
parse_midi
   ↓
existing render conversion
```

にする。

既存`render midi`のMulti-channel Merge / Warning意味を維持する。

今回のPattern Import向け仕様を理由に既存`render midi`のChannel意味を勝手に変更しない。

## 21.4 Time Signature Parse

現在無視しているMIDI Time Signature Meta Eventを`ParsedMidi`へ保持する。

ない場合はPattern Import時に4/4を補う。

---

# 22. MIDI Import

CLI：

```bash
sonalloy pattern import-midi input.mid \
  --output pattern.json
```

複数Note Channelがある場合：

```bash
sonalloy pattern import-midi song.mid \
  --channel 10 \
  --output drums.json
```

CLIの`--channel`は人間向けに`1..=16`で受け、内部では`0..=15`へ変換する。

## 22.1 Import Flow

```text
MIDI File
  ↓ parse_midi
ParsedMidi
  ↓ choose one note channel
Filtered MIDI Event
  ↓ pair notes in Tick domain
PatternDefinition
  ↓ validate
Pattern JSON
```

## 22.2 Note Pairing

同一Channel / Noteの重複発音を扱うため、

```rust
HashMap<u8, VecDeque<PendingMidiNote>>
```

相当でFIFO Pairingする。

Standard MIDIのNote OffにはSonalloyのNote IDがないため、同じChannel / Noteで重複発音する入力では、この順序規則が対応付けの正本になる。PatternからExportした演奏情報については、対応付けによる長さの入れ替わりを防ぐため、同音程の時間的な重複をExport前に拒否する。

`PendingMidiNote`は、

```text
start_tick
velocity
source order
```

を持つ。

NoteOn velocity 0はNoteOffとして扱う。

## 22.3 Zero-length Note

同一TickのNoteOn / NoteOffでDuration 0になるNoteはPatternへ入れない。

既存`read_midi()`と同様に発音しないEventとして除去する。

## 22.4 Unmatched Note Off

Matching NoteOnのないNoteOffはWarningを出して無視する。

## 22.5 Unmatched Note On

MIDI File終了時にNoteOffがないNoteOnはPatternへ正確なDurationとして変換できない。

Pattern ImportではErrorとする。

Pattern Endへ自動延長して補完しない。

## 22.6 Pattern Length

`length_ticks`はMIDI全Trackの終了Tickと、選択Channelの最後のEvent Tickを考慮して決める。

最低でも最後のNote End / Control Eventを含む。

Tempo / Time Signature Changeがちょうど現在の最終Tickに存在する場合は、そのMeta EventをPattern内へ保持できるよう`length_ticks`をChecked Addで1 Tick延長する。Silent Dropしない。

0 Tick MIDIはNoteが存在しないためErrorになる。

## 22.7 Tempo

MIDIにTick 0 Tempoがない場合は120 BPMを挿入する。

Tempo Changeは選択Channelに関係なくFile Global Metadataとして保持する。

## 22.8 Time Signature

MIDIにTick 0 Time Signatureがない場合は4/4を挿入する。

Time Signature ChangeもGlobal Metadataとして保持する。

---

# 23. MIDI Channel選択

Patternは1 InstrumentでありMIDI Channelを持たない。

MIDI Import / Audition時にChannelを1つ選ぶ。

## 23.1 自動選択

NoteOnが存在するChannelを列挙する。

- 1 Channelのみ → 自動選択
- 0 Channel → Error
- 2 Channel以上 → Error

複数Channelを黙って1 PatternへMergeしない。

Errorには利用可能Channelを表示し、`--channel`指定を案内する。

## 23.2 明示Channel

`--channel N`指定時はそのChannelのNote / ControlだけをPatternへ取り込む。

Tempo / Time SignatureはGlobalとして取り込む。

指定ChannelにNoteがなければError。

## 23.3 TrackとChannel

複数MIDI Trackが同じChannelを使う場合は1 Instrument Sequenceとして統合してよい。

PatternはMIDI Track構造を保存しない。

これは意図したLossであり、PatternがTrackを持たない責務境界に基づく。

---

# 24. MIDI Export

CLI：

```bash
sonalloy pattern export-midi pattern.json \
  --output pattern.mid
```

Channel指定：

```bash
sonalloy pattern export-midi drums.json \
  --channel 10 \
  --output drums.mid
```

`--channel`省略時はChannel 1を使う。

## 24.1 Output Format

Standard MIDI FileのSingle Trackを出力する。

```text
Format::SingleTrack
Timing::Metrical(pattern.ticks_per_beat)
```

1 Instrument Patternであるため、Pattern内部構造を複数MIDI Trackへ分けない。

## 24.2 Export対象

次を出力する。

```text
Tempo Meta
Time Signature Meta
Note On
Note Off
Pitch Bend
CC1 Mod Wheel
CC64 Sustain
Channel Aftertouch
End Of Track
```

## 24.3 MIDIで表現できないPattern

### Parameter Change

1件でも存在すればExport Error。

一部だけ黙ってExportしない。

### 同音程NoteのOverlap

Patternは、同じ音程のNoteが時間的に重なる演奏を許可する。AuditionとOffline Renderでは各Noteに内部Note IDを付けるため、発音の対応関係を保てる。

Standard MIDIのNote OffにはNote IDがないため、同じ音程のNoteが時間的に重なるPatternはExport Errorとする。隣接するNote（前のNote Offと次のNote Onが同じTick）はExportできる。

## 24.4 Event ordering

Tick昇順。

同一TickはSonalloyのCanonical Priorityを維持する。

Source Indexは最後のTie-breakとする。

## 24.5 Delta Tick

Absolute Tick Event列からChecked SubtractionでDelta Tickへ変換する。

`midly`のDelta表現範囲を超える場合は、必要なら規格上意味を変えないMeta Event分割ではなくErrorとする。

ただし通常Pattern範囲では到達しない。

## 24.6 End Of Track

`EndOfTrack`は`length_ticks`に置く。

Boundary Control / NoteOffも同Tickにある場合、EndOfTrackを最後にする。

---

# 25. MIDI量子化とRound Trip

MIDIは一部Control値のResolutionがSonalloy Patternより低い。

## 25.1 Exact Round Trip対象

`ParameterChange`を含まず、同じ音程のNoteが時間的に重ならないPatternでは、次をPattern → MIDI → PatternでTick単位に一致させる。Pattern自体では同音程の重複を許可するが、そのPatternはMIDI Exportの対象外である。

```text
Ticks Per Beat
Length Tick
Note Start Tick
Note Duration Tick
Note Number
Velocity
Time Signature
```

TempoはMIDI microseconds-per-beat整数化による微小差を許容する。

## 25.2 7-bit Control

`ModWheel` / `Aftertouch`は0..127へround-to-nearestする。

再Import後の誤差許容：

```text
<= 0.5 / 127 + float epsilon
```

SustainはBooleanなのでExact。

## 25.3 Pitch Bend

既存Normalizationの逆変換を`midi_common.rs`へ追加する。

```text
-1.0 → -8192
 0.0 → 0
+1.0 → +8191
```

Round-to-nearestし、Range Clampする。

再Import後はMIDI 14-bit Resolution内で一致することをTestする。

## 25.4 Tempo

BPMから、

```text
microseconds_per_beat = round(60_000_000 / bpm)
```

で変換する。

0または24-bit範囲外になるTempoはMIDI Export Error。

Pattern自体はFinite Positive BPMならValidのままとする。

---

# 26. Offline Pattern Render

新Command：

```bash
sonalloy render pattern instrument.json pattern.json \
  --tail 1.0 \
  --sample-rate 48000 \
  --block-size 257 \
  --output audition.wav
```

既存`render midi`と同じ、

```text
--analyze
--trace
--trace-every-frames
--json
```

を扱う。

`reset-check`は今回`render pattern`へ追加しない。Pattern試奏の中心機能ではなく、既存`render midi`にも存在しないためである。既存`render events --reset-check`はそのまま維持する。

## 26.1 Flow

```text
Load Instrument
      ↓
Load Pattern
      ↓
Structural Validate
      ↓
Compile Instrument
      ↓
Compile Pattern for Sample Rate
      ↓
RenderRequest
      ↓
render_instrument_with_tempo_map / trace
      ↓
Latency correction
      ↓
WAV
```

## 26.2 Tail

`--tail`は既存`render midi`と同じ秒指定、Default 1.0秒とする。

Pattern `length_ticks`にはTailを含めない。

## 26.3 Render Logic重複

`render pattern`追加を理由に`main.rs`へ既存`render midi`の大きなCopyを作らない。

Pattern / MIDI / Event Sequenceが最終的に、

```text
CompiledInstrument
RenderRequest
Vec<ScheduledEvent>
TempoMap
TraceRequest
```

へ揃った後の処理は小さな共通Helperへ寄せる。

既存挙動を変える大規模CLI Refactorはしない。

---

# 27. Realtime Audition

Top-level Commandを追加する。

```text
audition
├─ pattern
└─ midi
```

## 27.1 Pattern

```bash
sonalloy audition pattern instrument.json pattern.json
```

Loop：

```bash
sonalloy audition pattern instrument.json pattern.json --loop
```

## 27.2 MIDI

```bash
sonalloy audition midi instrument.json phrase.mid
```

複数Channel：

```bash
sonalloy audition midi instrument.json song.mid --channel 2
```

`audition midi`専用の第二Scheduled形式は作らない。MIDIをMemory上の`PatternDefinition`へ変換し、その後は`audition pattern`と同じPattern Compile / Scheduled Feedを使う。

## 27.3 Audio Options

両方で次を共有する。

```text
--audio-device <id>
--sample-rate <hz>
--buffer-size <frames>
--tail <seconds>
```

Default BufferはRealtime Performanceと同じ256。

Default Tailは1.0秒。

## 27.4 One-shot動作

Pattern / MIDIを最後まで再生し、TailとEngine Latency分をAudio Deviceへ流した後、自動終了する。

Enterを押さなくても終了する。

## 27.5 Loop動作

`--loop`はPattern Auditionのみ今回対応する。

MIDI AuditionのLoopは追加しない。MIDIをLoopしたければPatternへImportしてLoopする。

これによりLoop仕様の正本をPatternへ一本化する。

Loop中は既存`play`同様、Enterで停止する。

---

# 28. Realtime Scheduled Event Feed

現在の`AudioEngine`はLive MIDI Queueを直接所有する。

Scheduled Audition追加時にAudio Output処理を二重実装しない。

Private enumでEvent供給方式を分ける。

概念形：

```rust
enum RealtimeEventFeed {
    Live(LiveEventFeed),
    Scheduled(ScheduledEventFeed),
}
```

`AudioEngine`は、

```text
Runtime
Output Scratch
Process Event Scratch
Event Feed
Status
```

を所有する。

## 28.1 LiveEventFeed

現在のQueue、Timestamp、Sequence、Constant Tempoを保持する。

現行`play`の意味を変えない。

## 28.2 ScheduledEventFeed

次を所有する。

```text
Compiled Pattern Events
Tempo Map
Pattern Length Frames
Current Event Cursor
Loop Iteration
Loop flag
Playback End Frame
```

Audio Callbackへ現在BlockのEventとTempoを提供する。

## 28.3 Trait Objectを使わない

Realtime CallbackでDynamic Dispatchが必要な理由はない。

Private EnumによるStatic Dispatchで十分である。

Generic Audio Engineを複数Monomorphizeする必要もない。

---

# 29. Sample-accurate Event適用

Live MIDIはEvent受信時点で未来のAudio Clockへ正確にMappingできないため、Realtime Performanceでは次のCore Chunk Offset 0へ適用する。

Patternは未来のEventが既知なので、同じ制約を課さない。

## 29.1 Block内Event

例：

```text
ProcessBlock absolute_frame = 24000
frames = 256
Pattern Event absolute_frame = 24127
```

変換：

```text
sample_offset = 127
```

とする。

## 29.2 Block対象範囲

Scheduled Feedは、

```text
block_start <= event.absolute_frame < block_end
```

のEventを集める。

`event.absolute_frame == block_end`は次Blockのoffset 0とする。

## 29.3 Allocation

`ProcessEvent`用VecはStream開始前にEvent Feedが報告する`max_events_per_block`を上限としてCapacity確保する。

Scheduled Feedは保守的にCompiled Pattern Event総数を上限としてよい。Live Feedは現行Queue Capacity 4096を使う。

Callback中に`Vec`を拡張しない。

## 29.4 Canonical Order

Compiled Patternは既にAbsolute Frame / Priority / Source Index順にCanonical化されている。

Loop境界で前Iteration Endと次Iteration Startが同一Frameへ合流するときも同じ順序を保証する。

必要なら事前確保Scratchへ集めて`sort_unstable_by`してよい。Capacityを超えないことを事前計算する。

---

# 30. Tempo ChangeとAudio Block分割

`ProcessContext.tempo_bpm`は1つのProcessBlock内でConstantである。

したがってScheduled AuditionはTempo Changeを跨いだ1 ProcessBlockを作らない。

## 30.1 Block Size決定

各Core CallのFrame数は次の最小値とする。

```text
Host Callbackの残Frame
Configured max_block_size
次Tempo ChangeまでのFrame
Loop BoundaryまでのFrame
One-shot EndまでのFrame
```

0 Frame Blockは作らない。

## 30.2 Tempo Change at Block Start

Tempo Changeが現在Absolute Frameにある場合、そのChange後のTempoをそのBlockの`ProcessContext.tempo_bpm`へ使う。

## 30.3 Loop Tempo Reset

Loop境界では次IterationのTick 0 Tempoへ戻る。

前Iteration末Tempoと異なる場合、境界でTempoがJumpする。

これはPatternの正しい繰り返し意味である。

---

# 31. Loop Playback

LoopはAudio RuntimeをResetせず、Pattern Event Timelineだけを繰り返す。

## 31.1 Runtime Reset禁止

Loop境界で、

```text
InstrumentRuntime::reset()
```

を呼ばない。

理由：

- Reverb Tailが切れる
- Delay Tailが切れる
- Release Tailが切れる
- Smoother Stateが不自然にJumpする

## 31.2 Event Stateは連続

Sustain、Pitch Bend、Mod Wheel、Aftertouch、Parameter StateはPattern Eventの結果として連続する。

PatternがSustain Downのまま終われば次LoopでもDownのまま始まる。

自動Neutralizeしない。

利用者がLoopごとに戻したい場合はPattern終端に明示Eventを書く。

## 31.3 Note Identity

前述のIteration付きNote IDを使う。

## 31.4 Boundary Event

Pattern終端Eventと次Iteration開始Eventを同一Absolute Frameで適用する。

Canonical Priorityを維持する。

## 31.5 Iteration Overflow

`u32::MAX`を超えるIterationはFatal Errorで停止する。

Silent Wrapしない。

---

# 32. Realtime SafetyとAllocation契約

Scheduled AuditionでもRealtime Performanceの契約を維持する。

Audio Callback内で禁止：

- Heap Allocation
- File I/O
- JSON Parse
- MIDI File Parse
- Instrument Compile
- Pattern Compile
- Parameter String Resolve
- Blocking Mutex
- stdout / stderr
- Device Query
- Panic

Stream開始前に完了させる：

```text
Instrument Compile
Runtime Prepare
Pattern Parse
Pattern Validate
MIDI Parse
Pattern Compile
Tick → Frame conversion
Parameter Resolve
Event Sort
Tempo Map Build
Scratch allocation
```

## 32.1 Scheduler Scratch

Scheduled Feedが必要とするScratchは最大Event数から事前確保する。

## 32.2 Existing Allocation Test

現行Audio Callback allocation testをLiveとScheduled両Event Feedで通す。

少なくとも次をTestする。

```text
No-event scheduled block
Multiple scheduled note block
Tempo boundary
Loop boundary
```

---

# 33. Audio Device契約

既存Realtime PerformanceのAudio Device Selectionをそのまま使う。

新しいDevice abstractionを作らない。

`audition`も次を使う。

```text
Default output when omitted
Opaque device ID
Requested sample rate
Requested buffer size
All supported PCM formats
Stereo Core output
Extra device channels silence
```

MIDI Device Selectionは不要。

`audition`実行時にMIDI Input Deviceが0件でも正常に動くこと。

---

# 34. LatencyとTail

## 34.1 Realtime Latency

Realtime AuditionではOfflineのようにRendered Audioを後からLatency Correctionできない。

`reported_latency_frames`分の実音遅延はそのままAudio Deviceへ出る。

これはRealtime Performanceと同じである。

## 34.2 One-shot停止時刻

Patternの最終Musical FrameだけでStreamを止めると、Engine LatencyやRelease / Reverb Tailが切れる。

One-shotの処理終了Absolute Frameを次とする。

```text
one_shot_duration_frames
+ tail_frames
+ reported_latency_frames
```

Checked Addを使う。

## 34.3 Loop

Loop中はTailという終了概念を使わない。

停止時はUser StopでSessionをDropする。

---

# 35. CLI契約

Top-level構成：

```text
instrument
pattern
  ├─ init
  ├─ validate
  ├─ inspect
  ├─ import-midi
  └─ export-midi
render
  ├─ note
  ├─ events
  ├─ midi
  └─ pattern
audition
  ├─ pattern
  └─ midi
device
play
dev
```

## 35.1 `pattern init`

```bash
sonalloy pattern init pattern.json
```

Destinationが存在する場合は上書きしない。

生成内容：

```text
schema_version = 1
name = null
480 ticks_per_beat
1 bar / 4/4
120 BPM
C4 quarter note 1つ
```

すぐ`audition`可能なValid Patternを生成する。

## 35.2 `pattern validate`

```bash
sonalloy pattern validate pattern.json
sonalloy pattern validate pattern.json --json
```

Structural Validationのみ。

## 35.3 `pattern inspect`

```bash
sonalloy pattern inspect pattern.json
sonalloy pattern inspect pattern.json --json
```

表示項目：

```text
Name
Schema Version
Ticks Per Beat
Length Ticks
Tempo Change Count
Time Signature Change Count
Note Count
Note Range
Velocity Range
Sustain Event Count
Pitch Bend Event Count
Mod Wheel Event Count
Aftertouch Event Count
Parameter Change Count
Distinct Parameter IDs
Musical duration seconds
```

Musical duration secondsはTempo Timelineから計算し、Sample Rateに依存しない。

## 35.4 `pattern import-midi`

```bash
sonalloy pattern import-midi input.mid \
  --output output.json \
  [--channel 1..16] \
  [--json]
```

Output存在時は上書きしない。

## 35.5 `pattern export-midi`

```bash
sonalloy pattern export-midi input.json \
  --output output.mid \
  [--channel 1..16] \
  [--json]
```

Output存在時は上書きしない。

## 35.6 `render pattern`

既存Render Commandと同じNaming / Exit Code / JSON Report方針を使う。

## 35.7 `audition pattern`

```bash
sonalloy audition pattern instrument.json pattern.json \
  [--audio-device id] \
  [--sample-rate hz] \
  [--buffer-size frames] \
  [--tail seconds] \
  [--loop]
```

## 35.8 `audition midi`

```bash
sonalloy audition midi instrument.json file.mid \
  [--channel 1..16] \
  [--audio-device id] \
  [--sample-rate hz] \
  [--buffer-size frames] \
  [--tail seconds]
```

`--loop`は持たない。

---

# 36. Diagnostic / Error契約

既存`DiagnosticCode`を優先して使う。

新しいPattern専用DiagnosticCodeは、既存Codeでは利用者が原因を判別できない場合だけ追加する。

原則：

| Condition | Code |
|---|---|
| Pattern JSON parse | `JSON_INVALID` |
| Schema / Tick / Note / Value | `VALUE_OUT_OF_RANGE` または既存適切Code |
| Parameter ID missing | `PARAMETER_NOT_FOUND` |
| MIDI parse / export | `MIDI_ERROR` |
| Audio device | `AUDIO_DEVICE_ERROR` |
| Core process | `PROCESS_ERROR` |
| Output WAV | `WAV_OUTPUT_ERROR` |

## 36.1 Path

Pattern Validation Errorは可能な限り、

```text
tempo_changes[1].tick
events[4].duration_ticks
events[8].value
```

のPathを持つ。

## 36.2 MIDI Export Loss

表現不能な`ParameterChange`はWarningではなくError。

同音程の時間的なNote重複も、Standard MIDIでNote IDを保持できないためErrorとする。

Silent Data Lossを禁止する。

## 36.3 Multi-channel Import

複数Note Channelを自動MergeせずError。

利用可能ChannelをDetailへ表示する。

---

# 37. 外部依存

新規Dependencyを追加しない。

現行CLIには既に、

```text
midly = 0.5.3
cpal = 0.18.1
crossbeam-queue = 0.3.13
serde
serde_json
```

がある。

MIDI Writerは`midly`の既存機能を使う。

Pattern JSONは`serde` / `serde_json`。

Realtimeは既存CPAL。

新規Sequencer / MIDI / DAW Crateを導入しない。

---

# 38. File単位の変更計画

## 38.1 新規

### `crates/sonalloy-cli/src/pattern.rs`

責務：

```text
Pattern serde model
Pattern file load/write
Structural validation
Inspect data
Pattern → CompiledPattern
MIDI Import destination conversion
MIDI Export source conversion support
```

巨大化した場合のみ、実際に責務が分かれた時点で`pattern/` directoryへ分割する。最初から細かく分割しない。

### `crates/sonalloy-cli/src/musical_time.rs`

責務：

```text
Tick / Tempo → Frame conversion
Musical duration calculation
Overflow / finite validation helper
```

Pattern / MIDIで共有する。

### `crates/sonalloy-cli/src/realtime/scheduled.rs`

責務：

```text
Scheduled Event Feed
Block event collection
Tempo boundary
Loop boundary
Loop note-id remap
One-shot completion
```

CPAL Stream構築は`audio.rs`側を使う。

## 38.2 変更

### `crates/sonalloy-cli/src/main.rs`

- `mod pattern`
- `mod musical_time`
- CLI Args / Subcommand
- Dispatch
- Render共通Helper必要分

Pattern実装本体を`main.rs`へ書かない。

### `crates/sonalloy-cli/src/midi.rs`

- ParseをTick-domain `ParsedMidi`へ分離
- Time Signature抽出
- Pattern Import向けChannel Filter / Note Pairing support
- Existing `read_midi` behavior維持
- MIDI Writer supportは責務が収まるなら同File、肥大化するなら`midi/write.rs`等へ分割

既存32KBの`midi.rs`へ大量追加して可読性を壊す場合は、次の自然な分割を許可する。

```text
midi/
├─ mod.rs
├─ parse.rs
├─ render.rs
└─ write.rs
```

ただし単なる行数削減目的で分割せず、Parse / Render Conversion / Writeの責務境界で分ける。

### `crates/sonalloy-cli/src/midi_common.rs`

追加候補：

```text
denormalize_control
denormalize_pitch_bend
tempo BPM ↔ microseconds conversion
MIDI channel validation helper
```

MIDI固有変換だけ置く。

### `crates/sonalloy-cli/src/realtime/audio.rs`

- Event FeedをLive / Scheduledへ分離
- Existing PCM conversion維持
- Existing Status / Xrun / Fatal維持
- Scheduled block boundariesをFeedから受ける
- Scheduled allocation test追加

### `crates/sonalloy-cli/src/realtime/mod.rs`

- Existing `play`維持
- Audition Session開始処理
- One-shot completion wait
- Loop stop handling
- Shared Audio Device Option validation

必要なら`audition`実装を別private fileへ出す。

### `crates/sonalloy-cli/tests/cli.rs`

新CLI Integration Test。

### `docs/CONCEPT.md`

永続的な責務として次を自然に反映する。

- Instrument単体のAudition PatternはSonalloy範囲
- Arrangement / Recording / Mixing / Multi-instrument compositionはHost範囲
- MIDI Import / ExportはInterchange

「今回追加した」等の差分説明を書かない。

### `README.md`

- Pattern概要
- CLI Examples
- MIDI KeyboardなしAudition
- Riffra / DAWとの責務説明を必要最小限

### `.agents/skills/create-instrument/SKILL.md`

AI Instrument Authoring時に、単音だけでなく用途に合うPatternで試奏できることを記載する。

例：

```text
Bass → short bass phrase
Pad → chord
Lead → phrase + bend
Drum Kit → drum pattern
```

SkillがPatternを作る場合も1 Instrument試奏の範囲を超えない。

### `docs/pattern.md`

新規公開仕様。

内容：

```text
Purpose
Schema
Musical time
Event kinds
Validation
Same-tick ordering
MIDI portability
ParameterChange limitation
Loop semantics
Examples
```

---

# 39. Pattern Unit Test

テストは重複を避け、観点単位で最小にする。

## 39.1 Parse / Schema

- Valid minimal Pattern
- Unsupported schema_version
- Unknown top-level field
- Unknown event field
- Unknown event type

## 39.2 Timing

- ticks_per_beat 0 reject
- ticks_per_beat 32767 accept
- length 0 reject
- tempo first tick != 0 reject
- tempo duplicate / descending reject
- time signature first tick != 0 reject
- invalid denominator reject

## 39.3 Note

- note range
- velocity 0 reject
- duration 0 reject
- end > pattern reject
- end == pattern accept
- overlapping same pitch accept
- chord same tick accept
- MIDI Exportでoverlapping same pitchをreject

## 39.4 Control

- pitch bend limits
- mod wheel limits
- aftertouch limits
- control at length_ticks accept
- non-finite reject

## 39.5 Parameter

- empty ID reject
- non-finite native value reject
- valid descriptor resolves
- missing parameter diagnostic
- native range error

## 39.6 Compile Timing

同じPatternを、

```text
44.1 kHz
48 kHz
96 kHz
```

でCompileし、Musical Tickは同じだがFrame位置が正しく変わること。

Tempo Changeを跨ぐNote Eventも確認する。

---

# 40. MIDI Unit Test

## 40.1 Parser regression

既存MIDI Testは維持する。

- Note
- Tempo
- Pitch Bend asymmetric center
- CC1
- Sustain
- Aftertouch
- Multiple channels warning for existing render
- Multiple channels control conflict warning / no-warning cases
- Zero length note
- Different ticks that collapse to one render frame

## 40.2 Import

- Single channel auto-select
- Multiple channel requires `--channel`
- Explicit channel
- Same channel across multiple tracks merge
- Wrong channel error
- Note overlap FIFO pairing
- unmatched NoteOff warning
- unmatched NoteOn error
- Tempo preservation
- Time Signature preservation
- default 120 BPM / 4/4

## 40.3 Export

- SingleTrack header
- ticks_per_beat preserved
- EndOfTrack at length_ticks
- Note pair output
- Velocity
- CC1
- CC64
- Aftertouch
- Pitch Bend endpoints / center
- Tempo
- Time Signature
- ParameterChange reject
- Overlapping same-pitch notes reject

## 40.4 Round Trip

同じ音程のNoteが時間的に重ならず、`ParameterChange`を含まないPatternを使い、Pattern → MIDI → Patternで最低限次を比較する。

Exact：

```text
TPQ
Length
Note start
Note duration
Note number
Velocity
Time signature
Sustain
```

Tolerance：

```text
Tempo
Pitch bend
Mod wheel
Aftertouch
```

Source JSON array orderの一致は要求しない。

Canonical Musical Meaningを比較する。

---

# 41. Scheduled Realtime Unit Test

Audio DeviceなしでSchedulerを直接Testできる設計にする。

## 41.1 Basic Event Block

```text
block start 1000
frames 256
event 1127
```

→ `sample_offset = 127`

## 41.2 Boundaries

Event：

- Block Start
- Block End - 1
- Exactly Block End

Exactly Endは次Block offset 0。

## 41.3 Tempo

Tempo Changeの1 Frame前 / exact boundary / afterを確認する。

ProcessBlockがTempo Changeを跨がない。

## 41.4 Host Callback Split

Host callback sizeを、

```text
64
128
255
256
511
1024
```

で模擬し、Core max blockとTempo / Loop boundaryで正しく分割されること。

## 41.5 Loop

- length boundary exact
- End NoteOff + Start NoteOn
- Sustain Up at End + Note at Start
- IterationごとNote IDが異なる
- Runtime resetを要求しない
- Tempo resets at loop start

## 41.6 One-shot Completion

Pattern end + tail + latencyでCompleteになる。

それ以前にCompleteにならない。

## 41.7 Allocation

Scheduled Feedを含むAudio Callback Allocation Count = 0。

---

# 42. CLI / Integration Test

Deviceを必要としないCommandをCIでTestする。

## 42.1 Pattern Init

```text
pattern init
→ file created
→ pattern validate success
```

既存Destinationは失敗。

## 42.2 Pattern Inspect

Text / JSONの主要Fieldを確認。

## 42.3 MIDI Import / Export

Temporary MIDI Fixtureを使う。

Output existence policyも確認。

## 42.4 Render Pattern

Minimal Oscillator Instrument + PatternからWAVを生成し、

- Sample Rate
- Stereo
- Non-silent
- Expected approximate frame length

を確認する。

## 42.5 Existing Regression

以下を壊さない。

```text
render note
render events
render midi
play argument parsing
device list serialization
```

## 42.6 Audition CLI Parse

CIにAudio Deviceがなくても、Clap Parse / Invalid ArgsはTestする。

Realtime audio execution自体はPure Scheduler Unit TestとHuman Reviewで確認する。

---

# 43. Human Review

このフェーズでは物理MIDI Keyboardを必須にしない。

Audio Output Deviceのみで実行できること自体が重要である。

## 43.1 必須Review

WindowsまたはLinuxの少なくとも1環境で、Release Buildを使う。

### Single Note

```text
C4を短く鳴らす
```

- Start timingに異常なし
- 終了後Tailが切れない

### Chord

同Tick 3 Note。

- Chordとして同時に聞こえる
- Stuck Noteなし

### Velocity

同じNoteを異なるVelocityで鳴らす。

- InstrumentがVelocity対応なら差を確認

### Sustain

- Sustain Down
- Note
- Note release
- Sustain Up

Sustain意味を確認。

### Drum Pattern

Key mapped Drum Kitまたは複数Keyで反応するInstrumentを使い、Kick / Snare / Hat相当のPatternをLoopする。

- Loopに音切れGapなし
- Event欠落なし
- Stuck Voiceなし

### Loop Tail

Release / Reverbを持つInstrumentでLoopする。

- Loop境界でRuntime Resetされたような不自然なTail切断がない

### MIDI Interchange

- PatternをMIDI Export
- 再Import
- Audition

Musical timingが同等であること。

## 43.2 Realtime Status

Audition終了時も現行Realtimeと同様に可能な範囲で、

```text
XRuns
Realtime priority warning
Callback frame min / max / count
```

を表示する。

Xrun 0を目標とする。

---

# 44. Documentation / Agent Skill

## 44.1 `docs/CONCEPT.md`

責務を次の意味で明文化する。

> SonalloyはInstrument単体の設計・演奏処理と、そのInstrumentを評価するためのAudition Patternを扱う。複数Instrumentを組み合わせるArrangement、Recording、MixingはHost / DAWの責務とする。

Patternの具体SchemaをConceptへ大量記載しない。

## 44.2 `docs/pattern.md`

Patternの正本仕様にする。

## 44.3 README

利用者が最初に理解できる最低限のWorkflowを載せる。

```text
instrument init / create
pattern init / edit
audition
render
export-midi
```

## 44.4 AI Skill

Instrumentを作ったAgentが音を確認するとき、必要に応じてPatternも生成し、用途に合う演奏条件で評価するWorkflowを記載する。

ただしAgent Skillへ「曲を作る」責務を追加しない。

---

# 45. 実装順序

実装Agentは原則次の順で進める。

## 45.1 Pattern modelとStructural Validation

1. `pattern.rs`追加
2. Schema型
3. Parse / Serialize
4. Validation
5. `pattern init`
6. `pattern validate`
7. `pattern inspect`
8. Unit Test

この時点ではAudio / MIDIへ接続しない。

## 45.2 Musical Time共通化

1. `musical_time.rs`
2. Tick → Frame converter
3. Musical duration
4. Tempo timeline Test
5. 既存MIDIのTick→Frame計算を共通Helperへ寄せる
6. Existing MIDI regression Test

既存MIDI挙動が変わっていないことを先に確定する。

## 45.3 Pattern Compile

1. Note展開
2. Note ID assignment
3. Parameter resolve
4. Tick→Frame
5. Canonical sort
6. TempoMap
7. length_frames / one-shot duration
8. 44.1 / 48 / 96 kHz Test

## 45.4 Offline Render

1. `render pattern` CLI
2. Existing render common Helper抽出
3. Analyze / Trace接続
4. Integration Test

ここでPatternから実際のWAVが作れる状態にする。

## 45.5 MIDI Parser再構成

1. `ParsedMidi`
2. ParseとFrame conversion分離
3. Time Signature support
4. Existing `read_midi`をFacade化
5. 全Existing MIDI Test Green

既存`render midi`を壊した状態でImport / Exportへ進まない。

## 45.6 MIDI Import

1. Channel detection
2. Explicit channel
3. Note pairing
4. Control conversion
5. Tempo / Time Signature
6. Pattern serialization
7. Unit / CLI Test

## 45.7 MIDI Export

1. Pattern portability check
2. Event expansion / canonicalization
3. MIDI value denormalization
4. Delta conversion
5. SingleTrack write
6. Round Trip Test

## 45.8 Scheduled Realtime

1. Existing AudioEngineのEvent Feed責務分離
2. Live behavior regression
3. ScheduledEventFeed
4. sample_offset
5. Tempo boundary
6. One-shot completion
7. Loop boundary
8. Note ID remap
9. Allocation Test

## 45.9 Audition CLI

1. `audition pattern`
2. `audition midi`
3. Audio Device options
4. One-shot auto stop
5. Pattern loop stop
6. Status output

## 45.10 Documentation / Review

1. `docs/pattern.md`
2. `docs/CONCEPT.md`
3. README
4. create-instrument Skill
5. Examples
6. Full CI
7. Human Review
8. Self Review

---

# 46. 完了条件

次をすべて満たしたら完了とする。

## 46.1 Pattern

- Pattern Schema v1が文書化されている
- Unknown Fieldを拒否する
- Note / Chord / Drum Patternを記述できる
- Sustain / Pitch / Mod Wheel / Aftertouchを記述できる
- Sonalloy Parameter Changeを記述できる
- Tempo / Time Signature Changeを保持できる

## 46.2 Offline

- `render pattern`でWAVを生成できる
- 44.1 / 48 / 96 kHzでMusical Timingが正しい
- Existing Analyze / Traceが利用できる

## 46.3 Realtime

- MIDI Input Deviceなしで`audition pattern`できる
- Scheduled EventがBlock内sample_offsetへ正しく入る
- Tempo Changeを跨がない
- `--loop`で音声StateをResetせず繰り返せる
- Loop跨ぎNote ID Collisionがない
- Callback Allocation 0

## 46.4 MIDI

- MIDI → Pattern
- Pattern → MIDI
- Single Note Channelの意味が明確
- Multiple Channelを意図せずMergeしない
- Note timing / duration / velocityをRound Tripできる
- 同音程の時間的なOverlapをMIDI Exportで拒否する
- Control量子化が仕様内
- ParameterChangeをSilent Lossしない

## 46.5 境界

以下がSonalloyへ追加されていない。

```text
Track
Clip
Arrangement
Mixer
Recording
Multi-instrument composition
```

## 46.6 Quality

最低限実行：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

既存Native Test / Sanitizer / Fault Injection WorkflowもGreen。

デッドコード、不要`allow`、使われない将来抽象化を残さない。

---

# 47. 将来へ残すもの

今回完了後も次は別責務として残す。

## 47.1 Riffra Native Integration

```text
Riffra Timeline
      ↓
Sonalloy ProcessEvent
      ↓
InstrumentRuntime
```

MIDI Fileを必須中間形式にしない。

## 47.2 Riffra側Pattern Import

必要になればRiffraは、

- Standard MIDI
- Sonalloy Pattern JSON

のどちらかをImportできる。

Sonalloy側では今回Riffra dependencyを追加しない。

## 47.3 Rich Editor

Piano Roll、Step Sequencer、複数Track EditorはRiffra側で検討する。

Sonalloy CLIではJSON / AI Authoringを第一級とする。

## 47.4 Advanced Performance

別途、

```text
Monophonic
Legato
Portamento
MPE
Per-note expression
```

等を扱う。

## 47.5 External Sync

MIDI Clock、Host Transport、Beat Position等はRiffra / Plugin Integrationと合わせて検討する。

---

# 48. 実装Agent向け最終ルール

1. **目的は1 Instrumentを十分に試奏できること。DAWを作らない。**
2. PatternはMusical Tickを正本とし、FrameをFile Formatへ露出しない。
3. Pattern利用者へNote IDを管理させない。
4. CoreへPattern / Track / Bar / Beatモデルを持ち込まない。
5. Patternは既存`ScheduledEvent` / `TempoMap`へCompileする。
6. OfflineとScheduled Realtimeで同じCompile結果を使う。
7. Scheduled Realtimeでは未来EventをSample-accurateな`sample_offset`へ置く。
8. Tempo Change / Loop Boundaryを跨ぐ`ProcessBlock`を作らない。
9. Loop境界で`InstrumentRuntime::reset()`しない。
10. LoopごとにNote IDを一意化する。
11. Live MIDIのTimestamp / Sequence意味を変更しない。
12. Existing `render midi`のMulti-channel Merge挙動を今回勝手に変更しない。
13. Pattern ImportではSingle Instrument境界のため複数Note Channelを自動Mergeしない。
14. MIDIで表現不能なParameterChangeを黙って捨てない。
15. Pattern MIDI ExportへCustom SysEx等の独自拡張を入れない。
16. `midly`、CPAL、Serde等の現行Dependencyで実装し、新規Dependencyを追加しない。
17. Audio Callback内でHeap Allocation、File I/O、JSON Parse、Parameter Resolveを行わない。
18. Pattern / MIDI / Realtime追加を理由に`main.rs`へ処理を集中させない。
19. 同一処理をPattern / MIDIへ二重実装せずMusical Time等の正本を一つにする。
20. 将来使うかもしれないだけのTransport / Sequencer Frameworkを追加しない。
21. Public APIは必要最小限にする。
22. `#[allow(dead_code)]`で未使用設計を残さない。
23. Documentationは差分説明ではなく永続的な製品責務として更新する。
24. 実装後は「Patternが便利か」だけでなく「Riffraとの境界が壊れていないか」を自己レビューする。
25. 最終的な判断基準は、**Instrumentを作る人がMIDI Keyboardなしでも実際の演奏条件で音を判断でき、その演奏情報を外へ持ち出せるか**である。

---

## 参考：完成時の典型Workflow

### Bass Instrument

```bash
# Instrumentを作る
sonalloy instrument validate bass.json

# 試奏Phraseを作る / AIに書かせる
sonalloy pattern validate bass-phrase.json

# その場で聴く
sonalloy audition pattern bass.json bass-phrase.json --loop

# WAVでも確認
sonalloy render pattern bass.json bass-phrase.json \
  --output bass-preview.wav

# Riffraへ持っていく
sonalloy pattern export-midi bass-phrase.json \
  --output bass-phrase.mid
```

### Drum Kit

```bash
sonalloy audition pattern drum-kit.json drum-groove.json --loop

sonalloy pattern export-midi drum-groove.json \
  --channel 10 \
  --output drum-groove.mid
```

### Riffra / DAWから持ち込む

```bash
sonalloy pattern import-midi phrase.mid \
  --channel 2 \
  --output phrase.json

sonalloy audition pattern lead.json phrase.json
```

このWorkflowが成立し、Sonalloy側にTrack / Arrangement / Mixerを追加せず完結していることを最終確認する。
