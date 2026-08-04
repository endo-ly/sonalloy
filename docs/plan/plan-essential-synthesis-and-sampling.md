# Sonalloy Essential Synthesis / Sampling Expansion 詳細設計・実装計画

- **対象Repository**：`endo-ly/sonalloy`
- **正本要件**：`docs/CONCEPT.md`
- **前提実装**：Instrument Definition、Compile、Dynamic Parameter / Modulation、Processor Chain、CLI Offline Render
- **ロードマップ上の扱い**：旧Essential Synthesisと旧Sampling Expansionを統合した新しいP5
- **実装単位**：三単位。ただしBranchとPull Requestは一つに固定する
- **用途**：実装エージェントへ渡す詳細設計・実装計画
- **文書言語**：日本語。型名、API名、Parameter ID、File Pathのみ英語を使用する
- **成果物**：Markdownのみ。HTML版は作成しない

---

## 0. この計画書の位置づけ

本書は、現在のOscillator GeneratorとSample Generatorを、基本的な電子音生成と実用的なSample Instrumentを構築できる範囲まで拡張するための、Definition、Compile、Parameter、Runtime、CLI、Test、Sound Reviewの契約を定義する。

現在のGeneratorは次へ限定されている。

- Sine Oscillator
- PolyBLEP Saw Oscillator
- 単一Assetを使用するOne-shot Sample
- 一つのRoot Note
- Four-point Cubic Interpolation

新しいP5では、旧ロードマップのEssential SynthesisとSampling Expansionを一つの実装対象として扱い、次の三単位を一つのPull Request内で順番に完成させる。

1. **Basic Generator Expansion**
   - Square
   - Triangle
   - Pulse
   - PWM
   - White / Pink / Brown Noise
2. **Complex Oscillator**
   - Hard Sync
   - Waveshaping
   - Unison
   - Detune
   - Stereo Spread
   - Phase Distribution
3. **Sample Instrument Expansion**
   - Multi Sample Zone
   - Key Mapping
   - Velocity Layer
   - Deterministic Round Robin
   - Forward Loop
   - Explicit Slice

三単位は別Pull Requestへ分けない。各単位の完了時点でTestとReview Packageを成立させるが、最終的なMerge判定は三単位すべてが完成した一つのPull Requestに対して行う。

### 0.1 恒久的な機能名称

コード、Definition、CLI、現在仕様のDocumentでは、進行管理上の番号ではなく次の名称を使用する。

- `Oscillator Generator`
- `Noise Generator`
- `Sample Generator`
- `Sample Zone`
- `Round Robin Group`
- `Forward Loop`
- `Sample Region`
- `Unison`
- `Hard Sync`
- `Waveshaping`

`P5`、旧`P5`、旧`P6`などの番号は本Planの説明以外へ残さない。

### 0.2 実装判断の優先順位

判断に迷った場合は次の順序で優先する。

1. `docs/CONCEPT.md`
2. 本書で固定するDefinition、Generator責務、Zone選択、Lifecycle
3. 現在のProcess Contract、Parameter、Modulation、Processor Chain契約
4. 音質と人間による試聴結果
5. Realtime Safety、決定性、Block Size非依存性
6. 現在のRepository構造と依存方向
7. 実装の単純さ
8. 将来のGenerator追加

将来のWavetable、FM、Granular、Spectral、Time Stretchを理由に、自由なAudio Graph、汎用Generator Node Framework、動的Plugin登録、Trait Object中心のBackend Registryを導入しない。

一方で、Square、Triangle、Pulse、PWM、Hard Syncの音質を安易なNaive Oscillatorで済ませない。既存DaisySP内で利用できるAnti-aliased Oscillatorを優先し、SonalloyはDefinition、Parameter、Unison、Lifecycle、Error処理を所有する。

### 0.3 本書で固定するもの

- DSPとAsset処理の依存方針
- DaisySPの利用ModuleとNative Wrapper境界
- 新しい外部Dependencyを追加しない方針
- Generator Definitionの現在形
- Oscillator、Noise、SampleのParameterと値域
- Mono GeneratorとStereo GeneratorのLayer処理
- Hard Sync、Waveshaping、Unisonの信号順序
- Noiseの決定的生成とColor方式
- Sample Zoneの保持情報
- Zone重複、Velocity Layer、Round Robinの選択規則
- LoopとSliceの再生規則
- Asset Decode、共有、Cache、Partial Compile
- Parameter ID、Modulation、Smoothing
- Prepare、Process、Reset、Voice Stealing、Error時の挙動
- 三実装単位と一つのPull Requestの進め方
- Unit Test、Integration Test、Sound Review
- 完了条件

### 0.4 本書で固定しないもの

次は実装しない。

- Wavetable
- FM、PM、AM、Ring Mod
- Granular
- Additive
- Spectral / Resynthesis
- Modal / Waveguide
- Formant Generator
- Phase Distortion
- Wavefold専用Generator
- Oscillator Feedback
- Arbitrary Audio-rate Modulation Routing
- Oscillator間の自由接続
- User-defined Oscillator Graph
- Arbitrary Formula / DSP Script
- Stereo Sample Assetの保持
- Sample Streaming
- Disk Streaming
- Sample Start Position Automation
- Sample Scrub
- Sample Freeze
- Reverse Playback
- Ping-pong Loop
- Loop Crossfade
- Release Sample
- Key-switch Articulation
- Articulation Dimension
- Tempo Sync Loop
- Pitch ShiftとTime Stretchの分離
- Transient自動検出
- Slice自動検出
- Slice順序Randomization
- SFZ Import
- Kontakt Import
- Realtime Audio Device
- Realtime MIDI Device
- Riffra統合
- Public C ABI
- CLAP、VST3
- GUI
- Preset Migration
- Deprecated Field
- Legacy Definition Alias
- 旧Sample Definitionの互換読込
- `schema_version = 2`

---

# 1. DSP・依存実装方針

新しいP5の実装前に、各機能を既存Dependency、同Dependency内の追加Module、Rust独自実装、新しい外部Dependencyのどれで実現するかを固定する。

## 1.1 結論

| 機能 | 実装方式 | Dependency変更 | 判断 |
|---|---|---|---|
| Sine | 既存DaisySP `Oscillator` | なし | 現在の出力とPhase Reset契約を維持する |
| Saw | 既存DaisySP `Oscillator::WAVE_POLYBLEP_SAW` | なし | Processor Chain以前のBaselineを維持する |
| Square | DaisySP `Oscillator::WAVE_POLYBLEP_SQUARE` | Wrapper API拡張のみ | Naive Squareを新規実装しない |
| Triangle | DaisySP `Oscillator::WAVE_POLYBLEP_TRI` | Wrapper API拡張のみ | Band-limited Triangleを利用する |
| Pulse / PWM | DaisySP PolyBLEP Square + `SetPw` | Wrapper API拡張のみ | Pulse WidthをSampleごとにRamp可能にする |
| Hard Sync | DaisySP `VariableShapeOscillator` | `variableshapeosc.cpp`をBuild対象へ追加 | BLEP補正を持つ既存実装を利用し、単純な強制Phase Resetを独自実装しない |
| Waveshaping | Rust独自実装 | なし | Generator内部の小さな正規化Nonlinear処理として実装する |
| Unison | Rust Runtime + 複数DaisySP Handle | なし | Voice / Layer所有、Detune、Pan、NormalizationをSonalloyが管理する |
| White Noise | Rust独自PRNG | なし | 一Sample一更新の決定的Streamを実装する |
| Pink Noise | Rust独自Voss-McCartney方式 | なし | 新しいNoise Libraryを導入しない |
| Brown Noise | Rust独自Leaky Integration | なし | Sample Rate依存係数をPrepare時に計算する |
| Multi Sample Decode | 既存`sha2` / `symphonia` / `rubato` / `Arc` | なし | 現在のAsset PipelineをZone単位へ拡張する |
| Asset Cache | Rust標準`HashMap`、Compile時限定 | なし | 同一AssetのDecode / Resample / Memory重複を避ける |
| Loop / Slice | Rust Sample Runtime | なし | Prepared Buffer上のRegionとCursorとして実装する |
| Round Robin | Rust独自Runtime State | なし | Instrument Scopeで決定的Counterを所有する |
| 新しい外部Crate | 追加しない | なし | 現在の機能は既存Dependencyと限定的独自実装で成立する |

## 1.2 DaisySP固定Commit

DaisySPは現在と同じCommitを維持する。

```text
a0494a3adb67f549e18dfd71a35fa656f65b38b6
```

Commit更新はこのPull Requestの対象外とする。

理由：

- Oscillatorの音質変更と機能追加を同時に行うと回帰原因を切り分けられない
- 現在のSine / Saw Baselineを維持する必要がある
- `VariableShapeOscillator`は固定Commit内に存在する
- License条件を変更しない
- Native Build再現性を維持する

## 1.3 DaisySP Build対象

現在のBuild対象：

```text
Source/Synthesis/oscillator.cpp
Source/Filters/svf.cpp
```

新しいBuild対象：

```text
Source/Synthesis/oscillator.cpp
Source/Synthesis/variableshapeosc.cpp
Source/Filters/svf.cpp
```

追加するDaisySP Sourceは`variableshapeosc.cpp`だけとする。

次を追加しない。

- Upstream DaisySP aggregate target
- 全Source一括Build
- Noise Module
- Wavefolder Module
- Reverb Module
- Delay Module
- External Mutable Instruments Repository

`VariableShapeOscillator`はDaisySP固定Commit内のMIT License対象Sourceとして利用する。元実装に含まれる著作者表記とDaisySP Noticeを維持する。

## 1.4 Native Wrapperの境界

既存`DspOscillator`はSine、Saw、Square、Triangle、Pulseへ拡張する。

追加する責務：

- Waveform enumの追加
- Pulse Width設定
- Pulse Width Ramp
- 任意PhaseへのReset
- FrequencyとPulse Widthの同時Ramp
- 既存Fault Injection、無音化、Error Code契約の維持

Hard Sync用には別のOpaque Handleを追加する。

概念API：

```text
DspVariableOscillator
├─ prepare(sample_rate, waveform_shape)
├─ reset()
├─ process(master_frequency, slave_frequency, pulse_width)
├─ process_ramp(master_start, master_end,
│               slave_start, slave_end,
│               pulse_width_start, pulse_width_end,
│               output)
└─ fault injection test hook
```

`VariableShapeOscillator`には任意Phaseを指定する公開Reset APIがないため、Hard Sync HandleのResetはNative Wrapper内で`Init(sample_rate)`を再実行し、Waveform Shape、Sync有効状態、現在のStatic設定を復元する。これはHeap Allocationを伴わず、Note Onの`phase_reset = true`とInstrument Resetで使用する。`phase_reset = false`のNote Onでは再初期化しない。

既存`DspOscillator`へHard Sync用Stateを混在させない。

理由：

- 通常OscillatorのBaseline経路を変更しない
- Hard SyncだけがMaster / Slave二周波数を必要とする
- `VariableShapeOscillator`は通常`Oscillator`と内部Stateが異なる
- Rust側でBackend Typeを明示できる
- 一つの巨大Native Handleを作らない

## 1.5 WaveformとBackendの対応

| Definition | 通常 | Hard Sync有効 |
|---|---|---|
| Sine | `DspOscillator::Sine` | 不可 |
| Saw | `DspOscillator::PolyBlepSaw` | `DspVariableOscillator` shape 0.5 |
| Square | `DspOscillator::PolyBlepSquare`、PW 0.5 | `DspVariableOscillator` shape 1.0、PW 0.5 |
| Triangle | `DspOscillator::PolyBlepTriangle` | `DspVariableOscillator` shape 0.0、PW 0.5 |
| Pulse | `DspOscillator::PolyBlepSquare`、Dynamic PW | `DspVariableOscillator` shape 1.0、Dynamic PW |

Sine + Hard SyncはDefinition Validation Errorとする。

暗黙にSawへ変更しない。Hard Syncを無効化して継続しない。

## 1.6 Waveshaping

WaveshapingはDaisySPの別Moduleを導入せず、Rustで実装する。

信号順序：

```text
Oscillator Component生成
        ↓
Unison Mix / Stereo Placement
        ↓
Waveshaping
        ↓
Layer Processor Chain
```

処理式：

```text
shape = 1 + amount × 3
wet = tanh(shape × input) / tanh(shape)
output = input + (wet - input) × amount
```

要件：

- `amount = 0`で厳密にIdentity
- `amount`は0〜1
- 正負対称
- 有限入力から有限出力
- Mono / Stereoで同じ式
- Span内でSampleごとに補間
- Hidden Output Limiterを追加しない
- Oversamplingはこの実装単位では追加しない

WaveshapingはProcessor Driveとは配置が異なる。

- Generator Waveshaping：Unison Mix直後、Layer Processor前
- Drive Processor：Layer / Voice / Global Chain内

処理式が類似しても、Definition、Parameter ID、State所有、信号位置を統合しない。

高いAmountと高音域でAliasingが明確に使用不能な場合は、Sound Review不合格としてこの機能を完了扱いにしない。別Dependencyをその場で追加せず、同Pull Request内で処理式または限定的Oversamplingの再設計を行い、本Planを更新する。

## 1.7 Noise

`rand` Crateを追加しない。

理由：

- 必要なのは暗号学的Randomではなく決定的Audio Streamである
- Seed、Note ID、Layer ID、Channelを明示的に混合する必要がある
- 一SampleごとのState更新契約をSonalloy側で固定する必要がある
- Block Sizeと関係なく同じSequenceを得る必要がある

PRNGはRuntime Module内の小さな整数Stateとして実装する。

要件：

- 全Zero Stateを使用しない
- 同じSeed、Note ID、Layer ID、Resetから同じ出力
- 異なるNote IDで異なるStream
- 一Sampleにつき決まった回数だけ更新
- Process中Allocationなし
- `f32`変換後は-1〜1

## 1.8 Sample Asset

既存Dependencyを継続使用する。

| 処理 | Dependency |
|---|---|
| Path解決 / File Read | Rust標準Library |
| SHA-256 | `sha2` |
| WAV Probe / Decode | `symphonia` |
| Mono Downmix | Sonalloy独自実装 |
| Engine Sample Rateへの変換 | `rubato` |
| Prepared Buffer共有 | `Arc<[f32]>` |
| Definition / JSON | `serde` / `serde_json` |

新しいSample Format Decoder、SFZ Parser、Streaming Libraryは追加しない。

## 1.9 Asset Cache

同一Sample Generator内または別Layerで同じAssetを複数Zoneが参照できるため、Compile中だけ使用するAsset Cacheを追加する。

Cache Key：

- 解決済みの正規化Path
- 指定SHA-256
- Process Sample Rate

Cache Value：

- `Arc<PreparedSample>`
- Decode / Source Metadata
- Asset Diagnostic結果

Process中にCacheを参照しない。Compiled Zoneが`Arc<PreparedSample>`を直接保持する。

## 1.10 新しい外部Dependencyを追加しない理由

### DSP Framework

FunDSP、SynFX、Surge DSP等は、次の理由で導入しない。

- SonalloyのGenerator / Parameter / Runtime責務と重複する
- 一部機能だけのために大きなAPI面を持ち込む
- Reset、Fault、Allocation、Block契約の検証範囲が増える
- DaisySPとの二重Backendになる

### Sampler / SFZ Library

導入しない。

- Sonalloy Definitionが正本である
- SFZのOpcode全体をProduct責務へ持ち込む必要がない
- ImportとRuntimeを同時に扱うとScopeが拡大する
- Zone、Loop、Round Robinは現在のModelで直接表現できる

### Random Library

導入しない。

- Audio Noise StreamのAlgorithmとSeed規則を固定したい
- 依存のRandom Algorithm変更でRender結果を変えたくない

### Realtime Resampler

導入しない。

- Sample AssetはCompile時にEngine Sample Rateへ変換済みである
- RuntimeはPitch Mapping用のCubic Interpolationだけを行う
- Time Stretchは対象外である

## 1.11 LicenseとNotice

- DaisySP固定Commitを維持する
- `variableshapeosc.cpp`利用をNative BuildとNoticeへ反映する
- 新しいLicense種別を追加しない
- `Cargo.lock`へ新しいProduct Packageを追加しない
- `THIRD_PARTY_NOTICES.md`は実際の利用Moduleを示すために必要な範囲だけ更新する
- Dependency選定の長い経緯を恒久Documentへ記載しない

---

# 2. 機能と依存一覧

## 2.1 直接Dependency

| Dependency | Version / Pin | License | 現在用途 | 新P5での変更 |
|---|---|---|---|---|
| DaisySP | Commit `a0494a3adb67f549e18dfd71a35fa656f65b38b6` | MIT | Oscillator、SVF | `variableshapeosc.cpp`を追加し、Square / Triangle / Pulse / Hard Syncへ利用 |
| `cmake` | 現在Pin | MIT OR Apache-2.0 | Native Build | Source一覧だけ更新 |
| `serde` | 現在Pin | MIT OR Apache-2.0 | Definition | Generator / Zone Definitionを更新 |
| `serde_json` | 現在Pin | MIT OR Apache-2.0 | CLI / Test JSON | 新DefinitionとInspectを更新 |
| `thiserror` | 現在Pin | MIT OR Apache-2.0 | Error | Generator / Zone Errorを既存契約へ追加 |
| `sha2` | 現在Pin | MIT OR Apache-2.0 | Asset Hash | Zone Assetごとに利用 |
| `symphonia` | 現在Pin | MPL-2.0 | WAV Decode | Asset Cache経由で複数Zoneへ利用 |
| `rubato` | 現在Pin | MIT OR Apache-2.0 | Compile時Resample | CacheされたPrepared Sample生成へ利用 |
| `clap` | 現在Pin | MIT OR Apache-2.0 | CLI | Inspect表示のみ拡張 |
| `midly` | 現在Pin | MIT | MIDI | 変更なし |
| `hound` | 現在Pin | Apache-2.0 | WAV Encode | Review Package生成に継続利用 |
| `approx` | 現在Pin | MIT OR Apache-2.0 | Test | DSP / Region Testで利用 |
| `assert_cmd` / `predicates` / `tempfile` | 現在Pin | 現在License | CLI Test | 新Definition Testで利用 |

## 2.2 新規実装機能

| 実装単位 | 領域 | 機能 | 実装・依存 | 主な責務 |
|---|---|---|---|---|
| 1 | Oscillator | Square | DaisySP PolyBLEP | Band-limited波形 |
| 1 | Oscillator | Triangle | DaisySP PolyBLEP | Band-limited波形 |
| 1 | Oscillator | Pulse | DaisySP PolyBLEP Square | Pulse Widthを持つ波形 |
| 1 | Oscillator | PWM | DaisySP + 既存Modulation | Pulse Width Ramp |
| 1 | Noise | White | Rust独自 | 決定的PRNG |
| 1 | Noise | Pink | Rust独自 | Voss-McCartney State |
| 1 | Noise | Brown | Rust独自 | Leaky Integrator |
| 1 | Noise | Stereo Correlation | Rust独自 | Shared / Independent Stream Mix |
| 2 | Oscillator | Hard Sync | DaisySP VariableShapeOscillator | BLEP補正付きMaster / Slave |
| 2 | Oscillator | Waveshaping | Rust独自 | Generator内部Nonlinear処理 |
| 2 | Oscillator | Unison | Rust独自Runtime + DaisySP複数Handle | Count、Detune、Phase、Pan、Mix |
| 2 | Layer | Stereo Generator Signal | Rust独自 | Stereo Unison / NoiseをLayer Chainへ渡す |
| 3 | Sampling | Multi Sample Zone | 既存Asset基盤拡張 | Note / VelocityからZone選択 |
| 3 | Sampling | Velocity Layer | Rust独自Selection | 非重複Velocity Range |
| 3 | Sampling | Round Robin | Rust独自State | Definition順の決定的選択 |
| 3 | Sampling | Forward Loop | Rust独自Sample Runtime | Fractional CursorのLoop |
| 3 | Sampling | Explicit Slice | Sample Zone Region | 同一Assetの部分再生 |
| 3 | Sampling | Asset Cache | Rust標準HashMap | Decode / Resample / Memory共有 |

## 2.3 既存機能への接続

新機能は次の既存契約を再利用する。

- Layer Trigger
- Layer Gain / Pan / Tuning
- ADSR
- Note Start Fade
- Voice Allocation
- Voice Stealing
- Stable Parameter ID
- Parameter Catalog
- Parameter Change Event
- Smoothing
- 32 Frame Control Quantum
- Modulation Source / Route
- Layer Processor Chain
- Voice Processor Chain
- Global Processor Chain
- Compile時Asset Preparation
- Structured Diagnostic
- CLI Validate / Inspect / Render
- Existing Review Package

新しいAutomation System、Event Type、Graph Schedulerを作らない。

---

# 3. 目的と完成像

## 3.1 成立させること

```text
Instrument Definition
    │
    ├─ Oscillator Generator
    │    ├─ Basic Waveform
    │    ├─ Pulse Width / PWM
    │    ├─ Hard Sync
    │    ├─ Unison
    │    └─ Waveshaping
    │
    ├─ Noise Generator
    │    ├─ White / Pink / Brown
    │    └─ Stereo Correlation
    │
    └─ Sample Generator
         ├─ Zones
         ├─ Key / Velocity Mapping
         ├─ Round Robin
         ├─ One-shot Region
         └─ Forward Loop
    │
    ▼
Compile
    │
    ├─ Definition / Range / Overlap Validation
    ├─ Parameter Handle解決
    ├─ Asset Cache / Decode / Resample
    ├─ Zone Table / Round Robin Group構築
    └─ Runtime固定値計算
    │
    ▼
Prepare
    │
    ├─ Voice × Layer Generator State
    ├─ Unison Oscillator Handle
    ├─ Noise State
    ├─ Sample Cursor State
    ├─ Round Robin Counter
    └─ Mono / Stereo Scratch
    │
    ▼
Process
    │
    ├─ Note On時にZoneとRound Robinを確定
    ├─ GeneratorをMonoまたはStereo Render
    ├─ Generator ParameterをRamp
    ├─ Layer Processor
    ├─ Envelope / Gain / Pan
    ├─ Voice / Global Processor
    └─ Stereo Output
```

## 3.2 完成状態

> Square、Triangle、Pulse / PWM、Noise、Hard Sync、Waveshaping、Unisonを既存Dynamic Parameter / Modulationから制御でき、複数Sample Zone、Velocity Layer、決定的Round Robin、Forward Loop、Explicit Sliceを持つSample Instrumentを、同じInstrument Definition内でProcessor Chainと組み合わせ、決定的かつBlock Size非依存なStereo WAVとしてOffline Renderできる。

## 3.3 設計上の価値

- Generator Typeが増えてもVoice Engine全体へType別分岐を散在させない
- Basic Oscillatorの既存Sine / Saw Baselineを維持する
- Hard Syncだけを別Backendとして明示する
- Unison Countに応じたStateをPrepare時に固定する
- Stereo Generatorを自由GraphなしでLayer Pipelineへ接続する
- NoiseがBlock境界でSequenceを変えない
- Sample LayerとSample Zoneの責務を分離する
- Zone選択をNote On時に一度だけ行う
- Round RobinがVoice数、Block Size、Voice Stealingで不定にならない
- 同一Assetを複数Zoneで共有する
- Loop / SliceがAsset DecodeとRuntime Cursorの責務を混在させない
- Process中にPath、String ID、JSON、Asset Decode、HashMap検索を行わない

## 3.4 Reference Instrument

最終Reference Instrumentとして`Essential Hybrid Instrument`を追加する。

```text
Layer 1: Pulse Body
  Pulse + PWM
  Unison 5 Voices
  Waveshaping
  Layer Filter

Layer 2: Hard Sync Attack
  Saw Hard Sync
  Short Envelope
  Layer Drive

Layer 3: Noise Texture
  Pink Noise
  Stereo Correlation
  Low Gain

Layer 4: Sample Attack / Texture
  Multi Sample Zones
  Velocity Layer
  Round Robin
  Explicit Regions

Voice Processor
  Filter
  Drive

Global Processor
  Delay
  Reverb
```

別のSampling Referenceとして`Mapped Sample Instrument`を追加する。

- 低域、中域、高域のKey Zone
- Soft / Hard Velocity Layer
- 各層二つのRound Robin
- Sustain用Forward Loop
- Drum / Vocal Chop用Explicit Slice

---

# 4. 対象範囲

## 4.1 実装単位1：Basic Generator Expansion

含める。

- Square
- Triangle
- Pulse
- Pulse Width
- PWM Modulation
- White Noise
- Pink Noise
- Brown Noise
- Noise Seed
- Stereo Correlation
- Generator Parameter ID
- Generator Parameter Smoothing
- Mono / Stereo Generator出力
- Native Wrapper Test
- Sound Review

## 4.2 実装単位2：Complex Oscillator

含める。

- Hard Sync
- Dynamic Sync Ratio
- Waveshaping Amount
- Unison Voice Count
- Dynamic Detune
- Dynamic Stereo Spread
- Static Phase Distribution
- Equal-power Component Pan
- Unison Normalization
- Basic / Hard Sync Backend選択
- Voice Stealing / Reset
- Polyphony / CPU Regression
- Sound Review

## 4.3 実装単位3：Sample Instrument Expansion

含める。

- Sample GeneratorのZone配列化
- Zone ID
- Zone Asset
- Root Note
- Key Range
- Velocity Range
- Optional Round Robin Group
- One-shot Sample Region
- Forward Loop
- Explicit Slice表現
- Zone Overlap Validation
- Deterministic Round Robin Counter
- Asset Cache
- Missing Asset Partial Compile
- Zone選択のVoice Stealing統合
- CLI Inspect
- Sound Review

## 4.4 対象外

第0.4節の項目に加え、次を対象外とする。

- Unison Voiceごとの個別Definition
- User指定Detune Table
- User指定Pan Table
- Chord Memory
- Supersaw専用Algorithm
- Oscillator Drift
- Analog Random Drift
- Per-voice Random Detune
- Hard Sync Master Waveform選択
- Hard Sync Feedback
- Waveshaper Curve選択
- Oversampling設定
- Noise Filter Cutoff Parameter
- Noiseの任意Spectral Tilt
- Stereo Sample Decode
- Zone Crossfade
- Velocity Crossfade
- Round Robin Random Mode
- Round Robin Probability
- Round Robin Reset Policy選択
- Multiple Zone同時Layering
- Release Trigger Zone
- Round Robin GroupのLayer跨ぎ共有
- Loop Crossfade
- Loop Mode選択
- Slice自動生成

## 4.5 品質要件

1. 既存Sine / SawのDefinitionを新Schemaへ更新した後、同じ設定から明確な音質回帰を出さない
2. Square、Triangle、Pulseは高域でNaive Waveformより明確にAliasingを抑える
3. PWM変更でClickまたはBlock境界の不連続を出さない
4. Hard SyncはRatio変更時にも有限で、明確なPitch Jump以外の破綻を出さない
5. UnisonはVoice数増加で不自然なLevel Explosionを起こさない
6. Stereo Spread 0と1で明確な幅の差がある
7. NoiseはColorごとに聴感上の差があり、Reset後に再現する
8. Sample Zone選択がKey / Velocity / Round Robin規則と一致する
9. Loopの時間位置がBlock Sizeで変わらない
10. Explicit Sliceが指定Region外を再生しない
11. Missing Zone Assetが別の有効Zoneまたは別Layerを壊さない
12. Process中にAllocation、File I/O、JSON、HashMap検索を行わない
13. 44.1 / 48 / 96 kHzで有限出力を維持する
14. Block Size 32 / 64 / 257 / 1024で時間軸と選択結果を維持する
15. Reference Instrumentが技術検査だけでなく音色として成立する

---

# 5. GeneratorとLayer信号経路

## 5.1 全体

```text
Note On
  ↓
Layer Trigger
  ↓
Generator Note Selection / Reset
  ↓
Generator Render
  ├─ Mono Generator
  └─ Stereo Generator
  ↓
Layer Processor Chain
  ↓
Amplitude Envelope / Note Start Fade
  ↓
Layer Gain
  ↓
Layer Pan / Stereo Balance
  ↓
Voice Mix
  ↓
Voice Processor
  ↓
Voice Sum
  ↓
Global Processor
```

## 5.2 Generator出力形式

Compiled Generatorは出力形式を固定する。

```rust
pub enum GeneratorOutputMode {
    Mono,
    Stereo,
}
```

| Generator | Output Mode |
|---|---|
| Sine / Saw / Square / Triangle / Pulse、Unison 1 | Mono |
| Oscillator Unison 2以上 | Stereo |
| Noise | Stereo |
| Sample | Mono |

Runtime中にOutput Modeを切り替えない。

Stereo SpreadまたはNoise CorrelationがDynamicでも、Compiled Output ModeはStereoのままとする。

## 5.3 Mono Layer経路

現在の経路を維持する。

```text
Mono Generator
  → Mono Layer Processor Chain
  → Envelope / Gain
  → Constant-power Pan
  → Voice Stereo Mix
```

Sine / Saw、Unison 1、SampleのBaselineはこの経路を使用する。

## 5.4 Stereo Layer経路

```text
Stereo Generator
  → Stereo Layer Processor Chain
  → Envelope / GainをL/Rへ同量適用
  → Stereo Balance
  → Voice Stereo Mix
```

Layerへ配置可能なProcessorは引き続きFilterとDriveだけである。

Stereo Layer Processorは既存Stereo Processor Runtimeを再利用するが、Delay / Reverbを生成しない。

## 5.5 Stereo Balance

Stereo Generatorへ既存Mono Constant-power Panをそのまま適用しない。

中心ではGeneratorのStereo Imageを保持し、左右端では反対Channelを減衰するBalanceとする。

```text
pan <= 0:
  left_gain  = 1
  right_gain = cos(abs(pan) × π / 2)

pan >= 0:
  left_gain  = cos(pan × π / 2)
  right_gain = 1
```

要件：

- Pan 0でL/Rを変更しない
- Pan -1でRightを0
- Pan +1でLeftを0
- Span内でSampleごとに補間
- Mono Panの既存式を変更しない

## 5.6 Generator終了

| Generator | 終了条件 |
|---|---|
| Oscillator | Envelope終了 |
| Noise | Envelope終了 |
| One-shot Sample | Region終端またはEnvelope終了 |
| Forward Loop | Envelope終了 |

Generator終了だけでVoice全体を停止しない。すべてのActive Layerが終了し、Voice Processor出力も終了条件を満たしたときにVoiceをIdleへ戻す既存契約を維持する。

---

# 6. Instrument Definition

## 6.1 Generator Definition

概念Model：

```rust
pub enum GeneratorDefinition {
    Oscillator(OscillatorDefinition),
    Noise(NoiseDefinition),
    Sample(SampleDefinition),
}
```

JSON外形：

```json
{
  "generator": {
    "oscillator": { }
  }
}
```

```json
{
  "generator": {
    "noise": { }
  }
}
```

```json
{
  "generator": {
    "sample": { }
  }
}
```

すべて`deny_unknown_fields`を維持する。

## 6.2 Oscillator Definition

```rust
pub struct OscillatorDefinition {
    pub waveform: OscillatorWaveformDefinition,
    pub phase_reset: bool,
    pub phase: f32,
    pub hard_sync: Option<HardSyncDefinition>,
    pub waveshaping: Option<WaveshapingDefinition>,
    pub unison: Option<UnisonDefinition>,
}
```

### Waveform

文字列EnumをTagged Objectへ置き換える。

```rust
pub enum OscillatorWaveformDefinition {
    Sine,
    Saw,
    Square,
    Triangle,
    Pulse { pulse_width: f32 },
}
```

JSON：

```json
"waveform": { "type": "sine" }
```

```json
"waveform": { "type": "saw" }
```

```json
"waveform": { "type": "square" }
```

```json
"waveform": { "type": "triangle" }
```

```json
"waveform": {
  "type": "pulse",
  "pulse_width": 0.35
}
```

旧文字列形式を互換読込しない。

### 共通Field

| Field | Range | Dynamic | 説明 |
|---|---:|---:|---|
| `phase_reset` | Boolean | 不可 | Note OnでPhaseを初期化するか |
| `phase` | 0〜1 | 不可 | Reset時の開始Phase |

`phase_reset = false`の場合、`phase`はInstrument Reset後の初期Phaseだけに使用する。

### Pulse Width

| Field | Range | Dynamic | Scale | Smoothing |
|---|---:|---:|---|---:|
| `pulse_width` | 0.05〜0.95 | 可 | Linear | 5ms |

SquareはPulse Width 0.5固定であり、`pulse_width` Parameterを持たない。

## 6.3 Hard Sync Definition

```rust
pub struct HardSyncDefinition {
    pub ratio: f32,
}
```

JSON：

```json
"hard_sync": {
  "ratio": 2.0
}
```

| Field | Range | Dynamic | Scale | Smoothing |
|---|---:|---:|---|---:|
| `ratio` | 1〜16 | 可 | Log2 | 5ms |

規則：

- `hard_sync`省略時は無効
- Sineでは使用不可
- Master FrequencyはNote Frequency × Unison Detune
- Slave FrequencyはMaster Frequency × Ratio
- Slave FrequencyはBackend Safe上限へ制限
- Ratioを1未満にしない
- Hard Syncの有効 / 無効はDynamic対象外

## 6.4 Waveshaping Definition

```rust
pub struct WaveshapingDefinition {
    pub amount: f32,
}
```

| Field | Range | Dynamic | Scale | Smoothing |
|---|---:|---:|---|---:|
| `amount` | 0〜1 | 可 | Linear | 5ms |

`waveshaping`省略時は処理自体を構築しない。

Curve選択Fieldを追加しない。

## 6.5 Unison Definition

```rust
pub struct UnisonDefinition {
    pub voices: u8,
    pub detune_cents: f32,
    pub stereo_spread: f32,
    pub phase_spread: f32,
}
```

| Field | Range | Dynamic | Scale | Smoothing |
|---|---:|---:|---|---:|
| `voices` | 2〜8 | 不可 | - | - |
| `detune_cents` | 0〜100 | 可 | Linear | 10ms |
| `stereo_spread` | 0〜1 | 可 | Linear | 10ms |
| `phase_spread` | 0〜1 | 不可 | - | - |

`unison`省略時は一つのOscillator Componentとする。

Hard SyncとUnisonを併用できる。ただしHard Sync Backendは任意Phase Resetを公開しないため、`phase_spread`は0だけを許可する。

Hard Syncなしでは0〜1を許可する。

## 6.6 Oscillator JSON例

### PWM Pulse

```json
{
  "oscillator": {
    "waveform": {
      "type": "pulse",
      "pulse_width": 0.35
    },
    "phase_reset": true,
    "phase": 0.0,
    "hard_sync": null,
    "waveshaping": null,
    "unison": null
  }
}
```

### Hard Sync Unison

```json
{
  "oscillator": {
    "waveform": {
      "type": "saw"
    },
    "phase_reset": true,
    "phase": 0.0,
    "hard_sync": {
      "ratio": 3.0
    },
    "waveshaping": {
      "amount": 0.25
    },
    "unison": {
      "voices": 5,
      "detune_cents": 18.0,
      "stereo_spread": 0.85,
      "phase_spread": 0.0
    }
  }
}
```

`null`を省略可能にするかどうかはSerde表現の一貫性で決める。CLI `instrument init`とReference Definitionでは現在のCanonical出力を一つに固定する。

旧Definition分岐は作らない。

## 6.7 Noise Definition

```rust
pub struct NoiseDefinition {
    pub color: NoiseColorDefinition,
    pub seed: u64,
    pub stereo_correlation: f32,
}
```

```rust
pub enum NoiseColorDefinition {
    White,
    Pink,
    Brown,
}
```

JSON：

```json
{
  "noise": {
    "color": "pink",
    "seed": 812347,
    "stereo_correlation": 0.65
  }
}
```

| Field | Range | Dynamic | Scale | Smoothing |
|---|---:|---:|---|---:|
| `color` | White / Pink / Brown | 不可 | - | - |
| `seed` | `u64` | 不可 | - | - |
| `stereo_correlation` | 0〜1 | 可 | Linear | 10ms |

Correlation：

- 1：左右同一Stream
- 0：左右独立Stream
- 中間：SharedとIndependentを連続Mix

Noiseは常にStereo Output Modeとする。

## 6.8 Sample Definition

現在の単一Asset FieldをZone配列へ置き換える。

```rust
pub struct SampleDefinition {
    pub interpolation: SampleInterpolation,
    pub zones: Vec<SampleZoneDefinition>,
}
```

JSON：

```json
{
  "sample": {
    "interpolation": "cubic",
    "zones": [
      {
        "id": "c3_soft_a",
        "asset": {
          "path": "../assets/c3-soft-a.wav",
          "sha256": "..."
        },
        "root_note": 48,
        "key_min": 36,
        "key_max": 59,
        "velocity_min": 1,
        "velocity_max": 80,
        "round_robin_group": "c3_soft",
        "playback": {
          "type": "one_shot",
          "start_seconds": 0.0,
          "end_seconds": null
        }
      }
    ]
  }
}
```

旧`asset`、`root_note`、`playback_mode`をSample Definition直下で受け付けない。

## 6.9 Sample Zone Definition

```rust
pub struct SampleZoneDefinition {
    pub id: String,
    pub asset: AssetReference,
    pub root_note: u8,
    pub key_min: u8,
    pub key_max: u8,
    pub velocity_min: u8,
    pub velocity_max: u8,
    pub round_robin_group: Option<String>,
    pub playback: SampleZonePlaybackDefinition,
}
```

| Field | Range / Rule |
|---|---|
| `id` | Component ID Grammar、同じSample Generator内で一意 |
| `root_note` | 0〜127 |
| `key_min` / `key_max` | 0〜127、min <= max |
| `velocity_min` / `velocity_max` | 1〜127、min <= max |
| `round_robin_group` | Optional Component ID |
| `asset` | 既存Asset Reference契約 |
| `playback` | One-shotまたはForward Loop |

Zone数は1〜256とする。

空Zone配列はDefinition Validation Errorとする。

## 6.10 Sample Playback Definition

```rust
pub enum SampleZonePlaybackDefinition {
    OneShot {
        start_seconds: f32,
        end_seconds: Option<f32>,
    },
    ForwardLoop {
        start_seconds: f32,
        end_seconds: Option<f32>,
        loop_start_seconds: f32,
        loop_end_seconds: f32,
    },
}
```

### One-shot

```json
"playback": {
  "type": "one_shot",
  "start_seconds": 0.0,
  "end_seconds": null
}
```

### Forward Loop

```json
"playback": {
  "type": "forward_loop",
  "start_seconds": 0.0,
  "end_seconds": 2.4,
  "loop_start_seconds": 0.45,
  "loop_end_seconds": 1.85
}
```

`end_seconds = null`はAsset終端を意味する。

### Region規則

- `start_seconds >= 0`
- `end_seconds`指定時は`end > start`
- Asset Durationを超えるRegionはCompile Error
- Regionを暗黙Clampしない
- Round後に最低2 Frameを必要とする
- Forward LoopはRegion内に完全に含める
- `loop_end > loop_start`
- Loop Lengthは最低2 Frame
- Runtime FrameはEngine Sample Rateへ変換済みのBuffer上で保持する

## 6.11 Sliceの表現

Slice専用Runtime Variantを作らない。

Explicit Sliceは、同じAssetを参照し、異なるOne-shot Regionと単一Key Rangeを持つ複数Zoneとして表現する。

```json
{
  "id": "slice_kick",
  "asset": { "path": "../assets/break.wav" },
  "root_note": 36,
  "key_min": 36,
  "key_max": 36,
  "velocity_min": 1,
  "velocity_max": 127,
  "round_robin_group": null,
  "playback": {
    "type": "one_shot",
    "start_seconds": 0.000,
    "end_seconds": 0.240
  }
}
```

次のNoteは別Zoneとして記述する。

この方式により、Sliceだけの別Selection Modelを作らず、Zone選択、Asset Cache、Pitch Mapping、Region再生を再利用する。

## 6.12 Zone Overlap

曖昧な複数Zone選択を禁止する。

二つのZoneのKey RangeとVelocity Rangeが重なる場合、次をすべて満たす必要がある。

- 両方が同じ非空`round_robin_group`を持つ
- Key Rangeが完全一致する
- Velocity Rangeが完全一致する

一つでも満たさない場合はDefinition Validation Errorとする。

これにより次を明確にする。

- Velocity Layerは非重複Rangeで作る
- Key Zoneは非重複Rangeで作る
- 同じ条件の複数SampleだけをRound Robinにする
- Zone Crossfadeは行わない
- 複数Zoneを同時再生しない

## 6.13 Round Robin Group

同じSample Generator内でGroupを構築する。

規則：

- Group MemberはDefinition順を維持する
- Group Memberは同一Key / Velocity Rangeを持つ
- Note On Eventを受けた時点で選択する
- CounterはInstrument Runtimeに一つだけ所有する
- CounterはGroupごとに独立
- 別Layerの同名Groupは別Group
- `selected = counter % enabled_member_count`
- 選択後にCounterを1増やす
- ResetでCounterを0へ戻す
- Voice StealingでCounterを戻さない
- Pending NoteはEvent時点の選択を保持する
- Missing Assetで無効なMemberは選択候補から除外する

Group Memberが一つだけ有効な場合は常にそのZoneを選ぶ。

Group Memberがすべて無効な場合、その条件ではSample Layerを発音しない。

## 6.14 Velocity Layer

Velocity LayerはZoneの非重複Velocity Rangeとして表現する。

例：

```text
Soft:   1..=70
Medium: 71..=105
Hard:   106..=127
```

Range間のGapは許可する。そのVelocityではSample Layerを発音しない。

Velocity Crossfadeは行わない。

## 6.15 Unknown FieldとSchema境界

- 旧Oscillator Waveform文字列を受け付けない
- 旧Sample Definition直下のAsset Fieldを受け付けない
- Aliasを提供しない
- Migration Commandを提供しない
- Deprecated型を残さない
- Compile時に旧形式を判定する分岐を作らない
- `schema_version = 1`を維持する
- Current Definition、Example、Fixture、Review Definitionを同時更新する

---

# 7. Parameter IDとModulation

## 7.1 Generator Parameter ID

形式：

```text
layer.<layer_id>.generator.<parameter>
```

追加ID：

```text
layer.<layer_id>.generator.pulse_width
layer.<layer_id>.generator.sync_ratio
layer.<layer_id>.generator.waveshape
layer.<layer_id>.generator.unison_detune
layer.<layer_id>.generator.unison_spread
layer.<layer_id>.generator.noise_correlation
```

## 7.2 Parameter対応

| Definition Field | Parameter末尾 | Unit | Scale | Range |
|---|---|---|---|---:|
| Pulse `pulse_width` | `pulse_width` | Normalized | Linear | 0.05〜0.95 |
| Hard Sync `ratio` | `sync_ratio` | Ratio | Log2 | 1〜16 |
| Waveshaping `amount` | `waveshape` | Normalized | Linear | 0〜1 |
| Unison `detune_cents` | `unison_detune` | Cents | Linear | 0〜100 |
| Unison `stereo_spread` | `unison_spread` | Normalized | Linear | 0〜1 |
| Noise `stereo_correlation` | `noise_correlation` | Normalized | Linear | 0〜1 |

Static Field：

- Waveform Type
- Phase Reset
- Phase
- Hard Sync有効 / 無効
- Waveshaping有効 / 無効
- Unison Voice Count
- Unison Phase Spread
- Noise Color
- Noise Seed
- Sample Zone
- Sample Region
- Loop Point
- Round Robin Group
- Interpolation

Static FieldはParameter Catalogへ登録しない。

## 7.3 Parameter Owner

`ParameterOwner`へGenerator Parameterを追加する。

概念：

```rust
ParameterOwner::LayerGenerator {
    layer_index,
    parameter,
}
```

Audio PathではOwnerを検索しない。Compiled GeneratorがHandleを直接保持する。

## 7.4 Catalog順序

一つのLayer内のParameter順序を次で固定する。

1. Layer Gain
2. Layer Pan
3. Layer Tuning
4. Generator Parameter
5. Layer Processor Parameter

Generator Parameter内順序：

1. Pulse Width
2. Sync Ratio
3. Waveshape
4. Unison Detune
5. Unison Spread
6. Noise Correlation

存在するParameterだけを登録する。

Sample Generatorは新しいDynamic Parameterを持たない。

## 7.5 Source Scope

Generator ParameterはLayer Scope Targetである。

許可Source：

- Velocity
- Key Tracking
- LFO
- Modulation Envelope
- Random
- Pitch Bend
- Mod Wheel
- Aftertouch

既存Layer Gain / Pan / Tuningと同じSource Scopeを使用する。

## 7.6 PWM

PWMは新しいModulation Sourceではない。

Pulse Width Parameterへ既存LFO、Envelope、Mod Wheel等を接続することで成立する。

```json
{
  "source": "pwm_lfo",
  "target": "layer.body.generator.pulse_width",
  "amount": 0.35,
  "curve": "linear"
}
```

Pulse以外のWaveformへ`pulse_width` Targetは存在しない。

## 7.7 Hard Sync Sweep

```json
{
  "source": "sync_envelope",
  "target": "layer.attack.generator.sync_ratio",
  "amount": 0.5,
  "curve": "smooth_step"
}
```

RatioはLog2 DomainでModulationする。

## 7.8 Unison Detune

Detune ParameterはUnison全体の最大幅を表す。

各Component OffsetはCompiled Distribution係数にDetune Spanを掛ける。

```text
component_cents = distribution[i] × detune_cents
```

ComponentごとのParameterをCatalogへ登録しない。

## 7.9 Noise Correlation

CorrelationはLinear Domainで評価する。

0〜1へClampする既存Parameter契約を利用する。

Color変更はAutomation対象外とする。

---

# 8. Compiled Model

## 8.1 Compiled Generator

```rust
pub enum CompiledGenerator {
    Oscillator(CompiledOscillator),
    Noise(CompiledNoise),
    Sample(CompiledSample),
}
```

各Compiled Generatorは`GeneratorOutputMode`を返す。

## 8.2 Compiled Oscillator

概念Model：

```rust
pub struct CompiledOscillator {
    pub waveform: CompiledOscillatorWaveform,
    pub phase_reset: bool,
    pub phase: f32,
    pub backend: CompiledOscillatorBackend,
    pub parameters: CompiledOscillatorParameters,
    pub unison: CompiledUnison,
    pub waveshaping: Option<CompiledWaveshaping>,
    pub output_mode: GeneratorOutputMode,
}
```

```rust
pub enum CompiledOscillatorBackend {
    Basic,
    VariableShapeSync,
}
```

### Compiled Waveform

```rust
pub enum CompiledOscillatorWaveform {
    Sine,
    Saw,
    Square,
    Triangle,
    Pulse,
}
```

### Parameter Handle

```rust
pub struct CompiledOscillatorParameters {
    pub pulse_width: Option<ParameterHandle>,
    pub sync_ratio: Option<ParameterHandle>,
    pub waveshape: Option<ParameterHandle>,
    pub unison_detune: Option<ParameterHandle>,
    pub unison_spread: Option<ParameterHandle>,
}
```

## 8.3 Compiled Unison

```rust
pub struct CompiledUnison {
    pub voices: usize,
    pub detune_distribution: Box<[f32]>,
    pub pan_distribution: Box<[f32]>,
    pub phase_distribution: Box<[f32]>,
    pub normalization: f32,
}
```

Prepare前に長さを固定する。

Distribution生成：

```text
voices == 1:
  [0]

voices > 1:
  d[i] = -1 + 2 × i / (voices - 1)
```

Pan Distributionも同じ対称係数を使用する。

Phase Distribution：

```text
phase[i] = wrap(base_phase + phase_spread × i / voices)
```

Normalization：

```text
1 / sqrt(voices)
```

Hidden Limiterを追加しない。Reference InstrumentはLayer GainでPeakを調整する。

## 8.4 Compiled Noise

```rust
pub struct CompiledNoise {
    pub color: NoiseColor,
    pub seed: u64,
    pub correlation: ParameterHandle,
    pub layer_hash: u64,
    pub brown_coefficient: f32,
    pub output_mode: GeneratorOutputMode,
}
```

Sample Rate依存のBrown係数はCompileまたはPrepare前に計算する。

## 8.5 Compiled Sample

```rust
pub struct CompiledSample {
    pub zones: Box<[CompiledSampleZone]>,
    pub groups: Box<[CompiledRoundRobinGroup]>,
    pub interpolation: SampleInterpolation,
    pub output_mode: GeneratorOutputMode,
}
```

## 8.6 Compiled Sample Zone

```rust
pub struct CompiledSampleZone {
    pub id: String,
    pub source: Option<Arc<PreparedSample>>,
    pub root_note: u8,
    pub key_min: u8,
    pub key_max: u8,
    pub velocity_min: u8,
    pub velocity_max: u8,
    pub group: Option<RoundRobinGroupHandle>,
    pub playback: CompiledSamplePlayback,
    pub asset_path: String,
    pub asset_sha256: Option<String>,
    pub enabled: bool,
}
```

## 8.7 Compiled Playback

```rust
pub enum CompiledSamplePlayback {
    OneShot {
        start_frame: usize,
        end_frame: usize,
    },
    ForwardLoop {
        start_frame: usize,
        end_frame: usize,
        loop_start_frame: usize,
        loop_end_frame: usize,
    },
}
```

FrameはPrepared SampleのEngine Sample Rate Bufferを基準にする。

## 8.8 Compiled Round Robin Group

```rust
pub struct CompiledRoundRobinGroup {
    pub id: String,
    pub member_zone_indices: Box<[usize]>,
    pub enabled_member_zone_indices: Box<[usize]>,
}
```

Group HandleはSample Generator内のDense Indexとする。

String検索はProcess中に行わない。

## 8.9 Compiled Modelへ保存しないもの

- Native Oscillator Handle
- Noise PRNG State
- Pink Noise Row State
- Brown Noise Integrator State
- Current Sample Cursor
- Selected Zone
- Round Robin Counter
- Pending NoteのZone Selection
- Scratch Buffer
- Envelope State
- Processor State

これらはRuntimeが所有する。

---

# 9. CompilerとAsset Preparation

## 9.1 Compile順序

1. Instrument Definition全体をValidation
2. Layer / Processor / Zone IDをValidation
3. Oscillator組合せをValidation
4. Generator Parameter RangeをValidation
5. Sample Zone RangeとOverlapをValidation
6. Parameter CatalogをDefinition順に構築
7. Generator Parameter Handleを解決
8. Asset Cacheを初期化
9. Sample Zone AssetをDefinition順にPrepare
10. Region / Loop PointをFrameへ変換
11. Round Robin Groupを構築
12. Unison Distributionを計算
13. Noise固定値を計算
14. Layer ProcessorをCompile
15. Voice / Global ProcessorをCompile
16. Modulation Source / RouteをCompile
17. Route ScopeをValidation
18. ErrorがなければCompiled Instrumentを返す

## 9.2 Oscillator Validation

- Phase 0〜1
- Pulse Width 0.05〜0.95
- Hard Sync Ratio 1〜16
- Sine + Hard Sync拒否
- Waveshaping Amount 0〜1
- Unison Voices 2〜8
- Detune 0〜100 cents
- Stereo Spread 0〜1
- Phase Spread 0〜1
- Hard Sync + Phase Spread非Zero拒否
- Non-finite拒否

## 9.3 Effective Frequency

既存Note Frequency上限を維持する。

Basic Oscillator：

```text
min(requested_frequency, sample_rate × 0.45)
```

Variable Shape Hard Sync：

```text
master <= sample_rate × 0.24
slave  <= sample_rate × 0.24
```

DaisySP内部の0.25 Clampへ暗黙依存せず、Rust Runtimeで明示的にSafe上限を適用する。

Definition値だけではNote Range全体のClamp発生を判断できないため、Compile Warningを乱発しない。CLI InspectでBackend Effective Max Frequencyを表示する。

## 9.4 Sample Asset Cache

Compile Context内へCacheを持たせる。

処理：

1. Definition Pathを解決
2. Cache Keyを構築
3. Cache Hitなら`Arc`をClone
4. Cache MissならFile Read
5. SHA検証
6. Decode
7. Mono Downmix
8. Engine Sample RateへResample
9. Metadataを保存
10. Cacheへ登録
11. Zoneへ`Arc`を渡す

同じPathでも指定SHAが異なる場合は別Keyとし、Hash不一致を正しく検出する。

## 9.5 Region変換

```text
frame = round(seconds × process_sample_rate)
```

- Checked Arithmeticを使用
- Non-finite拒否
- Asset Durationを超える場合はCompile Error
- End nullはPrepared Sample Length
- Start / End / LoopのPathをZone位置まで含める

Diagnostic Path例：

```text
layers[2].generator.sample.zones[4].playback.loop_start_seconds
```

## 9.6 Missing Asset

既存Partial Compile方針をZoneへ拡張する。

- Missing File：Warning、Zone Disabled
- SHA不一致：Warningまたは既存契約に従いZone Disabled
- Decode失敗：Warning、Zone Disabled
- 別Zoneは継続
- 別Layerは継続
- 一つのSample Generatorで全Zone Disabled：Layerは発音不能
- Instrument全体で発音可能Layerがない場合は既存Compile規則に従う

不正Region、Overlap、ID重複はDefinition / Compile Errorであり、Zoneだけを無効化して継続しない。

## 9.7 Asset共有Test

同じAssetを参照する複数Zoneの`Arc`が同一Allocationを共有することをTestする。

Path文字列が異なる相対表現でも同じ正規化Pathへ解決される場合はCache Hitとする。

---

# 10. RuntimeとState所有

## 10.1 Generator Runtime

```rust
pub enum GeneratorRuntime {
    Oscillator(OscillatorRuntime),
    Noise(NoiseRuntime),
    Sample(SampleRuntime),
    Disabled,
}
```

Generator Typeごとの処理を`LayerRuntime::render_source`へ直接増やし続けない。DispatchはGenerator Runtime Moduleへ置く。

## 10.2 Module構成

```text
crates/sonalloy-core/src/runtime/
├─ generator/
│  ├─ mod.rs
│  ├─ oscillator.rs
│  └─ noise.rs
├─ sample.rs
├─ voice.rs
├─ modulation.rs
├─ processor/
└─ ...
```

`generator/mod.rs`：

- Runtime Enum
- Output Mode
- Target Span
- Start / Render / Reset Dispatch

`generator/oscillator.rs`：

- Basic / Variable Shape Handle
- Unison Component
- Detune / Pan / Phase Distribution
- Hard Sync Frequency
- Waveshaping

`generator/noise.rs`：

- PRNG
- White / Pink / Brown State
- Stereo Correlation

`sample.rs`：

- Selected Zone
- Region Cursor
- Loop
- Cubic Interpolation
- End Fade

GeneratorごとにCrateを分けない。

## 10.3 Oscillator Runtime

```rust
pub struct OscillatorRuntime {
    pub components: Vec<OscillatorComponentRuntime>,
    pub waveshaping: bool,
    pub output_mode: GeneratorOutputMode,
}
```

`components`はPrepare時にUnison Voice Countで確保し、その後Capacityを変更しない。

Component：

```rust
pub enum OscillatorComponentRuntime {
    Basic(DspOscillator),
    HardSync(DspVariableOscillator),
}
```

## 10.4 Noise Runtime

保持：

- Shared PRNG
- Left Independent PRNG
- Right Independent PRNG
- Pink State × 3 Stream
- Brown State × 3 Stream
- Current Note-derived Seed
- Color

Note OnごとにSeedを再構成する。

## 10.5 Sample Runtime

保持：

- Selected Zone Index
- Source `Arc<[f32]>`
- Current Fractional Position
- Start / End Frame
- Optional Loop Start / End
- End Fade Frame Count
- Finished

Zone Table全体をVoiceごとにCloneしない。Compiled Sampleを共有し、Runtimeは選択結果とCursorだけを持つ。

## 10.6 Round Robin State

`InstrumentRuntime`がSample GeneratorごとのCounter Tableを所有する。

概念：

```rust
pub struct RoundRobinState {
    counters: Box<[u64]>,
}
```

Compiled Group HandleからDense Indexで参照する。

Process中にGroup ID文字列を検索しない。

## 10.7 Pending Note Selection

Voice Stealing Fade中に新しいNoteがPendingとなる場合、Note Event時点で選択したZoneを保持する。

Voice RuntimeへLayer数分のSelection BufferをPrepare時に確保する。

```text
pending_zone_selection[layer_index] = Option<zone_index>
```

新しいNoteが実際にStartするまで再選択しない。

これによりRound Robinの順番がVoice Steal Fade長に依存しない。

## 10.8 Scratch Buffer

Voice Runtimeは最大Block Sizeに応じて次をPrepare時に確保する。

- Mono Layer Scratch
- Stereo Layer Left Scratch
- Stereo Layer Right Scratch
- Unison Component Scratchが必要な場合の固定Buffer

Control Span処理中に`Vec`を生成しない。

## 10.9 Prepare失敗

- Native Handle生成失敗
- Scratch Allocation失敗
- Compiled長不整合
- Invalid Process Spec

のいずれかでPrepare全体を失敗させる。

部分的にPrepareしたVoice / Handleを破棄し、RuntimeをNot Preparedへ戻す。

---

# 11. Oscillator処理契約

## 11.1 Basic Waveform

Basic Backendは一Componentごとに一つの`DspOscillator`を持つ。

- Sine
- Saw
- Square
- Triangle
- Pulse

Frequency、Pulse WidthはControl SpanのStart / EndをNative Wrapperへ渡す。

## 11.2 Phase Reset

`phase_reset = true`：

- Note Onで各ComponentをCompiled Initial PhaseへReset
- Unison Phase Distributionを加算

`phase_reset = false`：

- Note OnでPhaseを変更しない
- Instrument ResetではCompiled Initial Phaseへ戻す

## 11.3 Pulse Width

Pulseだけに適用する。

Runtime Effective RangeはFrequencyに応じてBackendの安全範囲へ制限する。

Definition / Parameter Rangeは0.05〜0.95を維持する。

Backendが高周波で内部Clampする場合でも、Rust側で同じ計算を行い、Block SizeでClamp位置が変わらないようにする。

## 11.4 Hard Sync

Master Frequency：

```text
note_frequency × tuning × unison_detune_ratio
```

Slave Frequency：

```text
master_frequency × sync_ratio
```

DaisySP `VariableShapeOscillator`へ次を設定する。

- Note Onで`phase_reset = true`の場合はWrapperの再初期化Reset
- `SetFreq(master)`
- `SetSyncFreq(slave)`
- `SetSync(true)`
- Waveformに応じた`SetWaveshape`
- Pulseの場合`SetPW`

Frequency / Ratio / Pulse WidthはSampleごとにexclusive-endpoint Rampする。

## 11.5 Unison Distribution

Component `i`：

```text
detune_cents_i = distribution[i] × current_detune_cents
frequency_i = base_frequency × 2^(detune_cents_i / 1200)
```

Pan：

```text
pan_i = pan_distribution[i] × current_stereo_spread
left_gain_i  = cos((pan_i + 1) × π / 4)
right_gain_i = sin((pan_i + 1) × π / 4)
```

Output：

```text
left  += component × left_gain_i  × normalization
right += component × right_gain_i × normalization
```

DetuneとSpreadはSampleごとにRampする。

## 11.6 Unison Output Mode

- Unisonなし：Mono
- Unisonあり：Stereo

Spread 0でもStereo Bufferを使用する。SpreadはDynamicであり、Process中にOutput Modeを切り替えないためである。

## 11.7 Waveshaping

Unison Mix後に適用する。

Monoでは一Buffer、StereoではL/Rへ独立適用する。

Amount Spanはexclusive-endpoint補間とする。

## 11.8 Non-finite

次のいずれかを検出した場合はGenerator Failureとする。

- Non-finite Frequency
- Non-finite Pulse Width
- Non-finite Sync Ratio
- Non-finite Detune / Spread / Waveshape
- Native Error
- Non-finite Native Output
- Non-finite Mix Output

対象Process Blockを既存契約どおり無音化し、Runtimeを未準備状態へ移行する。

## 11.9 CPU

Unison Voices最大8を固定する。

計測する構成：

- Polyphony 1 / 8 / 16
- Unison 1 / 4 / 8
- Basic Saw
- Hard Sync
- Waveshaping
- Processor Chain併用

この段階でRealtime期限を保証しないが、処理時間がUnison Countに対して異常な超線形増加を示さないことを確認する。

---

# 12. Noise処理契約

## 12.1 Seed

Runtime Seedは次から決定する。

```text
Definition Seed
Layer Stable ID Hash
Note ID
Stream Kind
```

Stream Kind：

- Shared
- Left Independent
- Right Independent

HashとMixは固定AlgorithmをRuntime Moduleへ記録する。

Rust標準Hasherを使用しない。Rust Versionで結果が変わり得るためである。

## 12.2 White Noise

PRNG整数出力の上位Bitを`f32`へ変換する。

-1〜1へ均等にMappingする。

## 12.3 Pink Noise

Voss-McCartney方式を固定する。

- 16 Row
- Sample Counter
- CounterのTrailing Zeroに応じて一Row更新
- 各SampleでWhite成分を一つ加える
- 固定Normalization

Block単位でRowを更新しない。一Sample単位で進める。

## 12.4 Brown Noise

White NoiseをLeaky Integratorへ入力する。

概念：

```text
state = coefficient × state + input_gain × white
output = state × normalization
```

CoefficientはSample Rateから計算する。

- 44.1 / 48 / 96 kHzで近い時間特性
- State Driftを制限
- Hard Clampを通常処理へ多用しない
- 極小値を0へ戻す

## 12.5 Stereo Correlation

各ColorについてShared、Left Independent、Right Independentを生成する。

```text
shared_gain = sqrt(correlation)
independent_gain = sqrt(1 - correlation)

left  = shared × shared_gain + independent_left  × independent_gain
right = shared × shared_gain + independent_right × independent_gain
```

Correlation 1では左右が一致する。

Correlation 0では独立Streamとなる。

## 12.6 Reset

Instrument Reset後、同じNote ID Sequenceから同じNoiseを生成する。

Voice Stealingで古いNoise StateをPending Noteへ引き継がない。

---

# 13. Sample Zone選択と再生契約

## 13.1 Note On時の順序

```text
Layer Trigger判定
  ↓
Sample GeneratorのKey / Velocity Matching
  ↓
Round Robin Group選択
  ↓
Zoneを一つ確定
  ↓
Region / Loop StateをRuntimeへ設定
  ↓
Envelope Note On
```

Layer TriggerがFalseならZone選択もCounter更新も行わない。

## 13.2 Zone Matching

Zoneは次を満たすとMatchする。

```text
key_min <= note_number <= key_max
velocity_min <= velocity <= velocity_max
zone.enabled == true
```

Matchなしの場合、Sample LayerはそのNoteでInactiveのままとする。

## 13.3 Round Robin選択

Matchが一つ：そのZone。

Matchが複数：Validation済みの同一Groupであるため、Group Counterから一つ選択する。

Counter更新は一Note Onにつき一回。

同じNote OnがVoice Stealingで遅延開始しても再更新しない。

## 13.4 Playback Ratio

```text
ratio = 2^((note_number - root_note) / 12) × layer_tuning_ratio
```

既存Log Ramp契約を維持する。

Region / Loop位置はSource Frame Domain、Cursor増分だけがRatioで変化する。

## 13.5 One-shot Region

- CursorはStart Frameから開始
- End Frameへ達したらFinished
- Region終端へ既存5ms End Fadeを適用
- Regionが5msより短い場合はRegion Length内へ縮小
- Region外をCubic補間で参照しない

## 13.6 Forward Loop

- Start Frameから再生開始
- CursorがLoop Endへ到達したらLoop StartへWrap
- Fractional Overshootを保持する

```text
position = loop_start + (position - loop_end)
```

OvershootがLoop Length以上の場合は`rem_euclid(loop_length)`を使用する。

LoopはEnvelope Release中も継続する。

Envelope終了でLayerを停止する。

Region EndはLoop End以降のBuffer Safety境界として保持するが、Loop中は到達しない。

Release後にLoopを抜けてRegion終端へ進む機能は実装しない。

## 13.7 Cubic Interpolation

One-shot Region：

- Region Startより前はStart SampleへClamp
- Region End以降はEnd直前SampleへClamp

Forward Loop：

- Loop境界付近ではLoop Region内へIndexをWrap
- Loop外のSampleを補間へ混ぜない

これによりBlock境界とLoop境界の参照を固定する。

## 13.8 Loop Click

Loop Crossfadeは対象外である。

Review AssetはLoop可能な境界を持つものを使用する。

それでもRuntime由来の不連続がないことを確認する。

Asset自体のLoop Point不一致によるClickと、Cursor / Interpolation Bugを区別する。

## 13.9 Explicit Slice

Slice ZoneはOne-shot Regionと同じRuntimeを使用する。

- Single Key Zone
- Shared Asset Cache
- Region Start / End
- Zone Root Note
- Existing Layer Envelope

Slice専用Note EventまたはSlice Index Eventを追加しない。

## 13.10 Reset

Reset対象：

- Selected Zone
- Source Arc参照
- Cursor
- Loop State
- Finished
- Pending Selection
- Round Robin Counter

Buffer自体はCompiled Assetとして共有し、Zero Clearしない。

---

# 14. Voice LifecycleとState

## 14.1 Note On

各Layerについて：

1. Trigger判定
2. Generator固有Selection
3. Generator Reset / Start
4. Processor Reset
5. Envelope Note On
6. Note Start Fade開始

## 14.2 Note Off

- Oscillator / Noise：Envelope Release
- One-shot Sample：再生継続、Envelope Release
- Forward Loop：Loop継続、Envelope Release
- Round Robin Counterは変更しない

## 14.3 Voice Stealing

古いVoice：

- 現在のGenerator / Processor OutputをSteal Fade
- Fade終了までStateを進める

Pending Note：

- Note Event時点のZone Selectionを保持
- Unison / Noise Seed情報を保持するためNote IDを保持
- Fade完了後にGeneratorをReset / Start

古いSample Cursor、Noise State、Oscillator PhaseをPending Noteへ引き継がない。

`phase_reset = false`のOscillatorだけは、同じVoice RuntimeのComponent PhaseをNote OnでResetしない既存意味を維持する。ただしInstrument Resetでは初期化する。

## 14.4 Voice Idle

VoiceがIdleへ戻るとき：

- Generator StateをIdleへ
- Layer Processor Reset
- Voice Processor Reset
- Sample Selection Clear
- Noise State Clear
- Pending Selection Clear

Round Robin CounterはInstrument ScopeなのでClearしない。

## 14.5 Instrument Reset

- 全Voice
- 全Generator
- 全Processor
- 全Modulation Source
- 全Parameter Smoother
- External Control
- Round Robin Counter
- Global Tail
- Absolute Frame

を初期化する。

同じDefinition、Process Spec、Event Sequenceから初回と同等の出力を得る。

---

# 15. Process ContractとError

## 15.1 Process中に行わないこと

- JSON Parse
- Definition Validation
- File I/O
- Path正規化
- SHA計算
- Asset Decode
- Asset Resample
- HashMap検索
- String比較によるZone / Group検索
- Native Handle生成
- Buffer Resize
- `Vec::push`
- Capacity増加
- Blocking Mutex
- Network
- Device操作
- Panic
- C++例外の越境

## 15.2 Generator Failure

追加Failure Kindは必要最小限とする。

- Native DSP Failure
- Non-finite Generator State
- Invalid Compiled Generator State
- Invalid Sample Cursor State

既存`ProcessError::ProcessorFailure`と混同しない場合だけGenerator専用Kindを追加する。

Error時：

- 対象Block全体を無音化
- RuntimeをNot Preparedへ移行
- 再利用にはPrepareが必要

## 15.3 Zone Selection Failure

正しくCompileされたZone TableでMatchなしはErrorではない。Layerが発音しないだけである。

Compiled Index不整合はRuntime Errorとする。

## 15.4 Zero Frame

既存契約を維持する。

- Eventなしのみ許可
- Oscillator Phaseを進めない
- Noise Streamを進めない
- Sample Cursorを進めない
- Round Robin Counterを進めない

## 15.5 Block Size

Generator Parameter Spanはexclusive-endpoint補間とする。

一つの32 Frame Control Spanを1 + 31、15 + 17等へ分割しても同じ出力を得る。

Noise、Loop、Round RobinはBlock Sizeに依存しない。

---

# 16. CLIとInspection

## 16.1 `instrument init`

Canonicalな基本Instrumentを新Oscillator Schemaで出力する。

- Saw Waveform Object
- `phase_reset`
- `phase`
- Hard Syncなし
- Waveshapingなし
- Unisonなし
- Existing Voice Filter
- Existing Velocity Route

Sample Zoneを含むTemplateを通常Initへ混ぜない。

## 16.2 `instrument validate`

追加Diagnostic：

- Waveform構造不正
- Oscillator Parameter範囲外
- Sine + Hard Sync
- Hard Sync + Phase Spread
- Noise範囲外
- Zone ID不正 / 重複
- Zone数不正
- Key / Velocity Range不正
- Zone Overlap不正
- Round Robin Group不整合
- Region不正
- Loop不正
- Asset Duration外
- Generator Target不正

## 16.3 `instrument inspect`

Human-readable / JSONで表示：

### Oscillator

- Waveform
- Backend
- Phase Reset / Phase
- Hard Sync有無
- Static / Dynamic Sync Ratio
- Waveshaping有無
- Unison Count
- Detune / Spread / Phase Spread
- Generator Parameter ID
- Effective Frequency上限

### Noise

- Color
- Seed
- Correlation Parameter
- Stereo Output

### Sample

- Zone Count
- Enabled / Disabled Count
- Asset共有数
- Zone ID
- Key / Velocity Range
- Root Note
- Round Robin Group
- Playback Type
- Region
- Loop Point
- Asset Metadata

InspectでProcess用CounterやCurrent Cursorは表示しない。

## 16.4 Render

既存Commandを維持する。

- `render note`
- `render events`
- `render midi`

PWM、Hard Sync、Unison等は既存Parameter Change / Modulationから利用する。

Sample Zone専用Render Commandを追加しない。

---

# 17. 現行DefinitionとReference更新

## 17.1 対象

- `examples/instruments/*.json`
- `testdata/definitions/*.json`
- `testdata/events/*.json`
- Review Script内Definition
- `review-output/*/definitions/*.json`
- CLI Test Fixture
- Documentation JSON例
- `.agents/skills/create-instrument/SKILL.md`

## 17.2 Oscillator Definition

全Sine / Saw Definitionを新Waveform Objectへ更新する。

旧文字列Waveformを現在仕様のFileへ残さない。

## 17.3 Sample Definition

すべての単一Sampleを一ZoneのSample Generatorへ更新する。

例：

```json
"sample": {
  "interpolation": "cubic",
  "zones": [
    {
      "id": "main",
      "asset": { "path": "../assets/metal-hit.wav" },
      "root_note": 60,
      "key_min": 0,
      "key_max": 127,
      "velocity_min": 1,
      "velocity_max": 127,
      "round_robin_group": null,
      "playback": {
        "type": "one_shot",
        "start_seconds": 0.0,
        "end_seconds": null
      }
    }
  ]
}
```

## 17.4 Existing Review

次を新Definitionで再生成する。

- Basic Poly Synth
- Metallic Hybrid
- Dynamic Parameters
- Processor Chain

既存音響値を維持するものは、許容差内のBaseline比較を行う。

---

# 18. Repository変更範囲

主な変更対象：

```text
native/daisysp-wrapper/
├─ CMakeLists.txt
├─ include/
│  └─ sonalloy_dsp.h
└─ src/
   └─ daisysp_wrapper.cpp

crates/sonalloy-dsp-sys/
├─ build.rs
├─ src/
│  └─ lib.rs
└─ tests/

crates/sonalloy-core/src/
├─ definition.rs
├─ parameter.rs
├─ compiler.rs
├─ diagnostics.rs
├─ lib.rs
└─ runtime/
   ├─ generator/
   │  ├─ mod.rs
   │  ├─ oscillator.rs
   │  └─ noise.rs
   ├─ sample.rs
   ├─ voice.rs
   ├─ instrument.rs
   ├─ modulation.rs
   └─ processor/

crates/sonalloy-core/tests/
└─ core_process.rs

crates/sonalloy-cli/src/
└─ main.rs

crates/sonalloy-cli/tests/
└─ cli.rs

.agents/skills/create-instrument/
└─ SKILL.md

docs/
├─ plan/
│  └─ plan-essential-synthesis-and-sampling.md
├─ instrument-definition.md
├─ runtime-processing.md
├─ architecture.md
├─ cli.md
├─ creating-an-instrument.md
└─ testing-and-sound-review.md

examples/instruments/
├─ existing definitions
├─ essential-hybrid-instrument.json
└─ mapped-sample-instrument.json

scripts/review/
└─ generate_essential_synthesis_sampling_package.py

review-output/
└─ essential-synthesis-sampling/

testdata/
├─ definitions/
├─ events/
└─ assets/

THIRD_PARTY_NOTICES.md
```

実際のRepository構成へ合わせる。計画上の分類だけを理由に不要なFile分割を行わない。

`generator/` ModuleはOscillatorとNoiseの責務が現在の`voice.rs`へ集中することを避けるために追加する。小さな型ごとにFileを分けない。

---

# 19. Test計画

## 19.1 Definition Unit Test

### Oscillator

- Sine / Saw / Square / Triangle / Pulse Parse
- Pulse Width最小 / 最大
- Pulse Width範囲外
- Phase最小 / 最大
- Hard Sync Ratio最小 / 最大
- Sine + Hard Sync拒否
- Hard Sync + Phase Spread拒否
- Waveshaping最小 / 最大
- Unison Voices 2 / 8
- Unison Voices 1 / 9拒否
- Detune / Spread / Phase Spread範囲
- Unknown Field
- 旧Waveform文字列拒否

### Noise

- White / Pink / Brown
- Seed
- Correlation最小 / 最大
- Correlation範囲外
- Unknown Field

### Sample

- 一Zone
- 複数Key Zone
- Velocity Layer
- Round Robin
- One-shot Region
- Forward Loop
- Zone ID重複
- Empty Zone
- 257 Zone拒否
- Key / Velocity Range逆転
- Ambiguous Overlap拒否
- Round Robin条件不一致拒否
- Region逆転
- Loop逆転
- LoopがRegion外
- 旧単一Sample形式拒否
- Unknown Field

## 19.2 Parameter Unit Test

- Generator Canonical ID
- ID Grammar
- Catalog順序
- Owner
- Descriptor Unit / Scale / Range / Default / Smoothing
- Pulse以外にPulse Widthなし
- Hard SyncなしにSync Ratioなし
- WaveshapingなしにWaveshapeなし
- UnisonなしにDetune / Spreadなし
- SampleにGenerator Dynamic Parameterなし
- Normalize / Denormalize
- Route Target解決

## 19.3 Native Oscillator Test

- Existing Sine Output
- Existing Saw Output
- Square周期 / Peak / Finite
- Triangle周期 / Peak / Finite
- Pulse Width 0.25 / 0.5 / 0.75
- Pulse Width Ramp
- Arbitrary Phase Reset
- Block分割Ramp一致
- Unsupported Waveform
- Invalid Pulse Width
- Not Prepared
- Null Buffer
- Fault Injection
- Exception越境なし
- Error時Output無音化

## 19.4 Variable Shape Native Test

- Prepare / Reset
- Saw Hard Sync
- Triangle Hard Sync
- Square Hard Sync
- Pulse Hard Sync
- Ratio 1 / 2 / 8 / 16
- Ratio Ramp
- Pulse Width Ramp
- 44.1 / 48 / 96 kHz
- Finite
- Deterministic Reset
- Fault Injection

## 19.5 Oscillator Runtime Test

- Unison 1と既存Basic一致
- Unison Component Count
- Symmetric Detune
- Even / Odd Count Distribution
- Stereo Spread 0 / 1
- Phase Spread
- Normalization
- Waveshape 0 Identity
- Waveshape増加でHarmonic変化
- Hard Sync Frequency計算
- High Note Safe Clamp
- Parameter Span Partition
- Voice間State分離
- Voice Stealing
- Reset再現性
- No Allocation確認可能な構造

## 19.6 Noise Unit Test

- White有限 / 範囲
- Pink有限
- Brown有限
- Color間の統計差
- Same Seed / Noteで同一
- Different Noteで差
- Correlation 1でL/R一致
- Correlation 0でL/R差
- Correlation Ramp Partition
- Block Size一致
- Reset再現性
- 44.1 / 48 / 96 kHz
- Brown DC Drift制限

## 19.7 Sample Compiler Test

- Asset Cache Hit
- Same AssetのArc共有
- SHA違いのCache分離
- Multiple Asset Decode
- Missing Zone Asset
- Partially Missing Round Robin
- All Missing Zone
- Region Frame変換
- Loop Frame変換
- Asset Duration外
- Round Robin Group Table
- Definition順保持
- Error時Compiled Instrumentなし

## 19.8 Zone Selection Test

- Key Zone境界
- Velocity境界
- Gapで発音なし
- Round Robin A/B/A/B
- Group別Counter
- Layer別同名Group独立
- Missing Member Skip
- Resetで先頭へ
- Voice Stealing Pending Selection保持
- Block Sizeで順序不変
- Polyphonyで順序不変

## 19.9 Sample Runtime Test

- One-shot Start / End
- Non-zero Start Region
- End Fade
- Short Region
- Forward Loop位置
- Fractional Ratio Loop
- Large Overshoot Wrap
- Loop Cubic Boundary
- Note Off中Loop継続
- Envelope終了で停止
- Slice Region外無音
- Root Note / Octave Ratio
- Tuning Ramp
- Reset
- Finite

## 19.10 Layer Runtime Test

- Mono Generatorの既存経路
- Stereo GeneratorのL/R
- Stereo Layer Filter
- Stereo Layer Drive
- Envelope / Gain両Channel適用
- Stereo Balance中心 / 左 / 右
- Mono Pan回帰なし
- Multiple Layer Mix
- Generator終了判定

## 19.11 Core Integration Test

- Essential Hybrid Compile / Prepare / Process
- Mapped Sample Instrument
- PWM LFO
- Sync Envelope
- Unison Detune Parameter Change
- Noise Correlation Parameter Change
- Multi Sample Melody
- Velocity Layer MIDI
- Round Robin Repeated Note
- Loop Hold / Release
- Slice MIDI
- Processor Chain併用
- Polyphony
- Voice Stealing
- Reset
- 44.1 / 48 / 96 kHz
- Block Size 32 / 64 / 257 / 1024

## 19.12 CLI Integration Test

- `instrument init`新Schema
- `instrument validate`全Generator
- Invalid Combination Exit Code
- `instrument inspect`Oscillator
- `instrument inspect`Noise
- `instrument inspect`Sample Zone
- JSON Inspect
- Parameter Event
- MIDI Render
- Missing Zone Asset Warning
- 旧Definition拒否

## 19.13 Dependency Regression

- DaisySP Commit不変
- Build対象はOscillator、VariableShapeOscillator、SVFだけ
- 新Product Dependencyなし
- `Cargo.lock`に新Packageなし
- `THIRD_PARTY_NOTICES.md`と実態一致
- External SFZ / Sampler / Random Libraryなし

---

# 20. Sound Review

## 20.1 Review Package

```text
review-output/essential-synthesis-sampling/
├─ audio/
│  ├─ technical/
├─ definitions/
├─ events/
├─ midi/
├─ assets/
├─ metrics.json
└─ review-summary.md
```

生成Script：

```text
scripts/review/generate_essential_synthesis_sampling_package.py
```

## 20.2 Review音源の正本と試聴

`audio/technical/`の生出力をReview音源の正本とする。

- Runtimeの生出力
- Metrics、SHA、Block Size比較へ使用
- 音量正規化しない
- 人間の試聴にも同じWAVを使用する

試聴専用の正規化コピーはReview Packageへ保存しない。

- 必要な音量調整は再生側で行う
- 正規化が必要な一時コピーを作成する場合も、MetricsやRuntime正当性の確認には使用しない

これにより、生出力と人間が試聴する音源の不一致を避け、Review Package内で同じWAVを一元管理する。

## 20.3 Scenario

### Basic Generator

1. Existing Sine Baseline
2. Existing Saw Baseline
3. Square
4. Triangle
5. Pulse Width 0.25
6. Pulse Width 0.75
7. PWM LFO
8. White Noise
9. Pink Noise
10. Brown Noise
11. Noise Correlation 1
12. Noise Correlation 0

### Complex Oscillator

13. Hard Sync Ratio 2
14. Hard Sync Ratio 6
15. Hard Sync Sweep
16. Waveshaping Amount 0.5
17. Waveshaping Sweep
18. Unison 3
19. Unison 5 Stereo
20. Unison 8
21. Hard Sync + Unison
22. Full Essential Synth Patch

### Sampling

23. Key Zone Scale
24. Velocity Layer Soft / Hard
25. Round Robin Repeated Hit
26. Forward Loop Hold
27. Forward Loop Release
28. Explicit Slice Sequence
29. Multi Sample Melody
30. Full Mapped Sample Instrument
31. Essential Hybrid Instrument

### Regression

32. Block Size比較
33. Sample Rate比較
34. Reset Repeat
35. Voice Stealing
36. Existing Review再生成

## 20.4 Review Asset

外部権利が不明なSampleを追加しない。

Sampling Review用Assetは次のいずれかとする。

- Repository内の既存Asset
- Scriptで決定的に生成したSynthetic WAV
- 明示的に利用許諾されたAsset

Round Robinは差が聞こえる二つ以上のSynthetic Hitを使用する。

Loop Assetは境界が検査可能な持続音を生成する。

Slice Assetは複数の明確に異なるTransientを一Fileへ配置する。

## 20.5 Metrics

- Finite
- Peak
- RMS
- DC
- Positive Zero Crossing
- Estimated Fundamental
- Maximum Adjacent Delta
- Large Discontinuity Count
- L/R Difference
- Stereo Correlation
- Block Size差分
- Sample Rate別Duration
- Reset SHA-256
- Round Robin選択順
- Loop周期
- Slice Region Duration
- Asset Cache Decode Count

Oscillator品質補助として、固定長FFTまたはDFTをReview Script内で実装し、次を記録する。

- Fundamental Energy
- Harmonic Energy
- Nyquist近傍Energy
- Hard Sync / Waveshapingの高域Energy比

新しいPython Packageを必須にしない。標準Libraryで固定長解析を行う。

## 20.6 人間の試聴

### Square / Triangle / Pulse

- Waveform差が明確か
- 高音域で耳障りなAliasが強すぎないか
- Pulse Width差が明確か
- PWMにClickがないか
- 低音でDC感が強すぎないか

### Noise

- White / Pink / Brownの差
- Brownの低域過多
- Pinkの不自然な周期性
- CorrelationによるStereo幅
- Resetで不自然な固定Clickを出さないか

### Hard Sync

- Ratioで倍音が変化するか
- Sweepが滑らかか
- 高音で破綻しないか
- Pitchが意図せず変化しないか

### Waveshaping

- Amount 0で同一か
- 中程度で有用な倍音変化か
- 高Amountで耳障りなAliasが許容範囲か
- Levelが過度に変わらないか

### Unison

- Detune 0で過度な位相キャンセルがないか
- Detune増加で幅とBeatが自然か
- Stereo SpreadがMono Compatibilityを壊しすぎないか
- 8 Voiceで濁りすぎないか
- Levelが不自然に増大しないか

### Sample Zone

- Key境界で意図したSampleへ切り替わるか
- Velocity Layer差が明確か
- Round Robin順が聞き取れるか
- Pitch Mappingが自然か

### Loop

- Loop周期
- Cursor由来のClick
- Release中の挙動
- Block境界でLoop位置が変わらないか

### Slice

- 対象Transientだけを再生するか
- 前後Sliceが混ざらないか
- End FadeでAttackを損なわないか

### Full Instrument

- Generator、Sample、Processorの各Layerが一体化しているか
- 技術Demoではなく音色として利用可能か
- 音量差だけを機能差と誤認しないか

## 20.7 完了判定

次の場合は機能を残したまま完了扱いにしない。

- Hard Syncが明確に使用不能なAliasを出す
- PWMがClickを出す
- Pink / Brown Noiseが発散または強いDCを出す
- UnisonがLevel Explosionを起こす
- Loop Runtime由来のClickが残る
- Round Robinが再現しない
- SliceがRegion外を読む
- Full Reference Instrumentが破綻する

---

# 21. Documentationと正本

## 21.1 `docs/CONCEPT.md`

変更しない。

現在の要件と今回の実装範囲に重大な矛盾が発見された場合だけ、実装前に別途判断する。通常の実装詳細、値域、進行管理、Dependency選定を追加しない。

## 21.2 必ず更新する文書

### `docs/instrument-definition.md`

必要な現在仕様だけを更新する。

- Generator Definition
- Waveform Object
- Oscillator / Noise / Sample Field
- Generator Parameter ID
- Sample Zone
- Overlap / Round Robin
- Region / Loop
- JSON例

### `docs/runtime-processing.md`

- Mono / Stereo Generator
- Generator State所有
- Zone選択
- Round Robin Counter
- Loop / Slice Cursor
- Prepare / Process / Reset
- Voice Stealing Pending Selection

## 21.3 実装差分がある場合だけ更新

### `docs/architecture.md`

- `runtime/generator` Module責務
- Native Variable Shape Wrapper
- Asset Cache責務

依存選定の比較やロードマップ番号を記載しない。

### `docs/cli.md`

- Validate / Inspectの公開表示差分

### `docs/creating-an-instrument.md`

- PWM、Unison、Sample Zoneを作成するために不可欠な例だけを追加する

### `docs/testing-and-sound-review.md`

- 新Review PackageのWAV正本と試聴用途

## 21.4 Documentation最小化

- 実装と直接関係しない章を整理しない
- 表現統一だけの大量変更をしない
- Test Documentへ実装工程を書かない
- Planの自己評価を書かない
- P5完了報告を恒久Documentへ追加しない
- READMEを機能一覧のためだけに肥大化させない

---

# 22. 三実装単位

三単位は一つのBranchと一つのPull Requestで実装する。

各単位の完了時点で全Workspace TestをGreenに保つ。

## 22.1 実装単位1：Basic Generator Expansion

### 目的

基本波形とNoiseを、既存Parameter / Modulation / Layer Runtimeで安全に利用できるようにし、Stereo Generatorを受け入れるLayer経路を成立させる。

### 作業

1. PlanをRepositoryへ追加
2. Oscillator Waveform DefinitionをTagged Objectへ変更
3. Existing Definitionを新Schemaへ更新
4. Native Waveform Enumを拡張
5. Square / Triangle / PulseをWrapperへ追加
6. Arbitrary Phase Resetを追加
7. Pulse Width / Ramp APIを追加
8. Generator Parameter IDを追加
9. Oscillator Target Spanを追加
10. Noise Definition / Compiled Modelを追加
11. Noise Runtimeを実装
12. Stereo Generator Output Modeを追加
13. Stereo Layer Processor / Balanceを統合
14. Unit / Integration / CLI Test
15. Basic Generator Reviewを生成
16. Existing Reviewを再生成

### 完了条件

- Sine / Saw Baseline維持
- Square / Triangle / Pulseが利用可能
- PWMが既存LFOから動作
- White / Pink / Brownが利用可能
- Correlation 0 / 1が成立
- Mono / Stereo Layer経路が成立
- Block Size / Reset Test成功
- CI成功
- Basic Generator試聴で致命的問題なし

### この単位で未実装

- Hard Sync
- Waveshaping
- Unison
- Sample Zone

ただしDefinition / Runtime構造を後続単位で再作成しないよう、Generator Runtime ModuleとOutput Modeをここで固定する。

## 22.2 実装単位2：Complex Oscillator

### 目的

基本波形をHard Sync、Waveshaping、Unisonへ拡張し、実用的なBass、Lead、Padを作れるOscillator Generatorへ到達させる。

### 作業

1. DaisySP `variableshapeosc.cpp`をBuild対象へ追加
2. Native Variable Shape Opaque Handleを追加
3. Hard Sync Prepare / Process / Reset / Ramp
4. Hard Sync Definition / Parameter
5. Basic / Hard Sync Backend Compile
6. Unison Definition / Distribution Compile
7. Unison Component Runtime
8. Dynamic Detune / Stereo Spread
9. Phase Distribution
10. Waveshaping Runtime
11. Voice Stealing / Reset統合
12. CPU / Memory計測
13. Unit / Integration / CLI Test
14. Complex Oscillator Review生成
15. Essential Synth Reference作成

### 完了条件

- Saw / Square / Triangle / Pulse Hard Sync
- Sync Ratio Parameter Change / Modulation
- Waveshape 0 Identity
- Unison 2〜8
- Detune / Stereo Spread Ramp
- Phase Distribution
- Hard Sync + Unison
- State分離
- Reset再現性
- Block Size非依存
- CI成功
- 人間試聴でHard Sync / Unisonが使用可能

## 22.3 実装単位3：Sample Instrument Expansion

### 目的

単一Sample再生を、複数音域、Velocity、Round Robin、Loop、Sliceを持つ実用的なSample Instrumentへ拡張する。

### 作業

1. Sample DefinitionをZone配列へ変更
2. Existing Sample Definitionを一Zoneへ更新
3. Zone ID / Range / Playback Validation
4. Overlap / Round Robin Validation
5. Asset Cache
6. Multiple Asset Compile
7. Region / Loop Frame Compile
8. Compiled Zone / Group
9. Instrument Round Robin State
10. Note On Zone Selection
11. Pending Note Selection
12. One-shot Region Runtime
13. Forward Loop Runtime
14. Loop-aware Cubic Interpolation
15. Explicit Slice Fixture
16. Missing Asset Partial Compile
17. CLI Inspect
18. Unit / Integration / CLI Test
19. Sampling Review生成
20. Mapped Sample Reference作成
21. Essential Hybrid最終Reference作成
22. 全Existing Review再生成
23. 全体Sound Review

### 完了条件

- Multi Sample Zone
- Key Mapping
- Velocity Layer
- Deterministic Round Robin
- Forward Loop
- Explicit Slice
- Asset共有
- Missing Zone Asset継続
- Voice Stealing時の選択固定
- Reset再現性
- Block Size非依存
- CI成功
- SamplingとFull Instrumentの人間試聴承認

---

# 23. 一つのPull Requestでの進行

## 23.1 Branch / PR

- Branchは一つ
- Pull Requestは一つ
- 三単位ごとに別PRを作らない
- Unit完了だけを理由にMainへMergeしない
- 最終Unit完成前はDraftまたは未完成であることをPR本文へ明記する

## 23.2 Commit構成

Commit数は固定しないが、責務を混在させない。

推奨順：

1. Plan
2. Basic Definition / Native Waveform
3. Noise / Stereo Layer
4. Basic Tests / Review
5. Variable Shape Native Wrapper
6. Hard Sync
7. Unison / Waveshaping
8. Complex Tests / Review
9. Sample Zone Definition / Compile
10. Sample Runtime / Round Robin
11. Sampling Tests / Review
12. CLI / Documentation / Final Regression

一つの巨大Commitにしない。

## 23.3 Unit Gate

各Unit完了時に実行する。

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Review Packageもその時点で生成可能にする。

Unit 1 / 2で一時的にTestを無効化し、Unit 3まで放置しない。

## 23.4 PR本文

最終PR本文へ記載：

- 三実装単位のSummary
- Dependency変更
- DaisySP Source追加
- DefinitionのCurrent Schema変更
- Compatibility / Migrationを提供しないこと
- Automated Verification
- Existing Review再生成
- New Review Package
- Human Listening状態

## 23.5 Final Review

最終Reviewは次をまとめて確認する。

- Dependency方針
- Definition全体
- Native Wrapper
- Generator State
- Stereo Layer経路
- Sample Zone / Round Robin
- Runtime Safety
- Existing Regression
- Sound Review
- Documentation最小性

Unitごとの設計が正しくても、三単位の統合でReference Instrumentが破綻する場合はMergeしない。

---

# 24. 完了条件

## Dependency

- [ ] DaisySP Commitが維持されている
- [ ] Build対象追加が`variableshapeosc.cpp`だけである
- [ ] 新しいProduct Crateがない
- [ ] SFZ / Sampler / Random / DSP Frameworkを追加していない
- [ ] `Cargo.lock`に意図しないPackage追加がない
- [ ] Third-party Noticeが実態と一致する

## Definition

- [ ] GeneratorがOscillator / Noise / Sampleを持つ
- [ ] WaveformがTagged Objectである
- [ ] Square / Triangle / Pulseが定義可能
- [ ] Hard Sync / Waveshaping / Unisonが定義可能
- [ ] Noise Color / Seed / Correlationが定義可能
- [ ] Sample Zone配列が定義可能
- [ ] Key / Velocity / Round Robin / Region / Loopが定義可能
- [ ] Unknown Fieldを拒否する
- [ ] 旧Waveform文字列を受け付けない
- [ ] 旧単一Sample形式を受け付けない
- [ ] Migration / Deprecated / Aliasがない

## Parameter / Modulation

- [ ] Generator Parameter ID
- [ ] Pulse Width
- [ ] Sync Ratio
- [ ] Waveshape
- [ ] Unison Detune
- [ ] Unison Spread
- [ ] Noise Correlation
- [ ] Catalog順序
- [ ] Unit / Scale / Range / Smoothing
- [ ] Existing Source / Routeで制御可能
- [ ] Static FieldがTarget外

## Compile

- [ ] Backend選択
- [ ] Unison Distribution
- [ ] Asset Cache
- [ ] Multiple Zone Asset
- [ ] Region / Loop Frame変換
- [ ] Overlap Validation
- [ ] Round Robin Group
- [ ] Missing Asset Partial Compile
- [ ] Error時Compiled Instrumentなし

## Runtime

- [ ] Square / Triangle / Pulse
- [ ] PWM
- [ ] White / Pink / Brown Noise
- [ ] Hard Sync
- [ ] Waveshaping
- [ ] Unison 2〜8
- [ ] Stereo Generator Layer
- [ ] Sample Zone選択
- [ ] Velocity Layer
- [ ] Round Robin
- [ ] One-shot Region
- [ ] Forward Loop
- [ ] Explicit Slice
- [ ] Voice Stealing Pending Selection
- [ ] Reset
- [ ] Process中Allocationなし
- [ ] Error時無音化 / Runtime無効化
- [ ] Block Size非依存

## CLI / Current Specification

- [ ] Init
- [ ] Validate
- [ ] Inspect
- [ ] Note / Events / MIDI Render
- [ ] Existing Definition更新
- [ ] Reference Instrument追加
- [ ] `docs/instrument-definition.md`が実装と一致
- [ ] `docs/runtime-processing.md`が実装と一致
- [ ] 必要な公開Documentだけ更新
- [ ] `docs/CONCEPT.md`を不要に変更していない
- [ ] 恒久Documentへロードマップ番号を残していない

## Test / Review

- [ ] Native Unit Test
- [ ] Definition Unit Test
- [ ] Parameter Unit Test
- [ ] Compiler Unit Test
- [ ] Oscillator Runtime Test
- [ ] Noise Runtime Test
- [ ] Sample Runtime Test
- [ ] Core Integration Test
- [ ] CLI Integration Test
- [ ] 44.1 / 48 / 96 kHz
- [ ] Block Size 32 / 64 / 257 / 1024
- [ ] Reset再現性
- [ ] Existing Review再生成
- [ ] New Review生成
- [ ] Review音源の正本とMetricsの一致
- [ ] 人間によるBasic Generator承認
- [ ] 人間によるComplex Oscillator承認
- [ ] 人間によるSampling承認
- [ ] 人間によるFull Instrument承認

## Pull Request

- [ ] 三単位が一つのBranchにある
- [ ] Pull Requestが一つだけである
- [ ] 各Unit Gateが成功している
- [ ] Final CIが成功している
- [ ] 未完成Unitを残していない
- [ ] Review指摘を解消している

---

# 25. 提供範囲

新P5完了後に利用可能：

- Sine
- Saw
- Square
- Triangle
- Pulse
- PWM
- White Noise
- Pink Noise
- Brown Noise
- Noise Stereo Correlation
- Hard Sync
- Waveshaping
- Unison
- Detune
- Stereo Spread
- Phase Distribution
- Single Sample
- Multi Sample Zone
- Key Mapping
- Velocity Layer
- Deterministic Round Robin
- One-shot Region
- Forward Loop
- Explicit Slice
- Oscillator / Noise / Sample Hybrid Layer
- Dynamic Parameter / Modulation
- Layer / Voice / Global Processor Chain
- CLI Validation / Inspection / Offline Render

未実装：

- Wavetable
- FM / PM / AM / Ring Mod
- Granular
- Additive
- Spectral
- Modal / Waveguide
- Formant
- Phase Distortion
- Wavefold専用方式
- Sample Streaming
- Reverse
- Ping-pong Loop
- Loop Crossfade
- Velocity Crossfade
- Articulation
- Tempo Sync
- Time Stretch
- Transient Slice Detection
- EQ
- Chorus
- Convolution
- Dynamics
- Realtime Device
- Riffra
- C ABI
- CLAP
- VST3

新P5は、基本的な電子音生成と実用的なSample Instrumentを、既存の半固定Pipeline、Dynamic Parameter、Processor Chainの中で安全に組み合わせられる状態を完成点とする。
