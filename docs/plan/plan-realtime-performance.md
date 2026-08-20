# Sonalloy Realtime Performance 詳細設計・実装計画

- **対象Repository**：`endo-ly/sonalloy`
- **基準Commit**：`9bb8671a03d170929c03cf202d2863e5d4f84579`
- **基準Main確認日時**：2026-08-19 JST
- **正本要件**：`docs/CONCEPT.md`
- **現行Definition Schema**：`schema_version = 2`
- **前提実装**：Physical / Modal Synthesis Expansionまでを含む現在の`main`
- **恒久名称**：`Realtime Performance`
- **用途**：実装Agentへそのまま渡し、設計判断を追加で発生させず実装を進めるための詳細計画
- **文書言語**：日本語。型名、API名、File Path、Command、MIDI / Audio固有名称だけ英語表記を使用する
- **成果物**：Markdownのみ

---

## 目次

1. この計画の位置づけ
2. 最新Mainの実装断面
3. 今回の到達点
4. 対象範囲と境界
5. 全体アーキテクチャ
6. 外部依存の選定
7. CLI契約
8. Device識別と列挙
9. Audio Config選択
10. Compile / Prepare順序
11. Realtime Sessionの所有関係
12. Audio Callback処理
13. Device Channel / Sample Format変換
14. Host CallbackとCore Blockの分割
15. Realtime Event Queue
16. Live MIDI Adapter
17. Note ID管理
18. Sustain Pedalの共通Event契約
19. SustainとVoice Lifecycle
20. SustainとVoice Stealing
21. Offline MIDI / Event Sequenceとの統合
22. Realtime SafetyとMemory契約
23. Error / Diagnostic / Stream Status
24. Latencyの扱い
25. 決定性と既存Offline機能の回帰
26. Platform / Build / CI / Release
27. Documentation / Agent Skill / License
28. Core Unit Test
29. Realtime Adapter Unit Test
30. CLI / Integration Test
31. Realtime Human Review
32. File単位の変更計画
33. 実装順序
34. 完了条件
35. 次フェーズへ残すもの
36. 実装Agent向け最終ルール
37. 参考資料

---

# 1. この計画の位置づけ

現在のSonalloyは、主要なGenerator、Processor、Dynamic Parameter / Modulation、Sample / Granular / Spectral、Physical / Modalまで実装され、Instrument DefinitionをCompileして決定的にOffline Renderできる状態にある。一方、製品コンセプトの完成像は、同じInstrument RuntimeをCLI、Riffra、CLAP、VST3等から共通Process Contractで利用し、実際に演奏可能なInstrumentとして扱える状態である。

今回のRealtime Performanceは、現在の「Offline Render中心の利用形態」から、物理MIDI DeviceとAudio Output Deviceを使ってInstrumentをリアルタイム演奏できる状態へ進める。

本フェーズの中心は次の二点である。

1. **既存Core RuntimeをそのままRealtime Audio Callbackから利用するAdapterを成立させる**
2. **現在Coreに欠けているSustain Pedalを共通Eventとして追加し、OfflineとRealtimeで同じ演奏意味を持たせる**

今回、Realtimeを理由にSonalloy CoreへOS固有Audio APIを取り込まない。Audio DeviceとMIDI DeviceはCLI Adapterが所有し、Coreは今までどおりProcess ContractとInstrument Runtimeだけを所有する。

本フェーズ完了時の状態を次の一文で固定する。

> **Sonalloyは、同一のInstrument DefinitionをOffline RenderとRealtime Performanceの両方で利用でき、物理MIDI鍵盤からNote、Pitch Bend、Mod Wheel、Channel Aftertouch、Sustain Pedalを入力し、Audio Deviceへ継続的にStereo出力できる。**

## 1.1 実装判断の優先順位

判断が衝突した場合は次の順序を使う。

1. `docs/CONCEPT.md`
2. 本計画で固定するRealtime Event / Audio Adapter / MIDI Adapter契約
3. 現行`ProcessSpec` / `ProcessBlock` / `InstrumentRuntime`の意味
4. Audio CallbackのRealtime Safety
5. Offline / Realtimeで同じ演奏意味を持つこと
6. Windows / Linuxでの実用性
7. 実装の単純さと保守性
8. 将来のRiffra / Plugin Adapter

将来のPlugin、External Audio Input、MPEを理由に、現在使用しない汎用Host Framework、Audio Graph、Device Abstraction Crate、Plugin Parameter Adapterを導入しない。

## 1.2 今回の設計原則

Realtime DeviceはCoreの外側に置く。

```text
Physical MIDI Device
        │
        ▼
sonalloy-cli / MIDI Input Adapter
        │
        ▼
ProcessEventKind
        │
        ├─────────────┐
        │             │
        ▼             │
Bounded Event Queue   │
        │             │
        ▼             │
Audio Callback        │
        │             │
        ▼             │
ProcessBlock ─────────┘
        │
        ▼
InstrumentRuntime
        │
        ▼
Planar Stereo f32
        │
        ▼
Audio Output Adapter
        │
        ▼
Physical Audio Device
```

CoreからCPAL、Midir、WASAPI、ALSA、CoreAudio、WinMM等を参照しない。

---

# 2. 最新Mainの実装断面

基準Commit `9bb8671a03d170929c03cf202d2863e5d4f84579` は、Physical String / Modal GeneratorをMainへ統合したPR #10のMerge Commitである。本計画はこのCommitを起点とする。

## 2.1 Workspace

現在のWorkspaceは三Crateである。

```text
sonalloy-cli
    ↓
sonalloy-core
    ↓
sonalloy-dsp-sys
    ↓
Native C ABI / DaisySP / Signalsmith
```

Realtime Performanceでもこの三Crate構成を維持する。

新しい`sonalloy-realtime` Crateは追加しない。

理由は、今回追加するAudio / MIDI Device処理がCLI Frontend固有の責務であり、RiffraやPluginがその実装を再利用する構造ではないためである。RiffraはJUCE Audio Callback、PluginはHost Callbackをそれぞれ所有し、共通化すべき対象はCoreのProcess Contractである。

## 2.2 現行Process Contract

現在の`ProcessSpec`は次を持つ。

```rust
pub struct ProcessSpec {
    pub sample_rate: f64,
    pub max_block_size: usize,
    pub output_channels: usize,
}
```

`output_channels`は現在Stereo固定であり、`2`以外を拒否する。

`ProcessContext`は次である。

```rust
pub struct ProcessContext {
    pub absolute_frame: u64,
    pub tempo_bpm: f64,
}
```

`ProcessBlock`は次である。

```rust
pub struct ProcessBlock<'a> {
    pub frames: usize,
    pub context: ProcessContext,
    pub events: &'a [ProcessEvent],
    pub output: &'a mut [&'a mut [f32]],
}
```

現行Coreには既にRealtime Adapterで必要となる次の性質がある。

- ProcessごとのFrame数は可変
- `frames <= max_block_size`
- EventはBlock内`sample_offset`を持つ
- `sample_offset`の昇順をCore側で検証し、同一Offsetは入力順で処理する
- `absolute_frame`の連続性をRuntimeが検証する
- OutputはPlanar Stereo `f32`
- `prepare()`でVoice / Scratch / DSP Stateを確保する
- Process中Allocationなしを複数のGenerator / ProcessorでTestしている

したがって、Realtime Performanceのために新しいAudio Engineを作らない。

## 2.3 現行Event

`ProcessEventKind`は次を持つ。

```text
NoteOn
NoteOff
ParameterChange
PitchBend
ModWheel
Aftertouch
SustainPedal
```

Offline Adapterが同一Offsetを正規化する優先順位は次である。

```text
SustainPedal
NoteOff
ParameterChange
PitchBend
ModWheel
Aftertouch
NoteOn
```

Coreは同一OffsetのEventを入力された順番で処理し、Offline Adapterだけがこの順序へ正規化する。

## 2.4 現行Performance Definition

現在の`PerformanceDefinition`は次だけを持つ。

```rust
pub struct PerformanceDefinition {
    pub polyphony: u16,
    pub voice_stealing: VoiceStealingDefinition,
}
```

Monophonic / Legato / PortamentoはまだDefinitionへ存在しない。

今回のSustain Pedalは外部演奏Eventであり、静的なInstrument設定ではない。そのためDefinition Schemaを変更せず、`schema_version = 2`を維持する。

## 2.5 現行Voice Runtime

現在のVoice Stateは次である。

```text
Idle
Active
Releasing
StealFading
```

VoiceはNote ID、Note Number、Velocity、Layer Runtime、Processor、Modulation Source、Pending Note、Steal Fade Stateを所有する。

Note Offを受けると、SustainがDownかどうかに応じて次を行う。

```text
Sustain Up
  → Generator Note Off
  → Layer ADSR Release

Armed note_off Layer
  → 発音開始

Modulation Envelope
  → Release

Voice State
  → Releasing

Sustain Down
  → Key状態だけを解除
  → VoiceとLayerを保持
```

SustainはこのRelease開始を延期する機能として統合する。

## 2.6 現行CLI

Top-level Commandは次である。

```text
instrument
render
device
play
dev
```

CLI Sourceは主に次のFileで構成する。

```text
crates/sonalloy-cli/src/main.rs
crates/sonalloy-cli/src/midi.rs
crates/sonalloy-cli/src/realtime/
```

`main.rs`にはCommand、Inspect、Render、JSON Report等が集約されている。Realtime Device処理まで`main.rs`へ直接追加せず、明確な責務を持つModuleへ分ける。

## 2.7 現行MIDI File Adapter

`midi.rs`はStandard MIDI Fileから次を変換する。

```text
Note On / Off
Pitch Bend
CC1 Mod Wheel
Channel Aftertouch
CC64 Sustain Pedal
Tempo
```

また同一Noteの重複発音を正しく扱うため、

```rust
HashMap<(channel, note), VecDeque<(note_id, frame)>>
```

でNote IDを管理している。この意味をLive MIDIでも維持する。

## 2.8 現行CLI Test

`crates/sonalloy-cli/tests/cli.rs`にCLI結合Testが集約されている。Realtime Deviceが存在することを前提としたCI Testは作らず、Device非依存のAdapter処理をUnit Testし、CLI Integration TestでSustain / Command Parse等を確認する。

---

# 3. 今回の到達点

## 3.1 利用者から見た完成像

最低限、次が成立する。

```bash
sonalloy device list
sonalloy play my-instrument.json --midi-device <id>
```

Audio Deviceを省略した場合はOSのDefault Output Deviceを使う。

複数のMIDI Inputが存在する環境では、`--midi-device`でStable IDを指定する。

演奏中は次が共通Core Eventとして反映される。

| MIDI入力 | Sonalloy Event |
|---|---|
| Note On | `NoteOn` |
| Note Off / NoteOn velocity 0 | `NoteOff` |
| Pitch Bend | `PitchBend` |
| CC1 | `ModWheel` |
| Channel Aftertouch | `Aftertouch` |
| CC64 | `SustainPedal` |

## 3.2 Offlineとの一貫性

SustainをRealtimeだけのCLI処理にしない。

次の三経路は最終的に同じ`ProcessEventKind::SustainPedal`へ到達する。

```text
render events JSON ─┐
                    ├─> ProcessEventKind::SustainPedal
render midi CC64 ───┤
                    │
play Live MIDI CC64 ┘
```

これにより、同じ演奏Event列をOfflineで再現できる。

## 3.3 製品状態の変化

完了後はREADMEとWorkspace Descriptionの「offline instrument engine」という説明を更新する。

完成後の説明は次の意味へ変更する。

> JSON Instrument Definitionから、Realtime PerformanceとOffline Renderingの両方を行えるHybrid Instrument Engine。

---

# 4. 対象範囲と境界

## 4.1 今回含める

### Core

- `SustainPedal` Process Event
- SustainによるNote Release延期
- Sustain解除時のRelease
- SustainとRelease Trigger Layerの統合
- SustainとVoice Stealing / Pending Noteの統合
- Reset時のSustain State初期化
- Event同一Offset優先順位の更新
- Event SequenceからSustain送信
- MIDI File CC64からSustain送信

### CLI Realtime Adapter

- Audio Output Device列挙
- MIDI Input Device列挙
- Stable Device ID表示 / 指定
- Audio Config選択
- Instrumentを選択Audio Sample Rate / Block SizeでCompile
- Instrument Runtime Prepare
- Audio Output Stream
- MIDI Input Connection
- MIDI ThreadからAudio ThreadへのBounded Queue
- Callback BufferのCore Block分割
- Planar StereoからDevice Interleaved Bufferへの変換
- Deviceが2chより多い場合のStereo配置
- Realtime Error / Xrun / Queue Overflow管理
- Engine Latencyの表示
- Graceful Stop

### Platform / Delivery

- Windows
- Linux
- macOSの既存Build維持
- Linux Build Dependency更新
- CI / Release Workflow更新
- License Notice更新
- CLI / Runtime / Architecture / Testing Document更新

## 4.2 今回残す

次は製品要件に残るが、本フェーズへ混在させない。

```text
Monophonic
Legato
Portamento
MPE
Polyphonic Aftertouch
Macro
MSEG
Step Modulator
Sample & Hold
Smooth Random
Vector
MIDI Clock / Start / Stop / Continue
Time Signature / Beat / Bar / Transport Context拡張
External Audio Input
Envelope Follower
Vocoder
Sidechain
Convolution
CLAP
VST3
Riffra Integration
Public C ABI
Runtime Hot Swap
Device Hot-plug Recovery
ASIO / JACK / PipeWire / PulseAudio Optional Backend
```

## 4.3 `Activate / Deactivate`の扱い

`docs/CONCEPT.md`の将来共通Lifecycleには`Prepare → Activate → Process → Reset/Deactivate`が書かれている。一方、現行`InstrumentProcessor`は`prepare / process / reset`であり、Core内部に「Activate時だけ所有するResource」は現在存在しない。

今回、形だけの`activate()` / `deactivate()`を追加しない。

Realtime CLIでは、

```text
Core prepare
↓
CPAL Stream build（Paused）
↓
MIDI Connection
↓
CPAL Stream play
↓
Core process
↓
Stream / Connection Drop
```

でLifecycleを構成する。

Core Public Lifecycleを拡張する作業は、CLAP / VST3 / Public C ABIの共通Host Contractを設計するフェーズで、実際に必要な状態遷移と合わせて行う。

## 4.4 Input Bufferの扱い

External Audio Inputは今回扱わないため、`ProcessBlock`へInput Bufferを追加しない。

Realtime化とExternal Inputを同時実装すると、Audio Output Device、Audio Input Device、Clock同期、Input Channel Mapping、Vocoder等が一つの変更へ混在する。External Audioは独立フェーズとする。

---

# 5. 全体アーキテクチャ

## 5.1 Crate依存

変更後も依存方向を次に固定する。

```text
sonalloy-cli
  ├─ CPAL
  ├─ Midir
  ├─ Midly
  ├─ Crossbeam Queue
  └─ sonalloy-core
          ↓
     sonalloy-dsp-sys
          ↓
     Native DSP
```

`sonalloy-core`はCPAL / Midirを知らない。

## 5.2 Realtime内部構造

```text
Main Thread
  │
  ├─ Device選択
  ├─ Definition読込 / Compile
  ├─ Runtime Prepare
  ├─ Stream / MIDI Connection生成
  └─ Session状態監視

MIDI Callback Thread
  │
  ├─ Raw MIDI bytes
  ├─ Midly LiveEvent parse
  ├─ Note ID変換
  └─ ArrayQueue.push(QueuedEvent)

Audio Callback Thread
  │
  ├─ CPAL interleaved output
  ├─ Queue drain
  ├─ timestamp / sequence order
  ├─ ProcessBlock
  ├─ Planar Stereo f32
  └─ Device sample formatへ変換
```

Thread間で共有する可変状態を最小にする。

```text
MIDI -> Audio
  ArrayQueue<QueuedEvent>

Audio / CPAL Error -> Main
  Atomic status / counters
```

Audio ThreadとMain Threadの間で`Mutex<InstrumentRuntime>`を共有しない。

---

# 6. 外部依存の選定

## 6.1 採用Dependency

`crates/sonalloy-cli/Cargo.toml`へ次を追加する。

```toml
cpal = { version = "0.18.1", features = ["realtime"] }
midir = "0.11.0"
crossbeam-queue = "0.3.13"
```

既存`midly = "0.5.3"`は維持する。

## 6.2 CPAL

CPALをAudio Device Adapterへ使用する。

採用理由：

- Rustから直接Audio Output Deviceを列挙できる
- Stable Device IDを利用できる
- Linux / Windows / macOSを既存SonalloyのRust CLIから扱える
- 可変Callback Sizeを扱える
- Runtime Sample Formatを取得できる
- SonalloyのMSRV `1.85`とCPAL 0.18.1の主要Backend MSRVが一致する
- `realtime` featureでAudio Callback Threadの優先度昇格を試行できる

今回有効化するfeatureは`realtime`だけとする。

次は有効化しない。

```text
asio
jack
pipewire
pulseaudio
realtime-dbus
```

`realtime-dbus`を使わないことでD-Bus build dependencyを増やさない。

LinuxでRealtime Priority昇格が許可されていない場合、CPALは`RealtimeDenied`をError Callbackへ通知するがStream自体は継続できる。このEventはWarningとして扱い、Sessionを停止しない。

## 6.3 Midir

MidirをRealtime MIDI Device Adapterへ使用する。

採用理由：

- Realtime MIDI Input Callbackを提供する
- Linux / Windows / macOSを扱える
- MIDI Input PortにOpaque Stable IDがある
- Raw MIDI bytesを既存Midlyへ渡せる

Midir側でInstrument Domainへ変換しない。MidirはDevice / Callbackだけを所有する。

## 6.4 Midly

既存Midly 0.5.3をLive MIDI解析にも使用する。

`midly::live::LiveEvent::parse()`はOS APIから来るcomplete MIDI messageを解析できるため、独自MIDI byte parserを追加しない。

Standard MIDI FileとLive MIDIの両方で同じ`MidiMessage`の意味を利用する。

## 6.5 Crossbeam ArrayQueue

MIDI Callback ThreadからAudio Callback ThreadへEventを渡すBounded Queueとして`crossbeam_queue::ArrayQueue`を使用する。

`ArrayQueue`は生成時に固定容量Bufferを確保し、Full時の`push()`が失敗として返る。Realtime経路で「必要になったら容量を増やす」挙動を持たせないために適している。

`SegQueue`は使用しない。

## 6.6 JUCEを今回使わない理由

現行CONCEPTではRiffraがJUCE Audio Callbackを所有する。CLI RealtimeのためだけにJUCEをSonalloy CLIへ追加すると、Rust CLIからC++ Device Lifecycleを経由する大きな境界が増える。

今回の共通資産はCore Process Contractであり、Device実装自体ではない。

```text
CLI     -> CPAL
Riffra  -> JUCE
CLAP    -> Host Process Callback
VST3    -> Host Process Callback
               │
               └─ 全てSonalloy Coreへ変換
```

この構造を維持する。

---

# 7. CLI契約

## 7.1 Command全体

変更後のTop-level Commandを次とする。

```text
sonalloy
├─ instrument
├─ render
├─ device
│   └─ list
├─ play
└─ dev
```

## 7.2 `device list`

```bash
sonalloy device list
sonalloy device list --json
```

表示対象は次である。

```text
Audio Output Devices
MIDI Input Devices
```

Audio Input DeviceはExternal Audioフェーズまで表示対象にしない。

Human-readable例：

```text
Audio outputs
* wasapi:{opaque-id}
  Speakers (USB Audio)
  default: 48000 Hz / 2 ch / f32
  buffer: 64..2048 frames

MIDI inputs
  20:0
  USB MIDI Keyboard
```

ID文字列は例であり、Backend固有文字列の構造をSonalloy仕様として解釈しない。

JSON Reportの概念形を次とする。

```json
{
  "audio_outputs": [
    {
      "id": "...",
      "name": "...",
      "default": true,
      "default_config": {
        "sample_rate": 48000,
        "channels": 2,
        "sample_format": "f32",
        "buffer_size": { "min": 64, "max": 2048 }
      }
    }
  ],
  "midi_inputs": [
    {
      "id": "...",
      "name": "..."
    }
  ]
}
```

`SupportedBufferSize::Unknown`の場合は`buffer_size: null`とする。

## 7.3 `play`

```bash
sonalloy play <definition>
```

Option：

| Option | Default | 内容 |
|---|---:|---|
| `--audio-device <id>` | OS Default | CPAL Stable Device ID |
| `--midi-device <id>` | 条件付き自動選択 | Midir Stable Port ID |
| `--sample-rate <hz>` | Device Default | 指定する場合はDevice対応Rateだけ許可 |
| `--buffer-size <frames>` | 256 | Audio Callbackの要求Frame数 |
| `--tempo <bpm>` | 120 | `ProcessContext.tempo_bpm` |

`--midi-device`省略時の規則：

```text
MIDI Input 0個  -> Error
MIDI Input 1個  -> そのPortを使用
MIDI Input 2個以上 -> Error。device listでID指定を要求
```

複数候補から「最初のDevice」を暗黙選択しない。

Audio Device省略時はOS Default Output Deviceを使用する。これはCPALのDefault Device Roleをそのまま利用する。

## 7.4 Session開始時の表示

Human-readable Modeでは開始前に最低限次を表示する。

```text
Instrument: <name>
Audio: <name> (<id>)
Sample rate: 48000 Hz
Device channels: 2
Sample format: f32
Requested buffer: 256 frames
Callback frames: measured at shutdown
MIDI: <name> (<id>)
Tempo: 120 BPM
Engine latency: <frames> (<ms>)
```

最後に、

```text
Playing. Press Enter to stop.
```

を表示する。

`play`は長時間実行Commandであるため、今回`--json`のStreaming Protocolを新設しない。Machine-readable Device選択には`device list --json`を使用する。

---

# 8. Device識別と列挙

## 8.1 Audio Device ID

Audio DeviceはCPAL `DeviceId`を文字列化した値で識別する。

- IDをIndexへ変換して保存しない
- Display順を識別子として使わない
- User入力IDはOpaque StringとしてParseする
- 指定されたIDが現在存在しなければError

## 8.2 MIDI Port ID

MIDI InputはMidir `MidiInputPort::id()`のOpaque Stringを使う。

- Port一覧のIndexをPublic CLI IDにしない
- `--midi-device`はStable IDを受ける
- Port名は表示用

## 8.3 Device列挙失敗

Device一覧の取得自体が失敗した場合、空配列として成功させずDiagnostic Errorを返す。

Audio Device ErrorとMIDI Device Errorを区別する。

---

# 9. Audio Config選択

## 9.1 基本方針

Audio Device側のSample Rateを先に決定し、そのSample RateでInstrumentをCompileする。

Offline既定48 kHzでCompileしたものをRealtime Device側で再Sample Rate変換する構造は作らない。

## 9.2 Sample Rate未指定

`--sample-rate`がない場合、選択Deviceの`default_output_config()`を基準とする。

Default Configが次を満たせばそのまま使う。

- 2 Channel以上
- PCM Sample Format
- 有効なSample Rate

Default ConfigがMonoまたはDSD等でSonalloy Outputに適さない場合だけSupported Configを探索する。

探索時は次を優先する。

1. 2 Channel
2. `f32`
3. 48 kHz
4. 44.1 kHz
5. 2 Channel以上のPCM Config

ここでの優先順位はDevice選択規則として一箇所に実装する。

## 9.3 Sample Rate指定

`--sample-rate`が指定された場合、そのRateを含むSupported Configだけを候補にする。

候補がなければErrorとする。

指定値を別のRateへ黙って変更しない。

## 9.4 Buffer Size

Default要求値：

```text
256 frames
```

選択Configの`SupportedBufferSize`がRangeなら、要求値が範囲内であることをStream Build前に確認する。

`SupportedBufferSize::Unknown`ならFixed値をCPALへ渡し、BackendがUnsupported Configを返した場合はそのまま明示Errorとする。

小さいBufferを自動的に大きくするFallbackは入れない。

## 9.5 Device Channel

CoreはStereo固定のため、Device Channelが1の場合は起動Errorとする。

Device Channelが2より多い場合は、

```text
Device ch 0 <- Sonalloy Left
Device ch 1 <- Sonalloy Right
Device ch 2..N <- Silence
```

とする。

Surround UpmixやChannel Matrixは今回作らない。

---

# 10. Compile / Prepare順序

現在の`CompileContext`は`ProcessSpec`を受けるため、Device Configより先にCompileできない。

`play`の起動順序を次で固定する。

```text
1. CLI Args Validate
2. Audio Device Resolve
3. Audio Output Config Resolve
4. MIDI Device Resolve
5. ProcessSpec作成
6. Definition読込
7. compile_instrument
8. CompiledInstrument取得
9. instantiate
10. InstrumentRuntime::prepare
11. Realtime Event Queue生成
12. Audio StreamをPaused状態でBuild
13. MIDI Input Connection生成
14. Audio Stream::play
15. Session監視
16. Stop時にConnection / Stream Drop
```

`ProcessSpec`は次とする。

```rust
ProcessSpec::new(
    selected_sample_rate as f64,
    requested_buffer_size as usize,
    2,
)
```

Audio Host Callbackが要求Bufferより大きい場合は、Adapter側で複数Core Blockへ分割するため、Coreの`max_block_size`をHost Callback最大値へ合わせて巨大化しない。

---

# 11. Realtime Sessionの所有関係

## 11.1 Main Thread

Main Threadが所有する。

- Audio Device選択結果
- MIDI Port選択結果
- `CompiledInstrument`の起動前参照
- CPAL Stream Handle
- Midir Connection Handle
- Session Status共有状態
- Stop待機

## 11.2 Audio Callback Closure

Audio Callback Closureが所有する。

```rust
struct RealtimeAudioEngine {
    runtime: InstrumentRuntime,
    events: Arc<ArrayQueue<QueuedEvent>>,
    block_events: Vec<ProcessEvent>,
    left: Vec<f32>,
    right: Vec<f32>,
    max_block_size: usize,
    tempo_bpm: f64,
    device_channels: usize,
    status: Arc<RealtimeStatus>,
}
```

`block_events`、`left`、`right`はStream開始前に必要容量を確保する。

## 11.3 MIDI Callback Data

MIDI Callback Dataが所有する。

```rust
struct LiveMidiState {
    events: Arc<ArrayQueue<QueuedEvent>>,
    active_notes: HashMap<(u8, u8), VecDeque<NoteId>>,
    serials: HashMap<(u8, u8), u32>,
    next_sequence: u64,
    status: Arc<RealtimeStatus>,
}
```

MIDI Callback ThreadでのHashMap / VecDeque AllocationはAudio Callback Safetyの対象外である。ただしNote変換は決定的にする。

## 11.4 Shared Status

概念形：

```rust
struct RealtimeStatus {
    fatal: AtomicU8,
    realtime_denied: AtomicBool,
    xrun_count: AtomicU64,
    callback_count: AtomicU64,
    callback_frames_min: AtomicU64,
    callback_frames_max: AtomicU64,
}
```

Status種別はStringを持たない固定値とする。

Audio Callbackから`String`を生成してMainへ渡さない。

---

# 12. Audio Callback処理

Audio Callbackは次の順で処理する。

```text
CPAL Output Buffer
        │
        ▼
全SampleをSilenceで初期化
        │
        ▼
Fatal Status確認
        │
        ├─ Fatal -> そのままReturn
        │
        ▼
Frame数計算
        │
        ▼
max_block_size以下へChunk分割
        │
        ▼
Queue Drain
        │
        ▼
Event順序確定
        │
        ▼
        InstrumentProcessor::process
        │
        ▼
Left / RightをDevice Sample Formatへ変換
        │
        ▼
Interleaved Device Bufferへ書込
```

## 12.1 最初にSilence

CPAL 0.18はOutput BufferをZero-fillするが、Sonalloy Adapter側でもFault時の出力契約を明確にする。

Callback処理の先頭で対象BufferをSample FormatのEquilibriumへ設定する。

その後、正常にCore処理できたFrameだけ上書きする。

## 12.2 Core Process Error

`InstrumentProcessor::process()`が失敗した場合：

1. 現在Chunk以降はSilenceのまま残す
2. `RealtimeStatus.fatal`を`Process`へ設定
3. Callback内ではLog / Format / Panicを行わない
4. 以後のCallbackもSilenceを返す
5. Main ThreadがFaultを検出してSession終了とDiagnostic表示を行う

Core Process Error後にRuntimeへ繰り返しProcessを試さない。

---

# 13. Device Channel / Sample Format変換

## 13.1 Core内部

Coreは現在どおりPlanar `f32` Stereoを使う。

```text
left[frames]
right[frames]
```

## 13.2 Device側

CPALはDeviceごとにSample Formatを持つ。

Realtime AdapterはPCM Sample Formatを型Dispatchし、`f32`からDevice Sampleへ変換する。

方針：

- `f32`は直接変換
- `f64` / Integer PCMはCPALのSample変換Traitを使う
- 24-bit型を含むCPAL PCM型を対応対象にする
- DSDはUnsupported Configとして拒否

Sample変換式をSonalloy独自に実装しない。

## 13.3 Interleave

Deviceが`C` Channel、Core Chunkが`N` Frameなら：

```text
for frame in 0..N:
    device[frame * C + 0] = left[frame]
    device[frame * C + 1] = right[frame]
    device[frame * C + 2 .. frame * C + C] = silence
```

Channel CountやBuffer Lengthの不整合はRealtime Faultとする。

---

# 14. Host CallbackとCore Blockの分割

CPALの`BufferSize::Fixed(N)`はCallback Sizeの要求であり、実際のCallback Frame数はBackend / Hardwareにより異なる可能性がある。

そのため、Host Callback SizeをCore Block Sizeと同一と仮定しない。

例：

```text
requested core max = 256
host callback = 641 frames

Core calls:
256
256
129
```

## 14.1 Chunk規則

```rust
while callback_frames_remaining > 0 {
    let frames = remaining.min(max_block_size);
    process_core_chunk(frames);
}
```

## 14.2 Absolute Frame

Adapter側に第二のFrame Counterを持たない。

各Core Callで：

```rust
ProcessContext {
    absolute_frame: runtime.absolute_frame(),
    tempo_bpm,
}
```

を使う。

これによりCore自身の連続性検証とFrontendのFrame位置がずれない。

## 14.3 Event Drain

Queueは各Core Chunkの先頭でDrainする。

Callback処理中にMIDI Eventが到着した場合、次の未処理Chunkで反映できる。

Live MIDI TimestampをAudio ClockのSample Offsetへ変換する処理は今回行わないため、DrainしたEventの`sample_offset`は全て`0`とする。ただしMidirの接続起点Microsecond TimestampはQueueへ保持し、`(timestamp_us, sequence)`の順序でTimestampと入力順を維持する。

---

# 15. Realtime Event Queue

## 15.1 Queue型

```rust
const REALTIME_EVENT_QUEUE_CAPACITY: usize = 4096;

struct QueuedEvent {
    timestamp_us: u64,
    sequence: u64,
    kind: ProcessEventKind,
}
```

```rust
Arc<ArrayQueue<QueuedEvent>>
```

をMIDI / Audioで共有する。

## 15.2 Sequence

`timestamp_us`はMidirがMIDI Callbackへ渡す接続起点のMicrosecond Timestamp、`sequence`はMIDI Callback Data内で単調増加する。

Queue自体のFIFO順だけに依存せず、Audio Clockへ変換しないLive Eventの順序を固定するために使用する。

Overflow時はSession Faultとなるため、`u64` Sequence OverflowもFaultとする。

## 15.3 Audio側Event Buffer

Audio Engineは次をPrepare時に確保する。

```rust
Vec::with_capacity(REALTIME_EVENT_QUEUE_CAPACITY)
```

Queue容量以上のEventは存在できないため、正常時に`block_events`がCapacityを拡張しない。

## 15.4 Event Order

QueueからDrainしたEventは全て`sample_offset = 0`である。

`sort_unstable_by_key`等、Heap Allocationを必要としないSortを使い、Keyを次にする。

```text
(timestamp_us, sequence)
```

Timestampが異なるEventはTimestamp順を絶対に維持し、同じTimestampでもMIDI Callbackへ到着した`sequence`の昇順を維持する。Live MIDIでは`kind.priority()`を適用しない。Coreは同じ`sample_offset`のEventをSlice内の順番で適用するため、Realtime Adapterも共通の`InstrumentProcessor::process()`を使用する。

Stable Sort実装が一時Allocationを必要とする可能性を避けるため、Stable Sortへ依存しない。

## 15.5 Queue Full

`ArrayQueue::push()`がFullを返した場合：

```text
Eventを黙って捨てて継続
```

を行わない。

特にNote Offを捨てるとStuck Noteを作るため、Queue FullをFatal Realtime Faultとして扱う。

処理：

```text
push failed
↓
status.fatal = EventQueueOverflow
↓
Audio CallbackはSilenceへ移行
↓
Main ThreadがSession終了
```

`force_push()`は使わない。

---

# 16. Live MIDI Adapter

## 16.1 Parse

Midir Callbackが受け取るRaw bytesを、既存Midlyの次へ渡す。

```rust
midly::live::LiveEvent::parse(bytes)
```

`LiveEvent::Midi`だけをPerformance Eventへ変換する。

System Common / System Realtime Eventは今回無視する。

## 16.2 MIDI Message対応

### Note On

```text
velocity > 0
-> NoteOn
```

### Note On velocity 0

```text
-> NoteOff
```

### Note Off

```text
-> NoteOff
```

### Pitch Bend

既存Offline Adapterと同じNormalized変換を使う。

```text
negative: value / 8192
positive: value / 8191
```

同じ式を二Fileへ複製せず、共通Helperへ置く。

### CC1

```text
value / 127
-> ModWheel
```

### Channel Aftertouch

```text
value / 127
-> Aftertouch
```

### CC64

```text
value >= 64 -> SustainPedal { down: true }
value < 64  -> SustainPedal { down: false }
```

## 16.3 今回無視するMessage

```text
Polyphonic Aftertouch
Program Change
Bank Select
MIDI Clock
Start / Stop / Continue
Song Position
SysEx
その他CC
```

Audio CallbackへWarning文字列を送らない。

## 16.4 MIDI Timestamp

Midir CallbackはMicrosecond Timestampを提供するが、その起点はCPAL Stream Clockと同一と保証されない。

今回、MIDI TimestampをAudio Sample位置へ無理に変換しない。

Realtime Eventは「Audio Threadが次にQueueをDrainしたCore ChunkのOffset 0」で適用する。

Timestampの異なるEventはTimestamp順、同一TimestampのEventはSequence順で適用する。Realtime AdapterはLive MIDIの到着順を保ったEvent列を共通の`InstrumentProcessor::process()`へ渡す。

これをAdapterのScheduling精度としてDocumentへ明記する。

CoreのSample-offset Event能力は変更しない。将来、共通Clock Mappingを実装する場合もCore Contractを変更する必要はない。

---

# 17. Note ID管理

## 17.1 原則

同じNote Numberを重複発音できるため、Note NumberをNote IDとして使わない。

Live MIDIでもFrontendが一意なNote IDを割り当てる。

## 17.2 既存Offline意味の再利用

現在のOffline MIDIは、Channel + NoteごとにSerialを持ち、同じNoteの複数発音をFIFOで対応付ける。

Live MIDIも同じ意味にする。

```text
(channel, note)
      │
      ▼
VecDeque<NoteId>
```

Note On：

```text
新しいNoteIdを生成
Queue末尾へ追加
NoteOn Event
```

Note Off：

```text
Queue先頭を取り出す
NoteOff Event
```

対応するNote OnがないNote OffはEventを生成しない。

## 17.3 Note ID値

既存Offline Adapterと同じ形式を共通Helperへ移せる場合は再利用する。

```text
channel
note
serial
```

から決定的な`u64`を構成する。

Live Session間で同じIDを保証する必要はない。Session内で一意であり、同一入力列に対して安定した意味を持てばよい。

---

# 18. Sustain Pedalの共通Event契約

## 18.1 ProcessEventKind

`crates/sonalloy-core/src/process.rs`へ追加する。

```rust
SustainPedal {
    down: bool,
}
```

SustainはModulation Sourceではない。

`BUILTIN_SOURCE_IDS`へ追加しない。

SustainはNote ReleaseのLifecycle Controlであり、Parameter Routeへ接続する値ではない。

## 18.2 Offline Event Canonicalization

Offline Adapterが同一OffsetのEventを正規化する優先順位を次へ定義する。

```text
0 SustainPedal
1 NoteOff
2 ParameterChange
3 PitchBend
4 ModWheel
5 Aftertouch
6 NoteOn
```

既存Event同士の相対順序は維持される。

SustainをNote Offより前にする理由は、Offline Event列で同じSample位置にPedal状態の変更とNote Offがある場合、その位置のNote Offへ新しいPedal状態を適用するためである。Live MIDIではこのPriorityを適用せず、Timestampと入力Sequenceを維持する。

例：

```text
same offset:
Sustain down
Note off

-> PedalがDownになった後にNote Offを評価
-> Releaseを延期
```

```text
same offset:
Sustain up
Note off

-> Pedal Upを先に適用
-> Note Offは通常Release
```

## 18.3 Validation

`down`はboolのためNumeric Range Validationは不要。

Coreの`ProcessBlock::validate_for()`はFrame形状、Event値、Offset範囲、Offsetの昇順だけを検証する。同じOffsetのEventはSlice内の順番で処理する。Offline Event JSON / MIDI File Adapterは`ProcessEventKind::priority()`で同じFrameを正規化し、Live MIDI Adapterは`(timestamp_us, sequence)`の順番を維持したまま、共通の`InstrumentProcessor::process()`へ渡す。

## 18.4 Reset

Instrument Reset後のSustain状態は必ず、

```text
false
```

へ戻す。

---

# 19. SustainとVoice Lifecycle

## 19.1 State表現

Sustain用に新しい`VoiceState::Sustained`を追加しない。

Sustain中のVoiceは音声処理上`Active`であり、Sustainは鍵盤状態とRelease延期を表す直交状態として保持する。

Voiceへ次を追加する。

```rust
key_down: bool,
sustain_held: bool,
```

またPending Noteには同じ意味を保持できる内部型を導入する。

```rust
struct PendingNote {
    request: NoteRequest,
    key_down: bool,
    sustain_held: bool,
}
```

既存の、

```rust
pending: Option<NoteRequest>
```

を、

```rust
pending: Option<PendingNote>
```

へ置き換える。

## 19.2 Note On

通常のNote開始：

```text
key_down = true
sustain_held = false
state = Active
```

## 19.3 Note Off / Pedal Up

Note Off処理を次へ分ける。

```text
release_note
  │
  ├─ sustain_down = true
  │      ├─ key_down = false
  │      ├─ sustain_held = true
  │      └─ state = Activeのまま
  │
  └─ sustain_down = false
         └─ begin_release
```

現在`release_note()`内に直接書かれている実際のRelease処理は、

```rust
begin_release(...)
```

へ集約する。

`begin_release()`が担当する。

- Active LayerのGenerator Note Off
- Active Layer ADSR Release
- Armed `note_off` Layerの開始
- Modulation Envelope Note Off
- `VoiceState::Releasing`

同じRelease処理をSustain用に複製しない。

## 19.4 Pedal Up

Instrument Runtimeで、

```text
true -> false
```

の遷移を検出する。

Pedal Up時、全VoiceへSustain Releaseを通知する。

```text
state = Active
key_down = false
sustain_held = true
```

のVoiceだけ`begin_release()`へ移行する。

鍵盤をまだ押しているVoiceはReleaseしない。

## 19.5 重複Pedal Event

```text
Down -> Down
Up -> Up
```

はIdempotentに扱う。

同じ状態を再設定してVoice状態を変化させない。

## 19.6 Release Trigger Layer

`LayerTriggerEvent::NoteOff`は「実際にNote Releaseを開始する位置」で発音する。

したがってSustain中は：

```text
Physical key release
↓
Sustain Hold
↓
note_off Layerはまだ発音しない
↓
Pedal Up
↓
begin_release
↓
note_off Layer発音
```

とする。

これはSample Release TriggerをSustain Pedalの実際のRelease位置へ合わせる意味になる。

---

# 20. SustainとVoice Stealing

Voice Stealingは現在5 ms Fadeを使い、次Noteを`pending`として保持する。

Sustain導入後はPending Noteの鍵盤状態を失わない。

## 20.1 Pending中Note Off + Sustain Down

```text
New NoteOn
↓
既存VoiceをSteal中
↓
New NoteはPending
↓
Sustain Down中にNew NoteOff
```

この場合、Pending Noteを即Cancelしない。

```text
pending.key_down = false
pending.sustain_held = true
```

として保持する。

Steal Fade完了時に発音を開始し、Sustain Held状態を引き継ぐ。

## 20.2 Pending中Note Off + Sustain Up

PedalがUpなら、発音開始前に鍵盤が離されたPending NoteはCancelする。

現在の挙動を維持する。

## 20.3 Pending中にPedal Up

Pending Noteが、

```text
key_down = false
sustain_held = true
```

で待っている間にPedal Upを受けた場合、そのPending NoteをCancelする。

まだ発音を開始していないため、Release Tailだけを生成しない。

## 20.4 Steal対象のSustain Held Voice

Sustain Held Voiceは`Active`として現在のVoice Stealing Policyへ参加する。

今回、

```text
Releasing
Sustain Held
Physical Key Held
```

を別Priorityへ分ける新Policyは追加しない。

現在の、

```text
Idle
-> quietest Releasing
-> oldest Active
```

を維持する。

---

# 21. Offline MIDI / Event Sequenceとの統合

## 21.1 `render events`

`EventSequenceKind`へ追加する。

```rust
SustainPedal {
    down: bool,
}
```

JSON例：

```json
{
  "events": [
    { "absolute_frame": 0,     "type": "note_on",        "note_id": 1, "note": 60, "velocity": 100 },
    { "absolute_frame": 12000, "type": "sustain_pedal", "down": true },
    { "absolute_frame": 24000, "type": "note_off",       "note_id": 1 },
    { "absolute_frame": 48000, "type": "sustain_pedal", "down": false }
  ]
}
```

EventをCompileした後、既存`ProcessEventKind::priority()`で同一FrameをOfflineのCanonical順へ並べる。

## 21.2 `render midi`

現在Warningで捨てているCC64をEvent化する。

```text
CC64 >= 64 -> down
CC64 < 64 -> up
```

## 21.3 Multi-channel MIDI

現在Offline MIDIは複数Channelを一InstrumentへMergeする。

Sustainも同じInstrument-wide StateへMergeされる。

複数Channel Noteを検出し、Sustain Eventも存在する場合は、Channel Sustainが一Instrument Stateへ統合されることをWarningとして明示する。

Channelごとの独立Pedal Stateは今回作らない。

## 21.4 Offline / Realtime一致Test

同じ論理Event列を、

```text
render events
render midi
Realtime Audio Engine Test Harness
```

へ渡し、SustainによるRelease位置が一致することを確認する。

---

# 22. Realtime SafetyとMemory契約

## 22.1 Audio Callbackで許可する処理

- Fixed-capacity Queue pop
- Preallocated Vec clear / push within capacity
- Allocation-free sort
- `InstrumentRuntime::process`
- Planar / Interleaved sample copy
- Numeric sample format conversion
- Atomic load / store / fetch_add

## 22.2 Audio Callbackで禁止する処理

```text
File I/O
JSON Parse / Serialize
Definition Compile
Asset Decode
Device Enumeration
Device Reconfiguration
Heap Allocation
Vec Capacity拡張
String生成
format!
println! / eprintln!
Blocking Mutex / RwLock
Network
Thread Join
Sleep
Panic前提の分岐
```

## 22.3 Preallocation

Audio Stream Build前に最低限次を確保する。

```text
InstrumentRuntime全State
left scratch[max_block_size]
right scratch[max_block_size]
block_events capacity 4096
Event ArrayQueue capacity 4096
```

Native DSP Handleは各Wrapperが一意に所有し、状態操作を`&mut self`へ限定する。Thread affinityのないNative Handle Wrapperだけが個別に`Send`を実装し、共有参照のための`Sync`は実装しない。`AudioEngine`全体へ一括の`unsafe impl Send`を置かない。

## 22.4 CLI側Allocation Test

`sonalloy-core`には既にTest用Counting Allocatorがある。

Realtime Adapterにも、Unit Test内でCallback処理一回あたりのAllocationを測定する小さなTest Helperを置く。

大きなBenchmark Frameworkや新しいDependencyは追加しない。

測定対象はCPAL Device APIではなく、Device非依存の`RealtimeAudioEngine` callback本体とする。

完了条件：

```text
Prepare済みEngine
+ pre-filled Event Queue
+ preallocated output buffer

-> callback処理 Allocation 0
```

## 22.5 MIDI Callback

MIDI CallbackはAudio Callbackではないため、Note Tracking用HashMap / VecDequeのAllocationは許容する。

ただしAudio QueueはBoundedであり、Queue Full時に拡張しない。

---

# 23. Error / Diagnostic / Stream Status

## 23.1 DiagnosticCode

`sonalloy-core::DiagnosticCode`へFrontend-neutralな次を追加する。

```text
AUDIO_DEVICE_ERROR
```

MIDI Device / MIDI Parseは既存、

```text
MIDI_ERROR
```

を使用する。

Realtime Event Queue Overflowは`PROCESS_ERROR`カテゴリのRealtime detailとして報告する。Audio CallbackのFatalは`AUDIO_DEVICE_ERROR`、MIDI CallbackのFatalは`MIDI_ERROR`、Core ProcessとQueueのFatalは`PROCESS_ERROR`へ分類する。Adapter内部のQueue実装名を恒久Diagnostic Codeへ固定しない。

## 23.2 起動前Audio Error

次は`AUDIO_DEVICE_ERROR`。

- Audio Host取得失敗
- Device ID不明
- Default Outputなし
- Output Config取得失敗
- Mono Device
- Unsupported Sample Rate
- Unsupported Buffer Size
- Unsupported Sample Format
- Stream Build失敗
- Stream Play失敗

## 23.3 起動前MIDI Error

次は`MIDI_ERROR`。

- MIDI Deviceなし
- 指定Port ID不明
- Port接続失敗

## 23.4 CPAL Error Callback

CPAL `ErrorKind`を次の三分類へ正規化する。

### Warning

```text
RealtimeDenied
```

Audio Streamは継続する。

Main Threadが一回だけWarningを表示する。

### Recoverable Counter

```text
Xrun
```

`xrun_count`を増やし、StreamがBackend側で継続できる限り演奏を継続する。

毎回Console出力しない。

### Fatal

例：

```text
DeviceNotAvailable
DeviceChanged
StreamInvalidated
PermissionDenied
BackendError
ResourceExhausted
```

Fatal StatusをSetし、Main ThreadがSessionを終了する。

`ErrorKind`はnon_exhaustiveとして扱い、未知のErrorはFatal `Other`へ正規化する。

## 23.5 Core Process Fault

Audio Callback内Core ErrorはFatal。

Main Threadの最終Diagnosticは`PROCESS_ERROR`とし、Queue Overflowも同じCodeへ分類する。Messageは少なくとも次の意味を含む。

```text
realtime processing failed
```

Audio Callbackから詳細Stringを運ぶためにMutexやHeapを追加しない。

## 23.6 Session終了時Status

正常終了時、Human-readableで次を表示する。

```text
Stopped.
XRuns: 0
Realtime priority: active / denied / not-applicable
```

Backend内部状態を推測して`active`と断定できない場合は、

```text
Realtime priority warning: none / denied
```

程度の事実だけを表示する。

Audio Callbackについて、観測したFrame数の最小値・最大値・Callback回数も表示する。Callbackが一度も実行されなかった場合は値を推測せず`none`とする。

---

# 24. Latencyの扱い

## 24.1 Engine Latency

`CompiledInstrument.reported_latency_frames`をそのままFrontendへ表示する。

Milliseconds：

```text
latency_ms = frames / sample_rate * 1000
```

## 24.2 Device Buffer

CPAL Callbackが渡したFrame数をSession中に観測し、Requested値とは別に最小値・最大値・Callback回数を表示する。Backendが要求値をそのまま返すと仮定しない。

```text
requested_buffer_frames
callback_frames_min
callback_frames_max
callback_count
```

## 24.3 合計Latency

今回、

```text
engine latency + buffer duration = end-to-end MIDI latency
```

という表示をしない。

実際のEnd-to-end Latencyには、

- MIDI Transport
- Scheduling
- Audio Host Buffering
- Hardware Pipeline
- DAC

等が含まれるためである。

## 24.4 Live MIDI Timing精度

Live MIDI Eventは次のCore Chunk先頭へ量子化される。

要求Buffer 256 / 48 kHzなら、Adapter Schedulingだけを見ると最大約5.33 msのChunk境界幅を持つ。

これは理論上のChunk幅としてDocumentationへ示してよいが、Hardware End-to-end Latencyと表現しない。

---

# 25. 決定性と既存Offline機能の回帰

Realtime Deviceは非決定的な外部時間を扱うが、Core Runtimeの決定性を崩さない。

## 25.1 Offline

次は従来通り同じ入力から同等結果を生成する。

```text
render note
render events
render midi
reset-check
```

## 25.2 Sustain追加による回帰

Sustain Eventが存在しないEvent列では、既存出力を変化させない。

Offline AdapterのCanonical順は既存Event同士で維持する。

## 25.3 Realtime Random

Random / Round Robin / Grain等は既存Note IDとSeed契約を使用する。

Realtime AdapterがNote IDを一意に付与することで、Runtime側へ新しいRandom Seedを導入しない。

---

# 26. Platform / Build / CI / Release

## 26.1 Windows

Audio：CPAL Default WASAPI Backend

MIDI：Midir Default Windows Backend

ASIOは今回追加しない。

理由：ASIO SDK / LLVM等の追加Build要件をRealtime基盤初回へ混在させず、まず現在のCross-platform Adapterを完成させるため。

## 26.2 Linux

Audio：CPAL ALSA

MIDI：Midir ALSA

CPAL / Midirは`sonalloy-cli`の通常Dependencyであるため、LinuxでOffline Commandだけを実行する場合も`sonalloy-cli`のBuildにはALSA開発Packageが必要になる。

Build DependencyとしてDebian / Ubuntu CIへ次を追加する。

```bash
pkg-config
libasound2-dev
```

既存の、

```text
cmake
g++
```

も維持する。

## 26.3 macOS

既存Release Targetを維持するため、CoreAudio / CoreMIDIでBuild可能な状態を維持する。

製品要件上のRealtime必須Review PlatformはWindows / Linuxとする。macOSはCI Compile / Unit Testを通す。

## 26.4 CI

`.github/workflows/ci.yml`の通常Linux Jobと`.github/workflows/release.yml`のLinux Buildへ、CLI全体のALSA Dependencyとして追加する。

Native DSPだけをBuildする、

```text
linux-native-fault-injection
linux-native-sanitizer
```

へALSA packageを追加しない。これらは`sonalloy-dsp-sys`だけをBuildするJobである。

Release Linux ArtifactはBuild後に`sonalloy --version`を実行し、生成Binaryが起動することを確認する。Physical Audio Deviceは要求しない。

## 26.5 DeviceなしCI

GitHub Actions RunnerのPhysical Deviceを前提に、

```bash
sonalloy play ...
```

をCI Smoke Testにしない。

CIでは次を検証する。

- Realtime Module Compile
- Device非依存Unit Test
- Sustain Core Test
- MIDI / Event Sequence Test
- CLI Command Parse / Error Test
- Existing Offline Smoke

## 26.6 Release

`.github/workflows/release.yml`のLinux x86_64 / arm64 Build Stepへ、

Linux Release Buildでは`pkg-config`と`libasound2-dev`をSystem Packageとして用意する。

を追加する。

Windows / macOSに追加System Packageは不要。

---

# 27. Documentation / Agent Skill / License

## 27.1 `README.md`

更新内容：

- Offline専用説明をRealtime + Offlineへ更新
- `device list`
- `play`
- Realtime Quick Start
- Linux source build dependency
- Realtime対応入力一覧

既存Generator / Processor一覧は維持する。

## 27.2 Root `Cargo.toml`

Workspace descriptionを現在の製品状態へ更新する。

例：

```text
JSON-defined hybrid instrument engine for realtime performance and offline rendering
```

Version Bump自体は本フェーズの実装完了条件へ含めない。Release運用に従う。

## 27.3 `docs/cli.md`

一箇所へ次を追加する。

- `device list`
- `play`
- Device ID
- Buffer Size
- Constant Tempo
- Live MIDI対応Message
- Realtime Error
- Stop方法

Sustain Event File形式も`render events`章へ追加する。

## 27.4 `docs/runtime-processing.md`

既存Note Lifecycle章をSustain込みへ更新する。

```text
Active + Key Up + Sustain Down
  -> Active / Held

Pedal Up
  -> Releasing
```

新しいVoiceStateを増やしたように記述しない。

## 27.5 `docs/architecture.md`

Crate表の`sonalloy-cli`責務へ追加する。

```text
Audio Device Adapter
Realtime MIDI Adapter
Realtime Session
```

Coreの「Audio Device APIへ依存しない」という境界は維持する。

## 27.6 `docs/testing-and-sound-review.md`

Realtime Performance Review手順を追加する。

既存Sound ReviewのWAV品質判定と、Realtime AdapterのXRuns / Input / Latency確認を分離する。

## 27.7 `.agents/skills/create-instrument/SKILL.md`

Instrument作成後の確認経路へ、Deviceが利用可能な場合のRealtime試奏を追加する。

Offline Render / Analysis / Traceを置き換えず、最終試奏の追加選択肢として記載する。

## 27.8 `THIRD_PARTY_NOTICES.md`

Rust direct dependencyへ追加する。

| Crate | Version | 用途 | License |
|---|---:|---|---|
| `cpal` | 0.18.1 | Realtime Audio Device I/O | Apache-2.0 |
| `midir` | 0.11.0 | Realtime MIDI Input | MIT |
| `crossbeam-queue` | 0.3.13 | Fixed-capacity Realtime Event Queue | MIT OR Apache-2.0 |

`midly`の用途はStandard MIDI File Decodeに加えてLive MIDI Message Parseも記載する。

---

# 28. Core Unit Test

## 28.1 Process Event

- `SustainPedal { down: true }`がValid
- `SustainPedal { down: false }`がValid
- 同じOffsetでは入力された順番で適用
- Offline Adapterは`priority()`でSustainをNote Offより前へ正規化
- Offsetの昇順違反を検出

## 28.2 Basic Sustain

```text
NoteOn
Sustain Down
NoteOff
```

後もVoiceが`Active`であること。

その後、

```text
Sustain Up
```

で`Releasing`へ移ること。

## 28.3 Key Held

```text
NoteOn
Sustain Down
Sustain Up
```

鍵盤Note OffがないためVoiceをReleaseしないこと。

## 28.4 Repeated Pedal

- Down / Downで二重State変化なし
- Up / Upで二重Releaseなし

## 28.5 Release Trigger Layer

- Sustainなし：Note Off時に開始
- Sustainあり：Physical Key Note Offでは開始しない
- Pedal Up時に開始

## 28.6 Modulation Envelope

Sustain Hold中はModulation EnvelopeをReleaseへ移行しない。

Pedal Up時にLayer ADSRと同じRelease位置でNote Offを受ける。

## 28.7 Generator Note Off

Sample Gate / Operator等、`GeneratorRuntime::note_off()`を利用するGeneratorについて、Sustain Hold中にGenerator Note Offが呼ばれないことを確認する。

## 28.8 Pending Note

### Sustain Down

- Voice Steal開始
- Pending Note作成
- Pending NoteのNote Off
- Pedal Down
- PendingをCancelしない
- Steal完了後にSustain Heldで開始

### Pedal Up before start

- Pending Sustain Held
- Pedal Up
- Pending Cancel

## 28.9 Voice Stealing

Sustain Held Active Voiceが既存Active Voiceと同じPolicyでSteal対象になること。

新しいVoice Allocation優先順位が混入していないこと。

## 28.10 Reset

- `sustain_down = false`
- Voice `key_down / sustain_held`初期化
- Pendingなし
- 既存Parameter / External Control初期値
- 同じEvent列をReset後にRenderすると一致

## 28.11 Block Size

同じSustain Event列を、

```text
1
32
64
257
1024
不均等Block列
```

でRenderし、Event位置とRelease位置が一致すること。

## 28.12 Sample Rate

最低限：

```text
44.1 kHz
48 kHz
96 kHz
```

SustainはFrame位置にのみ作用し、Sample Rate固有のState差を生まないこと。

## 28.13 Allocation

Prepare後に、

```text
NoteOn
Sustain Down
NoteOff
Sustain Up
```

を含むProcessを行いAllocation 0。

---

# 29. Realtime Adapter Unit Test

Physical Deviceを使わないPure Testとして行う。

## 29.1 Callback Frame分割

`max_block_size = 256`で、Host Callback Frame数：

```text
1
63
64
255
256
257
511
641
1024
```

を処理し、全Core Callが256以下であること。

## 29.2 Absolute Frame

複数Callbackを連続処理し、`runtime.absolute_frame()`が総Frame数と一致すること。

## 29.3 Stereo Device

2ch Device BufferへLeft / Rightが交互に正しく入ること。

## 29.4 Multi-channel Device

6ch等で：

```text
ch0 = Left
ch1 = Right
ch2..5 = Silence
```

を確認する。

## 29.5 PCM Sample Conversion

少なくとも代表値で、

```text
f32
f64
signed integer
unsigned integer
24-bit型
```

のDispatchがCompileされ、±1 / 0の変換方向が正しいこと。

すべてのCPAL PCM Match ArmがTest Buildされるようにする。

## 29.6 Queue Drain

- Empty Queue
- 1 Event
- 複数Event
- 4096 Event

でCapacityが変わらないこと。

## 29.7 Live Event Order

Queue投入順を意図的に、Timestampを含めて、

```text
NoteOn
NoteOff
SustainDown
PitchBend
```

等へ崩しても、Timestampの異なるEventは前後を維持し、同一TimestampでもSequence順になること。NoteOn → NoteOff、NoteOff → SustainDown、SustainDown → NoteOffの順序がVoice状態へ反映されること。

## 29.8 Queue Overflow

4097個目のPushで、

- 古いEventを書き換えない
- `force_push`しない
- Fatal StatusをSet
- Audio Outputは次CallbackからSilence

を確認する。

## 29.9 Process Fault

Fault Injection可能なTest Runtime経路または不正Contextを使い、Core Process Error時に：

- 現在Bufferの未処理範囲がSilence
- Fatal State
- 次CallbackもSilence

を確認する。

## 29.10 Xrun Status Mapping

CPAL Error KindをPure Functionへ正規化し、

```text
RealtimeDenied -> warning
Xrun -> counter
DeviceNotAvailable -> fatal
StreamInvalidated -> fatal
Other -> fatal
```

をTestする。

## 29.11 Callback Metrics

複数のHost Callbackを処理し、観測Frame数の最小値・最大値・Callback回数が正しく更新されること。Callbackがない場合は値を推測しない。

## 29.12 Allocation

Prepare済み`RealtimeAudioEngine`へ、

- Eventあり
- Eventなし
- Host Callback > Core max block
- Multi-channel output

を与え、Audio Callback本体Allocation 0を確認する。

---

# 30. CLI / Integration Test

## 30.1 `device list`

Device APIそのものは実Device依存のため、CLI Argument ParseとReport SerializeをUnit Testする。

Device Inventoryを内部Structへ変換するPure部分はFixtureでTestする。

## 30.2 `play` Args

- Definition必須
- Buffer 0を拒否
- Tempo 0 / NaNを拒否
- Sample Rate 0を拒否
- Unknown Device IDをDiagnosticへ変換

NaNはCLI parse経由で入力可能な場合に検証する。

## 30.3 Event Sequence Sustain

`render events`でSustain Event JSONを受け付ける。

- bool以外Parse Error
- Same-frame Priority
- Reset Check一致

## 30.4 MIDI CC64

Test用MIDIを生成して：

```text
NoteOn
CC64 down
NoteOff
CC64 up
```

をRenderする。

Release開始がPedal Up位置以降になることをAudio ActivityまたはTrace可能なState Testで確認する。

## 30.5 Existing MIDI

Sustainを含まない既存MIDI Render結果の主要Metricsが回帰しないこと。

## 30.6 Existing CLI

現在の、

```text
instrument init
instrument validate
instrument inspect
render note
render events
render midi
dev render-sine
```

のIntegration Testを全て維持する。

---

# 31. Realtime Human Review

Realtime PerformanceはCIだけで完成判定しない。

物理Deviceを使うReviewを行う。

## 31.1 Review対象Platform

必須：

```text
Windows
Linux
```

macOSはBuild / Unit Testを維持し、実機確認できる場合は追加記録する。

## 31.2 使用Instrument

新しい音色Definitionを大量に作らず、既存Referenceを使う。

最低限：

```text
testdata/instruments/basic-poly-synth.json
testdata/instruments/expressive-hybrid-lead.json
Physical String Reference
Modal Reference
Spectral / Granularを含む比較的重いReference
```

実際のRepositoryに存在するReference Pathを実装時に確定し、同じ内容を`review/realtime-performance/`へ複製しない。

## 31.3 Reviewケース

| ケース | 確認内容 |
|---|---|
| 単音 | Note Onの反応、Attack |
| 8音Chord | Polyphony、Mix |
| 16音Chord | Voice処理、CPU |
| 高速連打 | Note欠落、Stuck Note |
| 同音連打 | Note ID FIFO |
| Pitch Bend | Smooth反映 |
| Mod Wheel | Route反映 |
| Aftertouch | Route反映 |
| Sustain | Hold / Pedal Up Release |
| Sustain + Chord | 複数Voice同時Release |
| Voice Steal | Polyphony超過 |
| Global Reverb / Delay | Voice終了後Tail継続 |
| Physical / Modal | Stateful Generatorの安定 |
| Spectral / Granular | Heavy GeneratorのCallback安定 |

## 31.4 Buffer Size

最低限：

```text
256 frames: 完了判定対象
128 frames: 追加評価
```

256 Frameで通常演奏中に継続的なXrunが発生する場合は完了としない。

128 FrameはHardware / Instrument負荷差が大きいため、結果を記録するが初回フェーズの全環境必須条件にはしない。

## 31.5 長時間

Windows / Linuxそれぞれで、Release Buildを使い最低10分の連続演奏を行う。

確認：

- Fatal Realtime Faultなし
- Stuck Noteなし
- Queue Overflowなし
- Device Disconnect等を除く通常利用中Xrun 0を目標
- Memoryが継続増加しない

## 31.6 Realtime Review Package

新規：

```text
review/realtime-performance/
├─ README.md
├─ metrics.json
└─ review-summary.md
```

Audio WAVやDefinitionを複製しない。

`metrics.json`はHardware Review結果を次のように記録する。

```json
{
  "platform": "windows",
  "audio_device": "...",
  "midi_device": "...",
  "sample_rate": 48000,
  "requested_buffer_frames": 256,
  "callback_frames_min": 256,
  "callback_frames_max": 256,
  "callback_count": 112500,
  "engine_latency_frames": 0,
  "duration_seconds": 600,
  "xrun_count": 0,
  "queue_overflow": false,
  "fatal_fault": false
}
```

Machine-specific IDが公開に不適切な文字列を含む場合、Review ArtifactではDevice Name / Backendだけを記録し、Opaque IDそのものを残さなくてよい。

`review-summary.md`では、数値に加えて「実際に鍵盤で演奏可能な応答か」を人間が確認する。

---

# 32. File単位の変更計画

## 32.1 `crates/sonalloy-core/src/process.rs`

- `ProcessEventKind::SustainPedal`
- Offline AdapterのCanonical順
- Core Process Validation更新
- 共通`InstrumentProcessor::process()`によるRealtime Event処理
- Unit Test

## 32.2 `crates/sonalloy-core/src/runtime/instrument.rs`

- `sustain_down`
- Event適用
- Pedal Up時Voice通知
- Prepare / Reset初期化
- Realtime Safety Test

## 32.3 `crates/sonalloy-core/src/runtime/voice.rs`

- `key_down`
- `sustain_held`
- `PendingNote`
- `release_note(..., sustain_down)`相当の責務整理
- `begin_release()`
- Pedal Up処理
- Pending Cancel / Hold
- Reset
- Unit Test

## 32.4 `crates/sonalloy-core/src/diagnostics.rs`

- `AudioDeviceError`
- Serialize名`AUDIO_DEVICE_ERROR`
- Mapping Test

CPAL型はImportしない。

## 32.5 `crates/sonalloy-cli/Cargo.toml`

追加：

```toml
cpal = { version = "0.18.1", features = ["realtime"] }
midir = "0.11.0"
crossbeam-queue = "0.3.13"
```

## 32.6 `crates/sonalloy-cli/src/main.rs`

追加：

- `mod realtime`
- `mod midi_common`
- `Command::Device`
- `Command::Play`
- `DeviceCommand::List`
- `DeviceListArgs`
- `PlayArgs`
- Dispatch
- `run_device_list`
- `run_play`
- Event Sequence Sustain Variant

Realtime Audio Callback本体を書かない。

## 32.7 `crates/sonalloy-cli/src/midi_common.rs`

新規。

共有する最小Helperだけを置く。

- Pitch Bend normalize
- CC1 / CC64識別用Constant
- Note ID生成
- 必要ならMIDI channel / note key型

Standard MIDI File Parser全体を移動しない。

## 32.8 `crates/sonalloy-cli/src/midi.rs`

- CC64 Event
- 共通Helper利用
- Multi-channel Sustain Warning
- Test更新

## 32.9 `crates/sonalloy-cli/src/realtime/mod.rs`

- Public(crate) Realtime Entry
- Device Selectionとの接続
- Session起動順序
- Main Thread Status Loop
- Stop処理

## 32.10 `crates/sonalloy-cli/src/realtime/device.rs`

- CPAL Output Device列挙
- Midir Input Port列挙
- Stable ID
- Default Device
- Audio Config選択
- Buffer Range Validation
- Device Report用Struct
- Pure Selection Test

## 32.11 `crates/sonalloy-cli/src/realtime/audio.rs`

- `RealtimeAudioEngine`
- `QueuedEvent`
- Midir Timestamp保持
- `RealtimeStatus`
- CPAL Stream Build
- Sample Format Dispatch
- Callback Chunk処理
- Queue Drain / Sort
- Timestamp → Sequenceの順序
- Callback Frameの最小値・最大値・回数
- Channel Mapping
- Fault Handling
- Allocation Test

## 32.12 `crates/sonalloy-cli/src/realtime/midi.rs`

- Midir Connection
- `LiveEvent::parse`
- `LiveMidiState`
- Midir TimestampのQueue保存
- Note Tracking
- Event Queue Push
- CC64
- Queue Overflow Fault
- Unit Test

## 32.13 `crates/sonalloy-cli/tests/cli.rs`

- Event Sequence Sustain
- MIDI CC64 Render
- New Command argument validation
- Existing test regression

Physical Deviceを要求するTestは入れない。

## 32.14 `Cargo.lock`

Dependency追加に伴い更新する。

VersionをPlan記載値と一致させる。

## 32.15 `.github/workflows/ci.yml`

通常Linux Jobへ：

```text
pkg-config
libasound2-dev
```

追加。

## 32.16 `.github/workflows/release.yml`

Linux Build Asset Stepへ同じALSA build dependencyを追加。

## 32.17 `Cargo.toml`

Description更新。

Crate member追加なし。

## 32.18 `README.md`

Realtime説明、Quick Start、Command表、Build dependency更新。

## 32.19 `docs/cli.md`

Realtime CommandとSustain Eventを正本として追加。

## 32.20 `docs/runtime-processing.md`

Sustain LifecycleとRealtime Adapterから同じProcess Contractを使う説明。

## 32.21 `docs/architecture.md`

CLI Device Adapter責務、Dependency追加、Core境界。

## 32.22 `docs/testing-and-sound-review.md`

Realtime Review Procedure。

## 32.23 `.agents/skills/create-instrument/SKILL.md`

Realtime試奏手順。

## 32.24 `THIRD_PARTY_NOTICES.md`

CPAL / Midir / Crossbeam Queue。

## 32.25 `review/realtime-performance/`

- README
- metrics
- Human Review Summary

---

# 33. 実装順序

本フェーズは一つのPRとして扱う。

以下は安全に完成させるための実装順序であり、恒久機能名へ番号を残さない。

## P0. Baseline固定

1. 最新`main`が`9bb8671a03d170929c03cf202d2863e5d4f84579`であることを記録
2. `cargo fmt --all -- --check`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace`
5. `cargo build --workspace --release`
6. Basic / Dynamic / Processor / Physical Modalの既存Review状態を確認

完了条件：変更前Baseline Green。

## P1. Sustain Core Contract

1. `ProcessEventKind::SustainPedal`
2. Offline Event Canonicalization
3. Process Validation
4. `InstrumentRuntime.sustain_down`
5. Voice `key_down / sustain_held`
6. `PendingNote`
7. `begin_release`
8. Pedal Up
9. Reset
10. Unit Test
11. Allocation Test

この段階ではCPAL / Midirを追加しない。

完了条件：Sustainを`ProcessBlock`だけで完全に検証可能。

## P2. Offline Frontend統合

1. Event Sequence `sustain_pedal`
2. MIDI File CC64
3. Common MIDI Helper
4. CLI Test
5. DocumentationのEvent表更新

完了条件：Realtime DeviceなしでSustain契約をOffline再現可能。

## P3. Dependency / Build Foundation

1. CPAL 0.18.1
2. Midir 0.11.0
3. Crossbeam Queue 0.3.13
4. Cargo.lock
5. THIRD_PARTY_NOTICES
6. Linux CI dependency
7. Linux Release dependency
8. Windows / Linux / macOS Compile

完了条件：Device処理未使用でも全Platform CI Build可能。

## P4. Device Inventory

1. `realtime/device.rs`
2. Audio Output列挙
3. MIDI Input列挙
4. Stable ID
5. `device list`
6. JSON Report
7. Audio Config選択Pure Logic
8. Buffer validation

完了条件：Device情報をCoreへ漏らさずCLIから確認可能。

## P5. Audio Engine

1. `RealtimeStatus`
2. `QueuedEvent`
3. `RealtimeAudioEngine`
4. Preallocated Scratch
5. Callback Chunk分割
6. Core Process
7. Channel Mapping
8. Sample Format Dispatch
9. CPAL Stream Build
10. Error Callback
11. Xrun / RealtimeDenied分類
12. Unit Test
13. Allocation Test

この段階ではMIDI Deviceを接続せず、QueueへTest Eventを入れてAudio Engineを完成させる。

完了条件：Device非依存Callback TestがGreen。

## P6. Live MIDI

1. `realtime/midi.rs`
2. Midir Port Connection
3. LiveEvent Parse
4. Note ID Tracker
5. Pitch Bend
6. Mod Wheel
7. Aftertouch
8. Sustain
9. Queue Push
10. Overflow Fault
11. Unit Test

完了条件：Raw Live MIDI Messageから共通Eventまで確定。

## P7. `play` Session統合

1. CLI Args
2. Device Resolve
3. Audio Config
4. Compile
5. Runtime Prepare
6. Stream Build
7. MIDI Connection
8. Stream Play
9. Main Status Loop
10. Stop / Drop
11. Startup / Shutdown Report
12. Fault Handling

完了条件：物理鍵盤から実際に演奏できる。

## P8. Regression / Documentation / Review

1. Full Workspace Test
2. Existing Offline Render Regression
3. Sustain MIDI Review
4. README
5. CLI
6. Runtime Processing
7. Architecture
8. Testing Document
9. Agent Skill
10. Realtime Performance Review Package
11. Windows実機Review
12. Linux実機Review

完了条件：コード、恒久文書、実機確認が一致。

## P9. Release Candidate確認

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

既存Native Fault Injection / SanitizerもGreen。

Windows / Linux Release Buildで`sonalloy device list`と`sonalloy play`を実機確認する。

---

# 34. 完了条件

## 34.1 Core Event

- Sustain EventがPublic Process Contractへ存在
- Pedal Down / UpがSample Offsetで適用される
- Offline Adapterの同一Offset Canonical順が固定
- Existing Eventの意味が維持
- ResetでPedal State初期化

## 34.2 Voice

- Key Up + Sustain DownでRelease延期
- Pedal UpでRelease
- Key Held VoiceをPedal UpでReleaseしない
- Note-off Trigger LayerがPedal Up時に発音
- Pending NoteがSustain意味を保持
- Voice Stealingで破綻しない
- Stuck Noteを作らない

## 34.3 Offline

- Event Sequence Sustain
- MIDI CC64
- Reset Determinism
- Existing MIDI回帰
- Existing Render Command回帰

## 34.4 Device

- Audio Output一覧
- MIDI Input一覧
- Stable ID指定
- Default Audio選択
- Multiple MIDI時の明示選択
- Unsupported Configを明示Error

## 34.5 Realtime Audio

- Physical Audio Output
- Variable Host Callback Size
- Core max block以下へ分割
- Stereo / Multi-channel Device
- PCM Sample Format
- DSD拒否
- Engine absolute frame連続

## 34.6 Realtime MIDI

- Note On
- Note Off
- NoteOn velocity 0
- Pitch Bend
- Mod Wheel
- Channel Aftertouch
- Sustain
- 同音重複Note ID

## 34.7 Realtime Safety

- Audio Callback Allocation 0
- Blocking Mutexなし
- File / JSON / Device Queryなし
- Queue固定容量
- Queue Overflowを黙って継続しない
- Core Process Fault後Silence
- Callback中Log生成なし

## 34.8 Error

- RealtimeDeniedはWarningで継続
- XrunはCounter
- Device loss / Stream invalidationはFatal
- MIDI Connection Errorを構造化
- Audio Config Errorを構造化

## 34.9 Platform

- Windows CI Green
- Linux CI Green
- macOS CI Green
- Linux x86_64 Release Build
- Linux arm64 Release Build
- Windows Release Build
- macOS Release Build
- Windows Realtime Human Review
- Linux Realtime Human Review

## 34.10 Product

次が実際に成立すること。

```bash
sonalloy device list
sonalloy play testdata/instruments/basic-poly-synth.json --midi-device <id>
```

鍵盤を弾き、Pitch Bend / Mod Wheel / Aftertouch / Sustainを使い、Sonalloyの既存Generator / Processor / ModulationがRealtime経路でも機能する。

---

# 35. 次フェーズへ残すもの

Realtime Performance後は、Concept上の残差を次の順で扱う。

## 35.1 Performance / Modulation Completion

```text
Monophonic
Legato
Portamento
MSEG
Step Modulator
Sample & Hold
Smooth Random
Macro
Vector
Tempo / Transport Source
```

Realtimeで演奏できる状態があることで、これらを実際の演奏感として評価できる。

## 35.2 External Audio / Advanced Processing

```text
Process Input Buffer
Audio Input Bus
Envelope Follower
Vocoder
Cross Synthesis
Sidechain
Convolution
Frequency Shifter
Gate
Transient Shaper
Advanced Delay
```

## 35.3 Frontend / Host Contract

```text
Activate / Deactivate
Public C ABI
Runtime Config Publish / Swap
CLAP
VST3
Riffra
Latency Host Notification
State Save / Restore
```

この段階でConceptの完全Lifecycleを実際のHost Contractと合わせて固定する。

## 35.4 Realtime Timing精度向上

必要性を実測してから扱う。

```text
MIDI Timestamp -> Audio Stream Clock Mapping
Sample-accurate Live MIDI Offset
Device Hardware Timestamp
ASIO
JACK / PipeWire
```

初回Realtime Phaseの成立条件へ先行投入しない。

---

# 36. 実装Agent向け最終ルール

1. 基準は最新Main `9bb8671a03d170929c03cf202d2863e5d4f84579`と`docs/CONCEPT.md`である。
2. Audio Device APIを`sonalloy-core`へ入れない。
3. MIDI Device APIを`sonalloy-core`へ入れない。
4. 新しいRealtime専用Crateを追加しない。
5. CPAL / Midir / Crossbeam Queueは`sonalloy-cli`だけへ追加する。
6. CPALは0.18.1、`realtime` featureを使用する。
7. ASIO / JACK / PipeWire / PulseAudio / realtime-dbusを今回有効化しない。
8. Midirは0.11.0を使う。
9. Raw MIDI Parseは既存Midly 0.5.3の`LiveEvent`を使う。
10. MIDI ThreadとAudio Threadの間は固定容量`ArrayQueue`を使う。
11. Queue Full時にEventを捨てて演奏を継続しない。
12. `force_push()`を使わない。
13. Audio Callback内でAllocationしない。
14. Audio Callback内でStringを作らない。
15. Audio Callback内でLog出力しない。
16. Audio Callback内でMutexを取らない。
17. Host Callback SizeとCore Block Sizeが一致すると仮定しない。
18. Host Bufferが大きい場合は`ProcessSpec.max_block_size`以下へ分割する。
19. Absolute Frameは`InstrumentRuntime::absolute_frame()`を使い、第二Counterを作らない。
20. Core OutputはPlanar Stereo f32のまま維持する。
21. Device Sample Format変換はCLI Adapterで行う。
22. Deviceが2ch超なら先頭2chへStereoを出し、残りをSilenceにする。
23. Mono Output Deviceは起動Errorとする。
24. Sustainは`ProcessEventKind`として実装する。
25. SustainをModulation Sourceにしない。
26. SustainはDefinition Schemaへ追加しない。
27. Same-offsetではSustainをNote Offより先に適用する。
28. Voiceに`Sustained` Stateを追加せず、`key_down / sustain_held`として表現する。
29. 現在のRelease本体を`begin_release()`へ集約し、Sustain用に複製しない。
30. Sustain中はRelease Trigger LayerをPedal Upまで延期する。
31. Pending NoteのSustain状態を保持する。
32. Voice Stealing Policy自体を今回変更しない。
33. CC64はOffline MIDI / Live MIDI両方で同じEventへ変換する。
34. Live MIDI Timestampを無理にAudio Clockへ変換せず、Timestampの異なるEventの前後関係を保持する。
35. Monophonic / Legato / Portamentoを「ついで」に追加しない。
36. External Audio Inputを追加しない。
37. ProcessContextのTransport全面拡張を追加しない。
38. Core`activate / deactivate`を形だけ追加しない。
39. Hot Swap / Device Reconnectを追加しない。
40. RealtimeDeniedだけでSessionを停止しない。
41. Xrunを毎回Console出力せずCounterへ集約する。
42. Core Process ErrorとQueue Overflowは`PROCESS_ERROR`、Audio Callback Fatalは`AUDIO_DEVICE_ERROR`、MIDI Callback Fatalは`MIDI_ERROR`として、以後のOutputをSilenceへする。
43. Existing Offline Renderを壊さない。
44. SustainなしEvent列の音声を不要に変化させない。
45. 新Dependencyを`THIRD_PARTY_NOTICES.md`へ記載する。
46. Linux CI / ReleaseのALSA build dependencyを同時に更新する。
47. Physical DeviceをCI必須にしない。
48. Unit TestでDevice非依存Callback処理を十分検証する。
49. Release BuildでWindows / Linux実機Reviewを行う。
50. 自動Test成功だけでRealtime Performance完成と判定しない。
51. `main.rs`へAudio Callback本体を書き込まない。
52. Realtime Module内でもDevice / Audio / MIDIの責務を混在させない。
53. 将来Frameworkを理由にTraitを増やさない。
54. TODO / `unimplemented!()` / 仮Silence実装をMerge状態へ残さない。
55. 本計画と実コードが衝突した場合、Conceptと現行Contractを確認し、意味を変える修正は記録してから行う。

---

# 37. 参考資料

## Repository内

- `docs/CONCEPT.md`
- `docs/architecture.md`
- `docs/runtime-processing.md`
- `docs/cli.md`
- `docs/testing-and-sound-review.md`
- `docs/plan/plan-mvp.md`
- `docs/plan/plan-dynamic-parameters.md`
- `docs/plan/plan-physical-modal-expansion.md`
- `crates/sonalloy-core/src/process.rs`
- `crates/sonalloy-core/src/definition.rs`
- `crates/sonalloy-core/src/compiler.rs`
- `crates/sonalloy-core/src/runtime/instrument.rs`
- `crates/sonalloy-core/src/runtime/voice.rs`
- `crates/sonalloy-cli/src/main.rs`
- `crates/sonalloy-cli/src/midi.rs`
- `crates/sonalloy-cli/tests/cli.rs`
- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`
- `THIRD_PARTY_NOTICES.md`

## 外部Dependency判断

- CPAL 0.18.1公式Rustdoc / Package Metadata
  - Stable Audio Device ID
  - Output Stream / Buffer Size
  - Linux ALSA build dependency
  - MSRV 1.85
  - `realtime` feature
  - `RealtimeDenied` / `Xrun`等のError Kind
- Midir 0.11.0公式Rustdoc / Package Metadata
  - Realtime MIDI Input Callback
  - Stable MIDI Port ID
  - Linux ALSA / Windows / macOS Backend
- Midly 0.5.3公式Rustdoc
  - `LiveEvent::parse()`
- Crossbeam Queue 0.3.13公式Rustdoc
  - Fixed-capacity `ArrayQueue`
  - Full時`push()`失敗

---

# 最終到達イメージ

```text
                    Authoring / Offline
                    ┌──────────────────┐
Definition ─Compile─┤ render note      │
                    │ render events    │
                    │ render midi      │
                    └────────┬─────────┘
                             │
                             │ same Core Contract
                             │
Physical MIDI                │
    │                        │
    ▼                        │
Live MIDI Adapter            │
    │                        │
    ▼                        │
Bounded Event Queue          │
    │                        │
    ▼                        ▼
┌─────────────────────────────────────────────┐
│             Instrument Runtime              │
│                                             │
│ Note / Sustain / External Control Events    │
│ Voice Allocation / Layer / Generator        │
│ Modulation / Processor / Global Effects     │
└────────────────────┬────────────────────────┘
                     │
                     ▼
                Stereo f32
                     │
                     ▼
              Audio Adapter
                     │
                     ▼
             Physical Audio Device
```

この状態をRealtime Performanceの完成点とする。
