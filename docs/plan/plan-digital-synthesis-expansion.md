# Sonalloy Digital Synthesis Expansion 詳細設計・実装計画

- **対象Repository**：`endo-ly/sonalloy`
- **正本要件**：`docs/CONCEPT.md`
- **前提実装**：Instrument Definition、Compile、Dynamic Parameter / Modulation、Processor Chain、Essential Synthesis / Sampling Expansion
- **ロードマップ上の扱い**：次の開発Phase（P6）
- **実装単位**：三単位。BranchとPull Requestは一つとし、単位ごとに独立したCommit・Test・Sound Reviewを成立させる
- **用途**：実装エージェントへ渡す詳細設計・実装計画
- **文書言語**：日本語。型名、API名、Parameter ID、File Pathのみ英語を使用する
- **成果物**：Markdownのみ。HTML版は作成しない

---

## 0. この計画書の位置づけ

本書は、現在のSonalloyへDigital Synthesisの主要方式を追加し、基本OscillatorとSample中心の音源から、Wavetable、Operator Modulation、Phase-domain Oscillatorを含む音源へ拡張するための詳細設計・実装計画である。

製品全体の目的、責務、将来像は`docs/CONCEPT.md`を正本とする。

本書は、正本のうち次の機能を現在のコードベースへ実装可能な粒度まで具体化する。

- Wavetable Generator
- FM / PM / AM / Ring Modを扱うOperator Modulation Generator
- Phase Distortion
- Wavefold
- Oscillator Feedback
- 既存Unison、Parameter、Modulation、Processor Chainとの統合
- Wavetable AssetのCompile時準備と帯域制限
- Audio-rate相互作用を専用Generator内部へ閉じる固定Topology
- 新Generatorを含むCLI Inspect、Offline Render、Test、Sound Review

本Phaseでは、次の三単位を順番に完成させる。

1. **Wavetable Generator**
2. **Operator Modulation Generator**
3. **Complex Oscillator Completion**

三単位は別々の製品Phaseへ分けない。

一方、実装を一度に混在させない。各単位についてDefinition、Validation、Compile、Parameter、Runtime、CLI、Test、Review Packageまでを縦に完成させてから次へ進む。

### 0.1 恒久的な機能名称

コード、Definition、CLI、恒久Documentでは次の名称を使用する。

- `Wavetable Generator`
- `Operator Modulation Generator`
- `Operator`
- `Operator Algorithm`
- `Phase Modulation`
- `Frequency Modulation`
- `Amplitude Modulation`
- `Ring Modulation`
- `Phase Distortion`
- `Wavefold`
- `Oscillator Feedback`
- `Prepared Wavetable`
- `Wavetable Band`

`P6`という番号は本計画の進行上の識別子に限る。

型名、関数名、Module名、Parameter ID、Diagnostic、Reference Instrument、利用者向け恒久Documentへ`P6`を残さない。

### 0.2 実装判断の優先順位

判断に迷った場合は、次の順序で優先する。

1. `docs/CONCEPT.md`
2. 本書で固定する音響方式、信号順序、Definitionの意味
3. 既存Process Contract、Parameter、Modulation、Processor Chainの契約
4. 音質と人間による試聴結果
5. Realtime Safety、決定性、Block Size非依存性
6. 既存Instrumentの回帰を起こさないこと
7. 実装の単純さ
8. 将来のGenerator追加

将来のGranular、Additive、Spectral、自由Graphを理由に、現在使用しないNode Framework、動的Plugin Registry、Trait Object中心のGenerator登録、任意DSP Scriptを導入しない。

音質に関わる部分を「後で改善する」前提の仮実装で済ませない。

特に次を禁止する。

- Wavetableを一つのFull-band Tableだけで全音域再生する
- FMとPMを同じ名称で曖昧に扱う
- Operator間接続をDefinitionの任意Graphとして実装する
- Feedback値の発散をRuntime Clampだけへ任せる
- Wavefoldを単なる既存Driveの別名として実装する
- Audio ThreadでFFT、Asset Decode、Table生成、Vec拡張を行う
- 音が出たことだけでGenerator完成と判定する

### 0.3 本書で固定するもの

- 対象範囲と対象外
- 各GeneratorのDefinition
- Parameter ID、Unit、Range、Smoothing
- Wavetable Asset LayoutとCompile時準備
- Wavetableの帯域制限方式
- Operator数と固定Algorithm
- FM / PM / AM / Ring Modの意味
- Operator EnvelopeとNote Lifecycle
- Oscillator Phase Distortion、Wavefold、Feedbackの信号順序
- 既存Hard Sync、Waveshaping、Unisonとの組み合わせ制約
- Compiled Instrumentが保持する不変データ
- RuntimeがVoiceごとに保持する可変状態
- Generator Output Mode
- Asset不足、入力不正、Runtime失敗時の挙動
- CLI InspectとReference Instrument
- Unit Test、Integration Test、Sound Review
- 三実装単位の順序と完了条件

### 0.4 本書で固定しないもの

次は本Phaseへ含めない。

- Granular
- Time Stretch
- Pitch ShiftとTime Stretchの分離
- Reverse Sample
- Loop Crossfade
- Release Sample
- Wave Sequence
- Additive
- Spectral / Resynthesis
- Physical / Modal / Waveguide
- Formant Generator
- Vocoder
- Envelope Follower
- MSEG
- Step Modulator
- Macro
- Vector Synthesis
- Tempo Sync Source
- Sustain Pedal
- Mono / Legato / Portamento
- Realtime Audio Device
- Realtime MIDI Device
- Public C ABI
- Riffra統合
- CLAP
- VST3
- GUI Editor
- Wavetable編集UI
- Audio Fileからの自動Wavetable抽出
- Wavetable Spectral Editor
- DX7 SysEx Import
- DX7互換Algorithm番号、Envelope、Scaling
- 6 Operator以上
- 任意Operator Graph
- User-defined Audio-rate Routing
- User-defined Feedback Routing
- Arbitrary Formula Oscillator
- 一般用途のOversampling Framework
- Preset Migration
- Deprecated Field
- Legacy Definition Alias
- 新しいSchema Version

---

# 1. 目的と完成像

## 1.1 現在の実装状態

現在のSonalloyには、次が成立している。

- JSON Instrument Definition
- Definition Validation
- Instrument Definition / Compiled Instrument / Runtime Instanceの分離
- Sample AccurateなProcess Contract
- Polyphonic VoiceとVoice Stealing
- Layer Trigger、Layer Envelope、Gain、Pan、Tuning
- Dynamic Parameter、Parameter Change、Smoothing
- Velocity、Key Tracking、LFO、Envelope、Random、Pitch Bend、Mod Wheel、Aftertouch
- Layer / Voice / Global Processor Chain
- Filter、Drive、Delay、Reverb
- Sine、Saw、Square、Triangle、Pulse、PWM
- White、Pink、Brown Noise
- Hard Sync
- Waveshaping
- Unison、Detune、Stereo Spread、Phase Distribution
- Multi Sample Zone
- Key Mapping、Velocity Layer、Round Robin
- Forward Loop、Explicit Slice
- CLI Validate、Inspect、Note / Events / MIDI Offline Render
- Windows / Linux CI、Native Fault Injection、Sound Review Package

現在のGeneratorは、`Oscillator`、`Noise`、`Sample`の三種類である。

本Phaseはこの構造を壊さず、Generator Variantを追加する。

## 1.2 本Phaseで成立させること

完成時には次の流れが成立する。

```text
Instrument Definition
    │
    ├─ Wavetable Generator + Wavetable Asset
    ├─ Operator Modulation Generator + Fixed Algorithm
    └─ Oscillator + Phase Distortion / Wavefold / Feedback
    │
    ▼
Validation / Compile
    │
    ├─ Asset Resolve / Hash / Decode
    ├─ Wavetable Frame分割
    ├─ FFTによるBand Table生成
    ├─ Parameter Catalog / Handle解決
    ├─ Operator Topology解決
    ├─ Unison Distribution準備
    └─ Runtime Memory量確定
    │
    ▼
Instrument Runtime
    │
    ├─ Note / Parameter / Modulation Event
    ├─ VoiceごとのPhase / Envelope / Feedback状態
    ├─ Wavetable Position / Band選択
    ├─ Audio-rate Operator相互作用
    └─ Unison / Stereo Mix
    │
    ▼
Layer Envelope / Processor Chain
    │
    ▼
Offline Stereo WAV
```

## 1.3 完成状態

本Phaseの完成状態は次である。

> Wavetable、4 OperatorのFM / PM / AM / Ring Mod、Phase Distortion、Wavefold、Oscillator FeedbackをInstrument Definitionへ保存し、Compile時に必要なAssetと実行構造を準備し、既存のParameter、Modulation、Unison、Processor Chainへ統合したうえで、決定的かつBlock Size非依存なStereo AudioとしてRenderできる。

## 1.4 製品上の到達点

本Phase完了後、少なくとも次の音色をSonalloy単体で構築できる。

- Wavetable Bass
- Wavetable Pad
- Moving Digital Texture
- FM Bell
- FM Electric Piano
- FM Bass
- PM Metallic Lead
- AM Tremolo Texture
- Ring Mod Bell / Noise Texture
- Phase Distortion Brass
- Phase Distortion Lead
- Wavefold Lead
- Feedback Drone
- Wavetable + Operator + Sample LayerのHybrid Instrument

## 1.5 代表成果物

### Wavetable Motion Bass

```text
Wavetable Generator
  ├─ Position ← LFO
  ├─ Position ← Mod Wheel
  ├─ Unison Detune
  └─ Stereo Spread
        ▼
Layer Envelope
        ▼
Voice Filter / Drive
        ▼
Global Delay / Reverb
```

確認する価値：

- Frame間Morphが滑らかである
- 高音域でFull-band Table由来の明確なAliasが出ない
- Position変更がBlock Sizeへ依存しない
- UnisonとWavetable Positionが独立して機能する

### Four Operator FM Bell

```text
Operator 4 ─▶ Operator 3 ─▶ Operator 2 ─▶ Operator 1
                                          │
                                          ▼
                                       Output
```

確認する価値：

- Operator RatioとEnvelopeで時間変化する倍音を作れる
- AlgorithmとModulation Modeの違いが明確に聞き分けられる
- Feedbackが発散せず音色変化として機能する
- Note On / Off、Voice Stealing、ResetでOperator状態が混ざらない

### Digital Hybrid Lead

```text
Layer A: Phase Distortion + Feedback Oscillator
Layer B: Wavetable Generator
Layer C: Sample Attack
              │
              ▼
           Layer Mix
              │
              ▼
       Voice Processor Chain
```

確認する価値：

- 新Generatorと既存Sample Layerを一つの音色へ融合できる
- Wavefold、Feedback、Filter、Driveの役割が重複せず調整できる
- 音色として曲中で利用可能である

---

# 2. 対象範囲

## 2.1 Wavetable Generator

含める機能：

- Mono WAV AssetをWavetable Sourceとして利用
- Definitionで明示する`frame_length`
- Asset全体を連続する複数Frameとして分割
- 1 FrameだけのStatic Wavetable
- 複数FrameのWavetable Position
- Frame間Linear Interpolation
- Table内Four-point Cubic Interpolation
- Compile時FFT
- Harmonic上限の異なるBand Table生成
- RuntimeでのBand選択
- Band間Crossfade
- Phase Reset
- Initial Phase
- PositionへのParameter Change / Modulation
- Optional Unison
- Unison Detune / Stereo SpreadへのParameter Change / Modulation
- Missing Assetの部分読込
- CLI Inspect
- Offline Render
- Sound Review

含めない機能：

- Audioから周期境界を自動検出する処理
- Frame Lengthの自動推定
- Spectral Morph
- Formant Preserve
- Wavetable Warp
- Wavetable Phase Randomization
- Stereo Wavetable
- Sample Rateを意味に含むWavetable Format
- Serum、Vital、Ableton等のPreset / Table Import

## 2.2 Operator Modulation Generator

含める機能：

- 4 Operator固定
- Sine Operator
- 固定Algorithm
- Phase Modulation
- Frequency Modulation
- Amplitude Modulation
- Ring Modulation
- OperatorごとのFrequency Ratio
- OperatorごとのDetune
- OperatorごとのOutput Level
- OperatorごとのModulation Amount
- OperatorごとのADSR Envelope
- OperatorごとのInitial Phase
- OperatorごとのOne-sample Feedback
- Optional Unison
- Parameter Change / Modulation
- Voice Stealing / Reset / Note Off統合
- CLI Inspect
- Offline Render
- Sound Review

含めない機能：

- 2 / 3 / 5 / 6 Operatorの可変Operator数
- Operatorごとの任意Waveform
- 任意接続Graph
- User-defined Algorithm
- Operator間のCycle
- Operator間のDelay
- External AudioをOperatorへ入力
- DX7互換Envelope
- Keyboard Scaling
- Velocity Scaling専用構造
- Fixed Frequency Operator
- Sync Operator

## 2.3 Complex Oscillator Completion

含める機能：

- Sine OscillatorのPhase Distortion
- Sine OscillatorのOne-sample Phase Feedback
- 既存Oscillator全波形に対するWavefold
- Phase Distortion AmountへのParameter Change / Modulation
- Oscillator FeedbackへのParameter Change / Modulation
- Wavefold AmountへのParameter Change / Modulation
- Phase Distortion + Feedbackの組み合わせ
- Wavefold + Existing Waveshapingの組み合わせ
- Wavefold + Hard Syncの組み合わせ
- Wavefold + Unisonの組み合わせ
- DC成分の抑制
- 高周波数時の安全な周波数上限
- CLI Inspect
- Offline Render
- Sound Review

含めない組み合わせ：

- Hard Sync + Phase Distortion
- Hard Sync + Oscillator Feedback
- Pulse / Saw / Square / TriangleへのPhase Distortion
- Pulse / Saw / Square / TriangleへのOscillator Feedback
- Audio-rate Modulation Matrixからの任意Phase入力
- User-defined Feedback Routing

## 2.4 共通品質

次を必須とする。

1. 既存Definitionが新Fieldなしで従来どおりCompileできる
2. 新機能を使わない既存Oscillatorの通常経路を変更しない
3. Process中にFile I/O、JSON、FFT、Asset Decodeを行わない
4. Process中にHeap Allocationまたは容量拡張を行わない
5. NaN / Infinityを出力しない
6. Parameter Sweepで明確なClickを出さない
7. Block Sizeを変更しても時間軸とEvent位置が変わらない
8. Reset後に新規Runtimeと同等の出力を得る
9. Voice Stealingで旧VoiceのPhase、Envelope、Feedbackが新Voiceへ漏れない
10. Sample Rate 44.1 / 48 / 96 kHzでFiniteかつ非無音の出力を得る
11. 同じDefinition、Asset、Event、Seed、Process Specから同等の出力を得る
12. 技術的に動作するだけでなく、人間が音色として承認する

---

# 3. 既存設計との接続

## 3.1 維持する依存方向

```text
sonalloy-cli
    ↓
sonalloy-core
    ↓
sonalloy-dsp-sys
    ↓
DaisySP
```

新しいWorkspace Crateは追加しない。

Wavetable、Operator Modulation、Phase-domain Oscillatorの製品ModelとRuntimeは`sonalloy-core`が所有する。WavefoldのDSP本体だけは、既に採用済みのDaisySPに含まれるMITライセンスの`daisysp::Wavefolder`を`sonalloy-dsp-sys`経由で利用する。そのため本Phaseでは`sonalloy-dsp-sys`と`native/daisysp-wrapper`をWavefolder統合の範囲に限って拡張する。

DaisySPの型や名称をDefinitionへ公開しない。DefinitionとParameter Contractは引き続きSonalloyの製品Modelとして定義する。

## 3.2 維持する三層構造

```text
Definition
    ↓ Validate / Resolve / Compile
Compiled Instrument
    ↓ Instantiate
Runtime Instance
```

### Definitionが保持するもの

- Generator種類
- Wavetable Asset参照
- Frame Length
- Wavetable Position
- Operator Algorithm
- Operator Modulation Mode
- Operator Ratio、Detune、Level、Modulation Amount
- Operator Envelope
- Phase Distortion、Wavefold、Feedbackの初期値
- Unison設定

### Compiled Instrumentが保持するもの

- 解決済みParameter Handle
- Prepared Wavetable
- Wavetable Band Table
- Operator固定Topology
- Sample Rate固有のEnvelope Frame数
- Unison Distribution
- 周波数上限
- Runtime配列長

### Runtime Instanceが保持するもの

- Wavetable Phase
- Wavetable Band選択の一時値
- Operator Phase
- Operator Envelope State
- Operator Previous Output
- Oscillator Feedback Sample
- DaisySP WavefolderのNative Handle（Monoは1、Stereoは左右独立2）
- DC Blocker State
- Unison Component State
- 事前確保済みScratch Buffer

## 3.3 Generator契約

既存の`GeneratorDefinition`、`CompiledGenerator`、`GeneratorRuntime`へ次を追加する。

```text
GeneratorDefinition
├─ Oscillator
├─ Noise
├─ Sample
├─ Wavetable
└─ OperatorModulation

CompiledGenerator
├─ Oscillator
├─ Noise
├─ Sample
├─ Wavetable
└─ OperatorModulation

GeneratorRuntime
├─ Oscillator
├─ Noise
├─ Sample
├─ Wavetable
└─ OperatorModulation
```

Generatorは既存と同じLifecycleへ従う。

```text
new / prepare
    ↓
start(Note On)
    ↓
render(繰り返し)
    ↓
note_off
    ↓
reset
```

Operator Envelopeのため、Generator共通Lifecycleへ`note_off`を追加する。

既存Generatorの実装は次とする。

- Oscillator：`note_off`はNo-op
- Noise：`note_off`はNo-op
- Sample：`note_off`はNo-op
- Wavetable：`note_off`はNo-op
- Operator Modulation：全Operator EnvelopeをReleaseへ移行

`note_off`追加のためにGenerator Traitや動的Dispatchを導入しない。現在のEnum Matchを維持する。

## 3.4 Generator Output Mode

| Generator | 条件 | Output Mode |
|---|---|---|
| Oscillator | Unison 1 | Mono |
| Oscillator | Unison 2以上 | Stereo |
| Noise | 常時 | Stereo |
| Sample | 現在のMono Sample | Mono |
| Wavetable | Unison 1 | Mono |
| Wavetable | Unison 2以上 | Stereo |
| Operator Modulation | Unison 1 | Mono |
| Operator Modulation | Unison 2以上 | Stereo |

Output ModeはDefinition値からRuntimeで再判断しない。

`CompiledGenerator::output_mode()`を正本とし、Layer RuntimeとCLI Inspectの両方から使用する。

## 3.5 Parameter / Modulationとの接続

既存の次の流れを維持する。

```text
DefinitionのBase値
    ↓
Parameter Catalog
    ↓
Parameter ChangeによるBase値Smoothing
    ↓
Modulation Route加算
    ↓
Range Clamp
    ↓
ValueSpan
    ↓
Generator Runtime
```

Audio-rate Operator相互作用を通常Modulation Matrixへ載せない。

通常Modulation Matrixが扱うのは、Operator Ratio、Level、Index等の制御値である。

Operator出力同士のSample単位接続は`OperatorModulationRuntime`内部で処理する。

## 3.6 Processor Chainとの接続

新Generatorの出力後は既存Layer Pipelineをそのまま使用する。

```text
Generator
    ↓
Layer Envelope
    ↓
Layer Gain / Pan
    ↓
Layer Processor Chain
    ↓
Layer Mix
    ↓
Voice Processor Chain
    ↓
Voice Sum
    ↓
Global Processor Chain
```

Generator内部へFilter、Delay、Reverb等を追加しない。

Operator Envelopeは音響方式固有の倍音時間変化を作るためGenerator内部に置く。

Layer EnvelopeはGenerator全体の発音Lifecycleと最終振幅を管理する。

---

# 4. DSP・依存実装方針

## 4.1 結論

| 機能 | 採用方式 | 変更 | 判断 |
|---|---|---|---|
| Wavetable Asset Decode | 既存Symphonia経路を再利用 | なし | WAV Decode、Hash、Downmixの契約を共有する |
| Wavetable FFT / IFFT | RustFFT | `sonalloy-core`へ一Dependency追加 | Compile時だけ使用し、Band Tableを生成する |
| Wavetable Runtime | Rust独自実装 | なし | Phase、Interpolation、Band選択、UnisonをSonalloyが所有する |
| Operator Modulation | Rust独自実装 | なし | 固定Topology、Phase、Envelope、FeedbackをCoreが所有する |
| Sine Operator | Rust `sin` | なし | Phase / Frequency Modulation入力をSample単位で直接扱うため |
| Phase Distortion | Rust独自Phase Engine | なし | Phase MappingとFeedback Stateを同じRuntimeで扱う |
| Wavefold | DaisySP `Wavefolder` | `sonalloy-dsp-sys` / Native Wrapperを拡張 | 単体DSP部品として成熟した既存実装を利用し、製品Parameterと信号順序はSonalloyが所有する |
| Oscillator Feedback | Rust独自実装 | なし | One-sample Feedbackとして固定し任意Routingにしない |
| Existing Basic / Hard Sync | 現在のDaisySP Backendを維持 | なし | 新機能未使用時の既存音声経路を変えない |

### 外部DSP候補の比較と採否

本Phaseでは、外部実装を使えるからという理由だけでNative側へ寄せない。既存ライブラリがSonalloyの要求をそのまま満たす場合だけ採用する。

| 対象 | 候補 | 採否 | 理由 |
|---|---|---|---|
| Wavetable FFT | RustFFT / RealFFT / 独自FFT | RustFFT | Compile時のComplex FFT / IFFTを直接扱え、RealFFTを追加で重ねる利点が小さい |
| 4 Operator FM系 | DaisySP `Fm2` / Rust独自 | Rust独自 | `Fm2`は2 Operator FMを主対象とし、4 Operator固定Algorithm、PM / AM / Ring、Operator Envelopeまでを一つの契約として表現できない |
| Wavefold | DaisySP `Wavefolder` / Rust独自 | DaisySP `Wavefolder` | Sonalloy固有のTopologyを必要とせず、既存Native境界へ小さく追加できる。音の生成に関わる非線形DSPを実績ある実装へ委譲できる |
| Phase Distortion | DaisySP Oscillator群 / Rust独自 | Rust独自 | Sonalloyで固定するPhase MappingとOne-sample Feedbackを同一Phase Engineで管理する必要がある |
| Oscillator Feedback | 外部Module / Rust独自 | Rust独自 | 限定Topologyで小さく完結し、外部依存を増やす利点がない |

### DaisySP Wavefolderのライセンス境界

採用対象はDaisySP本体の`Source/Effects/wavefolder.h` / `wavefolder.cpp`にある`daisysp::Wavefolder`である。これはファイル自身にMIT-style licenseが明記され、DaisySP本体のMITライセンス範囲に含まれる。

**DaisySP-LGPLの`Effects/fold`は別Moduleであり、本Phaseでは使用しない。**

したがって次を守る。

- `DaisySP-LGPL`をDependencyへ追加しない
- `USE_DAISYSP_LGPL`を有効化しない
- LGPL版`Fold`のSourceをコピー、移植、参照実装として転用しない
- 既存`native/daisysp-wrapper/CMakeLists.txt`で固定しているDaisySP Commit `a0494a3adb67f549e18dfd71a35fa656f65b38b6`を維持し、同CommitのMIT版`Source/Effects/wavefolder.cpp`だけを追加Compileする
- DaisySP Aggregate Targetは使用せず、現在と同じく必要Sourceだけを明示列挙する。DaisySP-LGPLをBuild / Link対象へ含めない
- 固定Commitを変更する必要が生じた場合は、Wavefolderの存在とMIT Headerを確認してから新Commitへ固定する。`master`や`latest`へ追従しない

## 4.2 RustFFTの扱い

RustFFTはWavetableのCompile時準備に限定して使用する。

用途：

- Source FrameのFrequency Domain変換
- Harmonic上限を超えるBinの除去
- Band Tableの逆変換

禁止事項：

- Audio ThreadでFFT Planを作成する
- RuntimeでFFTを実行する
- FFT型をPublic Contractへ公開する
- RustFFTのComplex型をCompiled Instrumentへ保持する

本Phaseでは`rustfft = "6.4.1"`を採用候補として固定する。RustFFT 6.4.1はMIT / Apache-2.0のデュアルライセンスであり、現在のRust 1.85要件を満たす。実装時に別Versionへ変更する場合は、理由をPlanと`Cargo.lock`へ反映する。

Plan作成段階で将来のSpectral Engineを理由にFFT Wrapper Frameworkを作らない。

## 4.3 Asset Decodeの再利用

現在のSample Asset処理は、Hash確認、WAV Decode、Mono Downmix、Sample Rate変換を一つの経路で行う。

WavetableではSample Rate変換を行わない。

Wavetableの各Sampleは時間長ではなく、一周期内の位相位置を表すためである。

そのため`asset.rs`の処理を次へ整理する。

```text
read_and_verify_asset
    ├─ Path Resolve
    ├─ File Read
    └─ SHA-256 Verify

load_mono_audio
    ├─ WAV Decode
    ├─ Channel Validation
    ├─ Downmix
    └─ Finite Validation

prepare_sample_asset
    ├─ load_mono_audio
    └─ Target Sample RateへResample

prepare_wavetable_asset
    ├─ load_mono_audio
    ├─ Frame Layout Validation
    └─ Band Table生成
```

既存Sampleの出力を変更しない。

単にWavetableを追加するために、既存Sample用ResamplerやPrepared Sampleの意味を変更しない。

## 4.4 Native DSP境界

`sonalloy-dsp-sys`と`native/daisysp-wrapper`は、Wavefoldのために限定的に拡張する。Wavetable、Operator Modulation、Phase Distortion、Oscillator FeedbackはRust側へ残す。

### Wavefolder Native Wrapper

既存のOpaque Handle方式へ`DspWavefolder`を追加する。DaisySP型をRust CoreやDefinitionへ直接公開しない。

概念上のC ABI：

```text
sonalloy_dsp_wavefolder_create
sonalloy_dsp_wavefolder_destroy
sonalloy_dsp_wavefolder_prepare
sonalloy_dsp_wavefolder_reset
sonalloy_dsp_wavefolder_process
sonalloy_dsp_wavefolder_process_ramp
```

`prepare` / `reset`ではDaisySP `Wavefolder::Init()`を呼び、Offsetは`0.0`へ固定する。本PhaseではAsymmetric Fold用Offsetを製品Parameterとして公開しない。

`process` / `process_ramp`はBufferをin-placeで処理する。Core側の`wavefold` Amount 0〜1を次へ変換してNativeへ渡す。

```text
drive = 1 + amount × 7
mix   = amount
```

Native Wrapperは各SampleについてDaisySP `SetGain(drive)` → `Process(input)`を行い、最後にDry/Wetを線形補間する。Ramp版では`drive`と`mix`をSpan内で線形補間する。これによりAmount 0を厳密なIdentityとし、Amount Sweepを既存Parameter Spanへ統合する。

DaisySP `Wavefolder`自体の`SetOffset`は常に0とし、本PhaseのDefinitionへ露出させない。

### 変更するNative File

- `native/daisysp-wrapper/CMakeLists.txt`：現在の固定Commitと必要Source明示列挙方式を維持し、`Source/Effects/wavefolder.cpp`を追加。既存の「oscillatorだけを使用する」というCommentも実態に合わせて更新する
- `native/daisysp-wrapper/include/sonalloy_dsp.h`：Opaque HandleとWavefolder C ABIを追加
- `native/daisysp-wrapper/src/daisysp_wrapper.cpp`：Create / Destroy / Prepare / Reset / Process / Rampと例外境界を追加
- `crates/sonalloy-dsp-sys/src/ffi.rs`：FFI宣言を追加
- `crates/sonalloy-dsp-sys/src/wavefolder.rs`：安全なRust Wrapperを追加
- `crates/sonalloy-dsp-sys/src/lib.rs`：`DspWavefolder`を公開

### Native Error Contract

既存Native境界と同じ規則を適用する。

- C++例外をRustへ越境させない
- Null Handle、未Prepare、非Finite、Range外をResult Codeへ変換する
- `frames > 0`でBufferがNullならError
- Native処理失敗時は対象Output Bufferを無音化する
- Process中にAllocationしない
- Test HookによるFault InjectionをWavefolderにも適用する
- Guard付きTestでBuffer範囲外書込がないことを確認する

### Rust側に残す理由

Wavetable、Operator、Phase Distortion、FeedbackをNative C++へ実装しない理由：

- Definition、Compile、Parameter、Runtime StateをRustが所有している
- Operator間のSample単位接続はRust側の固定Topologyとして表現しやすい
- Wavetable AssetはRust側で準備される
- Phase DistortionとFeedbackは同一Phase Stateを共有する
- Wavefolder以外にOpaque HandleとFault Injection経路を増やす必然性がない

## 4.5 数値形式

- Runtime Sample：`f32`
- Wavetable Source / Band Table：`f32`
- FFT内部：`f32` Complex
- Phase：`f64`ではなく`f32`の0以上1未満
- Sample Rateと時間変換：既存どおり`f64`
- Operatorの中間Signal：`f32`

長時間連続発音時のPhase精度は、Phaseを毎Sample`rem_euclid(1.0)`または範囲内減算で保持することで維持する。

## 4.6 Nonlinear処理の方針

WavefoldとFeedbackは有限性と高域Aliasを明示的に検証する。

本Phaseでは一般的なOversampling Frameworkを導入しない。

代わりに次を必須とする。

- 連続なPhase Mappingを使用する
- Feedback入力をSoft Boundする
- WavefoldはDaisySP `Wavefolder`へ委譲し、Sonalloy Amount 0〜1を固定のDrive / Dry-Wet契約へ変換する
- DaisySP WavefolderのOffsetは0へ固定し、左右で同一Amount Spanを使用する
- Nonlinear機能を使用するBackendの基本周波数上限を保守的に設定する
- DC Blockerを新機能の経路だけへ適用する
- 高音域Reference RenderとSpectrum ReviewをMerge条件にする

人間の試聴で明確なAliasが残る場合、Wavefold Amount Range、Drive変換上限、または有効周波数上限を調整する。

この調整を行わず、「将来Oversamplingする」として完了扱いにしない。DaisySP採用を理由に音質Reviewを省略しない。

---

# 5. 信号モデル

## 5.1 Wavetable Generator

```text
Note Frequency + Layer Tuning
        │
        ├─ Unison Detune Distribution
        │
        ▼
Component Frequency
        │
        ├─ Wavetable Band選択
        ├─ Wavetable Position
        ├─ Frame間Interpolation
        └─ Table内Interpolation
        │
        ▼
Component Signal
        │
        ├─ Unison Pan Distribution
        └─ Normalization
        │
        ▼
MonoまたはStereo Generator Output
```

固定順序を変更可能にしない。

## 5.2 Operator Modulation Generator

```text
Note Frequency + Layer Tuning
        │
        ├─ Operator Ratio / Detune
        ├─ Operator Envelope
        ├─ Previous Sample Feedback
        └─ Fixed Algorithm
        │
        ▼
Modulator Operator評価
        │
        ▼
Carrier Operator評価
        │
        ▼
Carrier Sum / Normalization
        │
        ├─ Unison Pan Distribution
        └─ Unison Normalization
        │
        ▼
MonoまたはStereo Generator Output
```

Operatorは一SampleごとにTopologyの依存順で評価する。

## 5.3 Complex Oscillator

既存経路：

```text
DaisySP Basic / Hard Sync
        │
        ▼
Unison Mix
        │
        ▼
Existing Waveshaping
        │
        ▼
Generator Output
```

Phase-domain機能使用時：

```text
Base Phase
    │
    ├─ One-sample Feedback Phase Offset
    │
    ▼
Phase Distortion Mapping
    │
    ▼
Sine Generation
    │
    ▼
Unison Mix
    │
    ▼
Existing Waveshaping
    │
    ▼
DaisySP Wavefolder
    │
    ▼
DC Blocker
    │
    ▼
Generator Output
```

Wavefoldだけを使用する場合、既存Basic / Hard Sync出力の後へ適用する。

## 5.4 Parameter Span

各Dynamic Parameterは既存`ValueSpan`を使用する。

RuntimeはSpanの開始値と終了値を受け取り、Sample位置に応じて補間する。

Wavetable Position、Operator Ratio、Modulation Amount、Feedback、Phase Distortion、WavefoldをBlock先頭の一値だけで処理しない。

## 5.5 Zero-frame処理

`frames == 0`の場合：

- Bufferへ書き込まない
- Phaseを進めない
- Envelopeを進めない
- Feedback Stateを変更しない
- Errorを発生させない
- Generatorの終了状態だけを既存契約に従って返す

---

# 6. Instrument Definition

## 6.1 GeneratorDefinitionの拡張

概念上、次を追加する。

```rust
pub enum GeneratorDefinition {
    Oscillator(OscillatorDefinition),
    Noise(NoiseDefinition),
    Sample(SampleDefinition),
    Wavetable(WavetableDefinition),
    OperatorModulation(OperatorModulationDefinition),
}
```

Serde表現は既存と同じexternally taggedなsnake_caseを維持する。

未知Fieldは引き続き拒否する。

新しいGeneratorを追加するために`type` Fieldを持つ別形式へ全Definitionを移行しない。

## 6.2 WavetableDefinition

概念構造：

```rust
pub struct WavetableDefinition {
    pub asset: AssetReference,
    pub frame_length: u16,
    pub position: f32,
    pub phase_reset: bool,
    pub phase: f32,
    pub unison: Option<UnisonDefinition>,
}
```

### Field契約

| Field | 型 | 範囲 | Dynamic | 意味 |
|---|---|---:|---:|---|
| `asset` | `AssetReference` | — | No | Wavetable WAV Asset |
| `frame_length` | `u16` | 64〜4096、2の冪 | No | 一つの周期Frameに含まれるSample数 |
| `position` | `f32` | 0〜1 | Yes | 最初から最後のFrameまでの位置 |
| `phase_reset` | `bool` | — | No | Note OnでPhaseを初期化するか |
| `phase` | `f32` | 0〜1 | No | Initial Phase |
| `unison` | Optional | 既存契約 | 一部Yes | Wavetable Engine全体のUnison |

`frame_count`はDefinitionへ保存しない。

Asset Sample数を`frame_length`で割ってCompile時に算出する。

理由：

- 二重管理を避ける
- Asset差替え時の不整合をCompileで検出する
- AIがFrame Countを誤記しても暗黙補正しない

### JSON例

```json
{
  "wavetable": {
    "asset": {
      "path": "../assets/wavetables/digital-motion.wav",
      "sha256": "..."
    },
    "frame_length": 2048,
    "position": 0.25,
    "phase_reset": true,
    "phase": 0.0,
    "unison": {
      "voices": 5,
      "detune_cents": 14.0,
      "stereo_spread": 0.75,
      "phase_spread": 0.5
    }
  }
}
```

## 6.3 Wavetable Asset Layout

WAVのMono Sample列を次のように解釈する。

```text
Frame 0: samples [0 .. frame_length)
Frame 1: samples [frame_length .. frame_length * 2)
Frame 2: samples [frame_length * 2 .. frame_length * 3)
...
```

制約：

- MonoまたはStereo WAV
- Stereoの場合は既存規則でMonoへDownmixしWarning
- PCM16、PCM24、Float32の既存対応範囲
- Decode後Sample数が`frame_length`で割り切れる
- Frame Countは1〜256
- 各FrameはFinite
- Asset全体が無音でない
- Frame Lengthは2の冪
- Source Sample RateはWavetableの時間軸として使用しない

Frame境界にCrossfadeを加えない。

Wavetable Frameはそれぞれ独立した一周期波形として扱う。

## 6.4 OperatorModulationDefinition

概念構造：

```rust
pub struct OperatorModulationDefinition {
    pub mode: OperatorModulationMode,
    pub algorithm: OperatorAlgorithm,
    pub operators: [OperatorDefinition; 4],
    pub phase_reset: bool,
    pub unison: Option<UnisonDefinition>,
}
```

Serdeで固定長Arrayを扱いにくい場合、Definitionでは`Vec<OperatorDefinition>`を使用してよい。

ただしValidationで必ず4件を要求し、Compiled Modelでは`[CompiledOperator; 4]`へ変換する。

### OperatorModulationMode

```rust
pub enum OperatorModulationMode {
    Phase,
    Frequency,
    Amplitude,
    Ring,
}
```

JSON名：

- `phase`
- `frequency`
- `amplitude`
- `ring`

### OperatorDefinition

```rust
pub struct OperatorDefinition {
    pub ratio: f32,
    pub detune_cents: f32,
    pub level: f32,
    pub modulation_amount: f32,
    pub feedback: f32,
    pub phase: f32,
    pub envelope: AdsrDefinition,
}
```

### Field契約

| Field | 範囲 | Dynamic | 意味 |
|---|---:|---:|---|
| `ratio` | 0.25〜32 | Yes | Note Frequencyに対する倍率 |
| `detune_cents` | -100〜100 | Yes | Ratio適用後の微調整 |
| `level` | 0〜1 | Yes | CarrierとしてのOutput Level |
| `modulation_amount` | Mode依存 | Yes | 接続先を変調する強さ |
| `feedback` | 0〜1 | Yes | 自Operatorの一つ前のSampleを戻す量 |
| `phase` | 0〜1 | No | Initial Phase |
| `envelope` | 既存ADSR | No | Operator Outputへ適用するEnvelope |

`level`と`modulation_amount`を一つのFieldへ統合しない。

Carrier LevelとModulator Indexは音響上の役割が異なるためである。

### Mode別modulation_amount

| Mode | Definition Range | Parameter Unit | Runtime意味 |
|---|---:|---|---|
| Phase | 0〜8 | `index` | 最大8π rad相当のPhase Offset |
| Frequency | 0〜8 | `index` | Base Frequency比のDeviation量 |
| Amplitude | 0〜1 | `normalized` | Unipolar AM Depth |
| Ring | 0〜1 | `normalized` | DryからRing ProductへのMix量 |

### Feedback制約

- `phase`と`frequency`で使用可能
- `amplitude`と`ring`では0だけを許可
- 一つ前の自Operator出力だけを使用
- 同一Sampleの出力を直接戻さない
- 他OperatorへのFeedbackは扱わない

### JSON例

```json
{
  "operator_modulation": {
    "mode": "phase",
    "algorithm": "stack_4",
    "phase_reset": true,
    "operators": [
      {
        "ratio": 1.0,
        "detune_cents": 0.0,
        "level": 0.9,
        "modulation_amount": 0.0,
        "feedback": 0.0,
        "phase": 0.0,
        "envelope": {
          "attack_seconds": 0.001,
          "decay_seconds": 1.4,
          "sustain_level": 0.0,
          "release_seconds": 0.2
        }
      },
      {
        "ratio": 2.0,
        "detune_cents": 0.0,
        "level": 0.0,
        "modulation_amount": 2.5,
        "feedback": 0.0,
        "phase": 0.0,
        "envelope": {
          "attack_seconds": 0.001,
          "decay_seconds": 0.7,
          "sustain_level": 0.0,
          "release_seconds": 0.1
        }
      },
      {
        "ratio": 3.0,
        "detune_cents": 0.0,
        "level": 0.0,
        "modulation_amount": 1.8,
        "feedback": 0.0,
        "phase": 0.0,
        "envelope": {
          "attack_seconds": 0.001,
          "decay_seconds": 0.35,
          "sustain_level": 0.0,
          "release_seconds": 0.08
        }
      },
      {
        "ratio": 5.0,
        "detune_cents": 0.0,
        "level": 0.0,
        "modulation_amount": 1.0,
        "feedback": 0.2,
        "phase": 0.0,
        "envelope": {
          "attack_seconds": 0.001,
          "decay_seconds": 0.15,
          "sustain_level": 0.0,
          "release_seconds": 0.05
        }
      }
    ],
    "unison": null
  }
}
```

## 6.5 Operator Algorithm

固定Algorithmは次の8種類とする。

Operator番号は1〜4を利用者向け表記とし、内部Indexは0〜3とする。

### `stack_4`

```text
4 → 3 → 2 → 1 → Output
```

Carrier：1

### `stack_3_plus_carrier`

```text
4 → 3 → 2 ─┐
             ├→ Output
1 ──────────┘
```

Carrier：1、2

### `two_stacks`

```text
2 → 1 ─┐
        ├→ Output
4 → 3 ─┘
```

Carrier：1、3

### `fork_to_carrier`

```text
      ┌→ 2 ─┐
4 ────┤      ├→ 1 → Output
      └→ 3 ─┘
```

Carrier：1

### `two_modulators_plus_carrier`

```text
3 ─┐
   ├→ 1 ─┐
4 ─┘     ├→ Output
2 ───────┘
```

Carrier：1、2

### `three_modulators`

```text
2 ─┐
3 ─┼→ 1 → Output
4 ─┘
```

Carrier：1

### `shared_modulator`

```text
      ┌→ 1 ─┐
4 ────┼→ 2 ─┼→ Output
      └→ 3 ─┘
```

Carrier：1、2、3

### `parallel`

```text
1 ─┐
2 ─┤
3 ─┼→ Output
4 ─┘
```

Carrier：1、2、3、4

Algorithmを数値だけで保存しない。

Definitionでは意味の分かるsnake_case名を保存する。

Compiled Modelでは次を固定配列として保持する。

```rust
pub struct CompiledOperatorTopology {
    pub evaluation_order: [u8; 4],
    pub incoming_masks: [u8; 4],
    pub carrier_mask: u8,
    pub carrier_normalization: f32,
}
```

RuntimeでAlgorithm名をMatchしない。

## 6.6 OscillatorDefinitionの拡張

概念構造：

```rust
pub struct OscillatorDefinition {
    pub waveform: OscillatorWaveform,
    pub phase_reset: bool,
    pub phase: f32,
    pub hard_sync: Option<HardSyncDefinition>,
    pub waveshaping: Option<WaveshapingDefinition>,
    pub phase_distortion: Option<PhaseDistortionDefinition>,
    pub wavefold: Option<WavefoldDefinition>,
    pub feedback: Option<OscillatorFeedbackDefinition>,
    pub unison: Option<UnisonDefinition>,
}
```

追加Fieldは`#[serde(default)]`のOptionalとする。

新Fieldがない既存Definitionの意味を変えない。

### PhaseDistortionDefinition

```rust
pub struct PhaseDistortionDefinition {
    pub amount: f32,
}
```

Range：0〜1

### WavefoldDefinition

```rust
pub struct WavefoldDefinition {
    pub amount: f32,
}
```

Range：0〜1

### OscillatorFeedbackDefinition

```rust
pub struct OscillatorFeedbackDefinition {
    pub amount: f32,
}
```

Range：0〜1

### JSON例

```json
{
  "oscillator": {
    "waveform": {
      "type": "sine"
    },
    "phase_reset": true,
    "phase": 0.0,
    "hard_sync": null,
    "waveshaping": {
      "amount": 0.25
    },
    "phase_distortion": {
      "amount": 0.65
    },
    "wavefold": {
      "amount": 0.4
    },
    "feedback": {
      "amount": 0.3
    },
    "unison": {
      "voices": 3,
      "detune_cents": 9.0,
      "stereo_spread": 0.6,
      "phase_spread": 0.25
    }
  }
}
```

## 6.7 Schemaと互換性

`CURRENT_SCHEMA_VERSION`は1のまま維持する。

理由：

- 新Generator Variant追加は既存Definitionの意味を変更しない
- Oscillatorの新FieldはOptionalである
- 旧形式を読み替えるMigrationが不要である
- 未成熟段階でVersionを増やして互換分岐を固定しない

次を追加しない。

- `schema_version = 2`
- `legacy_wavetable`
- 旧Field Alias
- Deprecated Field
- 複数Definition Parser

---

# 7. Definition Validation

## 7.1 共通規則

新Fieldは既存と同じ方針でDefinition段階に検証する。

- 非Finite値を拒否
- Range外を拒否
- ID Grammar違反を拒否
- 構成上不可能な組み合わせを拒否
- Runtimeで暗黙Fallbackしない
- Unknown Fieldを拒否

## 7.2 Wavetable Validation

Definitionだけで検証できるもの：

- `frame_length`が64〜4096
- `frame_length`が2の冪
- `position`が0〜1
- `phase`が0〜1
- Unisonが既存Range内
- Wavetable Unisonは最大8 Voice
- Asset Pathが空でない
- SHA-256文字列形式が既存契約を満たす

Asset Decode後に検証するもの：

- Sample Countが0でない
- Sample Countが`frame_length`で割り切れる
- Frame Countが1〜256
- 全SampleがFinite
- 全Frameが無音ではない
- FrameごとのPeakが極端に小さくない

無音Frameが一部だけ存在する場合：

- Compile Errorにはしない
- `WAVETABLE_SILENT_FRAME` Warningを返す
- Frame IndexをDetailへ含める

判定基準：

- Silent Frame：RMSが`1.0e-6`未満
- DC Warning：Frame Meanの絶対値が`0.01`を超える

Asset全体が無音の場合：

- Wavetable Preparation失敗
- 対象Layerを無効として扱う
- Instrument全体は他Layerが有効ならCompile継続

## 7.3 Operator Validation

- Operator数がちょうど4
- Ratioが0.25〜32
- Detuneが-100〜100 cents
- Levelが0〜1
- Phaseが0〜1
- Feedbackが0〜1
- Envelopeが既存ADSR契約を満たす
- Mode別のModulation Amount Rangeを満たす
- AM / Ring ModeではFeedbackが0
- Unisonは最大4 Voice
- Carrier Maskに含まれるOperatorのLevelがすべて0の場合はError
- CarrierではないOperatorは`level = 0`を要求する
- 出力先を持たないOperatorは`modulation_amount = 0`を要求する
- Modulatorとして使われるOperatorのModulation Amountが0でも許可
- Parallel AlgorithmでModulation Amountが非0でもWarningにしない。未使用値として保持せずValidation Errorにする

意味のない設定を黙って無視しない。

Parameter CatalogもTopologyに従い、利用されないLevel、Modulation Amount、Feedbackを公開しない。

## 7.4 Oscillator組み合わせValidation

| 組み合わせ | 結果 |
|---|---|
| Sine + Phase Distortion | 許可 |
| Sine + Feedback | 許可 |
| Sine + Phase Distortion + Feedback | 許可 |
| Any Waveform + Wavefold | 許可 |
| Any Waveform + Existing Waveshaping + Wavefold | 許可 |
| Hard Sync + Wavefold | 許可 |
| Hard Sync + Existing Waveshaping + Wavefold | 許可 |
| Hard Sync + Phase Distortion | Error |
| Hard Sync + Feedback | Error |
| Non-Sine + Phase Distortion | Error |
| Non-Sine + Feedback | Error |

Diagnostic Pathは対象Fieldを指す。

例：

```text
layers[0].generator.oscillator.phase_distortion
```

## 7.5 Parameter ID Validation

新Parameterも既存Grammarを使用する。

Canonical ID例：

```text
layer.motion.generator.wavetable_position
layer.motion.generator.unison_detune
layer.fm.generator.operator.1.ratio
layer.fm.generator.operator.1.detune
layer.fm.generator.operator.1.level
layer.fm.generator.operator.1.modulation_amount
layer.fm.generator.operator.1.feedback
layer.lead.generator.phase_distortion
layer.lead.generator.wavefold
layer.lead.generator.oscillator_feedback
```

Operator番号は利用者向けに1始まりとする。

Runtime内部Indexへ変換するのはCompile時だけとする。

現在のParameter Grammarは`layer.<id>.generator.<suffix>`だけを受け付けるため、次のPatternを追加する。

```text
layer.<layer_id>.generator.operator.<1-4>.<operator_parameter>
```

`is_parameter_id()`へ専用Branchを追加し、数字だけのSegmentを一般的なComponent IDとして許可しない。

Operator番号位置だけを1〜4のASCII数字として検証する。

`operator_parameter`は次のいずれかとする。

```text
ratio
detune
level
modulation_amount
feedback
```

通常Generator Parameterは既存`generator_parameters::is_suffix()`を使用し、Operator Parameterは`is_operator_parameter()`等の専用関数を使用する。

## 7.6 Resource Limit Validation

本Phaseでは一般的なCPU Budget Frameworkを作らない。

次のHard Limitだけを検証する。

| 項目 | 上限 |
|---|---:|
| Wavetable Frame Length | 4096 |
| Wavetable Frame Count | 256 |
| Wavetable Unison | 8 |
| Operator Count | 4固定 |
| Operator Unison | 4 |
| Oscillator Unison | 既存上限 |
| Wavetable Band Level | Frame Lengthから算出、最大13 |

上限超過はCompile Errorとする。

暗黙に切り捨てない。

---

# 8. Parameter Contract

## 8.1 ParameterUnitの追加

Operator Modulation Indexを表すため、次を追加する。

```rust
pub enum ParameterUnit {
    Decibels,
    Pan,
    Cents,
    Hertz,
    Ratio,
    Normalized,
    Index,
}
```

`Index`は単位を持たない合成Indexを表す。

Normalized 0〜1へ押し込めて意味を隠さない。

## 8.2 GeneratorParameterSpec

既存`generator_parameters.rs`を正本として拡張する。

追加する共通Spec：

| Symbol | Suffix | Unit | Scale | Min | Max | Smoothing |
|---|---|---|---|---:|---:|---:|
| `WAVETABLE_POSITION` | `wavetable_position` | Normalized | Linear | 0 | 1 | 0.010 s |
| `PHASE_DISTORTION` | `phase_distortion` | Normalized | Linear | 0 | 1 | 0.005 s |
| `WAVEFOLD` | `wavefold` | Normalized | Linear | 0 | 1 | 0.005 s |
| `OSCILLATOR_FEEDBACK` | `oscillator_feedback` | Normalized | Linear | 0 | 1 | 0.005 s |

Operator ParameterはOperator番号とModeでDefault / Unit / Rangeが変わるため、単一のStatic `GeneratorParameterSpec`配列へ無理に押し込まない。

専用Builderを用意する。

```rust
fn push_operator_descriptors(
    descriptors: &mut Vec<ParameterDescriptor>,
    layer_id: &str,
    definition_index: usize,
    definition: &OperatorModulationDefinition,
)
```

## 8.3 Wavetable Parameter

| Canonical ID | Default | Range | Dynamic |
|---|---:|---:|---:|
| `layer.<id>.generator.wavetable_position` | Definition position | 0〜1 | Yes |
| `layer.<id>.generator.unison_detune` | Unison detune | 0〜100 cents | Yes |
| `layer.<id>.generator.unison_spread` | Unison spread | 0〜1 | Yes |

`phase`、`frame_length`、AssetはDynamic Parameterにしない。

## 8.4 Operator Parameter

Operator 1の例：

```text
layer.<id>.generator.operator.1.ratio
layer.<id>.generator.operator.1.detune
layer.<id>.generator.operator.1.level
layer.<id>.generator.operator.1.modulation_amount
layer.<id>.generator.operator.1.feedback
```

### Descriptor

| Parameter | Unit | Scale | Min | Max | Smoothing |
|---|---|---|---:|---:|---:|
| Ratio | Ratio | Log2 | 0.25 | 32 | 0.005 s |
| Detune | Cents | Linear | -100 | 100 | 0.005 s |
| Level | Normalized | Linear | 0 | 1 | 0.005 s |
| PM / FM Amount | Index | Linear | 0 | 8 | 0.005 s |
| AM / Ring Amount | Normalized | Linear | 0 | 1 | 0.005 s |
| Feedback | Normalized | Linear | 0 | 1 | 0.005 s |

TopologyとModeに応じてDescriptorを限定する。

- `ratio`と`detune`：常に公開
- `level`：Carrier Operatorだけ公開
- `modulation_amount`：少なくとも一つの出力先を持つOperatorだけ公開
- `feedback`：Phase / Frequency Modeだけ公開

公開しないFieldはDefinitionで0を要求し、Runtime Parameter Handleを持たない。

Operator EnvelopeのADSR値は本PhaseではDynamic Parameterにしない。

## 8.5 Complex Oscillator Parameter

| Canonical ID | Range | Smoothing |
|---|---:|---:|
| `layer.<id>.generator.phase_distortion` | 0〜1 | 0.005 s |
| `layer.<id>.generator.wavefold` | 0〜1 | 0.005 s |
| `layer.<id>.generator.oscillator_feedback` | 0〜1 | 0.005 s |

Optional機能がDefinitionに存在する場合だけParameter Catalogへ追加する。

機能がないのにParameterだけ公開しない。

## 8.6 LayerGeneratorTargetSpan

概念上、次へ拡張する。

```rust
pub enum LayerGeneratorTargetSpan {
    Oscillator {
        pulse_width: Option<ValueSpan>,
        sync_ratio: Option<ValueSpan>,
        waveshape: Option<ValueSpan>,
        phase_distortion: Option<ValueSpan>,
        wavefold: Option<ValueSpan>,
        oscillator_feedback: Option<ValueSpan>,
        unison_detune: Option<ValueSpan>,
        unison_spread: Option<ValueSpan>,
    },
    Noise {
        correlation: ValueSpan,
    },
    Sample,
    Wavetable {
        position: ValueSpan,
        unison_detune: Option<ValueSpan>,
        unison_spread: Option<ValueSpan>,
    },
    OperatorModulation {
        operators: [OperatorTargetSpan; 4],
        unison_detune: Option<ValueSpan>,
        unison_spread: Option<ValueSpan>,
    },
}
```

```rust
pub struct OperatorTargetSpan {
    pub ratio: ValueSpan,
    pub detune: ValueSpan,
    pub level: Option<ValueSpan>,
    pub modulation_amount: Option<ValueSpan>,
    pub feedback: Option<ValueSpan>,
}
```

`LayerGeneratorTargetSpan`をMapや文字列Lookupへ変更しない。

Generator種類ごとのTagged Enumを維持する。

## 8.7 VoiceTargetScratch

`VoiceTargetScratch::new()`で、Compiled Generator種類に一致するZero Targetを事前作成する。

現在のように全Layerを一度`Sample`で初期化し、後から上書きする方法は新Variant増加に伴い不明瞭になる。

次のCompiler側Helperを正本とする。

```rust
impl CompiledGenerator {
    fn zero_target_span(&self) -> LayerGeneratorTargetSpan
}
```

または同等のRuntime Helperを一つだけ置く。

Generatorごとに別箇所でZero Targetを再定義しない。

## 8.8 Modulation Route Scope

新ParameterのOwnerは既存どおり`ParameterOwner::LayerGenerator`とする。

Voice Source、Pitch Bend、Mod Wheel、AftertouchからRoute可能とする。

Global Processor向け制約は変更しない。

Operator間のAudio-rate SignalはRoute Sourceとして公開しない。

次を禁止する。

```text
operator.4.output → arbitrary layer parameter
operator.2.output → processor cutoff
wavetable audio → operator phase
```

---

# 9. Compiled Model

## 9.1 CompiledGenerator

```rust
pub enum CompiledGenerator {
    Oscillator(CompiledOscillator),
    Noise(CompiledNoise),
    Sample(CompiledSample),
    Wavetable(CompiledWavetable),
    OperatorModulation(CompiledOperatorModulation),
}
```

## 9.2 PreparedWavetable

概念構造：

```rust
pub struct PreparedWavetable {
    pub frame_length: usize,
    pub frame_count: usize,
    pub bands: Box<[PreparedWavetableBand]>,
    pub source_metadata: WavetableSourceMetadata,
}
```

```rust
pub struct PreparedWavetableBand {
    pub max_harmonic: usize,
    pub frames: Box<[PreparedWavetableFrame]>,
}
```

```rust
pub struct PreparedWavetableFrame {
    pub guarded_samples: Box<[f32]>,
}
```

各Frameの`guarded_samples`はFour-point Cubic Interpolationの境界処理を簡潔にするため、元Tableの前後Sampleを複製して保持する。

概念Layout：

```text
[last, sample0, sample1, ..., sampleN-1, sample0, sample1]
```

Runtimeで負Indexや剰余演算を多用しない。

## 9.3 CompiledWavetable

```rust
pub struct CompiledWavetable {
    pub prepared: Option<Arc<PreparedWavetable>>,
    pub phase_reset: bool,
    pub phase: f32,
    pub parameters: CompiledWavetableParameters,
    pub unison: Arc<CompiledUnison>,
    pub asset_path: String,
}
```

```rust
pub struct CompiledWavetableParameters {
    pub position: ParameterHandle,
    pub unison_detune: Option<ParameterHandle>,
    pub unison_spread: Option<ParameterHandle>,
}
```

Asset失敗時は`prepared = None`とする。

Compile ErrorでInstrument全体を失敗させずWarningを保持する。

RuntimeでFile再読込しない。

## 9.4 CompiledOperatorModulation

```rust
pub struct CompiledOperatorModulation {
    pub mode: OperatorModulationMode,
    pub topology: CompiledOperatorTopology,
    pub operators: [CompiledOperator; 4],
    pub phase_reset: bool,
    pub parameters: [CompiledOperatorParameters; 4],
    pub unison: Arc<CompiledUnison>,
    pub effective_max_frequency: f32,
}
```

```rust
pub struct CompiledOperator {
    pub envelope: CompiledAdsr,
    pub phase: f32,
}
```

```rust
pub struct CompiledOperatorParameters {
    pub ratio: ParameterHandle,
    pub detune: ParameterHandle,
    pub level: Option<ParameterHandle>,
    pub modulation_amount: Option<ParameterHandle>,
    pub feedback: Option<ParameterHandle>,
}
```

Definitionの文字列Algorithm名をRuntimeへ持ち込まない。

## 9.5 CompiledOscillatorの拡張

```rust
pub struct CompiledOscillatorParameters {
    pub pulse_width: Option<ParameterHandle>,
    pub waveshape: Option<ParameterHandle>,
    pub phase_distortion: Option<ParameterHandle>,
    pub wavefold: Option<ParameterHandle>,
    pub oscillator_feedback: Option<ParameterHandle>,
    pub unison_detune: Option<ParameterHandle>,
    pub unison_spread: Option<ParameterHandle>,
}
```

Backendへ次を追加する。

```rust
pub enum CompiledOscillatorBackend {
    Basic,
    VariableShapeSync { sync_ratio: ParameterHandle },
    PhaseDomain,
}
```

Backend選択：

| Definition | Backend |
|---|---|
| 通常Oscillator | Basic |
| Hard Sync | VariableShapeSync |
| Phase Distortionあり | PhaseDomain |
| Feedbackあり | PhaseDomain |
| Phase Distortion + Feedback | PhaseDomain |
| Wavefoldだけ | 既存Backendを維持 |

## 9.6 周波数上限

正本はCompiled BackendまたはGeneratorのCore関数へ一つだけ置く。

| Generator / Mode | Effective Max Frequency |
|---|---:|
| Existing Basic Oscillator | `sample_rate * 0.45` |
| Existing Hard Sync | `sample_rate * 0.24` |
| Phase-domain Oscillator | `sample_rate * 0.24` |
| Wavetable | `sample_rate * 0.45`、Band Table選択あり |
| Operator PM / FM | `sample_rate * 0.24` |
| Operator AM / Ring | `sample_rate * 0.45` |

CLI Inspect、Runtime Clamp、Testが同じ関数を使用する。

## 9.7 Generator Availability

Wavetable Asset失敗をLayer単位で扱うため、次を追加する。

```rust
impl CompiledGenerator {
    fn is_available(&self) -> bool
}
```

規則：

- Oscillator：true
- Noise：true
- Sample：選択可能なZoneが一つ以上
- Wavetable：`prepared.is_some()`
- Operator Modulation：true

Note OnのLayer Selection時にAvailabilityを確認する。

無効Generatorを持つLayerは発音対象にしない。

これにより、無音LayerのEnvelopeだけがVoiceを保持し続ける状態を避ける。

既存Sampleも同じ正本へ統合する。

---

# 10. Compile Pipeline

## 10.1 全体順序

```text
JSON Parse
    ↓
Definition Validation
    ↓
Parameter Catalog構築
    ↓
LayerごとのGenerator Compile
    ├─ Oscillator
    ├─ Noise
    ├─ Sample
    ├─ Wavetable
    └─ Operator Modulation
    ↓
Processor Compile
    ↓
Modulation Route解決
    ↓
Error有無確認
    ↓
Compiled Instrument公開
```

## 10.2 Wavetable Compile

```text
Asset Path Resolve
    ↓
Read / SHA Verify
    ↓
WAV Decode / Mono Downmix
    ↓
Frame Layout Validation
    ↓
各FrameのFFT
    ↓
Harmonic Band生成
    ↓
IFFT
    ↓
Guard Sample付与
    ↓
Arc<PreparedWavetable>
```

### Band生成

Frame Lengthを`N`とする。

Source FFTの正周波数Bin 1〜`N/2`をHarmonic番号として扱う。

次のHarmonic上限を降順で準備する。

```text
N/2, N/4, N/8, ..., 1
```

重複する値は一つにする。

各Bandでは`max_harmonic`を超える正負両側のBinを0にする。

DC BinはSourceの値を保持する。

Nyquist Binは実数信号の対称性を壊さないよう個別に扱う。

IFFT後は`1/N`でScaleする。

Bandごとの自動Peak Normalizeを行わない。

理由：

- Band切替時の音量差を人為的に拡大しない
- Source Wavetableの振幅関係を維持する
- Frameごとの意図したLevel差を保持する

### Band選択

RuntimeでComponent Frequencyを`f`、Sample Rateを`sr`とする。

```text
allowed_harmonic = floor((sr * 0.45) / f)
```

`allowed_harmonic`を超える内容を持たないBandを安全な候補として選ぶ。隣接BandとのCrossfadeは、
現在の再生周波数で両方のBandがこの条件を満たすOverlap範囲だけで行う。安全なOverlapを
持たない周波数では、より低いHarmonic上限のBandへ移行してからCrossfadeする。

Crossfadeの判定には`(sr * 0.45) / f`の連続値を使用し、Bandの安全性判定にはそのFloor値を
使用する。現在Bandの上限を`H`、次のBandの上限を`L`とすると、最初のBand以外は
`H`から直前Bandの上限まで、最初のBandは`H * (H / L)`までを安全なOverlapとする。

最もFull-band側または最もSine側を超えた場合は端のBandを使用する。

Band選択はUnison Componentごとの実周波数で行う。

Base Noteだけで一度選択しない。

## 10.3 Wavetable Asset Cache

Sample Asset CacheとWavetable Asset Cacheを区別する。

同じFile PathでもPreparationの意味が異なるため、同じCache Entryを共有しない。

概念Key：

```rust
struct WavetableAssetCacheKey {
    path: PathBuf,
    sha256: Option<String>,
    frame_length: usize,
}
```

Sample RateはKeyへ含めない。

Prepared WavetableはEngine Sample Rateに依存しないためである。

同じAsset + Frame Lengthを複数Layerが使う場合、`Arc`を共有する。

## 10.4 Operator Compile

- Algorithm EnumをTopologyへ変換
- Operator EnvelopeをSample Rate固有Frame数へ変換
- Parameter HandleをOperatorごとに解決
- Modeに応じたParameter Descriptor RangeとRuntime係数を決定
- Carrier数からNormalizationを計算
- Unison Distributionを準備
- Effective Max Frequencyを決定

TopologyはCompile後に不変とする。

RuntimeでAlgorithmの分岐を文字列Matchしない。

## 10.5 Complex Oscillator Compile

- 組み合わせValidation済みであることを前提とする
- Optional Parameter Handleを解決
- Backendを選択
- Unisonを既存HelperでCompile
- Phase-domain Backendの周波数上限を保存
- DC Blockerが必要かをStatic Flagとして保存

## 10.6 Partial Compile

| 状況 | 対応 |
|---|---|
| Wavetable Asset missing | Warning、Wavetable Layer無効、他Layer継続 |
| Hash mismatch | Warning、Wavetable Layer無効、他Layer継続 |
| Decode failure | Warning、Wavetable Layer無効、他Layer継続 |
| Layout invalid | Error。DefinitionとAssetの組み合わせが不正 |
| FFT準備中のNon-finite | Error |
| Operator値不正 | Error |
| Oscillator組み合わせ不正 | Error |

Assetが存在しない場合は外部環境要因として部分読込を許可する。

Assetが存在するがFrame LayoutがDefinitionと矛盾する場合はDefinition Errorとして拒否する。

## 10.7 Compile時Allocation

Compile側ではVec、HashMap、FFT Plan、Temporary Bufferを使用してよい。

ただしCompiled Instrumentへ不要なScratchを残さない。

保持対象：

- Prepared Wavetableの最終Band Table
- Metadata
- Parameter Handle
- Topology
- Static Distribution

保存しないもの：

- FFT Plan
- Complex Spectrum Scratch
- IFFT Temporary
- Asset File Byte列
- Decode Packet Buffer

---

# 11. Runtime共通統合

## 11.1 GeneratorRuntime

概念構造：

```rust
pub enum GeneratorRuntime {
    Oscillator(OscillatorRuntime),
    Noise(Box<NoiseRuntime>),
    Sample { sample: SampleRuntime },
    Wavetable(WavetableRuntime),
    OperatorModulation(Box<OperatorModulationRuntime>),
}
```

Operator RuntimeはState量が大きいためBox化してよい。

BoxはRuntime生成時に一度確保する。

Process中には確保しない。

## 11.2 Lifecycle

### `new`

- Compiled構造を受け取る
- Runtime配列を固定長で作成
- Unison Component数を確定
- Scratch Bufferを`ProcessSpec.max_block_size`に合わせて確保
- Phase、Envelope、Feedback、DC Blockerを初期化
- Asset Fileへアクセスしない

### `start`

- Note Onに対応
- Phase Reset設定を反映
- Operator EnvelopeをNote On
- Feedback Stateを0へ戻す
- Wavetable Asset unavailableの場合はLayer自体が選択されない

### `render`

- Buffer長を検証
- Generator VariantとTarget Span Variantの一致を検証
- Dynamic SpanをRange検証
- Process中Allocationなし
- 出力Finite性を検証

### `note_off`

- Operator EnvelopeだけReleaseへ移行
- 他GeneratorはNo-op

### `reset`

- Phase
- Envelope
- Feedback
- DC Blocker
- Scratchの意味上の状態

を初期状態へ戻す。

Scratch Buffer全体のZero Fillは必要な範囲だけ行う。

## 11.3 LayerRuntimeとの接続

Layer Note Off処理で、Layer EnvelopeとGeneratorの両方へNote Offを通知する。

順序：

```text
Generator note_off
    ↓
Layer Envelope note_off
```

同一Frameで両方をReleaseへ移行する限り、内部関数呼出し順は音声結果へ影響させない。

Voice StealingでPending Noteへ切り替える場合：

1. 旧VoiceのFadeを完了
2. Layer / GeneratorをReset
3. 新NoteのTargetとSelectionを適用
4. Generator Start
5. Layer Envelope Start

旧Operator EnvelopeやFeedbackを新Noteへ引き継がない。

## 11.4 Scratch Buffer

既存Layer Runtimeが持つMono / Left / Right Scratchを利用する。

WavetableとOperator用に、Process Block全体とは別の新しい大規模BufferをVoiceごとに増やさない。

必要な一時値は次へ限定する。

- Wavetable Componentを一時的にMono ScratchへRender
- Operator ComponentをMono ScratchへRender
- Stereo Unison時は既存Left / Right Scratchへ加算
- Operator Envelopeの現在値は固定長Array
- Operator Signalは固定長Array

## 11.5 Allocation検査

既存Thread-local Allocation Counterを利用し、次を検証する。

- Wavetable Note On
- Wavetable Render
- Wavetable Voice Stealing
- Operator Note On
- Operator Render
- Operator Note Off
- Operator Voice Stealing
- Phase-domain Oscillator Render

いずれもHeap Allocation 0とする。

## 11.6 Error時の状態

Generator Renderが失敗した場合：

- 対象Process出力を無音化
- RuntimeをInvalid状態として扱う
- 次回Processを継続しない
- Prepare / Instantiateし直すまで復帰しない

既存Process Failure Contractを維持する。

一部Sampleだけ進んだGenerator Stateを、そのまま次Blockから継続しない。

---

# 12. Wavetable Runtime詳細

## 12.1 Runtime State

```rust
pub struct WavetableRuntime {
    components: Vec<WavetableComponentRuntime>,
    prepared: Option<Arc<PreparedWavetable>>,
    phase_reset: bool,
    phase: f32,
    unison: Arc<CompiledUnison>,
}
```

```rust
pub struct WavetableComponentRuntime {
    phase: f32,
}
```

Prepared Dataは全Voiceで`Arc`共有する。

PhaseだけをVoice / Unison Componentごとに保持する。

## 12.2 Start / Reset

`phase_reset = true`の場合、各Component Phaseを次へ戻す。

```text
initial_phase = definition_phase + compiled_phase_distribution[index]
```

0以上1未満へWrapする。

`phase_reset = false`の場合、Note OnでPhaseを維持する。

Instrument Resetでは設定に関係なくInitial Phaseへ戻す。

## 12.3 Frequency

各Component Frequency：

```text
base_frequency
  × cents_to_ratio(layer_tuning)
  × cents_to_ratio(unison_position × unison_detune)
```

WavetableのFrequencyは正かつFiniteでなければならない。

上限を`sample_rate * 0.45`へClampする。

## 12.4 Position

Position 0はFrame 0、Position 1は最後のFrameを表す。

Frame Countを`F`とする。

```text
frame_position = position × (F - 1)
left_frame = floor(frame_position)
right_frame = min(left_frame + 1, F - 1)
frame_fraction = fract(frame_position)
```

Frame Count 1の場合、常にFrame 0だけを使用する。

Position Spanは各Sampleで補間する。

## 12.5 Table内Interpolation

Phaseを0以上1未満とする。

```text
table_position = phase × frame_length
index = floor(table_position)
fraction = fract(table_position)
```

Guard付きTableから4点を取得し、既存Sample Cubicと同じ係数式を使用する。

Cubic式をWavetable用に別実装しない。

Sample RuntimeのInterpolation Helperを一般化して共有するか、共通の小さな`interpolation.rs`へ移す。

大規模DSP Utility Frameworkは作らない。

## 12.6 Band選択とCrossfade

各Componentの現在FrequencyからAllowed Harmonicを求める。

FrequencyがSpan内で変化するため、Band選択もSample単位で変化し得る。

ただし毎Sample Binary Searchしない。

Prepared Band数は最大13であるため、次のどちらかを採用する。

- 小さなLinear Scan
- `leading_zeros`等を利用したLog2 Level算出

実装は読みやすい方を選ぶ。

Band境界では、両方のBandが現在のAllowed Harmonic以内に収まるOverlap範囲で二つのBand間を
Crossfadeする。Crossfade中にAllowed Harmonicを超えるBandを混ぜない。

Crossfadeしない離散切替は禁止する。

## 12.7 一Sampleの読み出し

概念順序：

```text
1. Frequencyを算出
2. Band PairとBand Mixを算出
3. PositionからFrame PairとFrame Mixを算出
4. 四つのTable組合せを読む
   - lower band / left frame
   - lower band / right frame
   - upper band / left frame
   - upper band / right frame
5. Frame方向を補間
6. Band方向を補間
7. Phaseを進める
```

Frame方向とBand方向の補間順で結果が変わらないLinear Interpolationを使用する。

## 12.8 Unison

既存`CompiledUnison`とConstant-power Panを再利用する。

Oscillator Runtimeに閉じたUnison Mix Helperは、WavetableとOperatorでも利用できる場所へ移す。

共有するもの：

- Position Distribution
- Phase Distribution
- Normalization
- Stereo Spread
- Detune
- Component Mix

共有しないもの：

- Oscillator Backend
- Wavetable Band選択
- Operator Topology

## 12.9 Missing Asset

`prepared = None`のWavetableはLayer Selectionで除外される。

防御的にRuntimeへ到達した場合：

- Invalid State Error
- 出力無音化

Silent Fallback Generatorへ自動変換しない。

## 12.10 Finite / DC

Wavetable SourceのDCは自動除去しない。

理由：Source Assetの意味を暗黙に変えないためである。

ただしCompile時にFrameごとのMean Absolute DCを測定し、閾値を超えたFrameへWarningを返す。

RuntimeはFiniteだけを保証する。

---

# 13. Operator Modulation Runtime詳細

## 13.1 Runtime State

```rust
pub struct OperatorModulationRuntime {
    components: Vec<OperatorComponentRuntime>,
    envelopes: [AdsrRuntime; 4],
    mode: OperatorModulationMode,
    topology: CompiledOperatorTopology,
    unison: Arc<CompiledUnison>,
}
```

```rust
pub struct OperatorComponentRuntime {
    phases: [f32; 4],
    previous_outputs: [f32; 4],
}
```

EnvelopeはUnison Component間で共有する。

同じNote内のUnison Componentは同じOperator Envelope時間軸を持つためである。

各Componentの初期Phaseは、Operator固有PhaseへCompiled Unison Phase Offsetを加えて作る。

## 13.2 Operator Envelope

- Note Onで4 EnvelopeをStart
- Note Offで4 EnvelopeをRelease
- ResetでIdle
- Voice Stealing後の新Noteで再Start

一SampleのEnvelope値を4 Operator分一度だけ計算し、全Unison Componentで共有する。

Envelope出力はOperator Waveformへ乗算する。

## 13.3 Evaluation Order

Compile済み`evaluation_order`に従う。

AlgorithmはCycleを持たない。

Self Feedbackだけが一Sample Delayを持つCycleである。

一Sampleごとに次を行う。

```text
for operator in evaluation_order:
    modulation_input = incoming operator outputs
    feedback_input = previous_outputs[operator]
    output = evaluate_operator(...)

carrier_sum = sum(output[carrier])
previous_outputs = current_outputs
```

`current_outputs`はSineへOperator Envelopeを乗算した値であり、Carrier LevelとModulation Amountを掛ける前のSignalとする。

Carrier LevelはCarrier Sum時、Modulation Amountは接続先で使用する。

Current Sampleで後に評価されるModulatorを参照しない。

## 13.4 Phase Modulation

OperatorのBase Phase AccumulatorはBase Frequencyで進める。

Modulationは読み出しPhaseへ加える。

```text
phase_offset_cycles
  = Σ(modulator_output × modulation_amount × 0.5)
```

`modulation_amount = 1`をπ rad相当、8を8π rad相当とする。

Cycles表現では0.5 cycleがπ radである。

Self Feedback：

```text
feedback_offset
  = tanh(previous_output × feedback_amount × 2.5) × 0.25 cycle
```

最終Read Phase：

```text
read_phase = base_phase + phase_offset + feedback_offset
```

Base Phase自体へOffsetを蓄積しない。

## 13.5 Frequency Modulation

Modulator出力からInstantaneous Frequency Ratio Offsetを作る。

```text
frequency_ratio_offset
  = Σ(modulator_output × modulation_amount)
```

```text
instantaneous_frequency
  = base_frequency × (1 + frequency_ratio_offset + feedback_offset)
```

Feedback OffsetはSoft BoundしたPrevious Outputから作る。

Instantaneous Frequencyは正負を許可するが、絶対値をEffective Max Frequency以下へClampする。

0付近の符号反転でNaNや停止を起こさない。

Phase Increment：

```text
instantaneous_frequency / sample_rate
```

## 13.6 Amplitude Modulation

Incoming ModulatorごとにUnipolar Multiplierを作る。

```text
multiplier = 1 + modulator_output × depth
```

Depth 0ではMultiplier 1、Depth 1では0〜2となる。

複数Incomingがある場合は順番に乗算する。

最終Multiplierは0〜4へClampする。

Feedbackは許可しない。

## 13.7 Ring Modulation

Incoming ModulatorごとにDry / ProductをCrossfadeする。

```text
product = carrier_signal × modulator_output
result = carrier_signal + (product - carrier_signal) × depth
```

複数Incomingがある場合はTopology順に適用する。

Depth 0は元Signal、Depth 1は完全なBipolar Productである。

Feedbackは許可しない。

## 13.8 Operator Sine

初期実装は`f32::sin(phase * TAU)`を使用する。

Lookup Tableを同時に導入しない。

理由：

- PM / FMの正しいPhase入力を最優先する
- 4 Operator × 最大4 UnisonというHard Limitがある
- 現時点ではRealtime Deviceを持たない
- 近似誤差を音質問題へ追加しない

Performance Reviewで明確に支配的な負荷となった場合だけ、同じ出力精度を検証したLookup Tableへ置き換える。

本Phase完了前に性能測定は行うが、計測前の最適化はしない。

## 13.9 Operator Frequency

```text
operator_frequency
  = note_frequency
  × operator_ratio
  × cents_to_ratio(layer_tuning + operator_detune + unison_detune)
```

Layer TuningとPitch Bendは4 Operatorすべてへ同じ音程比として反映する。

Operator Ratioはその後に適用する。

PM / FMではBase Frequencyを`sample_rate * 0.24`以下へClampする。

AM / Ringでは`sample_rate * 0.45`以下へClampする。

## 13.10 Carrier Sum

Carrier Maskに含まれるOperator出力を加算する。

Normalization：

```text
1 / sqrt(carrier_count)
```

各Carrierの`level`を乗算してから加算する。

Modulator専用Operatorの`level`はOutputへ直接寄与しない。

## 13.11 Unison

Operator Engine全体をUnison Componentとして複製する。

各Componentは独立PhaseとPrevious Outputを持つ。

Envelopeは共有する。

最大4 Componentとする。

DetuneはNote Base Frequencyへ適用し、全Operator Ratioへ伝播する。

Stereo SpreadはCarrier Sum後のComponent出力へ適用する。

## 13.12 Feedback Safety

- Feedback Amountは0〜1
- Previous OutputはFinite確認
- `tanh`でBound
- Phase / Frequencyへ加える係数を固定
- Runtimeで無限増幅するAccumulatorを持たない
- ResetでPrevious Outputを0

Feedback SignalをOutputへ直接加算しない。

## 13.13 Operatorの終了

Generator自体の終了判定は行わない。

Layer EnvelopeがVoice Lifecycleを所有する。

全Operator EnvelopeがIdleになってもLayer EnvelopeがActiveなら0を出力し続ける。

この挙動を最適化する場合も、Layer終了をGeneratorが勝手に決定しない。

---

# 14. Complex Oscillator Runtime詳細

## 14.1 既存経路の保持

次の条件では現在の`OscillatorRuntime`処理を変更しない。

- Phase Distortionなし
- Oscillator Feedbackなし
- Wavefoldなし

既存Waveshapingだけを使用するDefinitionも現在の経路を維持する。

音声回帰を避けるため、新しい共通Engineへ全面置換しない。

## 14.2 PhaseDomain Component

```rust
pub struct PhaseDomainOscillatorComponent {
    phase: f32,
    previous_output: f32,
}
```

Unison Componentごとに保持する。

WaveformはSine固定である。

## 14.3 Phase Distortion Mapping

Amount 0でIdentityとする。

Breakpoint：

```text
breakpoint = 0.5 - amount × 0.45
```

Amount 0で0.5、Amount 1で0.05となる。

Input Phaseを`p`とする。

```text
if p < breakpoint:
    warped = 0.5 × p / breakpoint
else:
    warped = 0.5 + 0.5 × (p - breakpoint) / (1 - breakpoint)
```

Mappingは0〜1の連続関数である。

Breakpointを0または1へ到達させない。

## 14.4 Oscillator Feedback

One-sample Previous OutputをPhaseへ戻す。

```text
feedback_phase
  = tanh(previous_output × amount × 2.5) × 0.25 cycle
```

処理順：

```text
base phase
    ↓ add feedback_phase
phase distortion mapping
    ↓
sine
    ↓ store previous_output
```

Phase Distortionなしの場合も同じPhase Engineを使用する。

## 14.5 Wavefold

WavefoldはUnison MixとExisting Waveshapingの後へ適用し、DSP本体にはDaisySP `daisysp::Wavefolder`を使用する。

SonalloyのDefinitionとParameterはDaisySPの`gain`や`offset`をそのまま公開しない。利用者向け契約は従来どおり`wavefold.amount` 0〜1だけとする。

### Amountの意味

AmountからDaisySPへ渡すDriveとDry/Wetを次の固定式で作る。

```text
drive = 1 + amount × 7
mix   = amount
```

- Amount 0：Stage Skip。入力を一切変更しない
- Amount 1：Drive 8、Wet 100%
- 中間値：DriveとWet量を同時に増やす
- Offset：常に0。Symmetric Foldのみ

Native Wrapper内部では各Sampleについて次を実行する。

```text
wavefolder.SetGain(drive)
folded = wavefolder.Process(input)
output = input + (folded - input) × mix
```

Dynamic Parameter時は`drive`と`mix`を`ValueSpan`から変換した開始値・終了値としてNative Ramp APIへ渡し、Native側でSampleごとに線形補間する。Rustから一SampleごとにFFI Callしない。

### Runtime所有

`OscillatorRuntime`はWavefoldがDefinitionで有効な場合だけ`DspWavefolder`を所有する。Native HandleはRuntime生成時に作成・Prepareし、Process中に生成・破棄しない。Reset時は`DspWavefolder::reset()`を呼ぶ。

Mono出力では1つの`DspWavefolder`を所有する。Stereo出力では左・右に独立した`DspWavefolder`を所有し、同じAmount Spanで各Bufferを処理する。現在のDaisySP `Wavefolder`は履歴状態を持たないが、Opaque Backendの内部実装へCoreが依存しないため、チャンネルごとにHandleを分離する。Offsetは両方とも0、Parameterは共通とする。

### DaisySP `Fold`との区別

使用するのはMIT版`Wavefolder`であり、DaisySP-LGPLにある`Fold`ではない。名称が似ているため、Wrapper、Document、Commentでは`Wavefolder`を正式名称として使用する。

既存Drive Processorとは別の音響機能である。Drive Processorの実装とWavefolderのNative Wrapperを共有しない。

## 14.6 DC Blocker

Phase Distortion、Feedback、Wavefoldのいずれかが有効な場合、Generator末尾へ一Pole DC Blockerを置く。

```text
y[n] = x[n] - x[n-1] + r × y[n-1]
```

`r`はSample Rateから10 Hz相当としてPrepare時に算出する。

Mono経路は一State、Stereo経路は左右独立Stateを持つ。

ResetでStateを0へ戻す。

## 14.7 組み合わせ時の順序

固定順：

1. Oscillator / Phase-domain生成
2. Unison Component Mix
3. Existing Waveshaping
4. DaisySP Wavefolder
5. DC Blocker

Definitionで順序を変更できない。

## 14.8 Frequency Clamp

Phase-domain Backendは`sample_rate * 0.24`を上限とする。

WavefoldだけをBasic Oscillatorへ追加する場合、Basic Frequency上限は現在値を維持する。

ただしSound Reviewで高音域Aliasが明確な場合は、Wavefold有効時だけより低い上限を採用する。

上限はCore側の一関数へ置き、CLI Inspectにも表示する。

## 14.9 Finite Safety

各Nonlinear Stage後にSampleごとのFinite Branchを入れない。

Span処理後にBuffer全体を`ensure_finite`で検査する既存方針を維持する。

ただしFeedback計算に使用するPrevious Outputは更新前にFinite確認する。

---

# 15. CLI / Inspect

## 15.1 Command追加

新しいTop-level Commandは追加しない。

既存Commandを拡張する。

- `instrument validate`
- `instrument inspect`
- `render note`
- `render events`
- `render midi`

Wavetable Asset生成や編集Commandは本Phaseに含めない。

## 15.2 Wavetable Inspect

表示項目：

- Generator Kind
- Asset Path
- Asset SHA指定有無
- Prepared / Unavailable
- Source Channel Count
- Source Frame Count
- Frame Length
- Frame Count
- Band Count
- 各BandのMax Harmonic
- Initial Position
- Position Parameter ID
- Phase Reset
- Initial Phase
- Unison Voice Count
- Detune / Spread Parameter ID
- Output Mode
- Effective Max Frequency
- Compile Warning

`inspect --json`ではBandごとの全Sampleを出力しない。

Metadataだけを返す。

## 15.3 Operator Inspect

表示項目：

- Generator Kind
- Modulation Mode
- Algorithm名
- Evaluation Order
- Carrier Operator
- Operator 1〜4
  - Ratio
  - Detune
  - Level
  - Modulation Amount
  - Feedback
  - Initial Phase
  - Envelope
  - Parameter ID
- Phase Reset
- Unison
- Output Mode
- Effective Max Frequency

Compiled Topologyを表示し、Definition文字列を再解釈しない。

## 15.4 Complex Oscillator Inspect

既存Oscillator項目へ追加する。

- Backend `phase_domain`
- Phase Distortion Enabled / Amount Parameter
- Wavefold Enabled / Amount Parameter
- Oscillator Feedback Enabled / Amount Parameter
- DC Blocker Enabled
- Combination制約
- Effective Max Frequency
- Signal Order

## 15.5 Render Events

既存Parameter Change Eventだけで新Parameterを操作できる。

新Event Typeは追加しない。

例：

```json
{
  "absolute_frame": 24000,
  "type": "parameter_change",
  "parameter": "layer.motion.generator.wavetable_position",
  "normalized": 0.8
}
```

Operator Ratio等もNormalized値からDescriptor経由でNative値へ変換する。

## 15.6 CLI Error

- Asset不足はWarning
- Layout不正はExit Code 1
- Parameter ID不正は既存契約
- Runtime DSP失敗はExit Code 3
- WAV出力失敗はExit Code 4

既存Exit Codeを変更しない。

---

# 16. Diagnostics

## 16.1 既存Codeを再利用するもの

- `VALUE_OUT_OF_RANGE`
- `PARAMETER_ID_INVALID`
- `PARAMETER_NOT_FOUND`
- `ROUTE_TARGET_INVALID`
- `ASSET_NOT_FOUND`
- `ASSET_HASH_MISMATCH`
- `ASSET_DECODE_FAILED`
- `ASSET_DOWNMIXED`
- `ASSET_HASH_MISSING`
- `ASSET_ABSOLUTE_PATH`
- `DSP_ERROR`

## 16.2 追加Code

必要最小限として次を追加する。

| Code | Severity | 用途 |
|---|---|---|
| `WAVETABLE_LAYOUT_INVALID` | Error | Sample Count、Frame Length、Frame Countの不整合 |
| `WAVETABLE_PREPARATION_FAILED` | Error | FFT / IFFT後にFinite Tableを生成できない |
| `WAVETABLE_SILENT_FRAME` | Warning | 一部Frameが無音 |
| `WAVETABLE_DC_OFFSET` | Warning | 一部FrameのDCが大きい |
| `GENERATOR_COMBINATION_INVALID` | Error | Hard Sync + Phase Distortion等 |
| `GENERATOR_RESOURCE_LIMIT_EXCEEDED` | Error | Frame / Unison等の上限超過 |

Operator数やRange違反ごとに専用Codeを増やさない。

PathとMessageで特定する。

## 16.3 Message要件

Diagnosticは次を含む。

- 何が不正か
- どのFieldか
- 許容範囲
- Asset由来の場合はAsset Path
- Wavetable Frame由来の場合はFrame Index

曖昧な`invalid generator`だけを返さない。

---

# 17. Realtime Safety / Memory / Performance

## 17.1 Audio Thread禁止事項

- File I/O
- SHA計算
- WAV Decode
- Resample
- FFT / IFFT
- Table生成
- JSON
- String生成
- HashMap Lookup
- Vec容量拡張
- Blocking Lock
- Network
- Runtime Algorithm構築

## 17.2 Memory所有

| データ | 所有 |
|---|---|
| Prepared Wavetable | Compiled Instrument、`Arc`共有 |
| Wavetable Phase | Voice Runtime |
| Operator Topology | Compiled Instrument |
| Operator Phase / Feedback | Voice Runtime |
| Operator Envelope | Voice Runtime |
| Unison Distribution | Compiled Instrument、`Arc`共有 |
| Audio Scratch | Voice / Layer Runtime、Prepare時確保 |
| FFT Scratch | Compile処理中だけ |

## 17.3 Wavetable Memory目安

最悪条件を無制限に許可しない。

Frame Length 4096、Frame Count 256、全Bandを単純複製するとMemoryが大きくなる。

そのためCompile時にPrepared WavetableのByte数を計算する。

Hard Limit：一つのPrepared Wavetableにつき256 MiB未満。

超過時は`GENERATOR_RESOURCE_LIMIT_EXCEEDED`とする。

同一Assetの`Arc`共有後のByte数をLayerごとに重複加算しない。

## 17.4 Operator計算量

概算単位：

```text
polyphony × operator_unison × 4 operators × sample frames
```

本Phaseでは一般的CPU Budgetを導入しない。

ただしReview Packageで次を測定する。

- 1 Voice / Unison 1
- 8 Voice / Unison 1
- 16 Voice / Unison 1
- 8 Voice / Unison 4
- 16 Voice / Unison 4

CLI込みRender時間とPeak Working Setを記録する。

Runtime単体Realtime保証値とは表現しない。

## 17.5 Fast Path

次のFast Pathを持つ。

- Wavetable PositionがStaticならPosition計算をSpan内固定
- Unison 1ならStereo Mixを通さない
- Operator Parameter SpanがStaticならSampleごとのSpan補間式を省略可能
- Topology上存在しないLevel / Modulation / FeedbackはRuntime Branch自体を持たない
- Feedback 0ならFeedback計算を省略
- Phase Distortion 0ならIdentity Mapping
- Wavefold 0ならStage Skip

Fast Pathのために別Runtime実装を複製しない。

明確なBranchで通常処理を共有する。

## 17.6 Performanceを理由に削らない品質

- Wavetable Band Table
- Band Crossfade
- Frame Interpolation
- Operator Envelope
- Feedback Bound
- Finite検査

性能不足を理由にFull-band Wavetableや離散Positionへ戻さない。

---

# 18. 実装単位

## 18.1 実装単位A：Wavetable Generator

### 目的

Assetから帯域制限されたWavetableをCompileし、Position ModulationとUnisonを持つGeneratorとしてRenderできる状態にする。

### 作業順序

1. Definition追加
2. Validation追加
3. Asset Decode共通部の整理
4. RustFFT Dependency追加
5. Prepared Wavetable生成
6. Compiled Model追加
7. Parameter Catalog追加
8. Target Span追加
9. Wavetable Runtime追加
10. Generator Output Mode統合
11. Availability統合
12. CLI Inspect追加
13. Unit / Integration Test
14. Review PackageへWavetable音源追加
15. 人間の試聴

### 単位A完了条件

- Static Tableが正しいPitchで鳴る
- Position 0 / 0.5 / 1で異なるFrameを確認できる
- Position Sweepが滑らか
- Band境界でClickがない
- High Registerで明確なFull-band Aliasが抑制される
- Unison 1 / 5 / 8が動作
- Missing Assetで他Layerが継続
- Block Size / Reset / Sample Rate検査が成功
- Human Review承認

## 18.2 実装単位B：Operator Modulation Generator

### 目的

4 Operator固定TopologyでPM / FM / AM / Ringを区別して生成し、Operator Envelope、Feedback、Unisonを演奏可能な状態にする。

### 作業順序

1. Definition / Algorithm Enum追加
2. Algorithm Topology Table追加
3. Validation追加
4. Parameter Descriptor追加
5. Compiled Model追加
6. Generator `note_off` Lifecycle追加
7. Operator Envelope統合
8. PM Runtime
9. FM Runtime
10. AM Runtime
11. Ring Runtime
12. Feedback統合
13. Unison統合
14. CLI Inspect追加
15. Unit / Integration Test
16. Review PackageへOperator音源追加
17. 人間の試聴

PMを最初に完成させ、その構造を確認してからFM / AM / Ringを追加する。

ただしPMだけをMerge可能状態としてPhase完了扱いにしない。

### 単位B完了条件

- 8 AlgorithmがTopologyどおり動作
- 4 Modeの音響差を確認できる
- Operator EnvelopeがNote Offへ反応
- Ratio / Index Sweepが連続
- Feedbackが有限
- Carrier Normalizationが機能
- Unison 1 / 4が動作
- Voice StealingでStateが漏れない
- Allocation 0
- Human Review承認

## 18.3 実装単位C：Complex Oscillator Completion

### 目的

既存OscillatorへPhase Distortion、DaisySP Wavefolder、Feedbackを追加し、既存音声経路を維持しながら高度なDigital Oscillatorを完成させる。

### 作業順序

1. Definition / Validation追加
2. Parameter追加
3. Compiled Backend追加
4. Phase-domain Runtime追加
5. Feedback追加
6. Phase Distortion追加
7. DaisySP WavefolderのMIT Licenseと固定Commit内Sourceを再確認
8. Native CMakeへ`wavefolder.cpp`追加
9. C ABI / `DspWavefolder` / Fault Injection Test追加
10. Oscillator RuntimeへWavefolder統合
11. DC Blocker追加
12. Hard Sync / Waveshaping / Unisonとの組み合わせ検証
13. CLI Inspect追加
14. Existing Oscillator Regression
15. Review PackageへComplex音源追加
16. 人間の試聴

### 単位C完了条件

- Amount 0でIdentity
- Phase Distortion Sweepが連続
- Feedback Sweepが有限
- DaisySP Wavefolderを経由したWavefold Sweepが連続
- Wavefold + Hard Sync / Unisonが動作
- Amount 0でNative Wavefolder Stageが厳密にIdentity
- DaisySP-LGPLをLinkしていない
- 既存Oscillator Referenceが許容差内
- High RegisterのAliasを試聴承認
- Human Review承認

## 18.4 最終統合

三単位完了後に次を行う。

- Digital Hybrid Reference Instrument作成
- 全Workspace Test
- Windows / Linux CI
- Sanitizer / Native Fault Injection
- Full Review Package再生成
- Documentation更新
- Dead Code / 重複契約Review
- Human Review Summary確定

---

# 19. Test計画

## 19.1 Unit Test：Wavetable

- Frame Length 64 / 2048 / 4096
- 2の冪以外を拒否
- Sample Count割切れ検証
- Frame Count上限
- FFT / IFFTでSineを再構成
- Harmonic Cutで高次Binが除去される
- IFFT Scale
- Guard Sample配置
- Position 0 / 1境界
- Frame Count 1
- Cubic InterpolationのWrap
- Band選択
- Band Crossfade
- Phase Reset
- Unison Phase Distribution
- Missing Asset
- Silent Frame Warning
- DC Warning
- Asset Cache共有

## 19.2 Unit Test：Operator

- 8 AlgorithmのIncoming Mask
- Evaluation Order
- Carrier Mask
- Carrier Normalization
- Operator数4以外拒否
- Mode別Amount Range
- AM / Ring Feedback拒否
- Ratio / Detune周波数
- PM Amount 0でSine
- FM Amount 0でSine
- AM Depth 0でIdentity
- Ring Depth 0でIdentity
- Feedback 0でIdentity
- Feedback 1でFinite
- Envelope Attack / Decay / Sustain / Release
- Note Off
- Reset
- Negative Instantaneous FM Frequency
- Phase Wrap
- Unison Component独立性

## 19.3 Unit Test：Complex Oscillator

- Phase Distortion 0でIdentity
- Breakpoint最小値
- Mapping連続性
- Feedback 0でIdentity
- Feedback Bound
- Wavefold 0でIdentity
- Amount 0 / 0.5 / 1のDrive / Mix変換
- DaisySP Wavefolder Native WrapperのProcess / Ramp
- WavefolderのMono 1 Handle / Stereo左右独立Handle
- Native Wavefolderの既知入力に対するReference値
- Wavefolder Null Handle / 未Prepare / Range違反 / 非Finite
- Wavefolder Fault Injection時の無音化とError伝播
- Wavefolder Guard Buffer境界
- DC Blocker Reset
- Invalid Combination
- Phase-domain Frequency Clamp

## 19.4 Core Integration Test

- Wavetable単音Render
- Wavetable Position Parameter Change
- LFO → Wavetable Position
- Mod Wheel → Wavetable Position
- Wavetable Unison Stereo
- Operator PM Bell
- Operator FM Bass
- Operator AM
- Operator Ring
- Operator Ratio Change
- Operator Index Change
- Operator Feedback Change
- Operator Note Off
- Operator Voice Stealing
- Phase Distortion Lead
- Feedback Drone
- Wavefold + Hard Sync
- Wavefold + Unison
- Wavetable + Operator + Sample Hybrid
- Missing Wavetable Asset + Existing Oscillator Layer

## 19.5 Block Size

次で同じEvent SequenceをRenderする。

- 32
- 64
- 257
- 1024

比較対象：

- Event位置
- LFO / Parameter Sweep時間軸
- Wavetable Position
- Operator Envelope
- Feedback State
- Output Finite性
- 波形差分

## 19.6 Sample Rate

- 44,100 Hz
- 48,000 Hz
- 96,000 Hz

各Sample Rateで次を確認する。

- Pitch
- Wavetable Band選択
- Operator Frequency Clamp
- DC Blocker係数
- Envelope時間
- Finite / Non-silent

## 19.7 Reset / Fresh Runtime

同じCompiled Instrumentから、次を比較する。

1. Fresh Runtime AのRender
2. AをReset後のRender
3. Fresh Runtime BのRender

決定的機能は許容差内で一致する。

比較名を「Fresh Runtime」と「Lifecycle Reset」で明確に区別する。

## 19.8 Voice Stealing

- Wavetable Phaseが新VoiceへResetされる
- Wavetable Position Targetが新Noteへ適用される
- Operator Envelopeが新Voiceで再開始
- Previous Outputが0へ戻る
- Phase Distortion Feedbackが新Voiceへ漏れない
- Pending NoteでAllocationしない

## 19.9 CLI Integration Test

- Validate成功 / 失敗
- Wavetable Asset Warning
- Wavetable Layout Error
- Operator Inspect JSON
- Complex Oscillator Inspect JSON
- Parameter Catalog表示
- Events Render
- MIDI Render
- Missing Asset時の継続
- Exit Code

## 19.10 Existing Regression

最低限、既存Review PackageのDefinitionを再Renderする。

- Basic Poly Synth
- Metallic Hybrid
- Dynamic Parameters
- Processor Chain
- Basic Generator
- Complex Oscillator
- Essential Synthesis / Sampling

新機能未使用のReferenceで意図しない音声差分がないことを確認する。

---

# 20. Sound Review

## 20.1 Review Package

保存先：

```text
review-output/digital-synthesis/
├─ assets/
├─ audio/
│  └─ technical/
├─ definitions/
├─ events/
├─ midi/
├─ inspect.json
├─ digital-hybrid-inspect.json
├─ complex-inspect.json
├─ complex-phase-inspect.json
├─ metrics.json
└─ review-summary.md
```

生成Script：

```text
scripts/review/generate_digital_synthesis_package.py
```

既存`scripts/review/common.py`を使用する。

共通処理を新Scriptへコピーしない。

## 20.2 Wavetable Review Audio

1. Sine Single Frame
2. Saw Single Frame Low Note
3. Saw Single Frame High Note
4. Multi-frame Position 0
5. Multi-frame Position 0.5
6. Multi-frame Position 1
7. Position Sweep
8. Position LFO
9. Unison 5 Stereo
10. Band Boundary Sweep

## 20.3 Operator Review Audio

11. PM Stack 4 Stress
12. FM Stack 4 Stress
13. AM Two-Operator Comparison
14. Ring Two-Operator Comparison
15. Algorithm Stack 4
16. Algorithm Two Stacks
17. Algorithm Shared Modulator
18. Ratio Sweep
19. Modulation Amount Sweep
20. Feedback Sweep
21. Operator Envelope Bell
22. Operator Unison 4 on a Two-Operator Patch
23. Operator Polyphony / Stealing on a Two-Operator Patch

## 20.4 Complex Oscillator Review Audio

24. Phase Distortion 0.25
25. Phase Distortion 0.75
26. Phase Distortion Sweep
27. Feedback 0.3
28. Feedback 0.8
29. Feedback Sweep
30. DaisySP Wavefolder Amount 0.25
31. DaisySP Wavefolder Amount 0.75
32. DaisySP Wavefolder Sweep
33. Waveshaping + Wavefold
34. Hard Sync + Wavefold
35. Unison + Wavefold

## 20.5 Musical Reference

36. Wavetable Motion Bass
37. Four Operator FM Bell
38. Phase Distortion Lead
39. Digital Hybrid Lead
40. Digital Hybrid Phrase

## 20.6 Regression Audio

- Block Size 32 / 64 / 257 / 1024
- Sample Rate 44.1 / 48 / 96 kHz
- Fresh Runtime A / B
- Reset Render
- High Polyphony / Unison Performance

## 20.7 Metrics

自動計測：

- Finite
- Peak
- RMS
- DC
- 推定Fundamental
- 単音RenderのFundamentalはMIDI Noteから算出し、複数音RenderではZero Crossing値を補助値として保持する
- 基準周波数が成立する単音RenderのFixed Length Spectrum
- 基準周波数が成立する単音RenderのSpectral Centroid
- 基準周波数が成立する単音RenderのHarmonic / Non-harmonic Energy参考値
- Stereo Difference
- Stereo Correlation
- Adjacent Frame最大差分
- Position Sweep境界差分
- Band Boundary差分
- Block Size差分
- Fresh Runtime差分
- Reset差分
- Parameter Sweep前後差分
- CLI Render時間
- Peak Working Set
- Prepared Wavetable Byte数

Spectrum値だけで音質合格としない。

初回承認後はBaselineとして保存し、以降の変更で大幅に悪化した場合は再試聴する。

## 20.8 人間の試聴

### Wavetable

- Frameごとの音色差
- Position Sweepの滑らかさ
- Band切替の不連続
- 高音域Alias
- 低音域の倍音保持
- UnisonのBeat
- Stereo幅
- Mono再生時のLevel

### Operator

- PMとFMの差
- AMとRingの差
- Algorithmの差
- Ratioによる倍音変化
- Envelopeによる時間変化
- Feedbackの粗さと安定性
- Index SweepのClick
- Note Release
- Polyphony

### Complex Oscillator

- Phase Distortionの音色範囲
- Feedbackの倍音とNoise化
- DaisySP WavefolderによるFold感
- Amount 0から立ち上がる変化の自然さ
- Waveshapingとの役割差
- Hard Syncとの組み合わせ
- 高音域Alias
- DC / Low Frequency偏り

### Musical

- Bass、Bell、Leadとして使えるか
- Sample Layerとの一体感
- Filter / Driveへ渡した後も音色が成立するか
- 音量を過度に下げないと使えない状態ではないか
- 技術Demoだけで終わっていないか

## 20.9 Review Summary

各音源について次を記録する。

- 確認日
- 再生環境
- 確認対象Commit
- 承認 / 要修正
- 指摘内容
- 修正後の再確認結果

音声が変わった場合、以前の承認をそのまま残さない。

---

# 21. Documentation更新

## 21.1 更新対象

- `README.md`
- `docs/architecture.md`
- `docs/runtime-processing.md`
- `docs/cli.md`
- `docs/instrument-definition.md`
- `docs/creating-an-instrument.md`
- `docs/testing-and-sound-review.md`
- `.agents/skills/create-instrument/SKILL.md`

`docs/CONCEPT.md`は正本の意味を変える必要がある場合だけ更新する。

本Phaseで既に記載済みの機能名を、実装状況を理由に削除しない。

## 21.2 Instrument Definition Document

追加内容：

- Wavetable JSON
- Asset Layout
- Operator JSON
- 8 Algorithm図
- Mode別意味
- Oscillator新Field
- Parameter ID
- Validation Error例

## 21.3 Creating Instrument Guide

作例：

- Wavetable Pad
- FM Bell
- Ring Mod Texture
- Phase Distortion Lead
- Digital Hybrid Instrument

単にField一覧を再掲せず、音作り上の役割を説明する。

## 21.4 Agent Skill

AIがDefinitionを生成する際の規則を追加する。

- Wavetable Frame LengthをAssetと一致させる
- Operatorは4件
- Algorithmで未使用のModulation Amountを0にする
- Carrier Levelを最低一つ非0にする
- AM / Ring Feedbackを0にする
- Hard SyncとPhase Distortionを併用しない
- Parameter IDをInspectから取得する
- RenderとSound Reviewを行う

---

# 22. 変更対象File

想定する主な変更：

```text
crates/sonalloy-core/Cargo.toml
crates/sonalloy-core/src/lib.rs
crates/sonalloy-core/src/asset.rs
crates/sonalloy-core/src/definition.rs
crates/sonalloy-core/src/diagnostics.rs
crates/sonalloy-core/src/generator_parameters.rs
crates/sonalloy-core/src/parameter.rs
crates/sonalloy-core/src/compiler.rs
crates/sonalloy-core/src/process.rs
crates/sonalloy-core/src/runtime.rs
crates/sonalloy-core/src/runtime/generator/mod.rs
crates/sonalloy-core/src/runtime/generator/oscillator.rs
crates/sonalloy-core/src/runtime/generator/wavetable.rs
crates/sonalloy-core/src/runtime/generator/operator.rs
crates/sonalloy-core/src/runtime/modulation.rs
crates/sonalloy-core/src/runtime/voice.rs
crates/sonalloy-core/tests/core_process.rs
crates/sonalloy-dsp-sys/src/lib.rs
crates/sonalloy-dsp-sys/src/ffi.rs
crates/sonalloy-dsp-sys/src/wavefolder.rs
crates/sonalloy-dsp-sys/tests/*
native/daisysp-wrapper/CMakeLists.txt
native/daisysp-wrapper/include/sonalloy_dsp.h
native/daisysp-wrapper/src/daisysp_wrapper.cpp
crates/sonalloy-cli/src/main.rs
crates/sonalloy-cli/tests/cli.rs
docs/architecture.md
docs/cli.md
docs/creating-an-instrument.md
docs/instrument-definition.md
docs/runtime-processing.md
docs/testing-and-sound-review.md
docs/plan/plan-digital-synthesis-expansion.md
examples/instruments/*
review-output/digital-synthesis/*
scripts/review/generate_digital_synthesis_package.py
.agents/skills/create-instrument/SKILL.md
```

Wavetable準備の実装量が`asset.rs`を不自然に肥大化させる場合だけ、次を追加してよい。

```text
crates/sonalloy-core/src/wavetable.rs
```

既存`asset.rs`をDirectory Moduleへ全面移行する作業は本Phaseで行わない。

---

# 23. 実装時の禁止事項

- 新Generator用の別Parameter Systemを作る
- RuntimeでParameter ID文字列を検索する
- Wavetable PositionをFrame Index整数だけで扱う
- Band Tableを作らない
- Asset Sample RateへWavetable Pitchを依存させる
- Operatorを任意Graphにする
- FMをPMとして実装しながら同じものとして説明する
- Operator EnvelopeをLayer Envelopeで代用する
- FeedbackをCurrent Sample Cycleにする
- Feedback発散時にSampleを0へ置換して継続する
- Invalid Combinationを黙って片方無効化する
- Existing Oscillatorを新Engineへ全面置換する
- DaisySP-LGPLの`Fold`をWavefolderとして採用する
- `USE_DAISYSP_LGPL`を有効化する
- DaisySPの`Wavefolder`型や`gain` / `offset`をDefinitionへ露出する
- 新Fieldなしの既存音色を変更する
- Test専用FieldをProduction Runtimeへ追加する
- Metricsを手入力する
- Human Review前に完了扱いにする
- Legacy Alias、Migration、Deprecated Fieldを追加する
- 将来用の抽象化だけを目的にCrateやTraitを追加する

---

# 24. 完了条件

## 24.1 Build / Static Check

- `cargo build --workspace`
- `cargo build --workspace --release`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

すべて成功する。

## 24.2 Wavetable

- Definition / Validation / Compile / Runtime / Inspectが完成
- Band-limited Tableが生成される
- Position Modulationが動作
- Unisonが動作
- Missing Assetが部分読込される
- Block Size / Sample Rate / Reset / Voice Stealingが成功
- Allocation 0
- Human Review承認

## 24.3 Operator Modulation

- 4 Operator固定
- 8 Algorithm
- PM / FM / AM / Ring
- Operator Envelope
- Feedback
- Parameter / Modulation
- Unison
- Note Off / Reset / Voice Stealing
- Allocation 0
- Human Review承認

## 24.4 Complex Oscillator

- Phase Distortion
- DaisySP `Wavefolder`によるWavefold
- `sonalloy-dsp-sys` / Native C ABIのWavefolder Wrapper
- DaisySP本体MIT版のみをLinkし、DaisySP-LGPLを導入しない
- Oscillator Feedback
- DC Blocker
- Existing Waveshaping / Hard Sync / Unisonとの許可された組み合わせ
- Invalid Combination診断
- Existing Oscillator回帰なし
- Human Review承認

## 24.5 Regression

- 既存Reference InstrumentがCompile可能
- 既存Review Packageの主要音源がFinite
- 新機能未使用時に意図しない音声差分がない
- Windows / Linux CI成功
- Sanitizer成功
- Wavefolderを含むNative Fault Injection成功
- Native Guard Buffer Test成功
- DaisySP-LGPLがBuild / Link対象に含まれていない

## 24.6 Document

- Planと実装が一致
- Definition Document更新
- Runtime Document更新
- CLI Document更新
- Testing / Review Document更新
- Agent Skill更新
- Review Summary更新

## 24.7 最終成果

最終的に次を満たす。

> Sonalloyが、Sample、基本Oscillator、Noiseに加えて、帯域制限されたWavetable、固定Topologyの4 Operator Modulation、Phase Distortion、Wavefold、Oscillator Feedbackを一つのInstrument内で組み合わせ、ParameterとModulationで演奏中に変化させ、再現可能なStereo Audioとして生成できる。

---

# 25. 次Phaseへ残す境界

本Phase完了後も次は未実装である。

- Granular
- Time Stretch
- Reverse / Release Sample / Loop Crossfade
- Wave Sequence
- Additive
- Spectral / Resynthesis
- Modal / Waveguide
- Formant
- Advanced Modulation / Performance
- Processor Expansion
- Realtime / Plugin Integration

次Phaseでは音声素材を時間方向に再構成するAdvanced Sampling / Granularを扱う。

本PhaseでそのためのScheduler、Grain Pool、Time Stretch Backend、Tempo Sync構造を先回りして追加しない。
