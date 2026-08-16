# Sonalloy v0.2 Processor Expansion 詳細設計・実装計画

- **対象Repository**：`endo-ly/sonalloy`
- **対象Version**：`v0.2.0` 想定
- **基準Version**：`v0.1.0`
- **正本要件**：`docs/CONCEPT.md`
- **前提実装**：`v0.1.0` 時点のInstrument Definition / Compiler / Dynamic Parameter / Modulation / Processor Chain / Runtime / CLI / Review Package
- **用途**：実装Agentへそのまま渡し、設計判断を追加せず実装を進められる詳細計画
- **文書言語**：日本語。型名、API名、Parameter ID、File Path、JSON fieldのみ英語を使用する
- **成果物**：Markdownのみ。HTML版は作成しない

---

## 目次

1. この計画の位置づけ
2. 現在地と今回の到達点
3. 対象範囲
4. DSP実装方式と依存判断
5. Processor配置と信号経路
6. Instrument Definition
7. Parameter / Modulation契約
8. Compiler契約
9. Runtime共通基盤
10. Filter Mode Expansion
11. EQ
12. Resonator
13. Bitcrusher
14. Chorus
15. Flanger
16. Phaser
17. Compressor
18. Limiter
19. Error / Diagnostic
20. CLI / Inspect / Documentation
21. Test戦略
22. Sound Review Package
23. File単位の変更計画
24. 実装順序
25. 完了条件
26. 次フェーズへ残すもの
27. 実装Agent向け最終ルール
28. 参考資料

---

# 1. この計画の位置づけ

`v0.1.0` では、複数Generator、Sample系Generator、Dynamic Parameter、Modulation、Layer / Voice / Global Processor Chain、Filter / Drive / Delay / Reverb、Offline Renderまでが一つの実行系として成立した。Generator側はOscillator、Noise、Wavetable、Operator Modulation、Additive、Formant、Sample、Granular、Wave Sequence、Spectralまで広がっており、音を発生させる方式はすでに幅広い。

一方、ProcessorはFilter、Drive、Delay、Reverbの4種類に留まる。`docs/CONCEPT.md` が想定するProcessorカテゴリにはTone、Resonance、Digital、Modulation FX、Dynamicsなどがあり、現在はGeneratorの種類に対して加工側の選択肢が少ない。

本フェーズは、新しいGeneratorを増やす前に既存Generator全体へ作用する加工能力を広げる。自由なAudio GraphやPlugin向け機能へ進まず、現在の固定Processor Chainを維持したまま主要カテゴリを一巡させる。

本フェーズ完了時の状態を次の一文で固定する。

> **Sonalloy v0.2は、v0.1で構築した多様なGeneratorを、Filter / Nonlinear / Tone / Resonance / Digital / Modulation FX / Time / Dynamicsの主要カテゴリで加工し、Layer・Voice・Globalの固定位置へ組み合わせて決定的にOffline Renderできる。**

## 1.1 実装判断の優先順位

判断に迷った場合は次の順序を使う。

1. `docs/CONCEPT.md` のProcessor分類と半固定Pipeline
2. 本書で固定するDefinition、配置、Parameter、DSP方式
3. `v0.1.0` のCompiler / Runtime / Parameter / Review契約
4. 音質と人間による試聴結果
5. Realtime SafetyとBlock Size独立性
6. 実装の単純さ
7. 将来のProcessor追加容易性

将来の自由度だけを理由に、Audio Graph、Node Framework、動的Plugin登録、Trait ObjectベースのDSP登録機構、Script DSPを導入しない。

## 1.2 今回の設計原則

今回追加するProcessorは、現在の`ProcessorDefinition -> CompiledProcessor -> ProcessorTargetSpan -> Runtime Chain`を拡張して実装する。Processorごとに独自の別経路を作らない。

DSP Stateは現在と同じ所有単位を維持する。

- Layer Processor State：`Voice × Layer × Processor`
- Voice Processor State：`Voice × Processor`
- Global Processor State：`InstrumentRuntime × Processor`

すべての連続Parameterは既存のParameter Catalog、Parameter Change、Modulation Route、Smoothingを通す。Processor内部で独自の外部制御系を作らない。

---

# 2. 現在地と今回の到達点

## 2.1 `v0.1.0` のProcessor基盤

現在のRuntimeには以下が存在する。

```text
ProcessorDefinition
        ↓
Compiler
        ↓
CompiledProcessorKind
        ↓
Parameter Catalog / Route Resolution
        ↓
ProcessorTargetSpan
        ↓
LayerProcessorChain / StereoProcessorChain
        ↓
Definition順に直列Process
```

`ProcessorTargetSpan`はProcessorごとのBlock内Start / End値を保持し、Layer用ChainとStereo用Chainが`CompiledProcessorKind`と同じ順序でRuntime Stateを持つ。FilterはDaisySP SVF、Drive / Delay / ReverbはRust側で処理する。

今回この構造は維持する。

## 2.2 今回埋めるカテゴリ

| Concept上のカテゴリ | v0.1 | v0.2で追加 |
|---|---|---|
| Filter | Low-pass Filter | High-pass / Band-pass / Notch |
| Nonlinear | Drive | 既存を維持 |
| Tone | なし | 3-band EQ |
| Resonance | なし | Tuned Resonator |
| Digital | なし | Bitcrusher + Sample-rate Reduction |
| Modulation FX | なし | Chorus / Flanger / Phaser |
| Time | Delay / Reverb | 既存を維持 |
| Dynamics | なし | Compressor / Limiter |

この表の主要カテゴリを全て開通させることが今回の目的である。

---

# 3. 対象範囲

## 3.1 実装するProcessor

本フェーズで実装する機能を次へ固定する。

1. Existing `filter` のMode拡張
   - `low_pass`
   - `high_pass`
   - `band_pass`
   - `notch`
2. `eq`
   - Low Shelf
   - Mid Peaking
   - High Shelf
3. `resonator`
   - Fractional Delayを使ったTuned Feedback Resonator
4. `bitcrusher`
   - Quantization
   - Sample-rate Reduction
5. `chorus`
6. `flanger`
7. `phaser`
8. `compressor`
9. `limiter`

## 3.2 Placement

配置可能箇所を次へ固定する。

| Processor | Layer | Voice | Global |
|---|:---:|:---:|:---:|
| Filter | ○ | ○ | ○ |
| Drive | ○ | ○ | ○ |
| EQ | ○ | ○ | ○ |
| Resonator | ○ | ○ | × |
| Bitcrusher | ○ | × | × |
| Chorus | × | × | ○ |
| Flanger | × | × | ○ |
| Phaser | × | × | ○ |
| Delay | × | × | ○ |
| Reverb | × | × | ○ |
| Compressor | × | ○ | ○ |
| Limiter | × | ○ | ○ |

配置は`docs/CONCEPT.md`のLayer / Voice / Globalの責務に合わせる。使い勝手だけを理由に配置範囲を広げない。

## 3.3 本フェーズで扱わない機能

以下はProcessor機能として重要だが、本フェーズの外へ置く。

| 機能 | 後続へ送る理由 |
|---|---|
| Ladder Filter | 別の非線形Filter方式として独立して音質評価した方がよい |
| Formant Processor | Formant Generatorとは別にFilter Bank設計が必要 |
| Frequency Shifter | Analytic Signal / Hilbert処理とLatency設計を伴う |
| Convolution | IR Asset、Partition、Latency、Memory契約が必要 |
| Gate | Dynamics拡張としてCompressor / Limiter後に追加可能 |
| Transient Shaper | Envelope分離方式の音質判断が必要 |
| Multi-tap / Tempo Sync Delay | 既存Delayの別フェーズで扱う |
| Reverb Freeze / Size Automation | 既存Reverb拡張として扱う |
| Vocoder / Cross Synthesis | External Audio Input Contractが先に必要 |
| Input Processing | External Audio Input Contractが先に必要 |
| Sidechain | Input Bus / Routing契約が先に必要 |
| Processor並列分岐 | Sonalloyの半固定Pipelineから外れる |
| Send / Return | Sonalloyの半固定Pipelineから外れる |
| RuntimeでのProcessor追加・削除・並び替え | Hot Swap / Recompile側の責務 |

本フェーズの完了条件へ上記を混ぜない。

---

# 4. DSP実装方式と依存判断

## 4.1 結論

| 機能 | 実装方式 | 理由 |
|---|---|---|
| Filter Mode | 既存DaisySP `Svf`を継続利用 | Low / High / Band / Notch出力を同一Stateから取得できる |
| EQ | Rust独自実装 | 固定3-bandのBiquadで十分。新Dependency不要 |
| Resonator | Rust独自実装 | 既存Delay / Buffer所有契約と自然に統合できる |
| Bitcrusher | Rust独自実装 | Quantizer + Sample/Holdは小規模で、Native境界を増やす利点がない |
| Chorus | Rust独自実装 | Sample Rate依存BufferをSonalloy側で正確に所有したい |
| Flanger | Rust独自実装 | Chorusと共通のFractional Delay基盤を使える |
| Phaser | Rust独自実装 | 小規模なAll-pass Cascadeで完結する |
| Compressor | Rust独自実装 | Stereo Link、Parameter、Attack / Release契約をSonalloy側で固定したい |
| Limiter | Rust独自実装 | Zero-latency、Stereo Link、Ceiling保証を明示的に実装する |
| 新規Rust Crate | 追加しない | 全対象が現依存だけで実装可能 |
| DaisySP Build対象 | `svf.cpp`等の現状を維持 | 新しいDaisySP moduleはBuildへ追加しない |

## 4.2 DaisySPを追加利用しない理由

現在のDaisySP固定版にはMIT側にChorus、Flanger、Phaser、Decimator、Limiterが存在する。一方、Bitcrush、Compressor、Comb等はライセンス分離された側に存在するため、本フェーズでそれらを直接採用しない。

MIT側のChorus等についても、Sonalloyは44.1 / 48 / 96 kHzを同じ契約で検証する。固定長Delayを内部に持つ既存ModuleへRuntime MemoryとSample Rateの意味を委譲するより、現在のRust Delayと同じくSample Rateから必要BufferをPrepare時に確定した方が、Block Size独立性、Reset、Memory算出、将来Realtime Safetyを管理しやすい。

この判断はDaisySPを避ける一般方針ではない。既存Filter / Oscillator / WavefolderではDaisySPを継続利用する。Physical / Modal等の後続フェーズでは再度DaisySPを候補に含める。

## 4.3 外部License

本フェーズでは新しい外部Dependencyを追加しないため、`THIRD_PARTY_NOTICES.md`へ新規Libraryの追記は発生しない。

DaisySPの使用Sourceも現在のMIT側Sourceのままとする。Filter Mode追加は既にBuild対象である`Source/Filters/svf.cpp`の利用方法だけを拡張する。

---

# 5. Processor配置と信号経路

## 5.1 全体Pipeline

信号順序は`v0.1.0`から変更しない。

```text
Per Voice
  ├─ Layer Generator
  ├─ Layer Processor Chain
  ├─ Layer Amplitude Envelope
  ├─ Layer Gain / Pan
  ├─ Layer Mix
  ├─ Voice Processor Chain
  └─ Voice Steal Fade
        ↓
Voice Sum
        ↓
Global Processor Chain
        ↓
Stereo Output
```

## 5.2 Layer Processor

Layer ProcessorはGenerator直後へ配置する。

```text
Generator
  ↓
Filter / Drive / EQ / Resonator / Bitcrusher
  ↓
Amplitude Envelope
  ↓
Gain / Pan
```

ResonatorのTailもLayer Envelopeの内側にあるため、Note終了後はLayer Envelopeに従って消える。このPhaseではLayer Resonatorを独立したTail Sourceとして扱わない。

## 5.3 Voice Processor

```text
Layer Mix
  ↓
Filter / Drive / EQ / Resonator / Compressor / Limiter
  ↓
Voice Steal Fade
```

Compressor / LimiterはStereo Linkで動作し、左右のGain Reductionを共通にする。Stereo ImageをDynamics処理で揺らさない。

## 5.4 Global Processor

```text
Voice Sum
  ↓
Filter / Drive / EQ
  ↓
Chorus / Flanger / Phaser
  ↓
Delay / Reverb
  ↓
Compressor / Limiter
```

上記は推奨例であり、実際の順序はDefinition配列順である。CompilerはTypeによる並べ替えを行わない。

Global ChainはActive Voiceが0件でも既存契約どおり実行する。Chorus / Flanger内部Delay、Compressor / LimiterのRelease Stateも同じ処理経路で進む。

---

# 6. Instrument Definition

## 6.1 `ProcessorDefinition`

現在の`ProcessorDefinition`へ次を追加する。

```rust
pub enum ProcessorDefinition {
    Filter(FilterProcessorDefinition),
    Drive(DriveProcessorDefinition),
    Eq(EqProcessorDefinition),
    Resonator(ResonatorProcessorDefinition),
    Bitcrusher(BitcrusherProcessorDefinition),
    Chorus(ChorusProcessorDefinition),
    Flanger(FlangerProcessorDefinition),
    Phaser(PhaserProcessorDefinition),
    Delay(DelayProcessorDefinition),
    Reverb(ReverbProcessorDefinition),
    Compressor(CompressorProcessorDefinition),
    Limiter(LimiterProcessorDefinition),
}
```

JSONは現在と同じ`#[serde(tag = "type", rename_all = "snake_case")]`を維持する。

## 6.2 Schema Version

`CURRENT_SCHEMA_VERSION`は本フェーズでは`1`を維持する。

理由は、新Processor Typeの追加が既存Definitionの意味を変更せず、Filter Modeも既存のFilterが暗黙にLow-passだった意味をそのままDefaultへできるためである。

Filterへ追加する`mode`だけは次の扱いにする。

```rust
#[serde(default)]
pub mode: FilterModeDefinition
```

`FilterModeDefinition::default()`は`LowPass`とする。新規Serializerは`mode`を明示して出力する。旧形式専用のMigration、Deprecated Field、Schema分岐は作らない。

## 6.3 Filter

```json
{
  "type": "filter",
  "id": "tone",
  "mode": "low_pass",
  "cutoff_hz": 8000.0,
  "resonance": 0.2
}
```

```rust
pub enum FilterModeDefinition {
    LowPass,
    HighPass,
    BandPass,
    Notch,
}
```

`mode`はCompile時固定。`cutoff_hz`と`resonance`は既存どおりDynamic。

## 6.4 EQ

EQは固定3-bandとする。自由Band配列は導入しない。同じEQ Processorを複数並べることで6-band以上を構成できる。

```json
{
  "type": "eq",
  "id": "tone_eq",
  "low_frequency_hz": 120.0,
  "low_gain_db": 2.0,
  "mid_frequency_hz": 1200.0,
  "mid_gain_db": -3.0,
  "mid_q": 1.0,
  "high_frequency_hz": 8000.0,
  "high_gain_db": 1.5
}
```

| Field | Range | Dynamic | 意味 |
|---|---:|:---:|---|
| `low_frequency_hz` | 20〜500 Hz | No | Low Shelf midpoint |
| `low_gain_db` | -24〜+24 dB | Yes | Low Shelf gain |
| `mid_frequency_hz` | 100〜12000 Hz | No | Peaking EQ center |
| `mid_gain_db` | -24〜+24 dB | Yes | Peaking EQ gain |
| `mid_q` | 0.25〜8.0 | No | Peaking EQ Q |
| `high_frequency_hz` | 2000〜20000 Hz | No | High Shelf midpoint |
| `high_gain_db` | -24〜+24 dB | Yes | High Shelf gain |

追加Validation：

```text
low_frequency_hz < mid_frequency_hz < high_frequency_hz
```

かつ各FrequencyはCompile対象Sample Rateの安全上限以下であること。

## 6.5 Resonator

```json
{
  "type": "resonator",
  "id": "body_resonance",
  "frequency_hz": 440.0,
  "decay_seconds": 1.2,
  "damping": 0.35,
  "mix": 0.4
}
```

| Field | Range | Dynamic | 意味 |
|---|---:|:---:|---|
| `frequency_hz` | 40〜12000 Hz | Yes | 共鳴基本周波数 |
| `decay_seconds` | 0.02〜10.0 s | Yes | おおよそのT60 |
| `damping` | 0〜1 | Yes | Feedback Loopの高域減衰 |
| `mix` | 0〜1 | Yes | Dry / Wet |

## 6.6 Bitcrusher

```json
{
  "type": "bitcrusher",
  "id": "digital_grit",
  "bit_depth": 8.0,
  "sample_rate_ratio": 0.25,
  "mix": 0.5
}
```

| Field | Range | Dynamic | 意味 |
|---|---:|:---:|---|
| `bit_depth` | 2〜16 | Yes | Quantization解像度。小さいほど粗い |
| `sample_rate_ratio` | 0.01〜1.0 | Yes | 1.0が元Sample Rate、0.5が半分相当 |
| `mix` | 0〜1 | Yes | Dry / Wet |

`bit_depth`は`f32`として扱う。Quantization level数は`2^bit_depth`から計算し、整数Bitへの丸めを行わない。これによりParameter Ramp中も不連続な設定切替を避ける。

## 6.7 Chorus

```json
{
  "type": "chorus",
  "id": "wide_chorus",
  "delay_ms": 15.0,
  "rate_hz": 0.35,
  "depth": 0.65,
  "feedback": 0.1,
  "width": 0.8,
  "mix": 0.3
}
```

| Field | Range | Dynamic | 意味 |
|---|---:|:---:|---|
| `delay_ms` | 5〜30 ms | No | Modulated Delayの中心値 |
| `rate_hz` | 0.01〜8 Hz | Yes | LFO速度 |
| `depth` | 0〜1 | Yes | Delay変調幅 |
| `feedback` | 0〜0.85 | Yes | Delay Feedback |
| `width` | 0〜1 | Yes | 左右LFO位相差 |
| `mix` | 0〜1 | Yes | Dry / Wet |

## 6.8 Flanger

```json
{
  "type": "flanger",
  "id": "jet",
  "delay_ms": 2.0,
  "rate_hz": 0.25,
  "depth": 0.8,
  "feedback": 0.55,
  "width": 0.5,
  "mix": 0.45
}
```

| Field | Range | Dynamic | 意味 |
|---|---:|:---:|---|
| `delay_ms` | 0.5〜10 ms | No | 中心Delay |
| `rate_hz` | 0.01〜10 Hz | Yes | LFO速度 |
| `depth` | 0〜1 | Yes | Delay変調幅 |
| `feedback` | -0.95〜0.95 | Yes | 正負Feedback |
| `width` | 0〜1 | Yes | 左右LFO位相差 |
| `mix` | 0〜1 | Yes | Dry / Wet |

## 6.9 Phaser

```json
{
  "type": "phaser",
  "id": "motion",
  "stages": 6,
  "center_hz": 900.0,
  "sweep_octaves": 3.0,
  "rate_hz": 0.3,
  "depth": 0.8,
  "feedback": 0.4,
  "width": 0.7,
  "mix": 0.5
}
```

| Field | Range | Dynamic | 意味 |
|---|---:|:---:|---|
| `stages` | 2 / 4 / 6 / 8 | No | All-pass段数 |
| `center_hz` | 100〜5000 Hz | No | Sweep中心周波数 |
| `sweep_octaves` | 0.25〜6 oct | No | Sweep幅 |
| `rate_hz` | 0.01〜8 Hz | Yes | LFO速度 |
| `depth` | 0〜1 | Yes | Sweep量 |
| `feedback` | -0.9〜0.9 | Yes | Phaser Feedback |
| `width` | 0〜1 | Yes | 左右LFO位相差 |
| `mix` | 0〜1 | Yes | Dry / Wet |

Compilerは`center_hz * 2^(sweep_octaves / 2)`が`0.45 * sample_rate`未満になることを検証する。

## 6.10 Compressor

```json
{
  "type": "compressor",
  "id": "glue",
  "threshold_db": -18.0,
  "ratio": 4.0,
  "attack_ms": 15.0,
  "release_ms": 180.0,
  "knee_db": 6.0,
  "makeup_gain_db": 2.0,
  "mix": 1.0
}
```

| Field | Range | Dynamic | 意味 |
|---|---:|:---:|---|
| `threshold_db` | -60〜0 dB | Yes | 圧縮開始Level |
| `ratio` | 1〜20 | Yes | Compression Ratio |
| `attack_ms` | 0.1〜200 ms | No | Gain ReductionのAttack |
| `release_ms` | 5〜2000 ms | No | Release |
| `knee_db` | 0〜24 dB | No | Soft Knee幅 |
| `makeup_gain_db` | -12〜24 dB | Yes | 後段Gain |
| `mix` | 0〜1 | Yes | Parallel Compression |

## 6.11 Limiter

```json
{
  "type": "limiter",
  "id": "ceiling",
  "ceiling_db": -1.0,
  "release_ms": 80.0,
  "input_gain_db": 0.0
}
```

| Field | Range | Dynamic | 意味 |
|---|---:|:---:|---|
| `ceiling_db` | -12〜0 dBFS | Yes | 出力Peak上限 |
| `release_ms` | 5〜1000 ms | No | Gainが1へ戻る時間 |
| `input_gain_db` | -24〜24 dB | Yes | Limiter前Gain |

Lookaheadは使用しない。Intrinsic Latencyは0 frameとする。

---

# 7. Parameter / Modulation契約

## 7.1 Parameter ID

既存形式を維持する。

```text
Layer:  layer.<layer_id>.processor.<processor_id>.<parameter>
Voice:  voice.processor.<processor_id>.<parameter>
Global: global.processor.<processor_id>.<parameter>
```

EQも固定3-bandなのでNested IDを導入しない。

### EQ

```text
...processor.<id>.low_gain_db
...processor.<id>.mid_gain_db
...processor.<id>.high_gain_db
```

### Resonator

```text
...frequency_hz
...decay_seconds
...damping
...mix
```

### Bitcrusher

```text
...bit_depth
...sample_rate_ratio
...mix
```

### Chorus / Flanger

```text
...rate_hz
...depth
...feedback
...width
...mix
```

### Phaser

```text
...rate_hz
...depth
...feedback
...width
...mix
```

### Compressor

```text
...threshold_db
...ratio
...makeup_gain_db
...mix
```

### Limiter

```text
...ceiling_db
...input_gain_db
```

## 7.2 Static Field

次はParameter Catalogへ登録しない。

- Filter `mode`
- EQ `low_frequency_hz`, `mid_frequency_hz`, `mid_q`, `high_frequency_hz`
- Chorus / Flanger `delay_ms`
- Phaser `stages`, `center_hz`, `sweep_octaves`
- Compressor `attack_ms`, `release_ms`, `knee_db`
- Limiter `release_ms`

変更にはDefinition再Compileを必要とする。

## 7.3 Smoothing

新しいDynamic Parameterは既存のProcessor Parameterと同じSmoothing契約を使う。Processor内部で別のParameter Smootherを追加しない。

ただしDynamicsのAttack / ReleaseはParameter SmoothingではなくDSP Stateであり、Definitionに指定されたDetector / Gain Reductionの時間定数として扱う。

## 7.4 Scope Validation

既存規則を拡張する。

- Layer / VoiceのTargetにはVoice Scope Sourceを接続可能
- Global TargetへVoice Scope Sourceを接続不可
- External Control由来のParameter ChangeはLayer / Voice / Globalへ適用可能

Global Chorus等へ`velocity`やVoice LFOを直接Routeしてはならない。Instrument Scope Sourceは今回追加しない。

---

# 8. Compiler契約

## 8.1 `CompiledProcessorKind`

```rust
pub enum CompiledProcessorKind {
    Filter(CompiledFilterProcessor),
    Drive(CompiledDriveProcessor),
    Eq(CompiledEqProcessor),
    Resonator(CompiledResonatorProcessor),
    Bitcrusher(CompiledBitcrusherProcessor),
    Chorus(CompiledChorusProcessor),
    Flanger(CompiledFlangerProcessor),
    Phaser(CompiledPhaserProcessor),
    Delay(CompiledDelayProcessor),
    Reverb(CompiledReverbProcessor),
    Compressor(CompiledCompressorProcessor),
    Limiter(CompiledLimiterProcessor),
}
```

## 8.2 Compiled State

CompilerはRuntimeで文字列、単位変換、Sample Rate依存の時間計算を行わないよう、以下を事前計算する。

### Filter

```rust
CompiledFilterProcessor {
    mode,
    parameters,
    effective_max_cutoff_hz,
}
```

### EQ

```rust
CompiledEqProcessor {
    low_frequency_hz,
    mid_frequency_hz,
    mid_q,
    high_frequency_hz,
    parameters,
}
```

Static Frequencyに対応する`sin` / `cos`等をCompile時ではなくRuntime作成時にSample Rateと合わせて計算してよい。Compiled InstrumentへRuntime Stateを入れない。

### Resonator

```rust
CompiledResonatorProcessor {
    parameters,
    max_delay_frames,
}
```

`max_delay_frames = ceil(sample_rate / 40) + interpolation_guard`

`interpolation_guard`は4-point cubic readに必要な余白を含める。

### Bitcrusher

Parameter Handleのみ。追加Heap Memoryなし。

### Chorus

```rust
CompiledChorusProcessor {
    delay_frames,
    max_delay_frames,
    parameters,
}
```

`max_delay_frames`は30 ms中心 + 0.9倍の最大Modulation幅 + Cubic interpolation guardをSample Rateから算出する。

### Flanger

同様に10 ms中心 + 最大Modulation幅から算出。

### Phaser

```rust
CompiledPhaserProcessor {
    stages,
    center_hz,
    sweep_octaves,
    parameters,
}
```

### Compressor

Attack / Release coefficientをSample Rateから事前計算する。

```text
attack_coeff  = exp(-1 / (attack_seconds  * sample_rate))
release_coeff = exp(-1 / (release_seconds * sample_rate))
```

### Limiter

Release coefficientを同様に事前計算する。

## 8.3 Placement Validation

`validate_processor_chain`をTypeごとのPlacement Matrixに置き換える。

現在のような`Delay | Reverb`だけの個別条件を増殖させない。次のPrivate関数へ集約する。

```rust
fn processor_allowed_at(
    processor: &ProcessorDefinition,
    placement: ProcessorPlacement,
) -> bool
```

Validation ErrorにはProcessor Type、ID、要求Placement、許可Placementが分かるMessageを返す。

## 8.4 Parameter登録

ProcessorごとにParameter登録関数を分離する。

```text
compile_filter_processor_parameters
compile_eq_processor_parameters
compile_resonator_processor_parameters
...
```

巨大な一つの`match`内でParameter Descriptor生成を繰り返さない。ただし新しいTraitやRegistryは導入しない。単純なPrivate関数で十分である。

## 8.5 Resource / Memory

Process中のHeap拡張を避けるため、Ring BufferはRuntime生成時に全容量を確保する。

想定最大MemoryをCompile時に計算可能な形へする。

- Resonator：`max_delay_frames × channel_count × sizeof(f32)`
- Chorus：`max_delay_frames × 2 × sizeof(f32)`
- Flanger：同様
- Phaser / EQ / Dynamics / Bitcrusher：固定小State

本フェーズで新しい公開Resource Budget Fieldは追加しない。ただし`instrument inspect --json`で既にCompiled構成を表示する箇所には新ProcessorのType / Static Field / Parameterを含める。

---

# 9. Runtime共通基盤

## 9.1 `ProcessorTargetSpan`

現行の固定Size / Copy可能な構造を維持する。新しいHeap所有型を入れない。

```rust
pub(crate) enum ProcessorTargetSpan {
    Filter { cutoff, resonance },
    Drive { amount, mix },
    Eq { low_gain, mid_gain, high_gain },
    Resonator { frequency, decay, damping, mix },
    Bitcrusher { bit_depth, sample_rate_ratio, mix },
    Chorus { rate, depth, feedback, width, mix },
    Flanger { rate, depth, feedback, width, mix },
    Phaser { rate, depth, feedback, width, mix },
    Delay { feedback, mix },
    Reverb { decay, damping, width, mix },
    Compressor { threshold, ratio, makeup_gain, mix },
    Limiter { ceiling, input_gain },
}
```

`zero_for()`と`clear()`は全Variantを必ず更新する。Processor追加時にTarget初期化漏れが起きないことをUnit Testで検証する。

## 9.2 Runtime Type

`runtime/processor/`へProcessorごとのModuleを追加する。

```text
runtime/processor/
├─ mod.rs
├─ delay.rs
├─ drive.rs
├─ reverb.rs
├─ eq.rs
├─ fractional_delay.rs
├─ resonator.rs
├─ bitcrusher.rs
├─ chorus.rs
├─ flanger.rs
├─ phaser.rs
├─ compressor.rs
└─ limiter.rs
```

`fractional_delay.rs`はResonator / Chorus / Flangerだけが使うPrivate Utilityとする。公開APIへ出さない。

## 9.3 `LayerProcessorRuntime`

```rust
Filter
Drive
Eq
Resonator
Bitcrusher
```

Layer ConstructorはGenerator Output Modeを受け取るよう変更する。

```rust
LayerProcessorChain::new(
    processors,
    spec,
    generator_output_mode,
)
```

目的はResonator等のBufferをMono Generatorへ不要に2channel分確保しないことである。

既存FilterのLeft / Right Handle所有は現在の構造を維持してよい。

## 9.4 `StereoProcessorRuntime`

```rust
Filter
Drive
Eq
Resonator
Chorus
Flanger
Phaser
Delay
Reverb
Compressor
Limiter
```

Voice ChainとGlobal Chainは同じ`StereoProcessorChain`を利用し続ける。Placement ValidationによりVoiceへGlobal専用Processorが到達しない。

## 9.5 Process Error

Rust独自Processorは次の場合に`ProcessError::ProcessorFailure`へ統合する。

- RuntimeとTarget Variantが一致しない
- Internal Buffer Stateが破損している
- 非有限値が検出された
- Native Filter境界が失敗した

通常のParameter Range違反はCompileで拒否するため、Audio Pathで文字列付きValidationを行わない。

## 9.6 Finite Guard

各ProcessorのProcess完了後に全Bufferを毎回走査する共通Guardは追加しない。CPU負荷を増やすためである。

代わりに、各DSP計算で非有限値が生成されうる境界を明示的に守る。

- `log10`入力へepsilon
- `powf`へClamp済み範囲
- Fractional Delay read indexをBuffer範囲へNormalize
- Feedback値をValidation範囲へ限定
- Biquad coefficientがFiniteかRuntime作成時 / Dynamic gain計算時に確認

Testでは出力全体のFinite性を必ず検査する。

---

# 10. Filter Mode Expansion

## 10.1 Backend

既存DaisySP `Svf`を継続する。`Svf::Process()`後の出力だけをModeに応じて選ぶ。

```text
low_pass  -> Svf::Low()
high_pass -> Svf::High()
band_pass -> Svf::Band()
notch     -> Svf::Notch()
```

## 10.2 Native API

C HeaderへFilter Mode enumを追加する。

```c
enum sonalloy_dsp_filter_mode {
    SONALLOY_DSP_FILTER_LOW_PASS = 0,
    SONALLOY_DSP_FILTER_HIGH_PASS = 1,
    SONALLOY_DSP_FILTER_BAND_PASS = 2,
    SONALLOY_DSP_FILTER_NOTCH = 3
};
```

既存Filter process APIへ`mode`引数を追加する。

```c
sonalloy_dsp_filter_process(handle, mode, cutoff, resonance, buffer, frames)
sonalloy_dsp_filter_process_ramp(handle, mode, ...)
sonalloy_dsp_filter_process_ramp_with_resonance(handle, mode, ...)
```

これはSonalloy内部Native ABIでありPublic C ABIではないため、旧Signatureを残さない。

## 10.3 Rust DSP Boundary

`sonalloy-dsp-sys`へ次を追加する。

```rust
pub enum DspFilterMode {
    LowPass,
    HighPass,
    BandPass,
    Notch,
}
```

`DspFilter::process*`はModeを受け取る。

Native Error時のBuffer無音化、Handle所有、Reset、Fault Injection契約は現行Filterと同じにする。

## 10.4 Test

- Low-pass既存結果の回帰
- High-passで低周波が抑制され高周波が残る
- Band-passでCutoff近傍が残る
- NotchでCutoff近傍が減衰する
- 4 ModeすべてでRampがFinite
- 44.1 / 48 / 96 kHz
- Native FaultでBuffer無音化
- Invalid Modeで`INVALID_ARGUMENT`

---

# 11. EQ

## 11.1 構造

固定3-bandを直列Biquadとして処理する。

```text
Input
  ↓
Low Shelf
  ↓
Mid Peaking
  ↓
High Shelf
  ↓
Output
```

各channelは独立したBiquad Stateを持ち、Parameter値は左右共通とする。

## 11.2 Biquad

CoefficientはW3C Audio EQ Cookbook / Robert Bristow-Johnson式を基準とする。

Low Shelf / High Shelfのslope `S`は`1.0`に固定する。

- Low Shelf：`low_frequency_hz`, `low_gain_db`
- Peaking：`mid_frequency_hz`, `mid_q`, `mid_gain_db`
- High Shelf：`high_frequency_hz`, `high_gain_db`

StateはTransposed Direct Form IIを使う。

```text
y = b0*x + z1
z1 = b1*x - a1*y + z2
z2 = b2*x - a2*y
```

`a0`で正規化済みCoefficientを保持する。

## 11.3 Dynamic Gain

Frequency / QはStaticなので、`sin(w0)` / `cos(w0)`等のFrequency依存値はRuntime生成時に計算して保持する。

GainだけがDynamicであり、各sampleでValueSpanから現在Gainを線形補間し、そのGainに対応する`A = 10^(gain_db / 40)`からCoefficientを更新する。

Performance上の最適化として、Gain SpanのStartとEndが同一ならBlock内Coefficientを一回だけ計算する。異なる場合だけsample単位で更新する。

この最適化は出力のBlock Size依存を生まない。

## 11.4 Identity

3つのGainがすべて0 dBなら入力と十分近い出力になること。

目標：

```text
max_abs_error < 1e-5
```

Biquad Stateが残るため、Parameterを0へ戻した瞬間の完全Identityではなく、最初から0 dBのFresh Runtimeで検証する。

## 11.5 Reset

全channel、全bandの`z1`, `z2`を0へ戻す。

---

# 12. Resonator

## 12.1 目的

Oscillator / Noise / Sample / Spectral等へ、明確なPitchを持つ共鳴を付加する。Physical / Modal Generatorそのものを実装するPhaseではない。

## 12.2 Algorithm

Feedback Delayを基本とする。

```text
input ────────────────┐
                      ├─ Dry/Wet Mix → output
input + feedback ─→ Delay → Damping LPF ─┘
                      ↑                 │
                      └──── feedback ───┘
```

Delay length：

```text
delay_samples = sample_rate / frequency_hz
```

Fractional部分は4-point cubic interpolationで読む。既存`cubic_interpolate()`を再利用する。

## 12.3 Decay

`decay_seconds`をおおよそのT60としてFeedback Gainを求める。

```text
loop_period_seconds = 1 / frequency_hz
feedback_gain = 10 ^ (-3 * loop_period_seconds / decay_seconds)
```

Validation範囲内で必ず`0 <= feedback_gain < 1`となること。

## 12.4 Damping

Feedback Loopへ1-pole Low-passを置く。

`damping = 0`で高域保持、`1`で強い高域減衰となるようCutoffへ変換する。

```text
max_cutoff = min(18000, sample_rate * 0.45)
cutoff = 200 + (1 - damping)^2 * (max_cutoff - 200)
a = exp(-2*pi*cutoff/sample_rate)
y = (1-a)*x + a*state
```

## 12.5 Stereo

Stereo入力では左右独立のDelay / Damping Stateを使う。Cross Feedbackは導入しない。

Mono Layerでは1channel分だけ確保する。

## 12.6 Frequency Modulation

`frequency_hz`はDynamic。ValueSpanをsample単位に展開してDelay read位置を変える。

Read Positionを突然整数Sampleへ量子化せずFractional interpolationするため、緩やかなModulationではPitch Glideとして動作する。

極端なAudio-rate Modulation品質は本フェーズの保証対象にしないが、Finite性とClick抑制は満たす。

---

# 13. Bitcrusher

## 13.1 Processing順序

```text
Input
  ↓
Sample-rate Reduction (Sample & Hold)
  ↓
Quantization
  ↓
Dry/Wet Mix
```

## 13.2 Sample-rate Reduction

各channelに`phase`と`held_sample`を持つ。

`sample_rate_ratio = 1`なら毎Sample更新する。

一般形：

```text
phase += sample_rate_ratio
if phase >= 1:
    phase -= 1
    held_sample = input
wet = held_sample
```

ratioが非常に小さくてもProcess Block境界でphaseをResetしない。

## 13.3 Quantization

```text
clamped = clamp(wet, -1, 1)
levels = 2 ^ bit_depth
quantized = round(clamped * (levels / 2 - 1)) / (levels / 2 - 1)
```

Denominatorが0にならないよう`bit_depth >= 2`をCompileで保証する。

Inputが±1を超えている場合、Wet経路のみ±1へClampする。Dry経路は元入力を保持する。

## 13.4 Stereo

左右は`phase`を共有し、`held_sample`だけ左右個別とする。同じ時刻にSampleを更新しStereo Imageを保つ。

## 13.5 Reset

`phase = 0`、held sampleを0へ戻す。

---

# 14. Chorus

## 14.1 Algorithm

左右独立のFractional DelayとSine LFOを使う。

```text
wet_delay_ms = delay_ms * (1 + 0.9 * depth * lfo)
```

`depth=1`でもDelayが負にならない。

LFO：

```text
left_phase  = phase
right_phase = phase + pi * width
```

`width=0`では左右同位相、`width=1`では180度差。

## 14.2 Feedback

```text
delay_input = input + delayed * feedback
```

Feedbackは0〜0.85。負FeedbackはChorusでは扱わない。

## 14.3 Mix

```text
output = dry * (1 - mix) + wet * mix
```

既存ProcessorのMix意味と合わせてLinear Dry / Wetとする。

## 14.4 State

- Ring Buffer L/R
- Write index
- LFO phase

LFO phaseはInstrument Runtime Resetで0へ戻す。Offline Renderの再現性を保証する。

## 14.5 Buffer

最大DelayはDefinitionの`delay_ms`と最大DepthからCompile時に算出する。96 kHzでも同じms範囲を維持する。

---

# 15. Flanger

## 15.1 Chorusとの差

同じFractional Delay基盤を使うが、Delay時間を短くし、正負Feedbackを許可する。

```text
wet_delay_ms = delay_ms * (1 + 0.95 * depth * lfo)
```

## 15.2 Feedback

`-0.95..0.95`を許可する。負FeedbackでNotch配置が変わることを意図した仕様とする。

## 15.3 State / Reset

Chorusと同じ種類のStateを持つが、Runtime型は別にする。巨大な共通「ModulationEffect」enumを新設しない。

Private `FractionalDelayLine`だけを共有する。

---

# 16. Phaser

## 16.1 Algorithm

1st-order All-passを2 / 4 / 6 / 8段直列にする。

1段のTransferは次を使用する。

```text
H(z) = (a + z^-1) / (1 + a z^-1)
```

処理式：

```text
y[n] = a*x[n] + x[n-1] - a*y[n-1]
```

係数：

```text
g = tan(pi * frequency_hz / sample_rate)
a = (1 - g) / (1 + g)
```

## 16.2 Sweep

```text
lfo = sin(phase)
mod = depth * lfo
frequency = center_hz * 2 ^ ((sweep_octaves / 2) * mod)
```

左右のphase差はChorusと同じ`pi * width`。

## 16.3 Feedback

All-pass CascadeのWet出力をInputへ戻す。

```text
stage_input = dry + last_wet * feedback
```

`feedback`を-0.9..0.9へ制限する。

## 16.4 Stereo

左右は独立All-pass State。Parameterは共通だがLFO phaseだけwidthにより異なる。

## 16.5 Reset

- 全Stageのinput / output history = 0
- feedback history = 0
- LFO phase = 0

---

# 17. Compressor

## 17.1 Detector

Voice / GlobalともStereo Linkする。

```text
level = max(abs(left), abs(right), EPSILON)
level_db = 20 * log10(level)
```

左右へ別々のDetectorを持たない。

## 17.2 Static Curve

`x = level_db - threshold_db`とする。

Hard Knee：

```text
x <= 0 : reduction_db = 0
x > 0  : reduction_db = -(1 - 1/ratio) * x
```

Soft Knee：`knee_db > 0`の場合、`-knee/2 .. +knee/2`を二次曲線で補間する。

```text
x < -k/2:
    reduction = 0
x > +k/2:
    reduction = -(1 - 1/ratio) * x
otherwise:
    reduction = -(1 - 1/ratio) * (x + k/2)^2 / (2*k)
```

## 17.3 Attack / Release

`gain_reduction_db`をStateとして持つ。

Targetが現在値より小さい、つまり圧縮量が増える方向ではAttack coefficientを使う。圧縮量が減る方向ではRelease coefficientを使う。

```text
state = coeff * state + (1 - coeff) * target
```

## 17.4 Makeup / Mix

```text
wet_gain = db_to_linear(state_reduction_db + makeup_gain_db)
wet_l = in_l * wet_gain
wet_r = in_r * wet_gain
out = dry*(1-mix) + wet*mix
```

## 17.5 Latency

Lookaheadを使わないため0 frame。

## 17.6 Test基準

- Ratio 1:1でThresholdに関係なくMakeup以外はIdentity
- 十分長い定常Sineで理論Gain Reductionへ収束
- 左だけ大きいStereo入力でも左右共通Gainが適用される
- Attackを短くするとTransient抑制が早くなる
- Releaseを長くするとGain復帰が遅くなる

---

# 18. Limiter

## 18.1 目的

透明なMastering Limiterではなく、Sonalloy Instrument内でPeakを明確なCeilingへ抑えるZero-latency Peak Limiterとする。

## 18.2 Process

まずInput Gainを適用する。

```text
pre_l = in_l * db_to_linear(input_gain_db)
pre_r = in_r * db_to_linear(input_gain_db)
peak = max(abs(pre_l), abs(pre_r))
ceiling = db_to_linear(ceiling_db)
```

Target Gain：

```text
if peak <= ceiling:
    target_gain = 1
else:
    target_gain = ceiling / peak
```

Attackは即時。

```text
if target_gain < current_gain:
    current_gain = target_gain
else:
    current_gain = release_coeff * current_gain + (1-release_coeff)
```

出力：

```text
out_l = pre_l * current_gain
out_r = pre_r * current_gain
```

## 18.3 Ceiling保証

InputがFiniteである限り、Process後Peakは`ceiling_linear + floating tolerance`以下にする。

Limiter後に別Processorを置けばそのProcessorでPeakが増えることは許容される。Ceiling保証はLimiter自身の出力に対する契約である。

## 18.4 Stereo Link

左右共通Gainを使う。

## 18.5 Reset

`current_gain = 1.0`。

---

# 19. Error / Diagnostic

## 19.1 Definition Validation

新Processorは既存`INVALID_PROCESSOR`系Diagnostic体系へ統合する。ProcessorごとにDiagnostic Codeを大量追加せず、既存CodeとPath / Messageで具体化する。

最低限、以下をCompile前に検出する。

- ID空文字 / 不正文字
- 同一Chain内Processor ID重複
- Field非Finite
- Field Range違反
- Placement違反
- EQ Frequency順序違反
- Sample Rateに対してEQ / Phaser Frequencyが高すぎる
- Phaser stagesが2 / 4 / 6 / 8以外

## 19.2 Compile-time Sample Rate Validation

Definition単体では許容される値でも、対象Sample Rateで安全に処理できない場合はCompile Errorとする。

対象：

- Filter cutoff上限
- EQ high frequency
- Phaser sweep upper frequency

## 19.3 Runtime Failure

Runtime内部Stateの不整合は既存`ProcessorFailureKind::InvalidState`へ統合する。

Native Filter Errorは現在の`ProcessError::from_filter_error`経路を維持する。

---

# 20. CLI / Inspect / Documentation

## 20.1 新CLI Command

追加しない。

既存Commandだけで新Processorが利用できることを完了条件とする。

```text
instrument init
instrument validate
instrument inspect
render note
render events
render midi
```

## 20.2 `instrument init`

Default InstrumentはProcessorを大量に含めない。既存の最小構成を維持する。

Filterが含まれる場合、新規出力JSONには`"mode": "low_pass"`を明示する。

## 20.3 `instrument inspect`

Human-readable / JSON両方で新Processor TypeとStatic Fieldを表示する。

Dynamic ParameterはParameter Catalogに既に載るため、別のProcessor専用Parameter一覧を追加しない。

## 20.4 `docs/instrument-definition.md`

Processor章を更新し、Placement Matrix、JSON例、Dynamic Parameterを正本として記載する。

## 20.5 `docs/runtime-processing.md`

Runtime Processing文書へ以下を追記する。

- Layer / Voice / Global新Processor
- Stereo-linked Dynamics
- Global Modulation FX State
- Fractional Delay Memory ownership
- Filter ModeのNative境界

## 20.6 `docs/testing-and-sound-review.md`

Package一覧へ`Processor Expansion`を追加する。

---

# 21. Test戦略

## 21.1 Definition Unit Test

Processorごとに以下を検証する。

- Serialize / Deserialize round trip
- Unknown Field拒否
- Min / Max境界
- NaN / Infinity拒否
- Placement Matrix
- Duplicate Processor ID
- Filter mode defaultがLowPass

## 21.2 Compiler Unit Test

- Dynamic Parameter IDが期待形式で登録される
- Static FieldがParameter Catalogへ入らない
- Modulation Routeが新Targetへ解決される
- Global ProcessorへVoice Scope SourceをRouteするとError
- Sample Rate依存上限が正しい
- Processor配列順がCompiled順に維持される

## 21.3 Runtime Unit Test共通

全Processorで以下を行う。

- 0 frames
- 1 frame
- 32 / 64 / 257 / 1024 frame
- 44.1 / 48 / 96 kHz
- Fresh Runtime再現性
- Reset再現性
- Parameter Start == End
- Parameter Ramp
- Silence Input
- ±1 Impulse
- Finite Output

## 21.4 Block Size独立性

同じ絶対時間上のEvent / Parameter Changeを次のBlock分割でRenderする。

```text
32
64
257
1024
不均等Block列
```

Processor StateをBlock境界でResetしないことを確認する。

完全bit-identicalが期待できるRust Processorは完全一致を目標とする。Native FilterやFloating operation順序で完全一致が保証されない場合は既存ProjectのToleranceに合わせる。

## 21.5 EQ Test

- 0 dB Identity
- 100 Hz Low Shelf boostで低域RMSが上がる
- Mid centerでboost / cut方向が正しい
- High Shelfで高域が変わる
- Cascade順序固定
- 0 dBへ戻した後もFinite

## 21.6 Resonator Test

- Impulse後に指定Frequency近傍の周期が現れる
- Decayを長くするとTailが長くなる
- Dampingを増やすと高域Energyが減る
- Mix 0でIdentity
- Frequency Rampで非有限値 / 大Clickを出さない

## 21.7 Bitcrusher Test

- ratio 1 / bit_depth 16で原音に近い
- bit_depthを下げると出力Level種類が減る
- sample_rate_ratioを下げると同値Sample連続数が増える
- Stereo更新時刻が左右一致
- Mix 0でIdentity

## 21.8 Chorus Test

- Mix 0でIdentity
- Depth 0で固定Delayになる
- Width 0でMono入力の左右Modulationが同位相
- Width 1で左右差が生じる
- Feedback上限で発散しない
- LFO周期がrate_hzと一致する

## 21.9 Flanger Test

- 正Feedback / 負Feedbackで結果が異なる
- Short DelayによるComb構造が得られる
- Max FeedbackでFinite
- Mix 0 Identity

## 21.10 Phaser Test

- All-pass単体のMagnitudeがおおむね1
- Dry + WetでNotchが生じる
- Stage数でNotch数 / 音が変わる
- rate_hz周期
- feedback正負で結果が変わる
- Sweep範囲外へ出ない

## 21.11 Compressor Test

- ratio 1 = Identity（makeup 0, mix 1）
- Threshold以下の定常音を変えない
- Threshold超過時に理論値へ近づく
- Attack / Release時間関係
- Soft KneeがHard Kneeより境界を滑らかにする
- Stereo Link
- mix 0 = Identity

## 21.12 Limiter Test

- input_gain 0、Ceiling 0 dBで±1以下
- Ceiling -6 dBでPeakが約0.501以下
- Stereo片側Peakでも両channel同じGain
- Release StateがBlockを跨ぐ
- ResetでGain 1へ戻る

## 21.13 Native Filter Test

既存`sonalloy-dsp-sys` Filter Testを4 Modeへ拡張する。

- Create / Prepare / Process / Reset / Destroy
- Invalid mode
- Null handle
- Not prepared
- Guard領域
- Fault Injection
- Error時Buffer無音化
- ASan / UBSan / Leak対象

## 21.14 Integration Test

Public API経路で最低限次を検証する。

1. Layer `eq -> resonator -> bitcrusher`
2. Voice `eq -> compressor`
3. Global `chorus -> delay -> reverb -> limiter`
4. 同一Type複数配置
5. Chain順序入替で結果が変わる
6. Parameter ChangeがSample Accurateな位置から反映される
7. Mod Wheel等からProcessor ParameterへRouteできる
8. Reset後Render一致

---

# 22. Sound Review Package

## 22.1 Package名

```text
review/processor-expansion/
```

既存`review/processor-chain/`はv0.1のFilter / Drive / Delay / Reverb回帰用として残す。上書きしない。

## 22.2 Directory

```text
review/processor-expansion/
├─ README.md
├─ definitions/
├─ events/
├─ audio/
│  └─ technical/
├─ inspect/
├─ metrics.json
└─ review-summary.md
```

生成Script：

```text
review/generate/generate_processor_expansion.py
```

共通Utilityを使い、既存Packageと別方式のMetrics生成器を作らない。

## 22.3 Review Definition

最低限以下を用意する。

### A. Filter Modes

Source：Saw + Noise

出力：

- `filter_low_pass.wav`
- `filter_high_pass.wav`
- `filter_band_pass.wav`
- `filter_notch.wav`

確認：4 Modeの差が明瞭で、Resonanceを上げても破綻しない。

### B. EQ

Source：Broadband Saw / Noise

- Flat
- Low Boost
- Mid Cut
- High Boost
- Combined Tone

EQが単なる音量差ではなく帯域差として聞こえること。

### C. Resonator

Source：Noise Burst / Short Sample

- 220 Hz
- 440 Hz
- Short Decay
- Long Decay
- Dark Damping

PitchとDecayの違いを確認する。

### D. Bitcrusher

Source：Wavetable / Drum-like Sample

- 16 bit / ratio 1
- 8 bit
- 4 bit
- rate ratio 0.25
- Combined crush

### E. Modulation FX

Source：Sustained Pad

- Chorus Narrow / Wide
- Flanger Positive / Negative Feedback
- Phaser 4-stage / 8-stage

Mono SourceでもStereo幅と時間変化が分かること。

### F. Dynamics

Source：Velocity差を持つPhrase

- Dry
- Compressor gentle
- Compressor strong
- Parallel Compressor
- Limiter

Compressorが単なるClipにならず、LimiterがPeakを抑えること。

### G. Full Chain

既存Generatorを複数使った完成音色を最低3個作る。

1. Digital Pad
   - Wavetable + Additive
   - EQ
   - Chorus
   - Reverb
   - Compressor
2. Metallic Pluck
   - Operator Modulation + Sample Attack
   - Resonator
   - Phaser
   - Delay
   - Limiter
3. Lo-fi Texture
   - Granular + Noise
   - Bitcrusher
   - EQ
   - Flanger
   - Reverb

技術Demoではなく、実際の曲へ使用可能かを人間が試聴する。

## 22.4 Metrics

既存共通Metricsに加えて次を記録する。

- Peak / RMS / DC / Finite
- Fresh Runtime一致
- Reset一致
- Block Size比較
- 44.1 / 48 / 96 kHz
- Release Build Render realtime比

Processor固有の自動判定はUnit / Integration Testへ置き、`metrics.json`を巨大なDSP検査表にしない。

## 22.5 人間Review項目

| Processor | 主観確認 |
|---|---|
| Filter | Mode差、Resonanceの耳障りな破綻 |
| EQ | Boost / Cutの自然さ、過度な位相感 |
| Resonator | Pitch感、金属的 / 弦的共鳴の使いやすさ |
| Bitcrusher | Digital Texture、耳障りさと用途 |
| Chorus | Stereo広がり、揺れ、濁り |
| Flanger | Sweep、Feedback、Jet感 |
| Phaser | Sweepの滑らかさ、段数差 |
| Compressor | Punch、Pumping、音量差だけになっていないか |
| Limiter | Peak抑制、過度な歪み |
| Full Chain | 既存Generatorの表現力が実際に広がったか |

Reviewで音質問題があれば、RangeやAlgorithm Constantを調整して再生成する。Metrics合格だけで完了にしない。

---

# 23. File単位の変更計画

以下は現行Repository構造を前提とした変更対象である。

## 23.1 `crates/sonalloy-core/src/definition.rs`

- `FilterModeDefinition`
- Filter `mode`
- 新Processor Definition 8種
- Placement Validation Matrix
- Field Range Validation
- EQ順序Validation
- Phaser stages Validation
- Sample Rate非依存の基本Validation
- Unit Test

## 23.2 `crates/sonalloy-core/src/compiler.rs`

- `CompiledProcessorKind`拡張
- ProcessorごとのCompiled Struct
- Parameter Catalog登録
- Sample Rate依存Validation
- Ring Buffer容量計算
- Compressor / Limiter coefficient計算
- Inspect用Compiled情報
- Compiler Test

## 23.3 `crates/sonalloy-core/src/runtime/processor/mod.rs`

- Module追加
- `ProcessorTargetSpan`拡張
- `zero_for()` / `clear()`
- `LayerProcessorRuntime`拡張
- `StereoProcessorRuntime`拡張
- Constructor
- Process dispatch
- Reset dispatch

`mod.rs`へDSP数式本体を書き込まない。dispatchと共通境界だけを置く。

## 23.4 新規Runtime File

```text
eq.rs
fractional_delay.rs
resonator.rs
bitcrusher.rs
chorus.rs
flanger.rs
phaser.rs
compressor.rs
limiter.rs
```

各Fileに対象ProcessorのUnit Testを置く。

## 23.5 `runtime/instrument.rs` / `runtime/voice.rs`

現行のProcessor Target Span生成・更新箇所へ新Variantを追加する。

Processor固有のDSPロジックをInstrument / Voiceへ書かない。

Layer Chain生成時に`GeneratorOutputMode`を渡す。

## 23.6 `native/daisysp-wrapper/include/sonalloy_dsp.h`

- Filter Mode enum
- Filter process signature更新

## 23.7 `native/daisysp-wrapper/src/daisysp_wrapper.cpp`

- Mode Validation
- `Svf::Low / High / Band / Notch`選択
- Ramp Processでも同じMode選択
- Error時Buffer無音化契約維持

## 23.8 `crates/sonalloy-dsp-sys/src/ffi.rs`

Filter Mode定数とFFI Signature更新。

## 23.9 `crates/sonalloy-dsp-sys/src/filter.rs`

- `DspFilterMode`
- Process APIへMode
- Test拡張

## 23.10 `docs/instrument-definition.md`

Processor章を現仕様へ更新。

## 23.11 `docs/runtime-processing.md`

Runtime State / Placement / Dynamics / Modulation FXを更新。

## 23.12 `docs/testing-and-sound-review.md`

Review Package一覧へProcessor Expansion追加。

## 23.13 `review/processor-expansion/`

新規Review Package。

## 23.14 `review/generate/generate_processor_expansion.py`

新規Package生成Script。

## 23.15 `testdata/`

既存FixtureのFilter JSONは`mode`省略でもValidなため一括変更必須ではない。ただし正本となる新規Fixtureでは`mode`を明示する。

既存Expected OutputをProcessor追加だけで変更しない。既存ProcessorなしDefinitionの出力が変化した場合はRegressionとして扱う。

---

# 24. 実装順序

実装Agentは次の順で進める。後段を先に実装しない。

## P0. Baseline固定

1. `main`のTestを全実行
2. `v0.1.0` Processor Chain Review Packageの再生成手順を確認
3. 既存Filter / Drive / Delay / ReverbのRegression基準を記録
4. 新しい依存を追加していないことを確認

**完了条件**：変更前BaselineがGreen。

## P1. Definition / Compiler Skeleton

1. 新Processor Definition追加
2. Placement Matrix追加
3. Validation追加
4. `CompiledProcessorKind`追加
5. Parameter ID / Descriptor追加
6. `ProcessorTargetSpan` Variant追加
7. Runtime dispatchへ未実装Placeholderを置かず、各Processor実装と同じCommit単位で段階的に追加する

一度に全Variantを追加して`todo!()`を残さない。

## P2. Filter Mode Expansion

1. C Header
2. C++ Wrapper
3. FFI
4. `DspFilterMode`
5. Core Filter Definition / Compile / Runtime
6. Native / Core Test

Filterは既存経路の変更なので最初に完了させる。

## P3. Tone + Digital

1. EQ
2. Bitcrusher
3. Definition / Compiler / Parameter / Runtime / Test

Heapを必要としないProcessorから共通拡張パターンを確立する。

## P4. Fractional Delay基盤 + Resonator

1. `FractionalDelayLine`
2. Ring read / write / Reset Test
3. Resonator
4. Mono / Stereo Memory所有
5. Frequency / Decay / Damping Test

## P5. Modulation FX

1. Chorus
2. Flanger
3. Phaser
4. Global Placement Integration
5. LFO Reset / Width / Block Size Test

## P6. Dynamics

1. Compressor
2. Limiter
3. Stereo Link
4. Attack / Release Test
5. Ceiling Test

## P7. Full Integration

1. Layer Chain組合せ
2. Voice Chain組合せ
3. Global Chain組合せ
4. Parameter Change
5. Modulation Route
6. Reset
7. Block Size独立性
8. Existing Processor Regression

## P8. Documentation / Review Package

1. `instrument-definition.md`
2. `runtime-processing.md`
3. `testing-and-sound-review.md`
4. Review definitions
5. Generate script
6. Metrics
7. Human listening
8. `review-summary.md`

## P9. Release Candidate確認

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

既存CIのNative fault-injection / Sanitizer jobもGreenであること。

Release BuildでReview Packageを再生成し、全WAVを最終試聴する。

---

# 25. 完了条件

本フェーズは以下を全て満たした時だけ完了とする。

## 25.1 Definition / Compile

- 新ProcessorがJSONで保存・読込可能
- Unknown Fieldを拒否
- Placement違反をCompile前に検出
- Dynamic Parameter IDが安定している
- Static FieldがParameter Catalogへ混入していない
- Definition順を維持

## 25.2 Runtime

- Layer / Voice / GlobalのState所有単位が正しい
- Process中にHeap Allocationを行わない
- Process中にFile I/O / JSON / String lookup / Blocking Lockを行わない
- ResetでStateを初期化
- 44.1 / 48 / 96 kHz
- 可変Block Size
- Finite Output

## 25.3 DSP

- Filter 4 Modeが明確に異なる
- EQがTone shapingとして実用になる
- ResonatorがPitchとDecayを持つ共鳴として機能する
- BitcrusherがQuantizationとSample-rate Reductionの両方を持つ
- Chorus / Flanger / Phaserがそれぞれ明確に異なる効果になる
- CompressorがStereo-linked Dynamicsとして機能する
- Limiterが自身の出力PeakをCeiling以下へ抑える

## 25.4 Regression

- ProcessorなしInstrumentの出力が変化しない
- Existing Low-pass FilterのDefault動作が回帰しない
- Drive / Delay / Reverbの既存TestがGreen
- Existing Review Packageの自動Metricsに不意な回帰がない

## 25.5 Sound Review

- `review/processor-expansion/metrics.json`生成済み
- `review-summary.md`記入済み
- Full Chain 3音色を人間が試聴済み
- 技術的に動くだけの状態で完了にしない

---

# 26. 次フェーズへ残すもの

Processor Expansion後もConcept全体では次が残る。

## 26.1 Advanced Processor Expansion候補

```text
Ladder Filter
Formant Processor
Frequency Shifter
Convolution
Gate
Transient Shaper
Tempo Sync / Multi-tap Delay
Advanced Reverb controls
```

ただし、今回の完了後に直ちに上記へ進むとは固定しない。次ロードマップではPhysical / Modal Generatorと比較して優先度を判断する。

## 26.2 External Audio依存機能

```text
Envelope Follower
Vocoder Analysis
Vocoder Carrier
Cross Synthesis
Sidechain
```

これらはProcess ContractへInput Bufferを導入してから扱う。

---

# 27. 実装Agent向け最終ルール

この章は実装中に迷った場合の最終判断として使用する。

1. 現在のProcessor Chainを置き換えず拡張する。
2. Processorを自由Graphへ一般化しない。
3. 新しいCrate / DSP Libraryを追加しない。
4. DaisySPの新ModuleをBuild対象へ追加しない。
5. Filter Modeだけ既存DaisySP SVF Wrapperを拡張する。
6. EQ / Resonator / Bitcrusher / Chorus / Flanger / Phaser / Compressor / LimiterはRustで実装する。
7. DSP本体を`instrument.rs`や`voice.rs`へ書かない。
8. ProcessorごとのStateは`runtime/processor/<name>.rs`へ置く。
9. `ProcessorTargetSpan`の固定Size / Copy可能な設計を維持する。
10. Dynamic Parameterは既存Parameter Catalog / Modulation / Smoothingへ統合する。
11. Static Field変更は再Compileとする。
12. Audio PathでAllocation、File I/O、JSON、文字列検索を行わない。
13. Definition配列順が処理順である。
14. RuntimeがProcessor Typeを並べ替えない。
15. Layer / Voice / GlobalのPlacement Matrixを厳守する。
16. Compressor / LimiterはStereo Linkする。
17. Chorus / Flanger / PhaserはGlobalだけでStateを一組持つ。
18. ResonatorのDelay BufferはPrepare時に確保する。
19. Block Size境界でLFO、Delay phase、Dynamics StateをResetしない。
20. Instrument Runtime ResetではすべてのProcessor StateをResetする。
21. Parameter範囲外をAudio Pathで黙って補正する設計を基本にせず、Definition / Compileで拒否する。Modulation後の値は既存Target Range Clampを使用する。
22. `todo!()`、`unimplemented!()`、仮のSilence実装をMerge状態へ残さない。
23. 新Processorなしの既存Instrumentを変化させない。
24. 自動Test合格後に必ずReview Packageを生成し、人間が試聴する。
25. 本書にないProcessorや新機能を「ついで」に追加しない。

---

# 28. 参考資料

## Repository内

- `docs/CONCEPT.md`
- `docs/plan/plan-processor-chain.md`
- `docs/instrument-definition.md`
- `docs/runtime-processing.md`
- `docs/testing-and-sound-review.md`
- `crates/sonalloy-core/src/definition.rs`
- `crates/sonalloy-core/src/compiler.rs`
- `crates/sonalloy-core/src/runtime/processor/mod.rs`
- `crates/sonalloy-dsp-sys/src/filter.rs`
- `native/daisysp-wrapper/include/sonalloy_dsp.h`
- `native/daisysp-wrapper/CMakeLists.txt`

## DSP方式

- W3C Audio Working Group, **Audio EQ Cookbook** — EQ Biquad coefficientの基準
- DaisySP `a0494a3adb67f549e18dfd71a35fa656f65b38b6`, `Source/Filters/svf.h` — Low / High / Band / Notchを持つ既存SVF
- DaisySP同CommitのEffects / Dynamics Source — 外部Module採否の確認用。Sonalloy本フェーズでは新Moduleを直接組み込まない

---

# 最終到達イメージ

```text
Generator
  ├─ Oscillator
  ├─ Noise
  ├─ Wavetable
  ├─ Operator Modulation
  ├─ Additive
  ├─ Formant
  ├─ Sample
  ├─ Granular
  ├─ Wave Sequence
  └─ Spectral
       │
       ▼
Layer Processing
  ├─ Filter: LP / HP / BP / Notch
  ├─ Drive
  ├─ EQ
  ├─ Resonator
  └─ Bitcrusher
       │
       ▼
Voice Processing
  ├─ Filter
  ├─ Drive
  ├─ EQ
  ├─ Resonator
  ├─ Compressor
  └─ Limiter
       │
       ▼
Global Processing
  ├─ Filter
  ├─ Drive
  ├─ EQ
  ├─ Chorus
  ├─ Flanger
  ├─ Phaser
  ├─ Delay
  ├─ Reverb
  ├─ Compressor
  └─ Limiter
       │
       ▼
Stereo WAV
```

この状態を`v0.2 Processor Expansion`の完成点とする。
