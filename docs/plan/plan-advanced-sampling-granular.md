# Sonalloy Advanced Sampling / Granular Expansion 詳細設計・実装計画

* **対象Repository**：`endo-ly/sonalloy`
* **正本要件**：`docs/CONCEPT.md`
* **前提実装**：Instrument Definition、Compile、Dynamic Parameter / Modulation、Processor Chain、Essential Synthesis / Sampling Expansion、Digital Synthesis Expansion
* **ロードマップ上の扱い**：次の開発Phase（P7）
* **実装単位**：四単位。BranchとPull Requestは一つとし、単位ごとに独立したCommit・Test・Sound Reviewを成立させる
* **用途**：実装エージェントへ渡す詳細設計・実装計画
* **文書言語**：日本語。型名、API名、Parameter ID、File Pathのみ英語を使用する

---

## 0. この計画書の位置づけ

本書は、現在のSonalloyへ高度なSample Playback、Time Stretch、Granular、Wave Sequenceを追加し、Audio Assetを単純に再生するSamplerから、時間方向へ加工・再構成して新しい音色を作れるInstrument Engineへ拡張するための詳細設計・実装計画である。

製品全体の目的、責務、将来像は`docs/CONCEPT.md`を正本とする。

現在のSample Generatorでは次が成立している。

* Multi Sample Zone
* Key Mapping
* Velocity Layer
* deterministic Round Robin
* One-shot
* Forward Loop
* Explicit Slice
* Root Noteを基準としたPitch Mapping
* Cubic Interpolation
* Compile時Asset Decode / Resample
* Partial Missing Asset
* Layer / Voice / Global Processorとの統合

一方、現在のSample AssetはRuntime用にMonoへ変換されるため、元のStereo ImageをSample Generatorから保持できない。

Pitch変更もPlayback Cursorの速度変更によって行っているため、Pitchと再生時間を独立して制御できない。

本PhaseではこのSample基盤を拡張したうえで、次の四単位を完成させる。

1. **Stereo Sample / Advanced Playback**
2. **Time Stretch / Tempo Sync**
3. **Granular Generator**
4. **Wave Sequence Generator**

四単位は別々の製品Phaseへ分割しない。

一方、実装は混在させない。

各単位について必要なDefinition、Validation、Compile、Runtime、Parameter、CLI、Test、Sound Reviewまで縦に完成させてから次へ進む。

### 0.1 このPhaseの中心目的

Digital Synthesis Expansionでは、WavetableやOperator Modulationによって「素材なしで電子音を生成する能力」を大きく拡張した。

本Phaseではもう一方の中心価値である、

> 任意のAudio素材をInstrumentとして利用し、素材そのものを時間方向へ再構成して新しい音色へ変換する能力

を強化する。

Field Recording、Vocal、Percussion、Texture、Loopなどを、

* 通常Sample
* Reverse
* Crossfade Loop
* Time Stretch
* Granular
* Wave Sequence

として同じInstrument内から利用可能にする。

### 0.2 恒久的な機能名称

コード、Definition、CLI、恒久Documentでは次を使用する。

* `Prepared Audio`
* `Sample Generator`
* `Sample Zone`
* `Reverse Playback`
* `Crossfade Loop`
* `Time Stretch`
* `Tempo Sync`
* `Release Trigger`
* `Granular Generator`
* `Grain`
* `Grain Scheduler`
* `Wave Sequence Generator`
* `Wave Sequence Step`

`P7`はロードマップ上の識別にのみ使用する。

型名、関数名、Module名、Parameter ID、Diagnostic、Fixture、Reference Instrument、利用者向け恒久Documentへ`P7`を残さない。

### 0.3 実装判断の優先順位

判断に迷った場合は次を優先する。

1. `docs/CONCEPT.md`
2. 本書で固定するPlayback、Granular、Wave Sequenceの意味
3. 現在のDefinition / Compile / Runtime分離
4. 現在のParameter / Modulation Contract
5. 音質
6. Realtime Safety
7. Determinism
8. Block Size非依存性
9. 実装の単純さ
10. 将来拡張

音が出るだけの仮実装を完成扱いしない。

特に次を禁止する。

* Stereo Assetを一度Monoへ潰して擬似Stereo化する
* Pitch変更をTime Stretchと呼ぶ
* GranularをSample Playbackへ大量の分岐を追加して実装する
* Grain生成時にHeap Allocationする
* RandomをProcess Block単位で消費する
* Wave SequenceをLayerの自動生成で代用する
* Tempo SyncをCLIだけの事前計算で実現する
* Time StretchのLatencyを無視する
* Granular PositionをBlock単位でしか更新しない
* Sample、Granular、Wave Sequenceごとに別々のAudio Asset Decoderを作る
* 将来のSpectral Engineを理由に一般的なAudio GraphやScheduler Frameworkを導入する

### 0.4 本Phaseで固定するもの

本書では次を固定する。

* Prepared AudioのMono / Stereo表現
* Sample Playback Definitionの再設計
* ReverseとLoopの意味
* Crossfade Loopの計算境界
* Release Triggerの実装位置
* Time Stretch BackendとNative境界
* PitchとDurationの分離
* Tempo Contextの利用方法
* Time StretchのLatency管理
* Granular Definition
* Granular Parameter
* Grain Window
* Grain Scheduler
* Random決定規則
* Grain数上限
* Wave Sequence Definition
* Step Duration
* Step遷移
* Direction
* Crossfade
* Tempo Sync
* Asset Failure時の扱い
* Realtime Safety
* CLI Inspect
* Test / Review
* 四実装単位の順序

---

# 1. 目的と完成像

## 1.1 完成後のSample系

```text
Audio Asset
    │
    ▼
Prepared Audio
Mono / Stereo
    │
    ├─────────────────────────────┐
    │                             │
    ▼                             ▼
Sample Generator             Granular Generator
    │                             │
    ├─ One-shot                  ├─ Position
    ├─ Loop                      ├─ Grain Size
    ├─ Reverse                   ├─ Density
    ├─ Crossfade Loop            ├─ Pitch
    ├─ Time Stretch              ├─ Randomness
    └─ Tempo Sync                └─ Pan Spread
    │                             │
    └──────────────┬──────────────┘
                   ▼
             Layer Pipeline
```

さらに複数Assetを時間方向へ切り替える方式として、

```text
Prepared Audio[]
        │
        ▼
Wave Sequence Generator
        │
        ├─ Step Duration
        ├─ Step Pitch / Gain
        ├─ Crossfade
        ├─ Direction
        ├─ Loop
        └─ Tempo Sync
```

を追加する。

## 1.2 完成状態

本Phaseの完成状態は次とする。

> Mono / Stereo Audio Assetを通常Sample、Reverse、Loop、Crossfade Loop、Time Stretch、Tempo Syncとして再生でき、同じAsset基盤からGranular GeneratorとWave Sequence Generatorを構築し、既存Parameter、Modulation、Processor、Polyphonic Voiceと統合して、再現可能かつBlock Size非依存なStereo Audioを生成できる。

## 1.3 代表的に作れるInstrument

* Stereo Piano / Drum Sample
* Reverse Texture
* Crossfade Loop Pad
* Release Noise / Release Sample付きInstrument
* Tempo Sync Drum Loop
* Tempo Sync Vocal
* Pitchを保ったSlow Texture
* Granular Pad
* Granular Drone
* Vocal Grain Texture
* Percussion Cloud
* Granular Freeze
* Position Scrub Texture
* Wave Sequence Pad
* Wave Sequence Rhythm
* Wave Sequence Texture
* Wavetable + Granular Hybrid
* FM + Granular Hybrid
* Sample Attack + Granular Body + Release Sample

---

# 2. 現在の実装との接続

## 2.1 維持する三層構造

```text
Definition
    ↓
Validation / Compile
    ↓
Compiled Instrument
    ↓
Instantiate
    ↓
Runtime Instance
```

新機能でもこの境界を維持する。

### Definition

利用者が保存・編集する意味を保持する。

例：

* Asset
* Region
* Playback Direction
* Loop
* Time Stretch Mode
* Source BPM
* Grain Size
* Grain Density
* Sequence Steps

### Compiled Instrument

Audio Threadから文字列、秒、Pathなどを解決しなくてよい状態へ変換する。

例：

* Prepared Audio
* Region Frame
* Loop Frame
* Parameter Handle
* Grain上限
* Sequence Step配列
* Stretch Backend設定
* Latency
* Tempo非依存のStatic State

### Runtime

発音ごとの可変Stateだけを保持する。

例：

* Sample Cursor
* Grain State
* Grain Serial
* Stretch State
* Sequence Step
* Crossfade進行
* Random State

## 2.2 Generator構造

完成後は概念上次となる。

```text
Generator
├─ Oscillator
├─ Noise
├─ Sample
├─ Wavetable
├─ Operator Modulation
├─ Granular
└─ Wave Sequence
```

GranularとWave Sequenceは独立Generatorとする。

通常Sample RuntimeへModeとして押し込まない。

## 2.3 Signal Pipeline

既存順序を変更しない。

```text
Generator
    ↓
Layer Envelope
    ↓
Layer Processor
    ↓
Layer Gain / Pan
    ↓
Layer Mix
    ↓
Voice Processor
    ↓
Voice Sum
    ↓
Global Processor
```

Stereo Sample、Granular、Wave Sequenceも同じPipelineへ入る。

Generator内部へFilter、Delay、Reverb等を追加しない。

---

# 3. DSP・外部依存方針

## 3.1 採用方針

| 機能                         | 実装                                  |
| -------------------------- | ----------------------------------- |
| WAV Decode                 | 既存Symphonia                         |
| Sample Rate Conversion     | 既存Rubato                            |
| Sample Interpolation       | 既存Cubic Interpolationを拡張            |
| Reverse                    | Rust Runtime                        |
| Crossfade Loop             | Rust Runtime                        |
| Granular                   | Rust Runtime                        |
| Grain Window               | Rust                                |
| Wave Sequence              | Rust Runtime                        |
| Time Stretch / Pitch Shift | Signalsmith Stretch                 |
| Tempo管理                    | Sonalloy Core                       |
| Random                     | Sonalloy既存Deterministic Random規則を拡張 |

GranularとWave Sequenceのためだけに新しい外部DSP依存は追加しない。

## 3.2 Signalsmith Stretch

Time Stretch BackendにはSignalsmith Stretchを使用する。

採用理由：

* Time StretchとPitch Shiftを分離して扱える
* Mono / Stereoに対応できる
* Streaming Processが可能
* Input / Output Latencyを取得できる
* Pitch変更をSemitone単位で指定できる
* `splitComputation`によって重いSpectral処理を分散できる
* MIT License
* Header-onlyで内部Native境界へ限定しやすい

Sonalloy DefinitionへSignalsmith固有概念を露出しない。

Sonalloy側では、

```text
Time Stretch
Pitch Shift
Latency
```

という製品上の意味だけを持つ。

## 3.3 Native境界

Signalsmithを既存`native/daisysp-wrapper`へ混在させない。

次を追加する。

```text
native/
├─ daisysp-wrapper/
└─ signalsmith-stretch-wrapper/
```

`sonalloy-dsp-sys`は引き続きRust CoreからNative DSPを利用する内部境界として使用する。

新Workspace Crateは追加しない。

概念API：

```text
stretch_create
stretch_prepare
stretch_reset
stretch_set_pitch
stretch_process
stretch_input_latency
stretch_output_latency
stretch_destroy
```

C++ ObjectをRustへ直接公開しない。

Rust側はOpaque Handleとして所有する。

## 3.4 Signalsmith固定Version

Floatingな`main`をBuild時に取得しない。

実装開始時に採用Revisionを一つ固定する。

固定Revisionは、

* Repositoryへ記録
* CMakeから同じRevisionを使用
* `THIRD_PARTY_NOTICES.md`へ記録

する。

Build時Network Downloadへ依存する構成は使用しない。

既存DaisySPと同じく、再現可能なNative Buildを維持する。

## 3.5 Native安全境界

C ABIで次を保証する。

* ExceptionをRustへ越境させない
* Null Handleを拒否する
* Channel数を検証する
* Sample Rateを検証する
* Input / Output Buffer長を検証する
* NaN / Infinityを拒否する
* Backend失敗をStatusへ変換する
* Destroy済みHandleを使用しない
* Fault Injection Testを追加する

`configure`はPrepare時に一度だけ行う。

Signalsmith内部BufferはPrepare時に必要Capacityを確保する。

Process中にCapacity増加がないことをAllocation Testで確認する。

---

# 4. Prepared Audio基盤

## 4.1 現在の問題

現在のSample Asset経路ではStereo WAVもDecode可能だが、Prepared Sampleへ入る前にMonoへDownmixされる。

高度なSample処理へ進む前にこの制約を除去する。

GranularやWave SequenceをMono前提で実装してからStereo対応すると、

* Sample Runtime
* Interpolation
* Grain State
* Sequence Playback
* Processor接続
* Generator Output Mode

を後から再設計する必要が生じる。

そのためStereo化は本Phaseの最初に行う。

## 4.2 Prepared Audio型

任意Channel数には一般化しない。

扱うのはMono / Stereoのみとする。

概念構造：

```text
PreparedAudio
├─ sample_rate
├─ frames
├─ source_metadata
└─ channels
   ├─ Mono
   │   └─ samples
   └─ Stereo
       ├─ left
       └─ right
```

StereoはPlanar形式とする。

Audio Runtimeでは、

```text
left[frame]
right[frame]
```

を直接参照できるようにする。

Interleaved BufferをAudio Threadで分離しない。

## 4.3 Source Metadata

維持する。

* Source Sample Rate
* Source Channel Count
* Bit Depth
* Source Frame Count

Prepared後のFrame Countは別に保持する。

Source FrameとPrepared Frameを混同しない。

## 4.4 Decode規則

WAV Decode：

* Mono → Mono
* Stereo → Stereo
* 3 Channel以上 → Error

既存の対応Format範囲は変更しない。

本Phaseを理由にFLAC / OGG等へFormat対応を広げない。

## 4.5 Resample

Mono：

```text
1 channel
→ Rubato
→ Mono
```

Stereo：

```text
L ─┐
   ├→ 同じResampling設定
R ─┘
```

左右で異なるOutput Frame数を許可しない。

片ChannelだけResample失敗した場合はAsset全体を失敗とする。

## 4.6 Wavetableへの影響

Prepared AudioのStereo対応によってWavetableの音響仕様を変更しない。

Wavetableは既存のWavetable Preparation契約に従う。

Sample向けStereo保持とWavetable向け素材処理を明確に分ける。

---

# 5. Sample Definition再設計

## 5.1 目的

現在の、

```text
OneShot
ForwardLoop
```

だけをVariantとして増やし続けると、

```text
ReverseOneShot
ReverseLoop
ReverseCrossfadeLoop
TimeStretchedLoop
...
```

のような組み合わせ爆発になる。

本PhaseではPlaybackの意味を直交した設定へ整理する。

## 5.2 Playback構造

概念構造：

```text
Sample Playback
├─ region
│  ├─ start_seconds
│  └─ end_seconds
├─ direction
│  ├─ forward
│  └─ reverse
├─ loop
│  ├─ none
│  └─ loop
│      ├─ start_seconds
│      ├─ end_seconds
│      └─ crossfade_seconds
└─ time
   ├─ resample
   ├─ fixed_stretch
   └─ tempo_sync
```

Definition上の具体的なStruct / Enum名はこの意味に沿わせる。

Variant数で全組み合わせを表現しない。

## 5.3 Region

Regionは現在同様、

```text
[start, end)
```

として扱う。

* Start：Inclusive
* End：Exclusive

Compile時にFrameへ変換する。

未指定EndはAsset終端とする。

## 5.4 Direction

```text
forward
reverse
```

を持つ。

Forward：

```text
start → end
```

Reverse：

```text
end → start
```

Prepared AudioをReverseしたCopyは作らない。

Cursor方向だけを変える。

## 5.5 Loop

Loopなし：

```text
region start → region end → finished
```

Loopあり：

```text
Region Start
    ↓
Loop Start
    ↓
Loop End
    └────→ Loop Start
```

Loop RegionはSample Region内に存在しなければならない。

## 5.6 Crossfade Loop

`crossfade_seconds = 0`なら通常Loop。

0より大きい場合はCrossfade Loop。

Loop終端側とLoop開始側を同時に読み、Constant-power Crossfadeする。

Linear Gain Crossfadeは使用しない。

Crossfade Frame数はCompile時に確定する。

Crossfade長はLoop長の半分以下とする。

Loop Region全体をCrossfade区間にしてしまう設定は拒否する。

## 5.7 Reverse + Loop

Playback DirectionとLoopを独立させる。

Reverse Loopでは、

```text
Loop End
    ↓
Loop Start
    └────→ Loop End
```

へ戻る。

Crossfadeも方向に応じて対応する二地点をBlendする。

Reverse専用Bufferを作らない。

---

# 6. Release Trigger

## 6.1 実装位置

Release TriggerのためにSample Runtimeへ二つのPlayback Engineを押し込まない。

現行設計ではLayerが、

* Trigger
* Envelope
* Generator

を所有する。

この構造を利用し、Layer Triggerへ発音Eventを追加する。

概念上：

```text
Layer Trigger
├─ event
│  ├─ note_on
│  └─ note_off
├─ key range
└─ velocity range
```

通常Layerは`note_on`。

Release Sample用Layerは`note_off`とする。

Sample Zoneへ同じTrigger情報を重複保存しない。

`docs/CONCEPT.md`でいうRelease Triggerを、現行Layer設計へ対応付けた実装とする。

## 6.2 Note On時

Note Onで、

* `note_on` Layer → Start
* `note_off` Layer → Armed

とする。

Armed LayerはまだAudioを生成しない。

## 6.3 Note Off時

Note Offで、

1. Activeな`note_on` LayerをReleaseへ移行
2. Armedな`note_off` LayerをStart
3. `note_off` LayerのEnvelopeをAttackから開始

する。

Release Sampleは通常Layer Envelopeとは独立したEnvelopeを持てる。

## 6.4 Voice Lifetime

Voiceは、

```text
Active Layerが存在する
OR
Armed Note-Off Layerが存在する
```

間は保持する。

Note On LayerがOne-shotで先に終了しても、Note Off LayerがArmedならNote IDを失わない。

## 6.5 Voice Stealing

Voice Stealingは演奏上のNote Offではない。

そのためArmed Release Layerを発音しない。

Steal Fade完了後にArmed Stateを破棄する。

---

# 7. Sample Compile / Validation

## 7.1 Region

Compile Error：

* Start < 0
* End <= Start
* StartがAsset終端以降
* EndがAsset範囲外
* RegionがInterpolationに必要なFrame数を持たない

## 7.2 Loop

Compile Error：

* Loop Start < Region Start
* Loop End > Region End
* Loop End <= Loop Start
* Crossfade < 0
* Crossfade > Loop Length / 2
* Cubic Interpolationを安全に行えないLoop長

## 7.3 Direction

DirectionはCompile時Enumへ解決する。

Runtimeで文字列比較しない。

## 7.4 Prepared Frame

秒指定はCompile時にEngine Sample Rate上のFrameへ変換する。

同じ秒→Frame変換関数をRegion、Loop、Crossfadeで共有する。

丸め規則を個別実装しない。

## 7.5 Partial Missing Asset

既存方針を維持する。

一つのZone AssetがMissingでも、

* 他Zone
* 他Layer
* 他Generator

が利用可能ならInstrument全体を失敗させない。

該当ZoneだけDisabledにする。

---

# 8. Sample Runtime

## 8.1 State

概念上次を保持する。

```text
SampleRuntime
├─ source
├─ channel_mode
├─ root_note
├─ position
├─ direction
├─ region
├─ loop
└─ finished
```

## 8.2 Cursor

Forward：

```text
position += ratio
```

Reverse：

```text
position -= ratio
```

Loop Wrapは大きなPlayback Ratioでも正しく動作する必要がある。

1 SampleでLoop Length以上進む場合も一回だけの加減算で処理しない。

`rem_euclid`等を利用してRegion内へ戻す。

## 8.3 Cubic Interpolation

既存Four-point Cubic Interpolationを維持する。

Stereoでは同じFractionを使用して、

```text
Left
Right
```

を別々にInterpolationする。

左右でCursorを分離しない。

## 8.4 Boundary Read

Interpolation NeighborがRegion外へ出る場合の規則を一箇所に集約する。

* Non-loop Region → Region端へClamp
* Loop Region → Loop内へWrap

Reverseでも同じ関数を利用できる設計にする。

## 8.5 End Fade

Non-loop SampleのRegion終端Fadeを維持する。

ReverseではRegion Start側が終端となる。

Playback方向に応じて残り距離を求める。

Stereo左右へ同じFade Gainを適用する。

---

# 9. Time Stretchの意味

## 9.1 Pitchと時間の分離

通常Sample Playback：

```text
Playback Speed
  =
Played Note
× Root Note差
× Layer Tuning
```

この方式ではPitchを上げると再生時間が短くなる。

Time Stretch有効時：

```text
Pitch
  =
Played Note
- Root Note
+ Layer Tuning

Duration
  =
Stretch Ratio / Tempo
```

として独立させる。

## 9.2 三つのTime Mode

### Resample

現在と同じ方式。

PitchとDurationが連動する。

### Fixed Stretch

DefinitionでDuration Ratioを指定する。

例：

```text
ratio = 2.0
```

元の2倍の長さ。

### Tempo Sync

Definitionへ素材の元Tempoを持つ。

```text
source_bpm
```

再生Ratio：

```text
duration_ratio
=
source_bpm / process_tempo_bpm
```

例：

```text
Source = 120 BPM
Host   = 60 BPM

duration_ratio = 2
```

## 9.3 Range

Time Stretchの対応範囲は、

```text
0.5 ～ 2.0
```

を標準契約とする。

Extreme StretchをこのPhaseの品質保証対象にしない。

範囲外はClampしない。

DefinitionまたはRuntime Diagnosticとする。

Granularで極端な時間変形を行えるため、Time Stretch側を無制限に広げない。

## 9.4 Reverseとの組み合わせ

本Phaseでは、

```text
Reverse + Resample
```

を対応する。

```text
Reverse + Time Stretch
```

は対応しない。

Definition Validationで拒否する。

Time Stretch BackendへのReverse Feedまで同時に品質保証すると検証範囲が大きくなるためである。

---

# 10. Time Stretch Runtime

## 10.1 Input Provider

Time Stretch Backendへ直接Asset全体を渡さない。

Sample Runtime側がPlayback Regionから必要なInput Framesを供給する。

```text
Prepared Audio
      ↓
Region / Loop Provider
      ↓
Stretch Input Buffer
      ↓
Signalsmith
      ↓
Stereo Output
```

これにより、

* Region
* Forward Loop
* Crossfade Loop

の意味をSample側に保持できる。

## 10.2 Scratch Buffer

必要なInput / Output ScratchはPrepare時に確保する。

最大Block SizeとStretch Ratio Rangeから必要Capacityを算出する。

Process中に`Vec` Capacityを増やさない。

## 10.3 Pitch

Played Note、Root Note、Layer TuningからPitch Semitoneを求める。

Stretch BackendへPitch設定として渡す。

Playback Cursor RatioへPitchを混ぜない。

## 10.4 Tempo変更

Tempo SyncではProcess SpanごとのTempoを利用する。

Tempo変更時にStretch Backend自体を再Configureしない。

Input / Output Frame比率だけを変更する。

Backend StateをResetしない。

## 10.5 Loop

LoopはSignalsmith内部で行わない。

Stretch Backendへ渡す前のSource ProviderがLoopを処理する。

これによりSample LoopのDefinition意味をBackend実装へ依存させない。

---

# 11. Tempo Context

## 11.1 Core Contract

既に存在する`ProcessContext.tempo_bpm`を実際の音声生成へ使用する。

Tempoは、

```text
finite
tempo > 0
```

を必須とする。

## 11.2 Block Contract

一つのProcess Call内でTempoは一定とする。

Tempo Change地点では呼出側がProcess区間を分割する。

Core内部で一Blockに複数Tempoを保持する構造は追加しない。

## 11.3 MIDI Renderer

現在MIDI TempoはEventをFrameへ変換するために利用されている。

本PhaseではTempo Map自体もRendererへ保持する。

概念構造：

```text
MidiRender
├─ events
├─ tempo_map
├─ duration_frames
└─ diagnostics
```

Rendererは次の最も近い境界まで処理する。

```text
Max Block Boundary
Event Boundary
Tempo Boundary
```

Tempo Boundaryを跨いで一つのProcessContextを使用しない。

## 11.4 Event Sequence / Note Render

MIDI以外のOffline RenderでもTempoを指定可能にする。

Default Tempoは一箇所で定義する。

CLI Commandごとに別Defaultを作らない。

---

# 12. Time Stretch Latency

## 12.1 Backend Latency

Signalsmithが公開する、

* Input Latency
* Output Latency

をNative Wrapperから取得する。

LatencyをSonalloy側で推測値としてハードコードしない。

## 12.2 Compiled State

Time Stretchを使用するCompiled GeneratorへLatency情報を保持する。

Layer単位で必要Latencyを算出する。

Instrumentは利用中Layerの最大Latencyを保持する。

## 12.3 Layer Alignment

同じVoice内で、

```text
Layer A = Oscillator
Layer B = Time Stretched Sample
```

をMixした際にLayer Bだけが遅れて聞こえる状態を許容しない。

Layerごとに、

```text
instrument latency
-
layer intrinsic latency
```

分のDelay Compensationを行う。

Delay BufferはPrepare時に確保する。

## 12.4 Latency検証

Time StretchのStretch Ratioによる時間基準を推測式だけで決定しない。

Impulse Fixtureを使用して、

* Backend単体
* Sample Runtime統合後
* Layer Compensation後

の実測位置を検証する。

その結果を一つのLatency計算Helperへ固定する。

複数箇所へ別々のLatency式を書かない。

## 12.5 CLI

Offline WAVでは利用者から見たMusical Timelineを維持する。

Reported Latency分のPre-roll / Tailを含めてRenderし、最終File生成時に補正する。

Reference WAVも補正後を正本とする。

---

# 13. Granular Generator Definition

## 13.1 Generatorとして追加

`GeneratorDefinition`へGranularを追加する。

Sample GeneratorのPlayback Variantにはしない。

Granularは一つのNote中で複数の独立したGrain Lifecycleを持つためである。

## 13.2 Definition

Granular Generatorは次を持つ。

```text
Granular
├─ asset
├─ root_note
├─ region
│  ├─ start
│  └─ end
├─ position
├─ grain_size
├─ density
├─ pitch
├─ randomness
├─ pan_spread
└─ seed
```

## 13.3 Parameter Range

| Parameter   |               Range |
| ----------- | ------------------: |
| Position    |           0.0 ～ 1.0 |
| Grain Size  |          5 ～ 500 ms |
| Density     |  1 ～ 100 grains/sec |
| Grain Pitch | -2400 ～ +2400 cents |
| Randomness  |           0.0 ～ 1.0 |
| Pan Spread  |           0.0 ～ 1.0 |

Root Noteは0〜127。

## 13.4 Grain Window

本PhaseではHann Window固定とする。

Window種類をDefinitionへ公開しない。

```text
gain
1.0       /\
         /  \
        /    \
0.0 ___/      \___
```

Grain開始・終了で不連続を発生させない。

---

# 14. Granular Compile

## 14.1 Prepared Asset

Sample Generatorと同じPrepared Audioを利用する。

Granular専用Decode経路を作らない。

## 14.2 Region

Start / EndをPrepared Frameへ変換する。

Grain Positionの0〜1はこのRegion内を意味する。

Asset全体に対するPositionではない。

## 14.3 Parameter Handle

次をParameter Catalogへ追加する。

```text
granular_position
grain_size
grain_density
grain_pitch
grain_randomness
grain_pan_spread
```

既存の、

```text
Base Value
→ Parameter Smoothing
→ Modulation
→ Clamp
→ ValueSpan
```

を利用する。

## 14.4 Runtime Resource

Compile時に最大Active Grain数を確定する。

本Phaseでは、

```text
64 grains / Granular Layer / Voice
```

を上限とする。

最大Densityと最大Grain Sizeの組み合わせでもこの範囲に収まるParameter Contractにする。

Compile可能なDefinitionなのに通常動作でPool Exhaustionする状態を作らない。

---

# 15. Grain Scheduler

## 15.1 Scheduling単位

Grain生成時刻はAbsolute Sample Timeline上で管理する。

Process Block先頭から毎回数え直さない。

これによりBlock Sizeが、

```text
64
257
1024
```

と変わってもGrain位置が変化しない。

## 15.2 Density

Densityは、

```text
grains per second
```

として扱う。

次Grainまでの時間：

```text
sample_rate / density
```

を基準にする。

Fractional Intervalを累積誤差なく扱うため、Scheduler Positionは整数Frameへ毎回丸め直さず高精度Accumulatorで保持する。

## 15.3 Grain開始時に固定する値

新Grain開始時に次をSnapshotする。

* Source Position
* Length
* Pitch
* Pan
* Random Offset

発音済みGrainへ後からParameter値を上書きしない。

PositionやPitchをModulationした場合、次に生成されるGrainから反映する。

## 15.4 Grain State

各Grain：

```text
Grain
├─ active
├─ source_position
├─ source_increment
├─ age
├─ length
├─ pan_left
└─ pan_right
```

固定Poolから取得する。

Heap Objectを生成しない。

## 15.5 Pool

毎Note On時にPoolをAllocateしない。

`GranularRuntime`構築時に固定配列または固定Capacity Storageを用意する。

Inactive Slotを再利用する。

---

# 16. Granular Random / Position

## 16.1 Determinism

Random結果は最低限次から決定する。

```text
Definition Seed
Layer Stable ID
Note ID
Grain Serial
```

Global RNGを順番に消費しない。

Voice処理順やBlock SizeによってRandom結果が変化してはならない。

## 16.2 Position

`position = 0`：

Region Start。

`position = 1`：

Region End側。

実際のGrain長を考慮し、GrainがRegion外を不正Readしない位置へ変換する。

## 16.3 Randomness

Randomness 0：

Positionから開始。

Randomness 1：

Region全体を最大分散範囲とする。

Random位置がRegion端を超えた場合は循環させる。

単純Clampによって端へGrainが集中する状態を避ける。

## 16.4 Scrub

PositionをLFO、Envelope、Mod Wheel等で動かすことでScrubを実現する。

Scrub専用Generatorを追加しない。

## 16.5 Freeze

Positionを固定しながらGrain生成を続けることでFreezeを実現する。

一つのSample値を停止する処理ではない。

Freeze専用Booleanも追加しない。

---

# 17. Granular Pitch / Stereo

## 17.1 Pitch

最終Grain Pitch：

```text
Played Note
- Root Note
+ Layer Tuning
+ Grain Pitch
```

GrainごとにSource Incrementへ変換する。

本PhaseのGranularではSignalsmith Stretchを使用しない。

Granular固有のPitch変更はGrain Playback Rateによる。

## 17.2 Mono Asset

Mono GrainはConstant-power PanによってStereo配置する。

`pan_spread = 0`では中央。

`pan_spread = 1`では左右最大範囲。

## 17.3 Stereo Asset

Stereo AssetをMonoへ変換しない。

左右Channelを同じPosition / Pitch / Windowで読む。

Pan SpreadはStereo Balanceとして作用させる。

左右を独立したRandom Positionへしない。

原音のStereo Imageを維持する。

---

# 18. Wave Sequence Generator

## 18.1 目的

Wave Sequenceは複数のSample / Wave素材を時間順に切り替えて一つのGeneratorとして再生する。

大量のLayerを自動生成して代用しない。

Step TimingはGenerator内部で所有する。

## 18.2 Definition

```text
WaveSequence
├─ root_note
├─ direction
├─ loop
├─ crossfade
└─ steps[]
```

Step：

```text
Step
├─ id
├─ asset
├─ region
├─ duration
├─ playback_direction
├─ gain_db
└─ pitch_cents
```

## 18.3 Step数

最大：

```text
128 Steps
```

0 StepはValidation Error。

1 Stepは有効。

## 18.4 Duration

二種類を持つ。

```text
seconds
beats
```

SecondsはTempo非依存。

BeatsはProcess ContextのTempoへ追従する。

## 18.5 Beat Position

Beat Stepでは、

```text
beats_per_sample
=
tempo / 60 / sample_rate
```

を積算する。

Tempo変更がStep途中で起きても、その時点以降の進行速度だけが変わる。

Stepを最初からやり直さない。

---

# 19. Wave Sequence Playback

## 19.1 Step内Playback

Step Assetは、

```text
one_shot
loop
```

を持つ。

One-shotがStep Durationより先に終了した場合：

```text
残りStep時間 = silence
```

とする。

勝手に次Stepへ早送りしない。

LoopはStep終了までRegionを繰り返す。

## 19.2 Direction

Sequence順序：

* Forward
* Reverse
* Ping Pong

を実装する。

例：

```text
Steps = A B C D

Forward
A B C D A B...

Reverse
D C B A D C...

Ping Pong
A B C D C B A B...
```

端Stepを二回連続再生しない。

## 19.3 Asset Playback Direction

Sequence DirectionとSample Playback Directionは別概念とする。

```text
Sequence Direction
=
どのStepを選ぶか

Playback Direction
=
そのStep Assetをどちら向きに読むか
```

## 19.4 Crossfade

隣接StepをOverlapさせる。

Definition値は0〜0.5の比率。

0：

Crossfadeなし。

0.5：

Step Durationの最大50%を次Stepと重ねる。

Constant-power Crossfadeを使用する。

## 19.5 Runtime State

必要なPlayback Slotは最大2つ。

```text
Current Step
Next Step
```

Crossfadeのためだけに128 Step分のRuntime Playback Stateを確保しない。

Compiled Stepは共有Immutable Dataとして保持する。

---

# 20. Wave Sequence Asset Failure

## 20.1 一部Step Missing

Missing StepをCompile配列から削除しない。

Step Durationを維持したままSilence Stepとして保持する。

理由：

```text
A  B  C  D
```

のB AssetがMissingした場合に、

```text
A C D
```

へTimingが変化するとDefinitionの時間構造が壊れるため。

## 20.2 全Step Missing

GeneratorをUnavailableとする。

他Layerが利用可能ならInstrument全体はCompile可能。

既存Partial Asset Failure方針へ合わせる。

---

# 21. Parameter / Modulation統合

## 21.1 Granular

Dynamic対象：

* Position
* Grain Size
* Density
* Pitch
* Randomness
* Pan Spread

## 21.2 Sample

本PhaseではDynamic化しない。

* Region Start / End
* Direction
* Loop Start / End
* Crossfade Length
* Time Stretch Mode
* Source BPM

これらは再Compile対象。

## 21.3 Wave Sequence

本PhaseではDynamic化しない。

* Step Asset
* Step Count
* Step Duration
* Sequence Direction
* Crossfade
* Step Gain
* Step Pitch

Sequence構造のAutomationは後続Phaseへ残す。

## 21.4 新Parameter Unit

必要に応じて次を追加する。

* Seconds
* PerSecond

既存`Normalized`へ意味の違う値を無理に押し込まない。

Parameter DescriptorからCLI / Agentが正しい単位を取得できるようにする。

---

# 22. Voice Lifecycle

## 22.1 Note On

1. Voice Allocation
2. Note On Layer選択
3. Note Off LayerをArmed
4. Sample Zone選択
5. Generator Start
6. Granular Scheduler初期化
7. Wave Sequence Initial Step決定
8. Modulation Source初期化
9. Layer Envelope開始

## 22.2 Note Off

1. Note IDからVoiceを取得
2. Active Layerへ`note_off`
3. Note Off LayerをStart
4. Voice Modulation EnvelopeをRelease
5. Voice StateをReleasingへ移行

## 22.3 Voice End

次がすべてなくなったらIdleへ戻す。

* Active Layer
* Armed Layer

## 22.4 Voice Stealing

Steal時：

* 現在Voiceを既存Fadeで終了
* Release Triggerは発生させない
* Grainを次Voiceへ持ち越さない
* Sample Cursorを持ち越さない
* Stretch Stateを持ち越さない
* Sequence Stepを持ち越さない
* Armed Layerを破棄する

Pending Noteは完全にFresh Stateから開始する。

## 22.5 Reset

Reset後に初期化する。

* Sample Cursor
* Loop State
* Stretch Backend
* Stretch FIFO
* Latency Delay
* Grain Pool
* Grain Serial
* Scheduler
* Wave Sequence Step
* Ping Pong Direction
* Crossfade State
* Armed Release Layer

Fresh Runtimeと同じ結果になることをTestする。

---

# 23. Realtime Safety / Resource Budget

## 23.1 Process中禁止

* File I/O
* Decode
* Resample
* JSON Parse
* Path Resolve
* Hash計算
* Stretch Configure
* Heap Allocation
* Vec Capacity増加
* Grain Pool拡張
* Sequence配列変更
* Blocking Lock

## 23.2 Prepare時に行う

* Asset Decode
* Sample Rate Conversion
* Stereo Planar化
* Frame境界計算
* Stretch Configure
* Scratch Buffer確保
* Grain Pool確保
* Parameter Handle解決
* Sequence Step Compile
* Latency Buffer確保

## 23.3 Resource上限

| Resource           |                 上限 |
| ------------------ | -----------------: |
| Granular Grain     | 64 / Layer / Voice |
| Wave Sequence Step |                128 |
| Sample Channels    |                  2 |
| Stretch Ratio      |          0.5 ～ 2.0 |

一般的なCPU Budget Frameworkは導入しない。

現在のPhaseに必要な明示上限だけを持つ。

---

# 24. Diagnostic

既存Diagnostic体系へ追加する。

カテゴリ例：

* Invalid Sample Region
* Invalid Loop Region
* Invalid Crossfade
* Unsupported Channel Count
* Unsupported Playback Combination
* Invalid Stretch Ratio
* Invalid Source Tempo
* Stretch Backend Failure
* Invalid Grain Region
* Invalid Grain Parameter
* Invalid Sequence
* Invalid Step Duration

Backend固有Error Messageをそのまま利用者向けContractにしない。

Sonalloy Diagnosticへ変換する。

Asset Path、Layer ID、Zone ID、Step IDなど、問題箇所を特定可能なPathを付ける。

---

# 25. CLI / Inspect

## 25.1 Sample

表示する。

* Source Channels
* Prepared Frames
* Direction
* Region
* Loop Region
* Crossfade
* Time Mode
* Stretch Ratio
* Source BPM
* Intrinsic Latency

## 25.2 Granular

表示する。

* Asset
* Source Channels
* Region
* Root Note
* Position
* Grain Size
* Density
* Pitch
* Randomness
* Pan Spread
* Seed
* Grain Pool Limit

## 25.3 Wave Sequence

表示する。

* Step Count
* Direction
* Loop
* Crossfade
* Step ID
* Asset
* Region
* Duration Type
* Duration
* Pitch
* Gain
* Playback Direction
* Availability

## 25.4 Instrument

Time Stretchを含む場合はReported Latencyを表示する。

---

# 26. Reference Instrument / Sound Review

## 26.1 Advanced Sample Reference

確認対象：

* Mono One-shot
* Stereo One-shot
* Reverse
* Forward Loop
* Reverse Loop
* Crossfade Loop
* Release Trigger

確認：

* Stereo Image
* Loop境界
* Reverse終端
* Release Timing
* Pitch Mapping
* Existing Sample回帰

## 26.2 Time Stretch Reference

確認対象：

* 0.5x
* 0.75x
* 1.0x
* 1.5x
* 2.0x
* Pitch +12 semitone
* Pitch -12 semitone
* Tempo Sync Loop
* Stereo Vocal
* Tempo Change

確認：

* Pitch / Duration独立
* Stereo Image
* Transient
* Loop境界
* Tempo追従
* Latency Alignment

## 26.3 Granular Reference

作る。

### Field Recording Pad

```text
Stereo Field Recording
→ Granular
→ Slow Position LFO
→ Randomness
→ Pan Spread
→ Reverb
```

### Vocal Freeze

```text
Vocal
→ Position固定
→ Dense Grain
→ Long Grain
→ Filter
→ Reverb
```

### Percussion Cloud

```text
Percussion
→ Short Grain
→ High Density
→ Position Randomness
→ Stereo Spread
```

確認：

* Position
* Grain Size
* Density
* Pitch
* Randomness
* Stereo
* Freeze
* Polyphony

## 26.4 Wave Sequence Reference

最低4 Step用意する。

確認：

* Forward
* Reverse
* Ping Pong
* Loop
* Crossfade
* Seconds
* Beats
* Tempo Change
* Step Pitch
* Step Gain
* Missing Step

## 26.5 Final Hybrid Instrument

```text
Layer A
  Wavetable

Layer B
  Granular Vocal Texture
  Position ← LFO
  Density ← Mod Wheel

Layer C
  Reverse Sample Attack

Layer D
  Release Sample
  Trigger = Note Off

Voice
  Filter
  Drive

Global
  Delay
  Reverb
```

この一つのInstrumentで、

* Existing Digital Synthesis
* New Sample Playback
* Granular
* Release Trigger
* Modulation
* Processor
* Polyphony

が同時に成立することを確認する。

---

# 27. Automated Test

## 27.1 Prepared Audio

* Mono Decode
* Stereo Decode
* Mono Resample
* Stereo Resample
* 3 Channel拒否
* Metadata保持
* Non-finite拒否
* Hash mismatch
* Missing Asset

## 27.2 Sample

* Forward
* Reverse
* Loop
* Reverse Loop
* Crossfade Loop
* Stereo Cubic Interpolation
* Large Cursor Overshoot
* Region Boundary
* End Fade
* Note Off Trigger
* Voice Stealing
* Reset

## 27.3 Time Stretch

Native：

* Create / Destroy
* Prepare
* Reset
* Mono
* Stereo
* Pitch
* Input Latency
* Output Latency
* Invalid Handle
* Invalid Buffer
* Fault Injection

Core：

* Fixed Ratio
* Pitch independence
* Tempo Sync
* Tempo Change
* Loop
* Stereo
* Layer Alignment
* Reset
* Voice Stealing
* Block Size independence

## 27.4 Granular

* Spawn Timing
* Grain Length
* Grain Window
* Position
* Density
* Pitch
* Randomness
* Pan Spread
* Grain Reuse
* Pool上限
* Determinism
* Stereo
* Scrub
* Freeze
* Reset
* Voice Stealing

## 27.5 Wave Sequence

* Single Step
* Forward
* Reverse
* Ping Pong
* Sequence Loop
* One-shot Step
* Loop Step
* Seconds
* Beats
* Tempo Change
* Crossfade
* Pitch
* Gain
* Missing Step
* All Missing
* Stereo / Mono混在
* Reset

## 27.6 Block Size

同じInputを、

```text
64
257
1024
```

でRenderする。

少なくとも、

* Grain Spawn Frame
* Sequence Step Boundary
* Tempo Boundary
* Sample Cursor
* Note Event
* Release Trigger

が一致することを確認する。

## 27.7 Sample Rate

```text
44.1 kHz
48 kHz
96 kHz
```

で検証する。

時間単位で定義した、

* Loop
* Grain Size
* Density
* Sequence Duration
* Tempo Sync

が同等の意味を維持する。

---

# 28. Allocation / Performance Test

## 28.1 Allocation

Prepare後の次経路でAllocation 0を確認する。

* Sample Note On
* Sample Note Off
* Loop Wrap
* Stretch Process
* Grain Spawn
* Grain End
* Sequence Step Change
* Sequence Crossfade
* Voice Stealing
* Reset後Process

## 28.2 Performance Case

記録する。

### Sample

* 16 Voice Stereo Sample

### Time Stretch

* 1 Voice
* 4 Voice
* 8 Voice

### Granular

* 1 Voice × 16 Grain
* 4 Voice × 32 Grain
* 8 Voice × 64 Grain

### Wave Sequence

* 1 / 8 / 16 Voice

測定する。

* Render時間
* Peak Working Set
* CPU負荷相当
* Allocation Count

一般的なRealtime保証値としてDocument化しない。

現在のRegression比較用Metricとして扱う。

---

# 29. 実装単位

## 29.1 Unit A — Stereo Sample / Advanced Playback

### 目的

後続すべてが共有するAudio AssetとSample Playback基盤を完成させる。

### 作業順

1. Prepared AudioをMono / Stereo化
2. Stereo Decode / Resample
3. Generator Output Mode更新
4. Sample Playback Definition再設計
5. Region / Direction / Loop Compile
6. Stereo Cubic Interpolation
7. Reverse
8. Crossfade Loop
9. Layer Trigger Event追加
10. Release Trigger
11. Voice Armed State追加
12. CLI Inspect
13. Unit / Integration Test
14. Sound Review

### 完了判定

Stereo、Reverse、Crossfade Loop、Release Triggerが既存Sample Instrumentと同じRuntime上で成立すること。

## 29.2 Unit B — Time Stretch / Tempo Sync

### 目的

Sample PitchとDurationを分離し、Tempoに追従できる状態を作る。

### 作業順

1. Signalsmith Revision固定
2. Native Wrapper追加
3. C ABI安全境界
4. Rust Wrapper
5. Time Mode Definition
6. Stretch Compile
7. Scratch / Backend State準備
8. Pitch分離
9. Fixed Stretch
10. Process Tempo利用
11. MIDI Tempo Map
12. Tempo Sync
13. Latency測定
14. Layer Compensation
15. CLI Latency補正
16. Allocation Test
17. Sound Review

### 完了判定

Stereo LoopをPitchを変えずにTempo Syncでき、非Stretch LayerとのTimingも一致すること。

## 29.3 Unit C — Granular Generator

### 目的

Audio素材をGrainへ再構成する専用Generatorを完成させる。

### 作業順

1. Definition
2. Validation
3. Prepared Audio統合
4. Parameter追加
5. Compiled Granular
6. Grain Pool
7. Hann Window
8. Scheduler
9. Deterministic Random
10. Position
11. Grain Size
12. Density
13. Pitch
14. Randomness
15. Pan Spread
16. Stereo
17. Scrub / Freeze Reference
18. CLI Inspect
19. Test
20. Human Review

### 完了判定

Granular Pad、Freeze、Vocal Textureが実用的な音質として成立すること。

## 29.4 Unit D — Wave Sequence / Final Integration

### 目的

複数Audio Assetを時間方向へ構成するGeneratorを完成させ、Phase全体を統合する。

### 作業順

1. Definition
2. Validation
3. Step Compile
4. Current / Next Playback Slot
5. Seconds Duration
6. Beats Duration
7. Forward
8. Reverse
9. Ping Pong
10. Sequence Loop
11. Step Playback Loop
12. Step Crossfade
13. Pitch / Gain
14. Stereo / Mono混在
15. Missing Step
16. CLI Inspect
17. Test
18. Final Hybrid Instrument
19. Final Review Package
20. Document更新

---

# 30. 主な変更箇所

```text
crates/sonalloy-core/
├─ src/
│  ├─ asset.rs
│  ├─ compiler.rs
│  ├─ definition.rs
│  ├─ diagnostics.rs
│  ├─ generator_parameters.rs
│  ├─ parameter.rs
│  ├─ process.rs
│  └─ runtime/
│     ├─ voice.rs
│     ├─ sample.rs
│     └─ generator/
│        ├─ mod.rs
│        ├─ granular.rs
│        └─ wave_sequence.rs
│
├─ tests/
│  └─ core_process.rs
│
crates/sonalloy-dsp-sys/
├─ build.rs
├─ src/
│  └─ lib.rs
└─ tests/
│
native/
├─ daisysp-wrapper/
└─ signalsmith-stretch-wrapper/
   ├─ CMakeLists.txt
   ├─ include/
   └─ src/
│
crates/sonalloy-cli/
├─ src/
│  ├─ main.rs
│  └─ midi.rs
└─ tests/
│
examples/
├─ assets/
└─ instruments/
│
testdata/
├─ assets/
├─ definitions/
└─ midi/
│
scripts/review/
│
review-output/
└─ advanced-sampling-granular/
```

実際の責務が小さい場合は既存Fileへ統合する。

計画書の分類だけを理由に細かいFileを大量に作らない。

---

# 31. Document更新

## `README.md`

現在利用可能な方式を更新する。

## `docs/instrument-definition.md`

追加：

* Stereo Sample
* Playback Direction
* Loop / Crossfade
* Layer Trigger Event
* Time Stretch
* Source BPM
* Granular
* Wave Sequence
* Parameter ID

## `docs/runtime-processing.md`

追加：

* Stereo Sample Runtime
* Armed Release Layer
* Time Stretch
* Tempo Context
* Latency
* Grain Lifecycle
* Sequence Lifecycle

## `docs/architecture.md`

追加：

* Prepared Audio
* Signalsmith Native境界
* Granular Generator
* Wave Sequence Generator

## `docs/cli.md`

追加：

* Tempo
* Latency
* Inspect内容

## `docs/creating-an-instrument.md`

追加例：

* Release Sample
* Tempo Sync Loop
* Granular Pad
* Wave Sequence

## `docs/testing-and-sound-review.md`

新Review Packageを追加する。

## `.agents/skills/create-instrument/SKILL.md`

AIが、

* Sample Playback
* Granular
* Wave Sequence
* Time Stretch

を正しいDefinitionで生成できるよう更新する。

`docs/CONCEPT.md`は実装結果が現在の正本と矛盾しない限り、本Phase実装完了報告だけを理由に書き換えない。

---

# 32. 対象外

本Phaseでは次を実装しない。

### Sampling Authoring

* Transient Detection
* Automatic Slice
* Beat Detection
* Sample Editor
* Destructive Editing
* Disk Streaming
* Large Sample Streaming
* SFZ Import
* Kontakt Import
* Sample Relink UI

### Time Stretch

* Formant Shift
* User-defined Frequency Map
* Reverse Time Stretch
* Ratio 0.5未満 / 2.0超の品質保証
* Time Stretch Quality Mode選択

### Granular

* Spectral Grain
* FFT Grain
* User Window
* Probability Grain
* Audio-rate Grain Routing
* Unlimited Grain Count

### Wave Sequence

* Random Step
* Probability
* Conditional Branch
* Per-Step Automation
* Nested Sequence
* Arbitrary Step Graph

### Generator

* Additive
* Spectral / Resynthesis
* Physical / Modal / Waveguide
* Formant

### Modulation

* MSEG
* Step Modulator
* Macro
* Tempo-synced LFO
* Generator固有Modulation Source追加

### Processor

* EQ
* Comb
* Resonator
* Chorus
* Flanger
* Phaser
* Convolution
* Dynamics
* Vocoder

### Frontend / Adapter

* Realtime Audio Device
* Realtime MIDI Device
* Public C API
* Riffra
* CLAP
* VST3
* GUI

---

# 33. 完了条件

以下をすべて満たした時点で本Phaseを完了とする。

## Build

* Workspace Build成功
* Release Build成功
* Format成功
* Clippy Warning 0
* 全Test成功
* Windows CI成功
* Linux CI成功

## Audio Asset / Sample

* Mono / Stereo保持
* Reverse
* Loop
* Crossfade Loop
* Release Trigger
* Existing Sample回帰なし

## Time Stretch

* Signalsmith Native統合
* Pitch / Duration分離
* Fixed Stretch
* Tempo Sync
* MIDI Tempo Change
* Stereo
* Latency報告
* Layer Alignment
* Process Allocation 0

## Granular

* Position
* Grain Size
* Density
* Pitch
* Randomness
* Pan Spread
* Stereo
* Scrub
* Freeze
* Determinism
* Allocation 0

## Wave Sequence

* Forward
* Reverse
* Ping Pong
* Loop
* Crossfade
* Seconds
* Beats
* Tempo Change
* Step Pitch / Gain
* Missing Step
* Stereo / Mono混在

## Quality

* Block Size Review成功
* Sample Rate Review成功
* Fresh Runtime比較成功
* Reset成功
* Voice Stealing成功
* Native Fault Injection成功
* Sample Sound Review承認
* Time Stretch Sound Review承認
* Granular Sound Review承認
* Wave Sequence Sound Review承認
* Final Hybrid Instrument承認

---

# 34. 本Phase完了後の境界

本Phase完了時点で、Sonalloyの主要なAudio Material系は次まで進む。

```text
Sampling
├─ One-shot
├─ Multi Sample
├─ Key / Velocity Mapping
├─ Round Robin
├─ Slice
├─ Mono / Stereo
├─ Reverse
├─ Loop
├─ Crossfade Loop
├─ Release Trigger
├─ Time Stretch
└─ Tempo Sync

Material-based Generator
├─ Sample
├─ Granular
└─ Wave Sequence

Digital Synthesis
├─ Basic / Complex Oscillator
├─ Noise
├─ Wavetable
└─ Operator Modulation
```

ここまでで、

> 音をゼロから合成する能力

と、

> 既存Audio素材をInstrumentへ変換・再構成する能力

の双方がかなり揃う。

次のGenerator拡張では、このPhaseで扱った時間領域のSample処理へ機能を足すのではなく、

* Additive
* Spectral / Resynthesis
* Physical / Modal / Waveguide
* Formant

など、異なる音生成原理を扱う領域へ進める。
