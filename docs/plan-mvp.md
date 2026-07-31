# Sonalloy Core MVP（P0〜P2）詳細設計・実装計画

- **版**：4.0
- **対象**：Sonalloyの最初のMVP実装
- **対象フェーズ**：P0〜P2
- **用途**：実装を担当するAIエージェントへの指示、実装レビュー、フェーズ完了判定
- **前提文書**：`Sonalloy 要件定義・基本設計`
- **正本**：本Markdownを正本とし、HTML版は本文を一切省略せず同じ内容から生成する
- **文書言語**：日本語。型名、API名、コマンド名、ファイル名だけ英語を使用する

---

## 0. この計画書の位置づけ

本書は、Sonalloy全体の要件を置き換える文書ではない。  
製品全体の機能・責務・将来像は「Sonalloy 要件定義・基本設計」を正本とし、本書はそのうち最初に実装するP0〜P2を、実装可能な粒度まで具体化する。

本書で固定するものは次のとおりである。

- MVPで実装する機能と、MVP後へ延期する機能
- Rust CoreとDaisySPの責務境界
- Instrument Definition、Compiled Instrument、Runtime Instanceの構造
- Process Contract、Event順序、Block内処理
- Voiceの状態、割り当て、終了、Voice Stealing
- Oscillator、Envelope、Filter、Sample再生のMVP実装
- Asset解決、Decode、Sample Rate変換、欠落時の挙動
- P0、P1、P2の作業順、成果物、テスト、完了条件
- AIエージェントが用意する試聴用音源と、人間による音質承認
- 実装と同時に整備するドキュメント

本書で先回りして固定しないものは次のとおりである。

- P3以降のRealtime Device実装
- Riffra、JUCE、CLAP、VST3の具体的なAdapter API
- Wavetable、FM、GranularなどMVP外Generatorの内部設計
- 自由なAudio Graph
- 長期的なPreset配布形式
- 本格的なCPU性能予算
- 将来機能だけを目的とした汎用Framework

### AIエージェントへの基本指示

実装エージェントは、各フェーズの「完了条件」を満たすことを目的にする。  
記載された名詞をすべてClassやTraitへ変換することを目的にしてはならない。

判断に迷った場合は、次の順序で優先する。

1. 元の要件定義
2. 本書の責務分離と不変条件
3. 現在のフェーズの完成状態
4. 実装の単純さ
5. 将来拡張

「将来使うかもしれない」という理由だけで、現在使わない抽象化、Crate、依存、設定項目を追加しない。

---

# 1. MVPの目的と完成像

## 1.1 MVPで証明すること

Sonalloyは、SamplerとSynthesizerを別々に提供する製品ではない。  
異なる音生成方式をLayerとして一つのVoice内で組み合わせ、一つのInstrumentとして保存・再構築・演奏できることが中心価値である。

P0〜P2では次を順番に証明する。

| フェーズ | 証明すること | その段階だけでは不足するもの |
|---|---|---|
| **P0：音声処理基盤** | RustからDaisySPを安全に利用し、共通Process経路から決定論的な音声を生成できる | Instrument、Voice、Layerはまだない |
| **P1：演奏可能シンセ** | JSON DefinitionからPolyphonic Synthを構築し、Note / MIDIからWAVを生成できる | 一般的なSynthの範囲で、Sonalloy固有価値はまだない |
| **P2：Hybrid Instrument** | Oscillator LayerとSample Layerを同じVoice内で融合し、一つの音色として成立させられる | Realtime DeviceやPlugin配布はまだない |

MVPの完成状態は次である。

> JSONで保存したInstrument DefinitionをCompileし、Oscillator LayerとSample Layerを一つのVoice内で発音・混合し、同じMIDI入力から同等のHybrid Instrument音声をWAVとして再現できる。

## 1.2 MVPの代表成果物

MVP完了時には、少なくとも次の二つのReference InstrumentをRepositoryへ含める。

### Basic Poly Synth

P1の成果物。  
Oscillator、Envelope、Voice、Filterの品質と演奏処理を確認する。

```text
Saw Oscillator
  → Layer ADSR
    → Layer Gain / Pan
      → Voice Low-pass Filter
        → Voice Sum
```

### Metallic Hybrid

P2の成果物。  
Sampleをアタック成分、Oscillatorを音の芯と余韻として利用する。

```text
Attack Layer
  Metal Hit Sample
  Short ADSR
  Velocity → Layer Gain

Body Layer
  SineまたはSaw Oscillator
  Medium ADSR

Layer Mix
  → Voice Low-pass Filter
    → Voice Sum
```

Metallic Hybridは技術Demoではなく、人間が「曲で使ってみたいか」を判断できる品質まで調整する。

---

# 2. MVPスコープ

このスコープは、実装計画を詳しくする過程で広げてはならない。

## 2.1 MVPに含める機能

### 共通基盤

- Windows / Linux
- Headless
- Rust Core
- DaisySPを利用する内部DSP Backend
- JSON形式のInstrument Definition
- Instrument Definition / Compiled Instrument / Runtime Instanceの分離
- Offline Render
- Stereo WAV出力
- MIDI Fileから共通Eventへの変換
- Sample Offset付きNote On / Note Off
- Polyphonic Voice
- Voice Stealing
- 同じ入力条件から同等のRender結果を得る再現性
- 構造化Diagnostics

### P1：Oscillator Instrument

- 一つのOscillator Layer
- Sine
- Saw
- ADSR
- Voice単位のLow-pass Filter
- Gain
- Pan
- Tuning
- VelocityによるLayer Gainへの反映
- VelocityによるVoice Filter Cutoffへの反映
- 連続値変更時の最低限のSmoothing
- CLIによるValidation、内容表示、単音Render、MIDI Render

### P2：Hybrid Instrument

- 複数Layer
- Oscillator Layer
- Sample Layer
- 一つのSample Layerにつき一つのSample Asset
- WAV Asset
- Root Note
- One-shot再生
- Sampleの音程展開
- LayerごとのKey Range
- LayerごとのVelocity Range
- LayerごとのGain / Pan / Tuning / ADSR
- Layer Mix
- Voice全体のLow-pass Filter
- Missing Assetの部分読込
- Oscillator + SampleのReference Hybrid Instrument

## 2.2 MVPに含めない機能

次は製品要件から削除するのではなく、MVP後へ延期する。

- Noise Generator
- Square / Triangleの正式対応
- 複数Sample Zone
- Round Robin
- Sample Loop
- 高度なMulti Sample Mapping
- 汎用Modulation Matrix
- LFO
- Random
- Mod Wheel
- Aftertouch
- Pitch Bend
- Layerごとの任意Processing Chain
- Drive
- EQ
- Chorus
- Delay
- Reverb
- Global Effects
- Wavetable
- FM
- Granular
- Realtime Audio Device
- Realtime MIDI Device
- JUCE
- Riffra統合
- CLAP
- VST3
- GUI
- 自由なAudio Graph
- Feedback Routing
- 本格的な性能測定と性能最適化

## 2.3 MVPで求める品質

品質を重視するとは、測定Frameworkや管理機能を大量に作ることではない。  
MVPで優先する品質は次のとおりである。

1. 音声Bufferを壊さない
2. NaN / Infinityを出さない
3. Note On / Note Offで明確なクリックを出さない
4. EnvelopeとFilterの変化が滑らかである
5. Voice Stealingが演奏を明確に壊さない
6. Sawの高音域が明確に耳障りでない
7. Sample終端に明確なクリックがない
8. SampleのPitch変換が確認範囲内で許容できる
9. OscillatorとSampleが別々の音ではなく一つの音色に聞こえる
10. 人間が試聴し、明示的に承認する

機械検査は、壊れた出力や明確な不具合を検出するために使う。  
音の魅力、自然さ、耳障りさ、Layerの一体感は人間が判断する。

---

# 3. 技術選定と責務境界

## 3.1 使用する技術

| 領域 | 選定 | MVPでの用途 |
|---|---|---|
| Core | Rust | Definition、Compiler、Voice、Layer、Runtime、Asset準備、Offline Render |
| DSP Primitive | DaisySP | OscillatorとFilter |
| Serialization | Serde / serde_json | Definitionの読込・保存 |
| Audio Decode | Symphonia | WAVのDecode |
| Sample Rate変換 | Rubato | Compile時にAssetをEngine Sample Rateへ変換 |
| MIDI File | midly | MIDI Fileを共通Eventへ変換 |
| WAV出力 | hound | Offline Render結果の保存 |
| Hash | sha2 | AssetのSHA-256 |
| CLI | clap | Commandと引数 |
| Native Build | CMake + build.rs | DaisySP WrapperのBuildとLink |

Versionは実装開始時に固定し、Lock FileまたはCommitで再現可能にする。

## 3.2 Rustが所有する責務

Rust側が所有する。

- Instrument Definition
- Validation
- Compile Pipeline
- Asset Path解決
- Asset Hash確認
- Audio Decode
- Sample Rate変換
- Prepared Sample
- Process Contract
- Event順序
- Voice Pool
- NoteとVoiceの対応
- Layer Trigger
- ADSR
- Gain
- Pan
- Tuning
- Velocity Response
- Sample Playback Cursor
- Sample補間
- Layer Mix
- Offline Renderer
- Diagnostics
- CLIのDomain API

ADSRは、VoiceとLayerの状態遷移に密接であり、Unit TestしやすくするためRustで実装する。

## 3.3 DaisySPへ委譲する責務

MVPでは、DaisySPへ次を委譲する。

- Sine Oscillator
- Saw Oscillator
- Low-pass Filter

DaisySPはSonalloyの仕様ではなく実装部品である。  
DaisySPのClass名、Enum、Parameter名をDefinitionやPublicなRust Modelへ露出させない。

## 3.4 JUCEの扱い

JUCEは、将来のRiffra、Standalone、VST3 Adapterで利用する方針を維持する。

P0〜P2で依存に追加しない理由：

- Offline Core MVPにAudio DeviceやPlugin APIは不要
- CoreとAdapterの境界を先に安定させる必要がある
- Buildと依存を増やさず、音源本体に集中する
- JUCE固有のLifecycleや型をCoreへ漏らさない

P0〜P2でJUCE型をDefinition、Compiled Instrument、Runtimeへ持ち込んではならない。

---

# 4. Repository・Module構成

## 4.1 Repository構成

```text
sonalloy/
├─ Cargo.toml
├─ Cargo.lock
├─ CMakeLists.txt
├─ README.md
├─ THIRD_PARTY_NOTICES.md
│
├─ crates/
│  ├─ sonalloy-core/
│  │  ├─ Cargo.toml
│  │  ├─ src/
│  │  │  ├─ lib.rs
│  │  │  ├─ definition/
│  │  │  ├─ compiler/
│  │  │  ├─ diagnostics/
│  │  │  ├─ asset/
│  │  │  ├─ process/
│  │  │  ├─ runtime/
│  │  │  └─ render/
│  │  └─ tests/
│  │     ├─ core_mvp.rs
│  │     └─ support/
│  │
│  ├─ sonalloy-dsp-sys/
│  │  ├─ Cargo.toml
│  │  ├─ build.rs
│  │  ├─ src/
│  │  └─ tests/
│  │     └─ ffi.rs
│  │
│  └─ sonalloy-cli/
│     ├─ Cargo.toml
│     ├─ src/
│     └─ tests/
│        └─ cli.rs
│
├─ native/
│  └─ daisysp-wrapper/
│     ├─ CMakeLists.txt
│     ├─ include/
│     └─ src/
│
├─ docs/
│  ├─ architecture.md
│  ├─ instrument-definition.md
│  ├─ runtime-processing.md
│  ├─ cli.md
│  └─ testing-and-sound-review.md
│
├─ examples/
│  └─ instruments/
│     ├─ basic-poly-synth.json
│     └─ metallic-hybrid.json
│
├─ testdata/
│  ├─ definitions/
│  ├─ assets/
│  │  └─ README.md
│  ├─ midi/
│  └─ expected/
│
└─ review-output/
```

## 4.2 Crateを三つに留める理由

### `sonalloy-core`

製品の中心となるDefinition、Compiler、Runtimeを同じCrateに置く。  
Definition、Compiler、Runtimeを別Crateへ分割すると、MVPでは型変換と依存管理だけが増えるため分けない。

### `sonalloy-dsp-sys`

C++ / DaisySP境界だけを隔離する。  
Unsafe FFIとNative BuildをCoreから切り離す。

### `sonalloy-cli`

File、Terminal、MIDI File、WAV出力など、利用環境側の責務をCoreから切り離す。

## 4.3 Moduleの責務

| Module | 責務 | 持ち込んではならないもの |
|---|---|---|
| `definition` | JSONへ保存するModel、Schema Version | Runtime状態、DaisySP型 |
| `compiler` | Validation、Asset準備、Compiled Instrument生成 | Audio Block処理 |
| `diagnostics` | Error / Warningの構造 | CLI表示装飾 |
| `asset` | Path、Hash、Decode、Resample、Prepared Sample | RuntimeのFile I/O |
| `process` | ProcessSpec、Context、Event、Buffer Contract | MIDI File Parser型 |
| `runtime` | Voice、Layer、ADSR、Generator、Filter、Mix | JSON、File Path |
| `render` | Processを繰り返すOffline Render | WAV WriterのCLI設定 |

## 4.4 テスト配置

Rustの一般的な配置に従う。

### Unit Test

対象コードと同じModuleへ置く。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_voice_is_selected_before_active_voice() {
        // ...
    }
}
```

Private状態や内部関数を確認するテストは同じModuleに置く。  
すべてのUnit Testを`tests/`へ移動してはならない。

### 結合テスト

CrateのPublic APIを利用するテストだけを、各Crate直下の`tests/`へ置く。

- `sonalloy-core/tests/core_mvp.rs`
- `sonalloy-dsp-sys/tests/ffi.rs`
- `sonalloy-cli/tests/cli.rs`

Test Fileを細かく増やしすぎず、一つのIntegration Test TargetからSupport Moduleを呼ぶ。

### Test Data

JSON、WAV、MIDIなど複数テストで共有するデータは`testdata/`へ置く。

Workspace RootにCargo Integration Test用の`tests/`は作らない。

---

# 5. Instrumentの三層モデル

## 5.1 Instrument Definition

Definitionは編集・保存・差分管理する正本である。  
Audio処理から直接利用しない。

### P2完了時の概念Model

```rust
pub struct InstrumentDefinition {
    pub schema_version: u32,
    pub metadata: InstrumentMetadata,
    pub performance: PerformanceDefinition,
    pub layers: Vec<LayerDefinition>,
    pub voice_filter: Option<FilterDefinition>,
    pub velocity_response: VelocityResponseDefinition,
}

pub struct InstrumentMetadata {
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
}

pub struct PerformanceDefinition {
    pub polyphony: u16,
    pub voice_stealing: VoiceStealingDefinition,
}

pub struct LayerDefinition {
    pub id: LayerId,
    pub enabled: bool,
    pub trigger: LayerTriggerDefinition,
    pub gain_db: f32,
    pub pan: f32,
    pub tuning_cents: f32,
    pub envelope: AdsrDefinition,
    pub generator: GeneratorDefinition,
}

pub struct LayerTriggerDefinition {
    pub key_min: u8,
    pub key_max: u8,
    pub velocity_min: u8,
    pub velocity_max: u8,
}

pub enum GeneratorDefinition {
    Oscillator(OscillatorDefinition),
    Sample(SampleDefinition),
}

pub struct OscillatorDefinition {
    pub waveform: OscillatorWaveform,
    pub phase_reset: bool,
}

pub struct SampleDefinition {
    pub asset: AssetReference,
    pub root_note: u8,
    pub playback_mode: SamplePlaybackMode,
    pub interpolation: SampleInterpolation,
}

pub struct AssetReference {
    pub path: String,
    pub sha256: Option<String>,
}

pub struct AdsrDefinition {
    pub attack_seconds: f32,
    pub decay_seconds: f32,
    pub sustain_level: f32,
    pub release_seconds: f32,
}

pub struct FilterDefinition {
    pub cutoff_hz: f32,
    pub resonance: f32,
}

pub struct VelocityResponseDefinition {
    pub layer_gain_amount: f32,
    pub filter_cutoff_octaves: f32,
}
```

名称は実装に合わせて調整してよいが、責務を変えてはならない。

## 5.2 Definitionの値と単位

| 項目 | 単位 | MVPの許容範囲 | 備考 |
|---|---|---:|---|
| `polyphony` | Voice数 | 1〜64 | Referenceは16 |
| `gain_db` | dB | -60〜+12 | Layer単位 |
| `pan` | 正規化 | -1〜1 | -1左、0中央、1右 |
| `tuning_cents` | cent | -1200〜+1200 | Layer共通Tuning |
| `key_min/max` | MIDI Note | 0〜127 | min ≤ max |
| `velocity_min/max` | MIDI Velocity | 1〜127 | min ≤ max |
| `attack_seconds` | 秒 | 0〜30 | 0を許可 |
| `decay_seconds` | 秒 | 0〜30 | 0を許可 |
| `sustain_level` | 正規化 | 0〜1 | 振幅 |
| `release_seconds` | 秒 | 0〜30 | 0を許可 |
| `cutoff_hz` | Hz | 20〜20000 | Compile時にSample Rate上限を適用 |
| `resonance` | 正規化 | 0〜1 | DaisySP Adapterで変換 |
| `root_note` | MIDI Note | 0〜127 | Sample原音の基準音 |
| `layer_gain_amount` | 正規化 | 0〜1 | Velocity→Gainの反映量 |
| `filter_cutoff_octaves` | octave | 0〜4 | 低VelocityでCutoffを下げる量 |

## 5.3 Velocity Response

汎用Modulation MatrixはMVPで作らない。  
Velocityに対する反応だけを明示的な構造として実装する。

### Layer Gain

```text
v = velocity / 127
velocity_gain = lerp(1.0, v, layer_gain_amount)
final_layer_gain = db_to_linear(gain_db) × velocity_gain
```

`layer_gain_amount = 0`ではVelocityの影響なし。  
`layer_gain_amount = 1`ではVelocityが振幅へ線形に反映される。

### Voice Filter Cutoff

```text
v = velocity / 127
octave_offset = filter_cutoff_octaves × (v - 1)
cutoff = base_cutoff × 2 ^ octave_offset
```

最大VelocityではDefinitionのCutoffを使用し、弱いVelocityではCutoffを下げる。

## 5.4 Definitionへ保存しないもの

- Decode済みSample
- Sample Rate変換済みBuffer
- ParameterのRuntime Index
- Oscillator Phase
- ADSRの現在Segment
- Voice Active状態
- Sample Playback Cursor
- Filter State
- Scratch Buffer
- File Handle
- DaisySP Handle

---

## 5.5 Compiled Instrument

Compiled Instrumentは、Definitionを実行可能な形へ変換した不変構造である。

```rust
pub struct CompiledInstrument {
    pub metadata: CompiledMetadata,
    pub performance: CompiledPerformance,
    pub layers: Box<[CompiledLayer]>,
    pub voice_filter: Option<CompiledFilter>,
    pub velocity_response: CompiledVelocityResponse,
    pub diagnostics: Box<[Diagnostic]>,
}

pub struct CompiledLayer {
    pub id: LayerId,
    pub trigger: CompiledLayerTrigger,
    pub gain_linear: f32,
    pub pan: f32,
    pub tuning_ratio: f32,
    pub envelope: CompiledAdsr,
    pub generator: CompiledGenerator,
}

pub enum CompiledGenerator {
    Oscillator(CompiledOscillator),
    Sample(CompiledSample),
}

pub struct CompiledSample {
    pub source: PreparedSample,
    pub root_note: u8,
    pub interpolation: SampleInterpolation,
    pub enabled: bool,
}
```

Compiled Instrumentが保持するもの：

- Validation済みの値
- dBからLinearへ変換済みのGain
- centからRatioへ変換済みのTuning
- Trigger判定に必要な範囲
- Decode済み・Sample Rate変換済みのPrepared Sample
- Missing Assetにより無効化されたLayer情報
- Voice生成に必要な構成

Compiled Instrumentが保持しないもの：

- VoiceごとのDaisySP Handle
- Oscillator Phase
- Envelope状態
- Sample Cursor
- Filter内部状態
- Audio Scratch Buffer

## 5.6 Instrument Runtime Instance

Runtime Instanceは、Compiled Instrumentから生成される演奏中状態である。

```rust
pub struct InstrumentRuntime {
    compiled: Arc<CompiledInstrument>,
    voices: Vec<VoiceRuntime>,
    scratch: RuntimeScratch,
    process_spec: ProcessSpec,
    absolute_frame: u64,
}
```

`voices`と`scratch`は`prepare()`時に必要容量を確保する。  
`process()`中に容量拡張しない。

---

# 6. Compile Pipeline

## 6.1 Compileの入力と出力

```text
Instrument Definition
+ Definition Fileの基準Directory
+ Process Spec
        │
        ▼
Load / Validate / Resolve / Decode / Resample / Prepare
        │
        ├─ Error → Compile失敗
        └─ Warning → 一部機能を無効化して継続
        ▼
Compiled Instrument
```

## 6.2 Compileの処理順

1. Schema Versionを確認
2. JSON構造をDefinitionへDeserialize
3. ID、Range、必須項目をValidation
4. Layer TriggerをValidation
5. Generator DefinitionをValidation
6. FilterをProcess Sample Rateに対してValidation
7. Asset PathをDefinition FileのDirectoryから解決
8. Asset存在とHashを確認
9. WAVをDecode
10. Source ChannelをMVP内部形式へ変換
11. RubatoでEngine Sample Rateへ変換
12. Prepared Sampleを生成
13. dB、cent、Velocity Responseを実行値へ変換
14. Compiled Layerを生成
15. Diagnosticsをまとめる
16. ErrorがなければCompiled Instrumentを返す

## 6.3 ErrorとWarning

### Compile Error

Instrument全体を実行できない状態。

- 未対応Schema Version
- JSON構造不正
- Layer ID重複
- Layerが0件
- Key / Velocity Range不正
- Parameterが範囲外
- 未対応Generator
- Polyphony不正
- Oscillator LayerがP1制約に違反
- Process Sample Rate不正

### Compile Warning

Instrumentは実行可能だが、一部が無効または補正される状態。

- Sample Assetが見つからない
- Asset Hashが一致しない
- Stereo SampleをMonoへDownmix
- CutoffがSample Rate上限を超え、Compile時に上限へ制限
- 絶対Pathが使われている
- Sample Pitchが品質確認範囲を超える可能性

## 6.4 Missing Asset

元要件どおり、Sample Asset不足だけでInstrument全体をCompile失敗にしない。

- 該当Sample LayerをDisabledとしてCompile
- WarningにLayer ID、Asset Path、理由を含める
- 他のOscillator Layerは通常どおり発音
- 全LayerがDisabledの場合もCompiled Instrumentを返し、Runtimeは無音を出力
- Definitionを修正して再Compileすれば復旧可能

MVPでは専用のAsset Relink Commandを必須にしない。  
JSONのPathとHashを修正し、再Validation・再Compileできればよい。

---

# 7. Process ContractとLifecycle

## 7.1 Lifecycle

MVPでCoreが提供するLifecycle：

```text
Compile
  → Instantiate
    → Prepare
      → Process（繰り返し）
        → Reset
```

### Compile

Control側。File I/O、Decode、Resampleを許可する。

### Instantiate

Compiled InstrumentからRuntime Instanceを作る。

### Prepare

- Sample Rate
- 最大Block Size
- 出力Channel数
- Voice数
- Scratch Buffer容量
- DaisySP Handle

を準備する。

### Process

Eventを適用し、Audio Bufferへ出力する。  
File I/O、Decode、JSON解析をしない。

### Reset

- 全VoiceをIdleへ戻す
- OscillatorをReset
- ADSRをReset
- Sample CursorをReset
- FilterをReset
- Absolute FrameをReset

同じ入力を再度与えたとき、同等の出力を得られる状態へ戻す。

## 7.2 Process Contract

```rust
pub struct ProcessSpec {
    pub sample_rate: f64,
    pub max_block_size: usize,
    pub output_channels: usize,
}

pub struct ProcessContext {
    pub absolute_frame: u64,
    pub tempo_bpm: f64,
}

pub struct ProcessBlock<'a> {
    pub frames: usize,
    pub context: ProcessContext,
    pub events: &'a [ProcessEvent],
    pub output: &'a mut [&'a mut [f32]],
}
```

### 制約

- `sample_rate > 0`
- `max_block_size > 0`
- MVP出力はStereo固定
- `frames <= max_block_size`
- 各Output Channelは`frames`以上
- Process開始時に対象範囲をZero Clear
- Process終了時に対象範囲の全Sampleを書き終える

## 7.3 Event Model

P0〜P2で扱うEvent：

```rust
pub enum EventKind {
    NoteOn {
        note_id: NoteId,
        note_number: u8,
        velocity: u8,
    },
    NoteOff {
        note_id: NoteId,
    },
}
```

MIDI FileにNote IDがない場合、CLI AdapterがChannel、Note Number、発音順から一意なIDを生成する。

## 7.4 Event順序

入力Eventは`sample_offset`昇順で渡す。

同一Offsetの規則：

1. 同じNote IDのNote Off
2. Note On
3. その他は入力順

CoreはDebug / Testで順序を確認し、不正なEvent列にはDiagnosticを返す。  
Release Buildでは処理可能な範囲で継続する。

## 7.5 Block内のSample Accurate処理

Block全体を一度にRenderしてからEventを反映してはならない。

```text
Block Start
  → Event 0までRender
  → Event 0を適用
  → Event 1までRender
  → Event 1を適用
  → Block EndまでRender
```

例：

```text
Offset 0          Offset 37               Offset 128
| Note Onを適用 | 37 Frame Render | Note Off | 残りをRender |
```

Block Sizeを変えても、Absolute Frame上のNote開始・終了位置は変わらない。

## 7.6 Process中に行わないこと

- JSON解析
- File I/O
- Asset Decode
- Sample Rate変換
- SHA-256計算
- Network Access
- Device操作
- Loggingのための同期I/O
- `Vec`や`String`の継続的な容量拡張
- Blocking Mutex
- Panic
- C++例外の境界越え

本格的なCPU BenchmarkはP3以降で行う。  
MVPでは、明らかに不必要なAllocationやI/OをAudio Pathへ入れないことだけを求める。

---

# 8. Rust–DaisySP境界

## 8.1 内部FFIと将来のPublic C ABIを分ける

P0で作るのは、RustからDaisySPを呼ぶための内部FFIである。

```text
Rust Runtime
  → sonalloy-dsp-sys
    → internal extern "C"
      → C++ DaisySP Wrapper
```

Riffraや外部アプリからSonalloyを呼ぶPublic C ABIではない。  
Public C ABIをP0で固定してはならない。

## 8.2 Opaque Handle

Native ObjectのLayoutをRustへ公開しない。

```c
typedef struct sonalloy_dsp_oscillator sonalloy_dsp_oscillator;
typedef struct sonalloy_dsp_filter sonalloy_dsp_filter;
```

最低限の関数群：

```c
sonalloy_dsp_oscillator* sonalloy_dsp_oscillator_create(void);
void sonalloy_dsp_oscillator_destroy(sonalloy_dsp_oscillator* handle);
int32_t sonalloy_dsp_oscillator_prepare(
    sonalloy_dsp_oscillator* handle,
    double sample_rate,
    int32_t waveform
);
int32_t sonalloy_dsp_oscillator_reset(
    sonalloy_dsp_oscillator* handle
);
int32_t sonalloy_dsp_oscillator_process(
    sonalloy_dsp_oscillator* handle,
    float frequency_hz,
    float* output,
    uint32_t frames
);

sonalloy_dsp_filter* sonalloy_dsp_filter_create(void);
void sonalloy_dsp_filter_destroy(sonalloy_dsp_filter* handle);
int32_t sonalloy_dsp_filter_prepare(
    sonalloy_dsp_filter* handle,
    double sample_rate
);
int32_t sonalloy_dsp_filter_reset(
    sonalloy_dsp_filter* handle
);
int32_t sonalloy_dsp_filter_process(
    sonalloy_dsp_filter* handle,
    float cutoff_hz,
    float resonance,
    float* buffer,
    uint32_t frames
);
```

正確なSymbol名は実装時に調整してよい。

## 8.3 FFIの規則

- CreateしたObjectは対応するDestroyで破棄
- Rust側のSafe Wrapperが所有権を持つ
- Safe Wrapperは`Drop`でDestroy
- Null Handleを検査
- C++例外をすべて捕捉しResult Codeへ変換
- Native側はProcessごとにHeap Allocationしない
- BufferはRust側が所有し、Call中だけ貸与
- Buffer長はRust側とNative側の両方で前提を確認
- Result Codeを無視しない
- Error時は対象Bufferを無音として扱う

## 8.4 Runtime内のNative Object

P1：

- Oscillator LayerごとにOscillator Handle
- VoiceのLeft / Right ChannelごとにFilter Handle

P2：

- Sample LayerはNative Handleを持たない
- Oscillator LayerだけがOscillator Handleを持つ
- Voice FilterはP1と同じ

RustがVoiceとLayerのLifecycleを所有し、DaisySPへVoice Allocationを任せない。

---

# 9. Voice Engine設計

## 9.1 Voice Runtime

```rust
pub struct VoiceRuntime {
    pub state: VoiceState,
    pub note_id: Option<NoteId>,
    pub note_number: u8,
    pub velocity: u8,
    pub started_at_frame: u64,
    pub estimated_level: f32,
    pub layers: Vec<LayerRuntime>,
    pub filter_left: DspFilter,
    pub filter_right: DspFilter,
    pub steal_fade: StealFade,
}
```

実際には`Vec`の容量をPrepare時に確保するか、Compiled Layer数に応じた固定Boxへする。

## 9.2 Voice State

```text
Idle
  └─ Note On → Active

Active
  ├─ Note Off → Releasing
  ├─ Voice Steal → StealFading
  └─ 全Layer終了 → Idle

Releasing
  ├─ 全Layer終了 → Idle
  └─ Voice Steal → StealFading

StealFading
  └─ Fade完了 → 新NoteでActive
```

## 9.3 Voice Allocation

Note On時：

1. Idle Voiceを探す
2. IdleがなければReleasing Voiceのうち`estimated_level`が最小のVoiceを探す
3. それもなければ`started_at_frame`が最も古いVoiceを選ぶ

## 9.4 Voice Stealing

VoiceをSample境界で即時切断するとクリックが出るため、固定の短いFadeを使用する。

- MVP初期値：5 ms
- 定数は一か所で管理
- Sound Reviewで調整可能
- Fade中に新Noteを同じVoiceへ即時重ねず、Fade完了後に開始
- 追加Voiceを一時確保しない

5 msが演奏Timingに明確な違和感を生む場合は、同じ設計のまま値だけを調整する。

## 9.5 Note Off

Note IDで対象Voiceを特定する。

- 対象Voice内のすべてのActive LayerをReleaseへ移行
- Oscillatorを即時停止しない
- Sample One-shotは再生を継続し、EnvelopeだけReleaseへ移行
- EnvelopeがIdleになったLayerを終了
- 全Layer終了でVoiceをIdleへ戻す

## 9.6 Voice終了条件

次の両方を満たしたときVoiceをIdleへ戻す。

- すべてのLayer Generatorが終了、またはEnvelopeがIdle
- Steal Fadeが動作していない

Filterの内部値はVoice終了時にResetする。

## 9.7 estimated_level

Voice Stealing選択用の厳密なLoudness計算は行わない。

MVPでは、直近BlockのVoice出力Peakを指数移動平均で保持し、`estimated_level`とする。

この値は音質処理へ使わず、Voice選択だけに使う。

---

# 10. Layer Runtimeと信号処理

## 10.1 Layer Runtime

```rust
pub struct LayerRuntime {
    pub active: bool,
    pub envelope: AdsrRuntime,
    pub generator: GeneratorRuntime,
}
```

Generator Runtime：

```rust
pub enum GeneratorRuntime {
    Oscillator(OscillatorRuntime),
    Sample(SampleRuntime),
}
```

## 10.2 Layer Trigger

Note On時に一度評価する。

```text
enabled
AND key_min <= note_number <= key_max
AND velocity_min <= velocity <= velocity_max
AND generator is available
```

Sample Asset不足でCompiled SampleがDisabledの場合、Trigger結果をFalseにする。

発音対象Layerが0件の場合、Voiceを開始しない。

## 10.3 Segment Render

Event間のSegmentごとに次を行う。

1. Voice ScratchをZero Clear
2. Active LayerごとにLayer ScratchをZero Clear
3. GeneratorがMono Signalを生成
4. ADSRをSample単位で乗算
5. Layer Gainを乗算
6. Constant-power PanでStereoへ変換
7. Voice Scratchへ加算
8. Voice Left / Right Filterを処理
9. Steal Fadeを適用
10. Output Bufferへ加算
11. Voice Peakからestimated_levelを更新
12. 終了したLayer / Voiceを整理

## 10.4 Scratch Buffer

RuntimeはPrepare時に次を確保する。

- `layer_mono[max_block_size]`
- `voice_left[max_block_size]`
- `voice_right[max_block_size]`

Voiceを一つずつ処理し、同じScratchを再利用する。  
Voice数・Layer数ごとに巨大な一時Bufferを持たない。

## 10.5 Gain

```text
linear_gain = 10 ^ (gain_db / 20)
```

Layer Gain、Velocity Gain、ADSRを乗算する。

複数Layer、複数VoiceでPeakが上がるため、Reference InstrumentではLayer GainにHeadroomを持たせる。

MVPではEngine内部へ自動Limiterを常時挿入しない。  
ClippingはDefinition調整とTestで防ぐ。

## 10.6 Pan

Constant-power Panを使用する。

```text
angle = (pan + 1) × π / 4
left_gain  = cos(angle)
right_gain = sin(angle)
```

`pan = 0`で左右が同等Powerになる。

## 10.7 Tuning

```text
semitones = note_number - 69
midi_frequency = 440 × 2 ^ (semitones / 12)
tuning_ratio = 2 ^ (tuning_cents / 1200)
frequency = midi_frequency × tuning_ratio
```

Sampleでは、NoteとRoot Noteの差へTuningを加えてPlayback Ratioを求める。

---

# 11. ADSR設計

## 11.1 State

```text
Idle
Attack
Decay
Sustain
Release
```

## 11.2 State遷移

- Note On：Attack
- Attack完了：Decay
- Decay完了：Sustain
- Note Off：現在値からRelease
- Release完了：Idle
- Attack 0秒：直ちにDecay
- Decay 0秒：直ちにSustain
- Release 0秒：直ちにIdle

## 11.3 Curve

MVPでは、単純なLinear Rampではなく、音量用途に適したExponential寄りのCurveを使用する。

実装方法は次のどちらかを採用してよい。

- Targetへ向かうOne-pole係数
- 事前に定義したExponential Curve

ただし、Attack / Decay / Releaseの指定時間が大きくずれないことをTestする。

Curveの正確な形は`docs/runtime-processing.md`へ記録する。

## 11.4 Release

Note Off時の現在Envelope値から連続してReleaseを開始する。  
Sustain Levelへ戻してからReleaseを開始してはならない。

## 11.5 ADSR Test

- 各時間0秒
- 通常Attack / Decay / Release
- Attack中のNote Off
- Decay中のNote Off
- Sustain中のNote Off
- Release中のReset
- Sample Rate違い
- Block境界
- Outputが0〜1を超えない
- Release終了後にIdle

---

# 12. Oscillator・Filter・Smoothing

## 12.1 Oscillator

P1の正式対応：

- Sine
- Saw

Square / TriangleはMVP外であり、Definitionへ指定された場合はCompile Errorにする。

### Phase Reset

- `phase_reset = true`：Note Onごとに同じPhaseから開始
- `phase_reset = false`：Voiceの前回Phaseを使わず、Voice開始時にEngine既定Phaseを利用

MVPでは再現性を優先し、Reference Instrumentは`phase_reset = true`を使う。

## 12.2 Sawの品質確認

DaisySPのSawを無条件に高品質とみなさない。

P1 Sound Reviewで次を確認する。

- C2、C4、C6付近の単音
- 44.1kHz、48kHz
- Filterを開いた状態
- Filterを閉じた状態
- Spectrumを参考として添付
- 人間が高音域のAlias感・耳障りさを判断

品質不足の場合：

1. Definitionを変更しない
2. RuntimeのOscillator Interfaceを変更しない
3. DaisySP Adapter内の実装だけを交換する
4. 同じ試聴音源を再生成して比較する

## 12.3 Filter

MVPではVoice Mix後のStereo Low-pass Filterだけを実装する。

- Left / Rightで同一設定、独立State
- CutoffはHz
- Resonanceは0〜1のSonalloy値
- DaisySP Adapterで実装値へ変換
- Sample Rate変更時にPrepareし直す

Cutoff上限：

```text
effective_max = min(20000 Hz, sample_rate × 0.45)
```

Definition値が上限を超えた場合、Compile時にClampしWarningを返す。

## 12.4 Velocity Filter Response

Voice開始時にVelocityからVoice固有Cutoffを計算する。

MVPではNote中にVelocityが変化しないため、Voice開始後の継続Modulationはない。

## 12.5 Smoothing

MVPでSmoothingが必要な対象：

- Voice開始時のFilter Cutoff設定
- Segment境界で値が変わる可能性があるGain
- 将来のParameter Changeに備えた最小共通Utility

ただし、汎用Parameter Automation Frameworkは作らない。

初期Smoothing時間：

- Gain：5 ms
- Filter Cutoff：10 ms

値はSound Reviewで調整可能。  
設定は一か所へ集約する。

---

# 13. Sample Engine設計

## 13.1 MVPのSample範囲

- WAV
- One-shot
- 一つのSample Layerにつき一つのAsset
- Root Note
- Key / Velocity TriggerはLayer側
- Compile時Decode
- Compile時Sample Rate変換
- Runtime Pitch Playback
- Mono内部再生
- Layer PanでStereo化
- Loopなし
- Round Robinなし

## 13.2 Reference Sample

P2ではLicenseが明確なReference Sampleを使用する。

優先順位：

1. ユーザー提供のSample
2. Projectが所有する録音
3. 実装エージェントが決定論的に生成したMetal Hit WAV

外部配布Sampleを無断でRepositoryへ含めない。

`testdata/assets/README.md`へ次を記録する。

- File名
- 作成・取得方法
- License / 所有者
- Sample Rate
- Channel数
- Bit Depth
- Root Noteの判断
- Reference Instrumentでの用途

## 13.3 Asset Path

- Definition FileのDirectoryを基準に相対Pathを解決
- 相対Pathを標準とする
- 絶対Pathは許可するがWarning
- Path正規化はCompile時
- RuntimeへPathを持ち込まない

## 13.4 Hash

`sha256`は任意Fieldだが、Reference Instrumentでは必須にする。

- Hash一致：利用
- Hash未指定：利用しWarning
- Hash不一致：LayerをDisabledにしWarning
- File不存在：LayerをDisabledにしWarning

## 13.5 Decode

SymphoniaでDecodeし、`f32`へ変換する。

MVP受入対象：

- PCM WAV
- 16-bit / 24-bit / 32-bit float
- Mono / Stereo
- 44.1kHz / 48kHz / 96kHz

Stereo AssetはMVP内部でMonoへDownmixする。

```text
mono = (left + right) × 0.5
```

Stereoの空間情報保持はMVP外。  
Stereo AssetをDownmixした場合はCompile Warningへ記録する。

## 13.6 Sample Rate変換

RubatoをCompile時に使用し、Engine Sample Rateへ変換する。

Runtime中にRubatoを呼ばない。

Prepared Sample：

```rust
pub struct PreparedSample {
    pub sample_rate: f64,
    pub samples: Arc<[f32]>,
    pub source_metadata: SampleMetadata,
}
```

## 13.7 Playback Ratio

```text
semitone_delta =
    note_number
    - root_note
    + tuning_cents / 100

playback_ratio = 2 ^ (semitone_delta / 12)
```

Prepared SampleはEngine Sample Rateへ変換済みなので、Sample Rate差をRatioへ含めない。

## 13.8 Playback Cursor

```rust
pub struct SampleRuntime {
    pub position: f64,
    pub playback_ratio: f64,
    pub finished: bool,
}
```

各Output Sample後に`position += playback_ratio`。

## 13.9 補間

MVPでは4-point Cubic Hermite相当の補間を使用する。

- Buffer先頭・終端で範囲外参照をしない
- 必要に応じて端点を複製
- Root Note再生時も同じ処理経路を使う
- Root Noteから±12 semitoneを人間による品質確認範囲とする
- 範囲外を禁止はしないが、品質保証外とする

## 13.10 Sample終端

- Cursorが終端へ到達したらGeneratorをFinished
- ADSRがまだActiveでも、Generator出力は0
- LayerはEnvelopeがIdle、またはGenerator Finishedかつ出力が0になった時点で終了
- 終端直前のInterpolationで範囲外参照をしない
- 終端に明確な不連続が出る場合は、短い終端Fadeを検討する
- 終端Fadeを導入する場合は固定値とし、文書化する

## 13.11 Note Off

One-shot SampleでもNote Offを無視しない。

- Playback Cursorは継続
- EnvelopeをReleaseへ移行
- Release完了またはSample終端でLayer終了

---

# 14. Diagnostics設計

## 14.1 構造

```rust
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: DiagnosticSeverity,
    pub path: Option<String>,
    pub message: String,
    pub detail: Option<String>,
}
```

Severity：

- `Error`
- `Warning`
- `Info`

## 14.2 主なCode

- `SCHEMA_UNSUPPORTED`
- `JSON_INVALID`
- `REQUIRED_FIELD_MISSING`
- `ID_DUPLICATED`
- `VALUE_OUT_OF_RANGE`
- `LAYER_RANGE_INVALID`
- `GENERATOR_UNSUPPORTED`
- `ASSET_NOT_FOUND`
- `ASSET_HASH_MISMATCH`
- `ASSET_DECODE_FAILED`
- `ASSET_DOWNMIXED`
- `FILTER_CUTOFF_CLAMPED`
- `EVENT_ORDER_INVALID`
- `DSP_ERROR`

## 14.3 Compile結果

```rust
pub struct CompileResult {
    pub instrument: Option<Arc<CompiledInstrument>>,
    pub diagnostics: Vec<Diagnostic>,
}
```

- Errorが一つでもある場合、`instrument = None`
- WarningだけならCompiled Instrumentを返す
- Missing AssetはWarning
- CLIはText表示とJSON表示を提供する

---

# 15. CLI設計

## 15.1 CLIの責務

- Definition Fileの読込
- Compile APIの呼び出し
- Diagnostics表示
- MIDI FileをProcess Eventへ変換
- Offline Rendererの呼び出し
- WAVの書き出し

Coreへ持ち込まないもの：

- `clap`のArgument型
- Terminalの色
- MIDI File固有型
- File選択
- Process終了Code

## 15.2 MVP Command

```bash
sonalloy instrument init <path>
sonalloy instrument validate <definition>
sonalloy instrument inspect <definition>
sonalloy render note <definition> [options]
sonalloy render midi <definition> <midi> [options]
```

### `instrument init`

最小のP1 Definitionを生成する。  
複雑なAuthoring CLIは作らない。

### `instrument validate`

- Parse
- Validate
- Compile
- Diagnostics表示
- WAV Renderは行わない

### `instrument inspect`

人間が読みやすい形で次を表示する。

- Metadata
- Polyphony
- Layer一覧
- Trigger
- Generator
- Asset状態
- Envelope
- Voice Filter
- Warning

### `render note`

主なOption：

- Note
- Velocity
- Gate Duration
- Tail
- Sample Rate
- Block Size
- Output

### `render midi`

主なOption：

- MIDI File
- Tail
- Sample Rate
- Block Size
- Output

## 15.3 Exit Code

| Code | 意味 |
|---:|---|
| 0 | 成功 |
| 1 | Definition / Compile Error |
| 2 | Input File Error |
| 3 | Render Error |
| 4 | WAV Output Error |

Warningだけの場合は0。

---

# 16. テスト戦略

## 16.1 使用するLibrary

MVP開始時点では次に限定する。

| 用途 | Library |
|---|---|
| Unit / Integration Test | Rust標準`#[test]` / `cargo test` |
| Float近似比較 | `approx` |
| 一時File / Directory | `tempfile` |
| CLI結合テスト | `assert_cmd` |
| CLI出力確認 | `predicates` |
| Spectrum参考解析 | `rustfft`（Dev Dependency） |

最初から導入しないもの：

- Property-based Test Framework
- Benchmark Framework
- Snapshot Framework
- 大規模C++ Test Framework

必要性が具体化した場合だけ追加する。

## 16.2 Unit Test

Unit Testは対象Codeと同じModuleに置く。

### Definition / Compiler

- Schema Version
- JSON Round Trip
- 必須Field
- ID重複
- 値Range
- Layer Range
- Unsupported Generator
- Missing Asset Warning
- Filter Clamp

### Process

- Event順序
- Offset 0
- Block終端Offset
- 同一OffsetのNote Off / Note On
- EventがBlockをまたぐ
- Invalid Offset

### Voice

- Idle優先
- Releasing Voice優先
- Oldest Voice
- 同じPitchで異なるNote ID
- Note Off対象
- Release完了
- Steal Fade
- Reset

### ADSR

- 0秒Segment
- 各State遷移
- Note Off時の現在値
- Sample Rate違い
- Output Range
- Release終了

### Layer

- Key Trigger
- Velocity Trigger
- Disabled Layer
- Layerが0件発音
- Gain
- Pan
- Tuning

### Sample

- Root Note Ratio
- ±12 semitone
- Cursor更新
- Buffer先頭・終端
- 補間
- One-frame Sample
- Sample終端
- Note Off Release

## 16.3 FFI結合テスト

`sonalloy-dsp-sys/tests/ffi.rs`。

- Create / Destroy
- Null Handle
- Invalid Sample Rate
- Sine生成
- Saw生成
- Filter生成
- Reset
- Buffer Bounds
- Result Code
- C++例外捕捉
- 異なるBlock Size

LinuxではAddressSanitizer / UndefinedBehaviorSanitizerを使用する。

C++ Wrapperに独自Domain Logicを入れないため、P0〜P2ではGoogleTest / Catch2を必須にしない。

## 16.4 Core結合テスト

`sonalloy-core/tests/core_mvp.rs`。

- Definition → Compile
- Compile → Instantiate → Prepare
- Note On → WAV
- Chord → WAV
- MIDI Event列 → WAV
- Block Size 64 / 257 / 1024
- Polyphony Limit
- Voice Stealing
- P1 Definition再読込
- P2 Hybrid
- Missing Asset
- Source Sample Rate違い
- Reset後の再Render

## 16.5 CLI結合テスト

`sonalloy-cli/tests/cli.rs`。

- `instrument init`
- `instrument validate`
- `instrument inspect`
- `render note`
- `render midi`
- Invalid JSON
- Missing Asset Warning
- Output File生成
- Exit Code
- `--json` Diagnostics

## 16.6 機械的な音声確認

自動で確認するのは明確な異常に限定する。

- Frame数
- Channel数
- NaN / Infinity
- Peak
- RMS
- DC Offset
- Sineの基本周波数
- Note開始Frame
- Note終了Frame
- Note境界の大きな不連続
- 明確なClipping
- Reset後の再現性
- Block Size変更時のTiming一致
- Root NoteのPlayback Ratio
- Sample終端の範囲外参照

Spectrumは参考情報として人間へ渡す。  
Alias量の単一閾値だけで音質を自動判定しない。

## 16.7 Expected Data

`testdata/expected/`には巨大なGolden WAVを大量に置かない。

保存するもの：

- 短いSineの期待Metrics JSON
- Event Timingの期待Frame
- Sample Ratioの期待値
- Reference DefinitionのCompile Diagnostics
- 必要な短いFixture

試聴用WAVは`review-output/`へ生成する。

## 16.8 Cross-platform

同一Platform・同一Buildでは可能な限り厳密な再現性を確認する。

Windows / Linux間では、浮動小数点実装差を考慮し、次で同等性を確認する。

- Frame数
- Event Timing
- Peak / RMS / DC
- 基本周波数
- Sampleごとの許容誤差
- 人間による試聴差

Cross-platform完全Bit一致をMVPの前提にしないが、差が大きい場合は原因を調査する。

---

# 17. 人間による音質確認

## 17.1 役割分担

| 担当 | 役割 |
|---|---|
| 自動Test | 壊れた出力、Timing、範囲外、再現性、明確な不連続を確認 |
| AIエージェント | 固定条件のWAV、Definition、MIDI、測定結果、確認点を整理 |
| 人間 | 耳障りさ、自然さ、演奏感、音の魅力、Layerの融合を判断 |

AIエージェントは音質を自己承認しない。

## 17.2 Review Package

P1 / P2終了時に次を一式で生成する。

```text
review-output/<phase>/
├─ audio/
├─ definitions/
├─ midi/
├─ metrics.json
└─ review-summary.md
```

`review-summary.md`に含めるもの：

- Commit
- Build環境
- Sample Rate
- Block Size
- 使用Definition
- 使用MIDI / Event
- 自動Test結果
- Metricsの要約
- 既知の制約
- AIが認識した懸念
- 人間へ確認してほしい項目
- 人間の回答欄

## 17.3 P1試聴用音源

### 01-sine-reference.wav

- C3、A4、C6
- 各1秒
- Filterなし相当の最大Cutoff
- 音程と不要Noiseの基準

### 02-saw-registers.wav

- C2、C3、C4、C5、C6
- 各1秒
- Velocity 100
- Filterを開いた状態と閉じた状態

確認：高音域の耳障りさ、低音の芯。

### 03-attack-release.wav

- 短いAttack
- 遅いAttack
- 短いRelease
- 長いRelease
- Note Off位置を明確に分ける

確認：Click、Envelopeの自然さ。

### 04-repeated-notes.wav

- C4を8分音符で16回
- Phase Reset On
- Velocity一定
- Releaseが少し重なる設定

確認：同音連打の機械感、不自然なClick。

### 05-polyphony-and-stealing.wav

- Polyphony 4
- 6音以上を順番に重ねる
- Release中Voiceを含める

確認：Stealingの違和感。

### 06-filter-and-velocity.wav

- Velocity 32 / 64 / 96 / 127
- 同一Note
- GainとCutoffの変化

確認：弱奏・強奏の自然さ。

### 07-musical-phrase.wav

- 4〜8小節
- Bass / Pluckに適したPhrase
- 和音と単音を含む

確認：実際に曲で使えるか。

## 17.4 P1の人間評価項目

- Sawの高音域が明確に耳障りでないか
- Note On / OffにClickがないか
- Attack / Releaseが自然か
- 同音連打が不自然でないか
- Voice Stealingが目立ちすぎないか
- Filterの変化が滑らかか
- Velocity Responseが自然か
- Bass / Lead / Pluckの素材として使いたいか

人間の明示的な承認までP1を完了扱いにしない。

## 17.5 P2試聴用音源

### 01-sample-source.wav

Decode前のReference SampleをWAVとして提示する。

### 02-sample-decoded-root.wav

Root NoteでRenderしたSample Layer。

確認：Decode・Resampleによる不要な変化。

### 03-sample-pitch-range.wav

- C3、G3、C4、G4、C5
- Root Note C4を想定

確認：±12 semitoneのPitch品質。

### 04-oscillator-only.wav

Metallic HybridのBody Layerだけ。

### 05-sample-only.wav

Attack Layerだけ。

### 06-hybrid-mix.wav

同じPhraseを二LayerでRender。

確認：Layerの一体感。

### 07-velocity-response.wav

Velocity 32 / 64 / 96 / 127。

確認：Sample AttackとFilterの反応。

### 08-musical-phrase.wav

4〜8小節の実用Phrase。

確認：曲で使いたい音か。

### 09-missing-asset-fallback.wav

Sample Assetを欠落させ、OscillatorだけでRender。

確認：部分読込が音声処理を壊さないか。

## 17.6 P2の人間評価項目

- Decode後に原音が不自然に変化していないか
- Sample Pitchが確認範囲で許容できるか
- Sample終端にClickがないか
- Sample Attackが役割を果たしているか
- Oscillatorが音の芯と余韻になっているか
- 二Layerが別々に鳴るのではなく一つの音色に聞こえるか
- Velocity変化が自然か
- 高音・低音で破綻しないか
- Musical Phraseで使いたい音になっているか

人間の明示的な承認までP2とMVPを完了扱いにしない。

## 17.7 修正と再評価

人間が修正を求めた場合：

1. 指摘を再現可能な条件へ変換
2. Definition調整かDSP修正か分類
3. 関係するUnit / 結合Testを追加
4. 修正
5. 自動Test
6. 同じ条件のWAVを再生成
7. 修正前後をReview Packageへ含める
8. 再度人間へ渡す

---

# 18. 実装計画の読み方

## 18.1 詳細設計と実装計画の関係

1〜17章は、Sonalloy Core MVPを**どのような仕組みにするか**を定める詳細設計である。  
19〜21章は、その設計を**どの順番で、どの成果物として実装するか**へ変換した実行計画である。

実装エージェントは、作業パッケージだけを読んで独自設計してはならない。各パッケージの「参照設計」に示す章を確認し、その設計を実装へ反映する。

一方で、1〜17章の内容を19〜21章へ全文転載はしない。重複による不整合を防ぐため、実装計画では次を具体化する。

- その作業が成立させる状態
- 着手前に完了しているべき作業
- 参照する詳細設計
- 主な対象Crate / Module
- 実装順序
- 作業中に守る不変条件
- Unit Testと結合テストの具体的な入力・期待結果
- 生成するFixture、Example、試聴資料
- 更新するドキュメント
- 完了判定
- その作業では行わないこと

## 18.2 作業パッケージの完了単位

各作業パッケージは、実装だけで完了としない。次を一組で完了させる。

1. Production Code
2. Unit Test
3. 必要な結合テスト
4. Diagnosticまたは失敗時の扱い
5. Fixture / Example
6. 関連ドキュメント更新
7. 作業報告

作業途中の仮実装を後続パッケージで直す前提にしない。ただし、後続機能がないと実行できない結合テストは、Test Skeletonと受入条件を先に置き、接続パッケージで有効化してよい。

## 18.3 Module Pathの扱い

本計画に記載するModule Pathは責務を示す推奨位置である。実装開始時点のRepository構成が異なる場合、名称を機械的に合わせるための大規模移動は行わない。

ただし、次の依存方向は変更しない。

```text
sonalloy-cli
    ↓
sonalloy-core
    ↓
sonalloy-dsp-sys
    ↓
DaisySP
```

次は禁止する。

- `sonalloy-core → sonalloy-cli`
- `sonalloy-core → clap / midly / houndのCLI固有型`
- `sonalloy-core → JUCE / CPAL / midir / CLAP / VST3`
- `definition → DaisySP型`
- `runtime → JSON / filesystem`
- `sonalloy-dsp-sys → SonalloyのDefinitionやVoice設計`

## 18.4 フェーズ間ゲート

```text
P0 自動受入
   ↓
P1 実装・自動テスト
   ↓
P1 試聴Package生成
   ↓
人間の音質承認
   ↓
P2 実装・自動テスト
   ↓
P2 試聴Package生成
   ↓
人間の音質・Hybrid価値承認
   ↓
Core MVP完了
```

P1の人間承認前にP2の実装へ進んではならない。  
P1でOscillator、Envelope、Filter、Voice遷移の品質に問題が残ったままSample Layerを追加すると、問題の原因が判別しにくくなるためである。

---

# 19. P0実装計画：音声処理基盤

## 19.1 P0の目的と完了状態

### 目的

Rust CoreとDaisySPの境界、共通Process Contract、Offline Renderを実コードで成立させる。P1以降が「音声生成の基盤を作り直す」必要がない状態を作る。

### P0完了時に成立している流れ

```text
CLI開発Command
  → Offline Renderer
    → Process Contract
      → Safe Rust DSP Wrapper
        → Internal C ABI
          → DaisySP Sine
            → Stereo WAV
```

### P0の受入Command

```bash
sonalloy dev render-sine \
  --frequency 440 \
  --duration 1.0 \
  --sample-rate 48000 \
  --block-size 257 \
  --output out/p0-sine.wav
```

Command名やOption構文は実装上の整合性に合わせて調整してよい。受入時に必要なのは、Sample Rate、Block Size、Duration、周波数、出力先を明示して同じ経路を実行できることである。

---

## 19.2 P0-1：Workspace・Native Build・依存境界

### 目的

Rust Workspace、C++ Wrapper、DaisySPのBuildとLinkをWindows / Linuxで再現可能にする。以降の作業が個別環境の手動設定に依存しない状態を作る。

### 前提

- なし
- 既存Repositoryがある場合は、現在のBuild方法と依存を先に確認する

### 参照設計

- §3 技術選定と責務境界
- §4 Repository・Module構成
- §8 Rust–DaisySP境界
- §16 テスト戦略

### 主な対象

- Workspace Root `Cargo.toml`
- RootまたはNative用`CMakeLists.txt`
- `crates/sonalloy-core`
- `crates/sonalloy-dsp-sys`
- `crates/sonalloy-cli`
- `native/daisysp-wrapper`
- CI設定
- `THIRD_PARTY_NOTICES.md`

### 実装順

1. 現在のRepository構成を確認し、既存のBuild資産を再利用できるか判断する。
2. 三つのCrateをWorkspace Memberとして定義する。
3. DaisySPのVersionまたはCommitを固定する。
4. `native/daisysp-wrapper`をCMake TargetとしてBuildできるようにする。
5. `sonalloy-dsp-sys/build.rs`からCMake BuildとNative Linkを行う。
6. Rustから呼べる最小のVersion / Capability関数をC ABIで公開する。
7. WindowsとLinuxでDebug / Release Buildを確認する。
8. CIへFormat、Clippy、Rust Test、Native Buildを追加する。
9. 直接依存とLicenseを`THIRD_PARTY_NOTICES.md`へ記録する。

### 不変条件

- DaisySP Headerを`sonalloy-core`からIncludeしない
- C++ Build設定をCLI Crateへ持ち込まない
- Native Libraryの探索を開発者固有の絶対Pathに依存させない
- DaisySPのSourceを無断で改変しない。必要なAdaptationはWrapper側に置く
- Warningを大量に無視する設定でBuildを通さない
- P0のためにJUCEやAudio Device Libraryを追加しない

### Unit / Build Test

#### Given

- Clean Checkout
- 対応CompilerとRust Toolchain
- WindowsまたはLinux

#### When

```bash
cargo build --workspace
cargo test --workspace
```

#### Then

- 三CrateがBuildされる
- Native Wrapperが一度だけBuild・Linkされる
- RustからNative Version関数を呼べる
- Debug / Release両方でLink Errorがない
- DaisySPのVersionをTest出力またはBuild Metadataで確認できる

### CI確認

- Windows Job
- Linux Job
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Native Build
- Linux Native Sanitizer JobはP0-2で有効化する

### 成果物

- 再現可能なWorkspace Build
- Native Wrapper Target
- Version / Capability Smoke Test
- CI
- Build手順
- Third-party Notice

### ドキュメント更新

- `README.md`
  - 必要Toolchain
  - Windows / Linux Build
  - Test Command
- `docs/architecture.md`
  - Crate構成
  - 依存方向
- `THIRD_PARTY_NOTICES.md`
  - DaisySPと直接依存

### 完了条件

- Clean Checkoutから両OSでBuildできる
- RustからNative関数を呼べる
- `sonalloy-core`がDaisySPやC++ Headerへ依存していない
- CIで同じ手順を再現できる
- Build手順と実装が一致している

### 非対象

- Oscillatorの音声生成
- Process Contract
- WAV出力
- Plugin / Device接続
- 大規模なPackaging

---

## 19.3 P0-2：内部DSP FFIとSafe Rust Wrapper

### 目的

DaisySPのOscillatorをOpaque Handle越しに所有・初期化・Reset・Block処理できる、安全なRust側Wrapperを作る。

### 前提

- P0-1完了

### 参照設計

- §8 Rust–DaisySP境界
- §12 Oscillator・Filter・Smoothing
- §16.3 FFI結合テスト

### 主な対象

- `native/daisysp-wrapper/include`
- `native/daisysp-wrapper/src`
- `sonalloy-dsp-sys::ffi`
- `sonalloy-dsp-sys::oscillator`
- `sonalloy-dsp-sys/tests/ffi.rs`

### Public Contract

`sonalloy-dsp-sys`内に、少なくとも次のSafe APIを提供する。

```rust
pub enum DspOscillatorWaveform {
    Sine,
    Saw,
}

pub struct DspOscillator { /* opaque native handle */ }

impl DspOscillator {
    pub fn new() -> Result<Self, DspError>;
    pub fn prepare(
        &mut self,
        sample_rate: f64,
        waveform: DspOscillatorWaveform,
    ) -> Result<(), DspError>;
    pub fn reset(&mut self) -> Result<(), DspError>;
    pub fn process(
        &mut self,
        frequency_hz: f32,
        output: &mut [f32],
    ) -> Result<(), DspError>;
}
```

名称は調整可能だが、CoreからRaw PointerやResult Codeを直接扱わせない。

### 実装順

1. C ABIのResult Code一覧を定義する。
2. C++側のOpaque Oscillator Objectを実装する。
3. Create / Destroyを実装する。
4. PrepareでSample RateとWaveformを設定する。
5. Resetを実装する。
6. Block単位Processを実装する。
7. 全C ABI関数でNull、引数、例外を処理する。
8. Rust Raw FFI宣言を追加する。
9. Safe Wrapperを追加し、`Drop`でDestroyする。
10. Result Codeを`DspError`へ変換する。
11. FFI結合テストとSanitizerを有効化する。

### 不変条件

- Native ObjectのLayoutをRustへ公開しない
- Safe Wrapper以外からRaw FFIを呼ばない
- Create成功後の所有者はSafe Wrapper
- `Drop`以外で二重Destroyしない
- C++例外を境界外へ出さない
- `process()`でNative Heap Allocationしない
- Output Sliceの所有権はRust側
- Error時に未初期化Bufferを残さない。呼び出し側で無音化できるErrorを返す
- Sample単位のFFI Callを行わない

### FFI結合テスト

#### Lifecycle

- Create → Prepare → Process → Reset → Process → Destroy
- Prepare前のProcessはError
- Invalid Sample RateはError
- Null HandleはCrashせずError
- Empty Bufferを安全に扱う

#### Signal

- 48 kHz、440 Hz、1秒Sine
- 全Sampleがfinite
- Peakが想定範囲内
- 基本周波数が大きくずれない
- Reset後に同じ初期波形
- 64 / 257 / 1024 Frameの分割で連結結果が同等

#### Safety

- AddressSanitizer
- UndefinedBehaviorSanitizer
- Buffer前後にGuard領域を置き、書換えがない
- Rust Test終了時にLeakがない

### 成果物

- Internal C ABI
- Safe Rust Oscillator Wrapper
- Error Mapping
- FFI Test
- Sanitizer設定

### ドキュメント更新

- `docs/architecture.md`
  - Opaque Handleと所有権
- `docs/runtime-processing.md`
  - DSP Wrapper Lifecycle
- `docs/testing-and-sound-review.md`
  - FFI TestとSanitizer

### 完了条件

- RustからSine / SawをBlock単位で生成できる
- Reset後の初期状態が再現する
- Invalid InputでCrashしない
- Leak、範囲外書込み、例外越境がない
- Core側へRaw FFIが漏れていない

### 非対象

- Sonalloy Voice
- Filter FFI
- Definition
- Audio Device
- Public C ABI

---

## 19.4 P0-3：Process ContractとRuntime Skeleton

### 目的

P1・P2と将来Adapterが共通利用するPrepare / Process / Resetの意味、Buffer、Frame、Contextを実装する。

### 前提

- P0-2完了

### 参照設計

- §7 Process ContractとLifecycle
- §10 Layer Runtimeと信号処理
- §16.2 Unit Test
- §16.4 Core結合テスト

### 主な対象

- `sonalloy-core::process`
- `sonalloy-core::runtime`
- `sonalloy-core::render`のInterface
- `sonalloy-core/tests/core_mvp.rs`

### Public Contract

最低限、次をCore APIとして定義する。

```rust
pub struct ProcessSpec {
    pub sample_rate: f64,
    pub max_block_size: usize,
    pub output_channels: usize,
}

pub struct ProcessContext {
    pub absolute_frame: u64,
    pub tempo_bpm: f64,
}

pub struct ProcessBlock<'a> {
    pub frames: usize,
    pub context: ProcessContext,
    pub events: &'a [ProcessEvent],
    pub output: &'a mut [&'a mut [f32]],
}

pub trait InstrumentProcessor {
    fn prepare(&mut self, spec: ProcessSpec) -> Result<(), ProcessError>;
    fn process(&mut self, block: ProcessBlock<'_>) -> Result<(), ProcessError>;
    fn reset(&mut self) -> Result<(), ProcessError>;
}
```

Trait採用が既存設計に不要な場合はConcrete APIでもよい。Lifecycleと意味は維持する。

### 実装順

1. `ProcessSpec`とValidationを実装する。
2. `ProcessContext`とAbsolute Frameの意味を定義する。
3. Output BufferのValidationを実装する。
4. Process開始時に対象Frame範囲をZero ClearするUtilityを作る。
5. P0用のSine Runtimeを実装する。
6. Prepare時にOscillatorを初期化する。
7. Processで可変Frame数を処理する。
8. Process完了後にAbsolute Frameを進める。
9. ResetでOscillatorとFrame位置を戻す。
10. Error時にOutputを無音に保つ。
11. Offline Rendererが呼べる入口を作る。

### 不変条件

- MVP Output ChannelはStereo。2以外はPrepare Error
- `frames <= max_block_size`
- 各Output Slice長は`frames`以上
- Process対象範囲を必ず全Sample書く
- `process()`中にJSON、File、Decodeを扱わない
- RuntimeがCLI Option型を知らない
- Absolute Frameは呼び出し側Contextと内部状態で矛盾させない
- 0 Frame ProcessはNo-opとして安全に処理する
- Error時に部分的なGarbage Outputを返さない

### Unit Test

- Sample Rate 0 / NaN / Infinityを拒否
- Max Block Size 0を拒否
- Channel数1 / 3を拒否
- `frames = 0`
- `frames = max_block_size`
- `frames > max_block_size`
- Output Slice不足
- Zero Clear
- Absolute Frame更新
- ResetでFrame 0へ戻る

### 結合テスト

#### Given

- 48 kHz
- Max Block 1024
- 440 Hz Sine Runtime

#### When

- Block Size 64、257、1024で1秒Render相当のProcessを繰り返す

#### Then

- 総Frame数が48,000
- Stereo両Channelがfinite
- 基本周波数が同等
- Reset後に同じ条件で同等の出力
- Buffer末尾Guardが不変

### 成果物

- Process Contract
- P0 Runtime Skeleton
- Validation
- Process Error
- Core結合テスト

### ドキュメント更新

- `docs/runtime-processing.md`
  - Lifecycle
  - Buffer Contract
  - FrameとContext
  - Error時の無音化
- `docs/architecture.md`
  - Core Process Boundary

### 完了条件

- P0 Sine Runtimeを共通Process経路で実行できる
- 可変Block Sizeを安全に処理できる
- Resetで初期状態へ戻る
- Error時のOutput規則がTestされている
- CLIやFile I/OがCore Processへ混入していない

### 非対象

- Event適用
- Voice
- ADSR
- Definition
- MIDI

---

## 19.5 P0-4：Offline Renderer・Diagnostics・CLI Smoke・P0受入

### 目的

共通Process Contractを繰り返して、指定時間のStereo WAVを生成し、失敗を構造化Diagnosticsとして利用者へ返す。P0をEnd-to-Endで完了させる。

### 前提

- P0-1〜P0-3完了

### 参照設計

- §7 Lifecycle
- §14 Diagnostics設計
- §15 CLI設計
- §16 テスト戦略

### 主な対象

- `sonalloy-core::render`
- `sonalloy-core::diagnostics`
- `sonalloy-cli`
- `sonalloy-cli/tests/cli.rs`
- CI Smoke Render

### Public Contract

Core RendererはFile PathやWAV Encoderを知らず、Render済みAudioまたはSinkへ書き込む。

推奨形：

```rust
pub struct RenderRequest {
    pub sample_rate: f64,
    pub block_size: usize,
    pub duration_frames: u64,
    pub tail_frames: u64,
}

pub struct RenderedAudio {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}
```

長時間RenderのMemory問題はMVPでは主要課題ではないが、CLI側でBlockごとにWAV Writerへ流せる構造が自然なら採用してよい。CoreとFile Writerの責務分離は維持する。

### 実装順

1. CoreのOffline Render Loopを実装する。
2. DurationとTailをFrameへ変換する。
3. 最終Blockの可変Frame数を正しく処理する。
4. `Diagnostic`、Code、Severityを実装する。
5. Process / Render ErrorをDiagnosticへ変換する。
6. CLIへ`dev render-sine`を追加する。
7. `hound`でStereo WAVを書き出す。
8. Text / JSON Error表示の最小形を実装する。
9. CLI結合テストを追加する。
10. CIで短いSmoke WAVを生成する。
11. P0受入音源とMetricsを生成する。

### 不変条件

- WAV Writer型をCoreへ持ち込まない
- Durationの丸め規則を一か所へ置く
- 最終Blockで余分なFrameを書かない
- Render Error時に破損した完成Fileを成功扱いしない
- WarningとErrorを混同しない
- CLIがDSP FFIを直接呼ばない
- CLIはCore Runtime / Renderer経由で処理する

### Unit Test

- Duration 0
- 1 Frame
- Block Sizeで割り切れる長さ
- 割り切れない長さ
- Tail追加
- Diagnostic Code / Severity
- Render途中Error

### CLI結合テスト

#### 正常系

```bash
sonalloy dev render-sine \
  --frequency 440 \
  --duration 1.0 \
  --sample-rate 48000 \
  --block-size 257 \
  --output <temp>/sine.wav
```

期待：

- Exit Code 0
- File存在
- 48,000 Frame
- Stereo
- Sample Rate 48,000
- finite
- 440 Hz付近

#### 異常系

- Sample Rate 0
- Block Size 0
- Output Directory不存在
- 書込み不可
- Native DSP Error

期待：

- 定義されたExit Code
- 人間が理解できるMessage
- JSON Modeでは機械可読
- 成功扱いしない

### P0 Review Artifact

```text
review-output/p0/
├─ audio/p0-sine.wav
├─ metrics.json
└─ summary.md
```

P0では人間承認をGateにしない。P1以降との基準比較に使う。

### 成果物

- Offline Renderer
- Diagnostics
- CLI Smoke Command
- WAV出力
- P0 Review Artifact
- CI Smoke Render

### ドキュメント更新

- `README.md`
  - Sine Render
- `docs/cli.md`
  - `dev render-sine`
- `docs/testing-and-sound-review.md`
  - P0 Metrics
- `docs/runtime-processing.md`
  - Offline Render Loop

### P0完了条件

- Windows / LinuxのClean Build成功
- 全P0 Test成功
- 共通Process Contract経由でStereo Sine WAV生成
- 64 / 257 / 1024のBlock SizeでFrame数・周波数が同等
- Reset後に同等出力
- Native Sanitizer成功
- P0 Review Artifact生成
- 関連文書が実装と一致

### 非対象

- Instrument Definition
- Voice / Event
- MIDI File
- Filter
- Sample
- 人間の音質承認
- Realtime Device

---

# 20. P1実装計画：演奏可能な高品質シンセ

## 20.1 P1の目的と完了状態

### 目的

JSONで保存した一つのOscillator Layerから、PolyphonicなBasic Poly SynthをCompile・演奏・Offline Renderできる状態を作る。Generator追加ではなく、Oscillator、ADSR、Filter、Voice遷移の品質を確立する。

### P1完了時のEnd-to-End

```text
basic-poly-synth.json
  → Parse / Validate
    → Compile
      → Instantiate / Prepare
        → MIDI Event
          → Voice Allocation
            → Saw + ADSR + Pan
              → Voice Filter
                → Stereo WAV
                  → Review Package
                    → 人間承認
```

### P1受入Command

```bash
sonalloy instrument validate examples/instruments/basic-poly-synth.json

sonalloy render midi \
  examples/instruments/basic-poly-synth.json \
  testdata/midi/p1-review.mid \
  --sample-rate 48000 \
  --block-size 257 \
  --tail 1.0 \
  --output out/p1-basic-poly-synth.wav
```

---

## 20.2 P1-1：Definition・Schema・Validation・Diagnostics

### 目的

P1 InstrumentをJSONとして保存・読込し、構造・値・MVP制約の誤りをAudio処理前に検出できるようにする。

### 前提

- P0完了

### 参照設計

- §5.1 Instrument Definition
- §5.2 Definitionの値と単位
- §5.3 Velocity Response
- §6 Compile Pipeline
- §14 Diagnostics設計

### 主な対象

- `sonalloy-core::definition`
- `sonalloy-core::diagnostics`
- `testdata/definitions`
- `docs/instrument-definition.md`

### 実装対象

- Schema Version
- Metadata
- Performance / Polyphony
- Layer ID
- Enabled
- Key / Velocity Range
- Gain / Pan / Tuning
- ADSR
- Oscillator Generator
- Voice Filter
- Velocity Response
- JSON Load / Save
- Validation Result

### P1制約

- Layerは配列で保存するが、P1では有効Layer数1だけを許可
- GeneratorはOscillatorだけ
- WaveformはSine / Sawだけ
- Voice Filterは0または1個
- Runtime状態をJSONへ保存しない
- Unknown Future Schemaを黙って読まない
- 未対応Fieldの扱いは一貫させる。推奨は未知FieldをErrorにし、Schema変更を明示する

### 実装順

1. Definition型とSerde表現を実装する。
2. `schema_version = 1`を定義する。
3. 構造Validationを実装する。
4. Field Range Validationを実装する。
5. ID重複を検査する。
6. P1のLayer数・Generator制約を検査する。
7. FilterをSample Rate非依存の範囲で一次Validationする。
8. Validation ErrorをField Path付きDiagnosticへ変換する。
9. JSON Round Trip Testを追加する。
10. P1 Valid / Invalid Fixtureを追加する。

### 不変条件

- DefinitionはFile Pathの基準Directoryを内部状態として持たない
- DefinitionにDecode済みAssetやDaisySP Handleを持たせない
- Deserialize成功とValidation成功を同一視しない
- 値を黙って補正しない。補正する場合はCompilerでWarning
- Layer IDを配列Indexの代わりに使わない
- P2拡張時にP1 JSONが読めなくなる変更をしない

### Unit Test

#### Valid

- 最小Sine Definition
- Saw + Filter Definition
- Metadata省略可能Field
- Boundary値

#### Invalid

- Unsupported Schema
- Layer 0件
- Layer 2件
- Duplicate ID
- Key min > max
- Velocity 0
- Gain範囲外
- Pan範囲外
- ADSR負値
- Sustain > 1
- Unsupported Waveform
- Polyphony 0 / 65
- NaN / InfinityをProgrammatic ConstructionでValidation

### 成果物

- P1 Definition Model
- Validation
- Diagnostic Paths
- Valid / Invalid Fixture
- JSON Round Trip

### ドキュメント更新

- `docs/instrument-definition.md`
  - P1 Field、型、単位、Range、完全JSON
- `docs/architecture.md`
  - Definitionの責務
- `docs/testing-and-sound-review.md`
  - Validation Test

### 完了条件

- Valid P1 JSONを損失なくRound Tripできる
- Invalid値をField Path付きで検出できる
- P1外Generator / Layer数を拒否できる
- DefinitionへRuntime / DaisySP型が混入していない
- Example JSONと文書が一致する

### 非対象

- Compile
- Voice
- Sample
- 汎用Parameter Catalog
- Modulation Matrix

---

## 20.3 P1-2：Compiler・Compiled Instrument・Instantiation

### 目的

Definitionを、Audio Pathで追加解釈せず実行できるCompiled Instrumentへ変換し、Runtime Instanceを生成できるようにする。

### 前提

- P1-1完了

### 参照設計

- §5.5 Compiled Instrument
- §5.6 Instrument Runtime Instance
- §6 Compile Pipeline
- §14 Diagnostics

### 主な対象

- `sonalloy-core::compiler`
- `sonalloy-core::runtime::instrument`
- `sonalloy-core::runtime::layer`
- `sonalloy-core::diagnostics`

### Public Contract

```rust
pub struct CompileContext {
    pub definition_base_dir: PathBuf,
    pub process_spec: ProcessSpec,
}

pub struct CompileResult {
    pub instrument: Option<Arc<CompiledInstrument>>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn compile_instrument(
    definition: &InstrumentDefinition,
    context: &CompileContext,
) -> CompileResult;
```

P1ではAssetを使わないが、P2でCompileContextへAsset基準Directoryを追加し直さないため、責務として定義してよい。

### 実装順

1. `CompiledInstrument`と`CompiledLayer`を実装する。
2. Validation結果をCompiler入口で必須にするか、Compiler内で必ず再Validationする。
3. dBをLinear Gainへ変換する。
4. centをTuning Ratioへ変換する。
5. ADSR秒をSample Rate依存のCompiled設定へ変換する。
6. Filter Cutoff上限をSample Rateから計算する。
7. Clamp時にWarningを追加する。
8. Velocity ResponseをRuntime計算しやすい値へ変換する。
9. Compiled Instrumentを`Arc`で不変共有できる形にする。
10. CompiledからRuntime Instanceを生成するAPIを追加する。
11. Runtime生成時にVoice数とLayer数を確定する。
12. Compiler結合テストを追加する。

### 不変条件

- Errorが一つでもあればCompiled Instrumentを返さない
- WarningだけならCompiled Instrumentを返す
- DefinitionをCompile中に変更しない
- Compiled InstrumentはRuntime状態を持たない
- RuntimeはJSON文字列やField Pathを参照しない
- dB / cent / ClampをProcess中に毎Block計算しない
- Compile失敗で既存Compiled Instrumentを破壊しないAPI形にする
- P1ではSample用の空Objectや未使用抽象化を作らない

### Unit Test

- dB → Linear
- cent → Ratio
- Filter Cutoff Clamp + Warning
- Valid Definition → Compiled
- Invalid Definition → None + Error
- 同じDefinition / Spec → 同等Compiled値
- Compile後にDefinitionを変更してもCompiledが変わらない
- WarningのみでCompiledを返す

### 結合テスト

#### Given

- `basic-poly-synth.json`
- 48 kHz ProcessSpec

#### When

- Parse → Compile → Instantiate

#### Then

- Voice数がDefinitionと一致
- Layer数1
- Gain / Tuningが変換済み
- Filter Cutoffが有効範囲
- RuntimeはPrepare前状態
- JSON TreeをRuntimeが保持しない

### 成果物

- Compiler
- Compiled Instrument
- Runtime Instantiation
- Conversion Test
- Compiler Diagnostics

### ドキュメント更新

- `docs/architecture.md`
  - Compile Pipeline
  - Compiled Instrument
  - Instantiation
- `docs/runtime-processing.md`
  - Prepare前後の状態
- `docs/instrument-definition.md`
  - Compile時補正とWarning

### 完了条件

- P1 DefinitionからCompiled Instrumentを生成できる
- Error / Warning規則がTestされている
- Runtime Instanceを生成できる
- Process中にDefinition再解釈が不要
- Compiled / Runtimeの責務が分離している

### 非対象

- Note Event
- Audio生成
- Sample Asset
- Runtime Hot Swap

---

## 20.4 P1-3：ADSR・Voice Pool・Note Lifecycle・Voice Stealing

### 目的

Note OnからVoiceを開始し、Note Off後に自然にReleaseし、Voiceを再利用できる状態を作る。Polyphony上限到達時に仕様どおりVoice Stealingする。

### 前提

- P1-2完了
- P0 Process Contract完了

### 参照設計

- §9 Voice Engine設計
- §11 ADSR設計
- §7.3 Event Model
- §10 Layer Runtime

### 主な対象

- `sonalloy-core::runtime::adsr`
- `sonalloy-core::runtime::voice`
- `sonalloy-core::runtime::layer`
- `sonalloy-core::runtime::instrument`
- `sonalloy-core::process::event`

### 実装順

1. ADSR Stateと係数計算をVoiceから独立して実装する。
2. `note_on`、`note_off`、`next_sample`、`reset`を実装する。
3. 0秒Segmentの遷移を実装する。
4. `VoiceState`とVoice Metadataを実装する。
5. Compiled LayerからLayer Runtimeを初期化する。
6. Fixed-capacity Voice PoolをRuntime生成時に作る。
7. Idle Voiceを選ぶAllocationを実装する。
8. Note IDによるNote Offを実装する。
9. 全Layer終了によるVoice終了を実装する。
10. `estimated_level`の保持と更新入口を追加する。
11. Releasing Voice優先のSteal候補選定を実装する。
12. Oldest Active Voice選定を実装する。
13. 5 msのSteal Fade Stateを実装する。
14. Fade完了後にPending Noteを開始する。
15. Voice終了時にLayer、ADSR、FilterをResetする。
16. Instrument RuntimeへVoice APIを接続する。

### 不変条件

- Voice PoolはPrepare後に増やさない
- Note NumberだけでNote Off対象を決めない
- 同じNote Numberの複数Note IDを同時に扱える
- Release中もVoiceを占有している
- ADSRがIdleになる前に通常終了させない
- Steal時に波形をSample境界で即時0へ切らない
- Steal用の追加Voiceを確保しない
- Pending Noteは一Voiceにつき最大1
- Voice終了時にDSP StateをResetする
- `estimated_level`は音声Gainへ使わず、Steal選択だけに使う

### Unit Test：ADSR

#### Attack中Note Off

Given：

- Attack 100 ms
- 50 ms時点でNote Off

Then：

- 現在値からRelease開始
- Sustain LevelへJumpしない
- 出力に大きな不連続がない

#### 0秒Segment

- Attack 0 → Decay
- Decay 0 → Sustain
- Release 0 → Idle
- Infinite Loopしない

#### Sample Rate

- 44.1 / 48 / 96 kHzで指定時間から大きくずれない

### Unit Test：Voice Allocation

#### Idle優先

Given：

- Voice 4
- Active 2、Releasing 1、Idle 1

When：

- 新Note On

Then：

- Idleを選ぶ
- ReleasingをStealしない

#### Releasing優先

Given：

- Idle 0
- Releasing 2（estimated_level 0.1 / 0.3）
- Active 2

Then：

- 0.1のReleasing Voiceを選ぶ

#### Oldest

Given：

- 全Voice Active
- Start Frameが異なる

Then：

- 最古Voiceを選ぶ

#### Note ID

- 同PitchのNote ID A / B
- AのNote OffでBはReleaseしない

### Unit Test：Steal Fade

- 選定直後はPending Note
- Fade期間中に旧Voiceが減衰
- Fade完了後に新Note Active
- Fade終了前にVoiceをIdleへしない
- ResetでPending Noteを破棄

### 結合テスト

- 1音Note On / Off
- Chord
- Release重なり
- Polyphony 4で6音
- 同Pitch連打
- Resetで全Voice Idle

この段階ではOscillator出力がまだ未接続でも、Test GeneratorまたはConstant Signalを使ってVoice Stateを検証してよい。Test GeneratorをProduction Definitionへ露出させない。

### 成果物

- ADSR
- Voice Pool
- Note Lifecycle
- Voice Stealing
- Voice Unit / Integration Test

### ドキュメント更新

- `docs/runtime-processing.md`
  - ADSR Curve
  - Voice State
  - Allocation
  - Steal Fade
- `docs/testing-and-sound-review.md`
  - Voice Test

### 完了条件

- Note IDで発音・Releaseを管理できる
- ADSRの全Stateが動作する
- Voice Allocation優先順位がTestされている
- Voice StealingがFadeを経由する
- Resetで全状態を初期化できる
- Process中にVoice Poolを拡張しない

### 非対象

- Sustain Pedal
- Monophonic
- Legato
- Portamento
- Pitch Bend
- Audio Device

---

## 20.5 P1-4：Sample Accurate Event SchedulerとSegment Renderer

### 目的

Block内の正確なSample位置でNote Eventを適用し、Block Sizeが変わってもNote Timingを維持する。

### 前提

- P1-3完了

### 参照設計

- §7.3 Event Model
- §7.4 Event順序
- §7.5 Sample Accurate処理
- §10.3 Segment Render

### 主な対象

- `sonalloy-core::process::event`
- `sonalloy-core::runtime::instrument`
- `sonalloy-core::runtime::render_segment`

### 実装順

1. `ProcessEvent`と`EventKind`を実装する。
2. Event列のValidationを実装する。
3. 同一OffsetのNote Off / Note On順序を定義する。
4. Process BlockをEvent OffsetごとのSegmentへ分割する。
5. Segment開始前に該当Eventを適用する。
6. Event間をVoice EngineでRenderする。
7. 最終EventからBlock末尾までRenderする。
8. Absolute FrameへSegment Offsetを正しく加える。
9. Eventが0件のFast Pathを実装する。
10. 不正OffsetをDiagnostic / Process Errorへ変換する。
11. Block Size独立性Testを追加する。

### 不変条件

- EventをBlock先頭へ丸めない
- Segment長0を安全に扱う
- Event順序をProcess中に不安定Sortしない
- 同一Offsetの順序規則をCLIとCoreで重複実装しない
- Block末尾より大きいOffsetを黙ってClampしない
- Absolute Frame上のNote開始位置をBlock Sizeに依存させない
- Process中にEvent用Containerの容量拡張を行わない

### Unit Test

- Eventなし
- Offset 0
- Offset `frames - 1`
- 同一OffsetにNote Off / Note On
- 同一Offsetに異なるNote
- Segment長0
- Offset = frames（不正として扱う規則を固定）
- Offset > frames
- 非昇順Event列

### 結合テスト

#### Given

- 48 kHz
- Note On Absolute Frame 100
- Note Off Absolute Frame 1,100

#### When

- Block Size 64
- Block Size 257
- Block Size 1024

#### Then

- Note On / OffのAbsolute Frameが一致
- Render総長が一致
- ADSR Stateの開始・Release位置が一致
- 許容誤差内で波形が同等

### 成果物

- Event Model
- Event Validation
- Segment Renderer
- Block Size独立性Test

### ドキュメント更新

- `docs/runtime-processing.md`
  - Event順序
  - Segment Render図
- `docs/testing-and-sound-review.md`
  - Timing Test

### 完了条件

- Sample OffsetどおりにNote On / Offを適用できる
- Block SizeでTimingが変わらない
- 不正Eventを安全に拒否できる
- EventなしBlockも通常処理できる
- Voice Lifecycleと接続している

### 非対象

- MIDI File Parser
- Parameter Change Event
- Pedal / Pitch Bend
- Realtime Host Event

---

## 20.6 P1-5：Oscillator・Stereo Mix・Voice Filter・Smoothing統合

### 目的

Voice Engineへ実際のOscillator音声を接続し、ADSR、Gain、Pan、Tuning、Velocity Response、Stereo Filterを通したP1信号経路を完成させる。

### 前提

- P1-4完了
- P0 Oscillator FFI完了

### 参照設計

- §10 Layer Runtimeと信号処理
- §11 ADSR設計
- §12 Oscillator・Filter・Smoothing
- §5.3 Velocity Response
- §8 Runtime内Native Object

### 主な対象

- `sonalloy-dsp-sys::filter`
- Native Filter Wrapper
- `sonalloy-core::runtime::oscillator`
- `sonalloy-core::runtime::filter`
- `sonalloy-core::runtime::mix`
- `sonalloy-core::runtime::smoothing`
- Voice / Layer Runtime

### 実装順

1. P0のFFIパターンでFilter HandleとSafe Wrapperを追加する。
2. Voice Runtime生成時にOscillatorとLeft / Right Filterを準備する。
3. MIDI Note、TuningからFrequencyを計算する。
4. OscillatorでLayer Mono Scratchを生成する。
5. ADSRをSample単位で乗算する。
6. Velocity GainとLayer Gainを乗算する。
7. Constant-power PanでStereo Voice Scratchへ加算する。
8. VelocityからVoice Cutoffを計算する。
9. Left / Right Filterを処理する。
10. Voice出力をInstrument Outputへ加算する。
11. Gain / Cutoff用のSmoothing Utilityを追加する。
12. Voice Peakからestimated_levelを更新する。
13. Voice / Layer終了をAudio処理後に確定する。
14. Signal Test、FFI Filter Test、Click候補Testを追加する。

### 不変条件

- Layer Scratch / Voice ScratchはPrepare時に確保
- Voiceごとに巨大な可変Bufferを持たない
- Oscillator FFIをSampleごとに呼ばない
- Panの左右Gain規則を一か所へ置く
- Voice FilterはLayer Mix後に一度だけ適用
- Filter Left / Rightは独立State
- DefinitionなしにLimiter / Soft Clipを挿入しない
- Clippingを隠すためにOutputを自動Normalizeしない
- Smoothingを汎用Automation Frameworkへ拡大しない

### Unit Test

- MIDI Note 69 → 440 Hz
- ±12 semitone
- cent Tuning
- dB Gain
- Pan -1 / 0 / 1
- Velocity Gain 0 / 1 Amount
- Velocity Cutoff
- Cutoff Clamp
- Smoothing開始・終了
- Filter Reset
- Voice Peak更新

### FFI Filter Test

- Prepare / Reset
- Constant Input
- Cutoff差
- Resonance境界
- Stereo独立State
- finite
- Invalid Cutoff

### Core結合テスト

- Sine単音
- Saw単音
- Chord
- Velocity段階
- Pan左右
- Filter開閉
- Note Off Release
- Voice Stealing
- Block Size差
- Reset再現性

### 機械的音声確認

- finite
- Peak / RMS / DC
- Sine周波数
- Note境界の大きな不連続
- 明確な0 dBFS超過
- Saw Spectrum（参考）
- Filter Cutoff差のSpectrum（参考）

### 成果物

- Filter FFI / Wrapper
- P1信号経路
- Scratch / Mix
- Velocity Response
- Smoothing
- Signal Test

### ドキュメント更新

- `docs/runtime-processing.md`
  - 信号経路
  - Pan
  - Tuning
  - Velocity Response
  - Filter
  - Smoothing
- `docs/architecture.md`
  - DSP責務
- `docs/testing-and-sound-review.md`
  - Signal Metrics

### 完了条件

- Sine / SawをP1信号経路でStereo出力できる
- Note / Velocity / Pan / Tuning / FilterがDefinitionどおり反映
- Voice Stealingを含め明確なClickが機械検査で検出されない
- Human Reviewへ渡せる音声生成が可能
- Audio PathにFile / JSON /継続Allocationがない

### 非対象

- Sample
- Layer Processing Chain
- Drive / Effect
- Runtime Parameter Automation
- Oversamplingの自動導入

---

## 20.7 P1-6：CLI・MIDI Adapter・Basic Poly Synth

### 目的

DefinitionをCLIからValidation・Inspectし、単音またはMIDI Fileを同じCore RuntimeでRenderできるようにする。P1 Reference Instrumentを完成させる。

### 前提

- P1-5完了

### 参照設計

- §15 CLI設計
- §16.5 CLI結合テスト
- §17 P1試聴用音源

### 主な対象

- `sonalloy-cli`
- `examples/instruments/basic-poly-synth.json`
- `testdata/midi/p1-review.mid`
- CLI Test

### 実装順

1. `instrument init`で最小P1 JSONを生成する。
2. `instrument validate`でParse / Compile / Diagnosticsを実行する。
3. `instrument inspect`で構成を人間向けに表示する。
4. `render note`でNote On / Off Event列を生成する。
5. `midly`を用いてMIDI FileをAbsolute Frame Eventへ変換する。
6. Tempo Mapを最低限解釈し、EventをSample Offset付きBlockへ供給する。
7. `render midi`でOffline Rendererを実行する。
8. `hound`でStereo WAVを書き出す。
9. Text / JSON Diagnosticsを実装する。
10. Exit Codeを仕様どおり返す。
11. Basic Poly Synth Definitionを調整する。
12. CLI結合テストを追加する。

### MIDI AdapterのMVP規則

- Note On velocity 0はNote Offとして扱う
- MIDI ChannelはNote ID生成に利用する
- Note IDはChannel、Note Number、発音Serialから生成
- Tempo Eventを解釈する
- Sustain Pedalは無視し、InfoまたはWarningを出すか明示的に非対応とする
- Pitch Bend等のMVP外Eventは無視する規則を文書化
- Track順による同一Tick Event順序を安定させる

### 不変条件

- Coreへ`midly`型を渡さない
- MIDI TickをCore Process中に変換しない
- CLIがVoiceを直接操作しない
- `render note`と`render midi`は同じRuntime / Rendererを使う
- InspectがDefinitionの意味を再実装しない。CoreのCompiled / Definition APIを利用
- Invalid DefinitionでWAVを生成しない
- WarningだけならRender可能

### CLI結合テスト

- init → validate成功
- inspectにLayer / Waveform / Polyphony表示
- render noteでFile生成
- render midiでFile生成
- Invalid JSON Exit 1
- Missing Input Exit 2
- Output Error Exit 4
- JSON DiagnosticがParse可能
- Tempoを含むMIDIのEvent Frame
- 同Pitch重複Note

### Reference Instrument完成条件

- Headroomを持つ
- P1全機能を使用
- Bass / Pluck向けの明確なCharacter
- Review PhraseでClippingしない
- Sine / Saw切替版を必要なら比較可能
- DefinitionにMVP外Fieldを使わない

### 成果物

- P1 CLI
- MIDI Adapter
- Basic Poly Synth
- Review MIDI
- CLI Test

### ドキュメント更新

- `README.md`
  - P1 Quick Start
- `docs/cli.md`
  - Command、Option、Exit Code、例
- `docs/instrument-definition.md`
  - Basic Poly Synth完全JSON
- `docs/testing-and-sound-review.md`
  - P1 Review入力

### 完了条件

- CLIだけでDefinitionのValidation / Inspect / Renderが可能
- MIDI FileからSample Accurate Eventを生成できる
- Basic Poly Synthを同じCore RuntimeでRenderできる
- 全CLI Test成功
- P1 Review Package生成の前提が揃う

### 非対象

- GUI Editor
- 完全なCLI Authoring
- MIDI Device
- Sustain Pedal
- Plugin Host

---

## 20.8 P1-7：音質Review・修正・P1完了判定

### 目的

P1の自動テストで壊れていないことを確認したうえで、AIエージェントが比較可能な試聴資料を生成し、人間がOscillator Instrumentとしての音質を承認できる状態にする。

### 前提

- P1-1〜P1-6完了
- 全P1自動テスト成功

### 参照設計

- §17.1〜17.4 P1 Review
- §12.2 Saw品質確認
- §16 機械的音声確認

### 主な対象

- `review-output/p1`
- Review生成ScriptまたはCLI Command
- `docs/testing-and-sound-review.md`

### 実装順

1. Review専用DefinitionとMIDIを固定する。
2. Build情報、Commit、Platformを記録する。
3. §17.3の全WAVを48 kHz / Block 257で生成する。
4. 必要に応じて44.1 kHz版のSaw Register比較を生成する。
5. Peak / RMS / DC /基本周波数 /大きな不連続を計算する。
6. Saw Spectrumを参考としてMetricsへ追加する。
7. `review-summary.md`へ各WAVの目的と懸念を書く。
8. AIは音質合否を記載せず、人間へ具体的な確認項目を提示する。
9. 人間の回答を記録する。
10. 修正指示があれば再現条件へ落とし込み、関連Taskへ戻る。
11. 同じ条件で修正前後WAVを再生成する。
12. 人間の明示的承認後にP1を完了する。

### Review Package

```text
review-output/p1/
├─ audio/
│  ├─ 01-sine-reference.wav
│  ├─ 02-saw-registers.wav
│  ├─ 03-attack-release.wav
│  ├─ 04-repeated-notes.wav
│  ├─ 05-polyphony-and-stealing.wav
│  ├─ 06-filter-and-velocity.wav
│  └─ 07-musical-phrase.wav
├─ definitions/
├─ midi/
├─ metrics.json
└─ review-summary.md
```

### 人間に求める判断

- Sawの高音域は許容できるか
- Note境界にClickがないか
- Attack / Releaseは自然か
- 同音連打が不自然でないか
- Voice Stealingが目立たないか
- Filter / Velocity Responseは自然か
- 楽曲で使いたい基礎音色か
- P2へ進めてよいか

### 不変条件

- AIは「機械検査合格＝音質合格」としない
- Review条件を修正ごとに変更しない
- Definition調整とDSP修正を区別して報告する
- 不都合なWAVを省略しない
- 人間承認なしでP2へ進まない

### 成果物

- P1 Review Package一式
- 人間の評価結果
- 修正があった場合の前後比較WAV
- P1承認時のDefinition / MIDI / Metrics
- P2着手可否の記録

### ドキュメント更新

- `docs/testing-and-sound-review.md`
  - P1 Review条件
  - 人間の評価項目
  - 修正・再評価手順
- `README.md`
  - 承認済みBasic Poly Synthの再現Command

### 完了条件

- 全P1 Code / Test / Docs完了
- Review Package完全
- 既知の制約が記録済み
- 人間がP1音質を承認
- 承認時のDefinition / MIDI / Metricsを特定可能
- P2着手可能と明示

### 非対象

- Sample Layer
- Noise
- Modulation Matrix
- Realtime Device
- JUCE
- P1の音質問題をSampleやEffectで隠すこと

---

# 21. P2実装計画：最小Hybrid Instrument

## 21.1 P2の目的と完了状態

### 目的

P1で承認されたOscillator InstrumentへSample Layerを追加し、同じNoteから複数Layerを発音・混合する。Sonalloyの差別化である「異なる方式の融合」を実際の音で成立させる。

### P2完了時のEnd-to-End

```text
metallic-hybrid.json
  → Compile
    ├─ Oscillator Layer準備
    └─ Sample Asset Resolve / Decode / Resample
      → Runtime
        → Note On
          ├─ Attack Sample Layer
          └─ Body Oscillator Layer
            → Layer ADSR / Gain / Pan
              → Voice Mix / Filter
                → Hybrid WAV
                  → Human Review
```

### P2受入Command

```bash
sonalloy instrument validate examples/instruments/metallic-hybrid.json

sonalloy render midi \
  examples/instruments/metallic-hybrid.json \
  testdata/midi/p2-review.mid \
  --sample-rate 48000 \
  --block-size 257 \
  --tail 1.0 \
  --output out/p2-metallic-hybrid.wav
```

---

## 21.2 P2-1：複数Layer Definition・Compile・Runtime

### 目的

P1の一Layer制約を解除し、一つのNoteから複数LayerをTrigger・初期化・Render・終了できるようにする。

### 前提

- P1完了・人間承認済み

### 参照設計

- §5 三層モデル
- §10 Layer Runtime
- §2.2 半固定パイプライン（元要件）
- §9 Voice終了条件

### 主な対象

- `sonalloy-core::definition::layer`
- `sonalloy-core::compiler::layer`
- `sonalloy-core::runtime::layer`
- `sonalloy-core::runtime::voice`
- P2 Definition Fixture

### 実装順

1. P1の有効Layer数1制約をP2 Compilerから解除する。
2. Layer ID重複Validationを維持する。
3. Layer TriggerをCompiled値へ変換する。
4. Voice RuntimeがCompiled Layer数分のLayer Runtimeを持つようにする。
5. Note On時に各Layer Triggerを一度評価する。
6. TriggerされたLayerだけを初期化する。
7. Layerごとに独立ADSRを開始する。
8. Voice内でLayerを順番にRenderし、Voice Scratchへ加算する。
9. Trigger Layerが0件ならVoiceを開始しない。
10. 全Layer終了時にVoiceを終了する。
11. LayerごとのGain / Pan / Tuningを適用する。
12. 複数Layer用のUnit /結合テストを追加する。

### 不変条件

- Layerは同じVoice内で発音する
- Layerごとに別Voiceを割り当てない
- Note OffはVoice内の全Active Layerへ伝える
- Layerごとに独立ADSRを持つ
- Voice FilterはLayer Mix後に一度だけ適用
- Layer順で音量結果が変わらない加算にする
- Layer数に応じたRuntime容量はInstantiate / Prepareで確定
- P1 Definitionをそのまま読み込める

### Unit Test

- Layer ID重複
- Enabled false
- Key Range
- Velocity Range
- Trigger境界
- Trigger Layer 0件
- LayerごとのTuning
- LayerごとのPan
- 一Layer終了・他Layer継続
- 全Layer終了

### 結合テスト

#### Two Oscillator Layers

- Sine + Saw
- 異なるADSR
- 異なるPan
- 同じNote ID
- Note Offで両方Release
- Voice数は1

#### Trigger

- Key Rangeで一Layerだけ
- Velocity Rangeで一Layerだけ
- 両方一致で二Layer
- どちらも不一致で無音・Voice未開始

### 成果物

- Multi Layer Definition / Compiler / Runtime
- Trigger
- Layer Mix
- P1互換Test

### ドキュメント更新

- `docs/architecture.md`
  - 複数LayerとVoice
- `docs/instrument-definition.md`
  - Layer配列とTrigger
- `docs/runtime-processing.md`
  - Layer開始・終了・Mix

### 完了条件

- 一つのVoice内で複数Oscillator Layerを発音できる
- TriggerがKey / Velocityどおり
- Note OffとVoice終了が正しい
- P1 Definition / Review音が退行していない
- Runtime容量がProcess中に増えない

### 非対象

- Sample
- Layer Processing Chain
- Layer間Routing
- Round Robin
- Modulation Matrix

---

## 21.3 P2-2：Asset Reference・Decode・Downmix・Resample・Prepared Sample

### 目的

Definitionが参照するWAV AssetをControl側で解決・検証・Decode・Engine Sample Rateへ準備し、RuntimeがFile I/Oなしで利用できるCompiled Sampleを作る。

### 前提

- P2-1完了

### 参照設計

- §6 Compile Pipeline
- §6.4 Missing Asset
- §13.2〜13.6 Sample Asset
- §14 Diagnostics

### 主な対象

- `sonalloy-core::definition::sample`
- `sonalloy-core::asset`
- `sonalloy-core::compiler::asset`
- `testdata/assets`
- `testdata/assets/README.md`

### Definition追加

```rust
pub struct SampleDefinition {
    pub asset: AssetReference,
    pub root_note: u8,
    pub playback_mode: SamplePlaybackMode,
    pub interpolation: SampleInterpolation,
}
```

MVPで許可：

- `playback_mode = one_shot`
- `interpolation = cubic`
- 一Layer一Asset

### 実装順

1. `GeneratorDefinition::Sample`を追加する。
2. Asset ReferenceとRoot NoteをValidationする。
3. Definition Fileの基準DirectoryからPathを解決する。
4. 相対 / 絶対Pathの規則を実装する。
5. File存在を確認する。
6. SHA-256があれば照合する。
7. SymphoniaでWAVをDecodeする。
8. 対応Format / Channel / Sample Rateを検証する。
9. StereoをMonoへDownmixする。
10. RubatoでEngine Sample Rateへ変換する。
11. finite、Frame数、Peakを確認する。
12. `PreparedSample`へ変換する。
13. `CompiledGenerator::Sample`へ格納する。
14. Missing / Hash mismatch / Decode failureの扱いを実装する。
15. Asset FixtureとTestを追加する。

### Error / Warning規則

| 状態 | Severity | Compiled結果 |
|---|---|---|
| Path不存在 | Warning | Sample Layer Disabled |
| Hash不一致 | Warning | Sample Layer Disabled |
| Decode失敗 | Warning | Sample Layer Disabled |
| Unsupported Format | Warning | Sample Layer Disabled |
| Stereo Downmix | WarningまたはInfo | Sample Layer Enabled |
| Hash未指定 | Warning | Sample Layer Enabled |
| Absolute Path | Warning | Sample Layer Enabled |

Sample Assetだけの問題でInstrument全体をErrorにしない。

### 不変条件

- RuntimeへPath / File Handle / Decoderを渡さない
- Decode / ResampleをProcess中に行わない
- Missing AssetでOscillator Layerを無効化しない
- Hash不一致Assetを黙って使用しない
- Stereo Downmix規則を一か所へ置く
- Resample後Sample RateはProcess Specと一致
- Prepared Sampleは不変共有する
- 外部SampleをLicense確認なしでRepositoryへ追加しない

### Unit Test

- Relative Path解決
- Absolute Path Warning
- Path正規化
- SHA-256一致 / 不一致
- Hash未指定
- Mono WAV
- Stereo WAV Downmix
- 44.1 → 48 kHz
- 96 → 48 kHz
- 16 / 24 / float WAV
- Empty WAV
- Corrupt WAV
- NaNを含むInputへの扱い

### 結合テスト

#### Missing Asset

Given：

- Oscillator Layer有効
- Sample LayerのFile不存在

Then：

- CompileResultにInstrumentあり
- Warningあり
- Sample Compiled Layer disabled
- OscillatorはRender可能

#### Resample

- 44.1 kHz Source
- 48 kHz ProcessSpec
- Prepared Sample Rate 48 kHz
- Durationが許容範囲

### Reference Asset

Assetが未提供の場合、実装エージェントは決定論的なMetal Hit Fixtureを生成する。  
生成方法、License、Sample Rate、Bit Depth、Root Noteの根拠を`testdata/assets/README.md`へ記録する。

### 成果物

- Sample Definition
- Asset Resolver
- Hash
- Decoder
- Downmix
- Resampler
- Prepared Sample
- Asset Diagnostics
- Fixture

### ドキュメント更新

- `docs/instrument-definition.md`
  - Sample Generator
- `docs/architecture.md`
  - Asset Compile
- `docs/runtime-processing.md`
  - Prepared Sample
- `testdata/assets/README.md`
  - Provenance
- `docs/testing-and-sound-review.md`
  - Asset Test

### 完了条件

- 対応WAVをCompile時にPrepared Sampleへ変換できる
- Missing / Hash mismatchで部分Compileできる
- RuntimeがFile I/O不要
- Asset Fixtureの出所が明確
- Test Format / Sample Rateを網羅している

### 非対象

- Sample Playback
- Multiple Zone
- Loop
- Stereo Sample保持
- Streaming Sample

---

## 21.4 P2-3：Sample Runtime・Pitch Playback・Interpolation・終了処理

### 目的

Prepared SampleをRoot Noteから音程展開し、One-shot Sample LayerとしてADSR、Gain、Panを通してVoiceへMixできるようにする。

### 前提

- P2-2完了

### 参照設計

- §13 Sample Engine設計
- §10 Layer Runtime
- §11 ADSR
- §5.3 Velocity Response

### 主な対象

- `sonalloy-core::runtime::sample`
- `sonalloy-core::runtime::interpolation`
- `sonalloy-core::runtime::layer`
- Sample Unit Test

### Runtime Contract

```rust
pub struct SampleRuntime {
    source: Arc<[f32]>,
    position: f64,
    playback_ratio: f64,
    finished: bool,
}
```

実際にはMetadataをPrepared Sample参照経由で保持してよい。

### 実装順

1. Root Note / Note Number / TuningからPlayback Ratioを計算する。
2. Sample Runtimeの初期化を実装する。
3. Fractional Cursorを実装する。
4. 4-point Cubic Hermite補間を独立Utilityとして実装する。
5. Buffer先頭 /終端の端点規則を実装する。
6. Segment単位でMono ScratchへRenderする。
7. 各Sample後にCursorを進める。
8. 終端判定と`finished`を実装する。
9. Note OffでADSR Releaseへ移行する。
10. Generator Finished時のLayer終了規則を接続する。
11. Layer Gain / Pan / ADSRを通してVoice Mixへ接続する。
12. Sample Unit / Integration Testを追加する。

### 不変条件

- RuntimeでRubatoを呼ばない
- Playback RatioへSource Sample Rate差を二重に含めない
- Cursorの範囲外参照をしない
- Root Noteでも同じ補間経路を使う
- One-shot SampleをLoopしない
- Note OffでCursorを即時停止しない
- Sample終端後にGarbageを出さない
- Sample Layerの状態をOscillator Layerと共有しない
- Sample Source BufferをVoiceごとに複製しない

### Unit Test：Pitch

- Root Note → Ratio 1
- +12 semitone → 2
- -12 semitone → 0.5
- +100 cent → 2^(1/12)
- Root 0 / 127境界

### Unit Test：Interpolation

- Integer Positionで期待値
- Fractional Position
- 先頭
- 終端
- 1 Frame
- 2 / 3 Frame
- Constant Signal
- Ramp Signal
- finite

### Unit Test：Runtime

- Cursor進行
- Block境界
- 終端
- Finished後無音
- Note Off Release
- Reset
- 異なるPlayback Ratio

### 結合テスト

- Sample-only Layer
- C3 / C4 / C5
- Velocity Range
- Pan
- ADSR
- Source 44.1 kHz → Engine 48 kHz
- Block Size 64 / 257 / 1024
- Reset再現性

### Sample終端Click

自動Testでは、終端前後の最大Sample差を記録する。  
大きな不連続がある場合は、人間Review前に原因を確認する。

短い終端Fadeを導入する場合：

- 固定値を一か所で管理
- Sourceを破壊しない
- Runtimeで適用
- `docs/runtime-processing.md`へ記録
- Fade有無の比較WAVを人間へ渡す

### 成果物

- Sample Runtime
- Pitch Playback
- Cubic Interpolation
- End Handling
- Sample Integration

### ドキュメント更新

- `docs/runtime-processing.md`
  - Cursor
  - Ratio
  - Interpolation
  - Note Off
  - End
- `docs/testing-and-sound-review.md`
  - Pitch / End Test

### 完了条件

- Root Noteと±12 semitoneをRenderできる
- Cursorと補間がBounds-safe
- One-shot終端が正しい
- Note Off / ADSRと接続
- VoiceへStereo Mixできる
- Block Size / ResetでTimingが崩れない

### 非対象

- Multiple Zone
- Loop
- Time Stretch
- Formant Preservation
- Streaming

---

## 21.5 P2-4：Hybrid統合・Velocity Response・Missing Asset動作

### 目的

Oscillator LayerとSample Layerを同じVoice内で同時発音し、Velocity ResponseとMissing Asset部分読込を含むHybrid Runtimeを完成させる。

### 前提

- P2-3完了

### 参照設計

- §10 Layer Runtimeと信号処理
- §5.3 Velocity Response
- §6.4 Missing Asset
- §17.6 P2人間評価

### 主な対象

- `sonalloy-core::runtime::voice`
- `sonalloy-core::runtime::layer`
- `sonalloy-core::runtime::instrument`
- Hybrid Integration Test

### 実装順

1. Voice開始時にOscillator / Sample各LayerをTriggerする。
2. 各Layerを独立Scratchで順番にRenderする。
3. Velocity Gainを各Layerへ適用する。
4. Layer Mix後にVoice Filterを適用する。
5. VelocityからVoice Filter Cutoffを計算する。
6. Sample Layer Disabled時はTriggerしない。
7. Oscillator LayerだけでもVoiceを開始できるようにする。
8. 全Layer Disabled時はVoiceを開始せず無音。
9. Note Offを全Active Layerへ伝える。
10. 全Layer終了でVoiceを終了する。
11. Hybrid Integration Testを追加する。
12. P1 Regression Testを再実行する。

### 不変条件

- Oscillator / Sampleは同じNote IDとVoiceを共有
- Layerごとに別Voiceを作らない
- Voice FilterはMix後
- Velocity Filter CutoffをLayerごとに二重適用しない
- Missing Asset WarningをRuntimeで再生成しない
- Disabled LayerをProcessしない
- Sample不足時にOscillator音を変化させない
- Hybrid追加でP1 Basic Poly Synthの出力を不要に変えない

### 結合テスト

#### Hybrid

- Oscillator-only
- Sample-only
- Both
- BothのOutputが各Soloと整合
- Note Offで両Layer Release
- Sample終了後Oscillator継続
- Oscillator終了後Sample継続
- 全終了でVoice Idle

#### Velocity

- 32 / 64 / 96 / 127
- Sample Attack Gain差
- Filter Cutoff差
- Max VelocityでBase Cutoff
- Amount 0で変化なし

#### Missing Asset

- Sample File存在
- File削除
- 再Compile
- Warning
- Oscillator-only Render
- Definition修正後にSample復旧

#### Regression

- P1 Review Definition
- P1 Metrics
- Event Timing
- Voice Stealing

### 成果物

- Hybrid Voice Runtime
- Velocity Response
- Missing Asset Runtime動作
- Regression Test

### ドキュメント更新

- `docs/architecture.md`
  - Hybrid Layer
- `docs/runtime-processing.md`
  - Hybrid Segment Render
  - Missing Asset
- `docs/testing-and-sound-review.md`
  - Hybrid Test

### 完了条件

- 同じVoiceでOscillator + Sampleを発音
- Layer単体とMixを生成可能
- Velocity Responseが仕様どおり
- Missing AssetでOscillatorが継続
- P1 Regression成功
- Review用音声を生成できる状態

### 非対象

- Generic Modulation Route
- Layer Effect
- Sample Zone選択
- Noise
- Global Effect

---

## 21.6 P2-5：Metallic Hybrid・CLI統合・Fixture完成

### 目的

P2機能を一つのReference Instrumentとしてまとめ、CLIからValidation・Inspect・MIDI Renderできる状態を作る。

### 前提

- P2-4完了

### 参照設計

- §1.2 Metallic Hybrid
- §15 CLI
- §17.5 P2試聴用音源
- §13.2 Reference Sample

### 主な対象

- `examples/instruments/metallic-hybrid.json`
- `testdata/assets`
- `testdata/midi/p2-review.mid`
- `sonalloy-cli`
- CLI Integration Test

### 実装順

1. Reference Sampleを確定する。
2. AssetのProvenance、Format、Root Noteを記録する。
3. Attack Layerを定義する。
4. Body Layerを定義する。
5. Layer Gain / ADSR / Pan / Tuningを調整する。
6. Voice FilterとVelocity Responseを調整する。
7. `instrument validate`でAsset状態を表示する。
8. `instrument inspect`でLayer / Asset / Disabled状態を表示する。
9. `render note`でHybridをRenderする。
10. `render midi`でP2 Review MIDIをRenderする。
11. Missing Asset Fixtureを用意する。
12. CLI結合テストを追加する。
13. Clippingや明確なClickを自動確認する。

### Reference Hybridの設計意図

#### Attack Layer

- Sample
- Attack 0または極小
- 短いDecay
- Sustain 0付近
- Velocity Gainを比較的強く反映
- 音の初速・金属的Transientsを担当

#### Body Layer

- SineまたはSaw
- Attack Layerより長いDecay / Release
- 音程の芯と余韻を担当
- FilterでSampleとなじませる
- Sampleより低いPeakを基本とする

### 不変条件

- Effectで問題を隠さない
- P2外FieldをDefinitionへ追加しない
- 外部SampleのLicense不明状態でRepositoryへCommitしない
- Review MIDIを修正ごとに都合よく変更しない
- CLIがSample Decodeを直接行わない
- InspectがCore Diagnostic / Compiled情報を利用する

### CLI結合テスト

- Valid Hybrid Exit 0
- Inspectに二Layer
- Asset Path / Hash状態
- Hybrid Note WAV
- Hybrid MIDI WAV
- Missing Asset Warning + Exit 0
- Missing AssetでもWAV生成
- Invalid HashでSample Disabled
- Definition Path基準の相対Asset解決

### 成果物

- Metallic Hybrid Definition
- Reference Sample + Provenance
- P2 Review MIDI
- Missing Asset Fixture
- CLI P2対応
- End-to-End Test

### ドキュメント更新

- `README.md`
  - Metallic Hybrid Quick Start
- `docs/instrument-definition.md`
  - P2完全JSON
- `docs/cli.md`
  - Asset Warning / Hybrid Render
- `testdata/assets/README.md`
  - Provenance
- `docs/testing-and-sound-review.md`
  - P2 Review入力

### 完了条件

- CLIでHybridをValidation / Inspect / Renderできる
- Reference Assetの権利と技術情報が明確
- Missing Asset Fixtureが再現可能
- P2 Review Package生成の入力が固定
- 全P2自動テスト成功

### 非対象

- Asset Relink Command
- GUI Asset Browser
- Multiple Sample
- Preset Browser
- Distribution Package

---

## 21.7 P2-6：音質・Hybrid価値Review・MVP完了判定

### 目的

Hybrid Instrumentの機械的正常性だけでなく、SampleとOscillatorが一つの音色として成立しているかを人間が判断できる資料を生成し、Core MVPを正式に完了する。

### 前提

- P2-1〜P2-5完了
- P1 Regressionを含む全自動テスト成功

### 参照設計

- §17.5〜17.7 P2 Review
- §2.3 MVP品質
- §24 MVP全体完了条件

### 主な対象

- `review-output/p2`
- Review生成Script / CLI
- `docs/testing-and-sound-review.md`
- MVP完了Report

### 実装順

1. P2 Review Definition、MIDI、Assetを固定する。
2. Source Sample原音をPackageへ含める。
3. Root Note Decode / Resample版を生成する。
4. ±12 semitone Pitch Rangeを生成する。
5. Oscillator-onlyを生成する。
6. Sample-onlyを生成する。
7. Hybrid Mixを同じPhraseで生成する。
8. Velocity段階を生成する。
9. Musical Phraseを生成する。
10. Missing Asset Fallbackを生成する。
11. Peak / RMS / DC / Pitch /終端不連続をMetricsへ記録する。
12. AIが各Layerの意図、既知の制約、確認点をまとめる。
13. 人間へ音質・Hybrid価値の判断を依頼する。
14. 修正指示があれば、Definition調整かDSP修正か分類する。
15. 同じ入力条件で修正前後を再生成する。
16. 人間承認後、MVP完了Reportを作る。

### Review Package

```text
review-output/p2/
├─ audio/
│  ├─ 01-sample-source.wav
│  ├─ 02-sample-decoded-root.wav
│  ├─ 03-sample-pitch-range.wav
│  ├─ 04-oscillator-only.wav
│  ├─ 05-sample-only.wav
│  ├─ 06-hybrid-mix.wav
│  ├─ 07-velocity-response.wav
│  ├─ 08-musical-phrase.wav
│  └─ 09-missing-asset-fallback.wav
├─ definitions/
├─ midi/
├─ assets/
├─ metrics.json
└─ review-summary.md
```

### 人間に求める判断

- Decode / Resampleで原音が不自然に変化していないか
- Pitch RangeがMVP用途で許容できるか
- Sample終端にClickがないか
- Attack Layerが役割を果たしているか
- Body Layerが芯と余韻を作っているか
- SoloとMixを比べ、二Layerが自然に一体化しているか
- Velocity Responseは自然か
- Musical Phraseで使いたい音か
- SonalloyのHybrid価値を確認できたか
- Core MVPを完了してよいか

### 不変条件

- AIはHybrid価値を自己承認しない
- Source / Solo / Mixをすべて提示する
- 修正ごとに入力条件を変えない
- Sampleの権利情報をPackageから外さない
- 音質問題をMVP外Effect追加で解決しない
- 人間承認前にMVP完了と記録しない

### MVP完了Report

以下を記載する。

- 完了したScope
- MVP外へ残した機能
- Build / Test結果
- P1 / P2 Review承認
- Reference Instrument
- 既知の制約
- P3へ持ち越す課題
- 文書一覧
- Reproduction Command

### 成果物

- P2 Review Package一式
- 人間の評価結果
- 修正があった場合の前後比較WAV
- MVP完了Report
- 承認済みMetallic Hybrid Definition / MIDI / Asset / Metrics

### ドキュメント更新

- `README.md`
  - 承認済みMetallic Hybridの再現Command
- `docs/testing-and-sound-review.md`
  - P2 Review条件
  - Hybrid価値の評価項目
  - 修正・再評価手順
- 関連する全設計文書
  - 実装との差分がないことを最終確認

### 完了条件

- P2全自動テスト成功
- P1 Regression成功
- P2 Review Package完全
- 人間が音質とHybrid価値を承認
- MVP完了Report作成
- README / Architecture / Definition / Runtime / CLI / Test文書更新
- Windows / Linuxで受入Command成功
- Scope外機能が混入していない

### 非対象

- Noise
- Multiple Zone
- Round Robin
- Loop
- Generic Modulation
- Effects
- Realtime
- JUCE
- Plugin
- P3の設計開始

---

# 22. 実装と同時に整備するドキュメント

## 22.1 README.md

- Sonalloyの概要
- MVPでできること
- Windows / Linux Build
- Test
- CLI最短例
- Basic Poly Synth
- Metallic Hybrid
- 関連文書Link

## 22.2 docs/architecture.md

- Sonalloyの責務
- Crate構成
- Rust / DaisySP境界
- Definition / Compiled / Runtime
- Compile Pipeline
- Voice / Layer
- Asset準備
- 将来Adapterの位置

## 22.3 docs/instrument-definition.md

- Schema Version
- 全Fieldの意味
- 単位とRange
- Oscillator Layer
- Sample Layer
- Velocity Response
- P1 / P2完全JSON
- Validation Error例

## 22.4 docs/runtime-processing.md

- Lifecycle
- Process Contract
- Event順序
- Segment Render
- Voice State
- ADSR
- Voice Stealing
- Layer Mix
- Filter
- Sample Cursor
- Missing Asset Runtime

## 22.5 docs/cli.md

- Command
- Option
- Exit Code
- Text / JSON Diagnostics
- 使用例
- Error例

## 22.6 docs/testing-and-sound-review.md

- Unit Test配置
- 結合Test配置
- Library
- CI
- Metrics
- Review Package
- P1 / P2のWAV生成条件
- 人間の評価
- 再評価

## 22.7 THIRD_PARTY_NOTICES.md

- DaisySP
- Symphonia
- Rubato
- midly
- hound
- その他直接依存

文書は日本語で整備する。  
License名や原文のCopyright Noticeは改変しない。

---

# 23. AIエージェントの作業ルール

## 23.1 実装前

1. 元要件と本書を読む
2. Repositoryの現状を確認
3. 対象フェーズとの差分を整理
4. 変更対象をフェーズ内に限定
5. 既存Codeの責務を確認

## 23.2 実装中に禁止すること

- 複数フェーズを同時に完了させる
- MVP外機能を先行実装
- Crateを勝手に増やす
- DSP Libraryを勝手に増やす
- JUCEを追加
- Definitionを将来機能向けに一般化
- 汎用Modulation Matrixを作る
- Audio Graphを作る
- Unit Testをすべて別Directoryへ移す
- 自動Testだけで音質合格と判断
- 人間Reviewを省略

## 23.3 計画外変更が必要な場合

次を報告する。

- 必要な変更
- 現在の設計では不可能な理由
- MVPへの影響
- 代替案
- 追加依存
- 後戻り範囲

承認前にScopeを変更しない。

## 23.4 フェーズ終了報告

1. フェーズ目的
2. 実装内容
3. 主要File
4. Contract変更
5. Unit Test結果
6. 結合Test結果
7. 機械的音声確認
8. Review Package
9. 既知の問題
10. 文書更新
11. 人間に求める判断
12. 次へ進めるか

---

# 24. MVP全体の完了条件

## 24.1 機能

- JSON Definitionを保存・読込
- DefinitionをCompile
- Sine / Saw
- ADSR
- Voice Filter
- Polyphonic Voice
- Voice Stealing
- Sample Accurate Event
- WAV Sample
- Root Note Pitch
- 複数Layer
- Oscillator + Sample
- Velocity Response
- Missing Asset部分読込
- MIDIからStereo WAV

## 24.2 品質

- NaN / Infinityなし
- 明確なNote Clickなし
- 明確なSteal Clickなし
- 明確なSample終端Clickなし
- Reset再現性
- Block SizeでTiming不変
- Windows / Linux Test成功
- P1人間承認
- P2人間承認

## 24.3 ドキュメント

- README
- Architecture
- Instrument Definition
- Runtime Processing
- CLI
- Testing and Sound Review
- Third-party Notices

実装と一致している。

---

# 25. 自己レビュー

## 25.1 スコープ整合

- P0〜P2の目的は元要件の「音声素材と音響合成の融合」に一致している。
- MVPのGeneratorはSine / Saw Oscillatorと単一Sampleに限定した。
- Noise、複数Sample Zone、Round Robin、Loop、汎用Modulation、Effectsを戻していない。
- P1でSynth品質を承認してからP2へ進むため、問題原因を分離できる。
- P2で同じVoice内のLayer融合を実装し、Sonalloy固有価値を検証できる。
- JUCE、Realtime Device、PluginはAdapter段階まで延期している。

## 25.2 詳細設計と実装計画の対応

各主要設計に、対応する作業パッケージが存在する。

| 詳細設計 | 主な実装パッケージ |
|---|---|
| Repository /依存境界 | P0-1 |
| Rust–DaisySP FFI | P0-2、P1-5 |
| Process Contract | P0-3 |
| Offline Render / Diagnostics | P0-4 |
| Definition / Validation | P1-1 |
| Compiled Instrument | P1-2 |
| ADSR / Voice / Stealing | P1-3 |
| Sample Accurate Event | P1-4 |
| Oscillator / Filter / Mix | P1-5 |
| CLI / MIDI | P1-6 |
| P1音質Review | P1-7 |
| 複数Layer | P2-1 |
| Asset Compile | P2-2 |
| Sample Runtime | P2-3 |
| Hybrid / Missing Asset | P2-4 |
| Reference Hybrid / CLI | P2-5 |
| P2音質Review / MVP完了 | P2-6 |

設計だけ存在して実装順へ落ちていない主要項目、または設計にない大きな作業は残していない。

## 25.3 各作業パッケージの実行可能性

P0-1〜P2-6の全パッケージについて、次を記載した。

- 目的
- 前提
- 参照設計
- 主な対象
- 実装順
- 不変条件
- Unit / 結合テスト
- 成果物
- ドキュメント更新
- 完了条件
- 非対象

実装エージェントが作業名だけから独自設計する必要を減らし、1〜17章の設計へ戻れる構造にした。

## 25.4 オーバーエンジニアリングの抑制

- Crateは三つ
- Public C ABIは作らない
- JUCEは追加しない
- Realtime Deviceは作らない
- Generic Modulation Frameworkは作らない
- Benchmark / Property Test Frameworkは初期導入しない
- 大量のADRを要求しない
- Voice Loudnessは厳密解析ではなくSteal選択用の簡易推定
- Sampleは一Layer一Asset、One-shot、Mono内部再生
- CLIはValidation / Inspect / Render中心
- 音質問題をMVP外Effectで隠さない

## 25.5 品質確認

- 自動TestはBuffer、Timing、状態遷移、再現性、明確な不連続を確認する。
- Spectrumなどは人間Reviewの参考情報に留める。
- P1 / P2で固定条件のReview Packageを生成する。
- AIは音質を自己承認しない。
- 人間の承認をフェーズ完了条件とする。
- 修正時は同じ入力条件の前後比較を生成する。

## 25.6 Test配置と依存

- Unit Testは対象Rust Moduleと同居
- Public API結合テストは各Crateの`tests/`
- 共有データはWorkspace Rootの`testdata/`
- FFIはRust結合テストとSanitizer
- 補助Libraryは`approx`、`tempfile`、`assert_cmd`、`predicates`、`rustfft`
- Workspace RootにCargo Integration Test用の`tests/`を作らない

## 25.7 ドキュメント

実装中に次を日本語で整備する。

- `README.md`
- `docs/architecture.md`
- `docs/instrument-definition.md`
- `docs/runtime-processing.md`
- `docs/cli.md`
- `docs/testing-and-sound-review.md`
- `THIRD_PARTY_NOTICES.md`
- `testdata/assets/README.md`

各作業パッケージに更新対象を紐づけ、MVP終了時だけまとめて書く構造にしていない。

## 25.8 HTML / Markdown同一性

Markdownを唯一の正本とする。HTMLは同じMarkdown本文を省略せず変換する。HTML固有の要約版は作らない。

生成時に次を検査する。

- 全見出しがHTMLに存在する
- Markdown本文の主要TextがHTMLに存在する
- P0-1〜P2-6の全作業パッケージがHTMLに存在する
- HTML / Markdownの本文量に大きな差がない

## 25.9 最終判断

本書は、Sonalloy Core MVPについて、次の二つを一つの文書で接続している。

1. どのような仕組みにするかを定める詳細設計
2. その設計をどの順番で実装・検証・承認するかを定める実装計画

中心成果はFrameworkや文書量ではない。

> 保存可能なInstrument Definition、安定したVoice / DSP処理、Sample Layerとの融合を通じて、人間が実際に使いたいと思えるHybrid Instrumentを一つ完成させること。

P0〜P2のScopeを超える機能を先回りせず、実装エージェントが各作業を具体的に開始・終了判定できる粒度まで落とし込んだ。
