# Sonalloy Physical / Modal Synthesis Expansion 詳細設計・実装計画

- **対象Repository**：`endo-ly/sonalloy`
- **基準Commit**：`6d62fe6347ddeb135eea7113d573759351fe5a5c`
- **正本要件**：`docs/CONCEPT.md`
- **現行Definition Schema**：`schema_version = 2`
- **前提実装**：AI Instrument Authoring / Render Diagnostics、Processor Expansionまでを含む現在の`main`
- **用途**：実装Agentへそのまま渡し、追加の設計判断を極力発生させず実装を進めるための詳細計画
- **文書言語**：日本語。型名、API名、Parameter ID、File Path、JSON field、固有のDSP用語のみ英語表記を使用する
- **成果物**：Markdownのみ

---

## 目次

1. この計画の位置づけ
2. 現在のコードベースと今回利用する既存基盤
3. 今回の到達点と音としての守備範囲
4. 対象範囲と明確な境界
5. 全体アーキテクチャ
6. DSP実装方式と依存ライブラリ判断
7. DaisySP利用範囲の固定
8. 共通Exciter設計
9. Instrument Definition
10. Parameter / Modulation契約
11. Compiler契約
12. Runtime共通基盤
13. Physical String Generator
14. Modal Generator
15. DaisySP Native Wrapper拡張
16. `sonalloy-dsp-sys`拡張
17. 決定性・Seed・Voice独立性
18. Pitch / Sample Rate / Tuning契約
19. Note LifecycleとLayer Envelopeの関係
20. Realtime Safety / Memory / Performance
21. Error / Diagnostic
22. Inspect / Analysis / Trace
23. CLI / Documentation / Agent Skill
24. Unit Test戦略
25. Integration Test戦略
26. Block Size / Sample Rate / Reset検証
27. Sound Review Package
28. Review用Definitionと試聴観点
29. File単位の変更計画
30. 実装順序
31. 完了条件
32. 次フェーズへ残すもの
33. 実装Agent向け最終ルール
34. 参考資料

---

# 1. この計画の位置づけ

現在のSonalloyは、Oscillator、Noise、Additive、Formant、Sample、Granular、Wave Sequence、Wavetable、Spectral、Operator ModulationをGeneratorとして持ち、Filter、Drive、EQ、Resonator、Bitcrusher、Chorus、Flanger、Phaser、Delay、Reverb、Compressor、LimiterをProcessorとして持つ。 さらに、現在の`main`ではDefinition Schema 2、意味のある単位で記述するModulation Depth、Native値によるParameter Change、Inspect、Audio Analysis、Runtime Parameter Traceまで成立している。したがって、次のGenerator追加は単に「音が鳴る」ことだけを目標にせず、既存のParameter / Modulation / Inspect / Trace / Review契約へ最初から統合する。 `docs/CONCEPT.md`は、Generatorの対象として`Physical/Modal/Waveguide`を明示し、同時に「生楽器の完全物理再現」は意図的に対象外としている。また、Physical ModelingのようにAudio-rate Feedbackを必要とする処理は、自由なGraphではなく専用Generator内部の固定Topologyとして実装する方針を固定している。 今回の目的は、この空白を次の二つの汎用方式で埋めることである。

1. **Physical String Generator**
   - Delay Lineを中心とするDigital Waveguide / Extended Karplus-Strong方式
   - 弦を弾く、はじく、硬い物体を振動させる方向の音
2. **Modal Generator**
   - 複数の共鳴Modeを同時に励振するModal Resonator方式
   - 棒、板、ベル、金属、木、ガラス、膜のような共鳴音

本フェーズ完了時の状態を次の一文で固定する。

> **Sonalloyは、実在楽器の完全再現を行わず、弦のFeedback振動と複数Modeの共鳴という二つの主要なPhysical / Modal原理から、撥弦・打撃・金属・木質・ガラス・膜的な音と、それらを逸脱した架空の物理楽器音を決定的に生成できる。**

## 1.1 実装判断の優先順位

判断が衝突した場合は、次の順序を使う。

1. `docs/CONCEPT.md`
2. 本書で固定するDefinition / DSP / Runtime契約
3. 現行Schema 2のParameter / Modulation / Inspect / Trace契約
4. 決定性とBlock Size独立性
5. Realtime Safety
6. 音としての有用性と人間による試聴
7. 実装の単純さと保守性
8. 将来拡張のしやすさ

将来のTube、Bowed String、Reed、Pianoなどを理由に、Generic Physical Graph、Node System、任意Feedback Routing、汎用Waveguide Networkを今回導入しない。

## 1.2 今回の設計原則

新しいGeneratorは既存の次の流れへ追加する。

```text
GeneratorDefinition
↓
ParameterCatalog
↓
Compiler
↓
CompiledGenerator
↓
LayerGeneratorTargetSpan
↓
GeneratorRuntime
↓
Layer Processor Chain
↓
Layer Envelope / Gain / Pan
```

新しい制御経路、独自Parameter System、独自Modulation Matrixは作らない。 Physical StringとModalはどちらも**Generator**であり、現在存在する`resonator` Processorとは責務を分ける。

```text
Modal Generator
= 自分でExciterを発生させ、自分で共鳴して音源になる

Resonator Processor
= 前段Generatorから来た信号へ共鳴を付加する
```

既存Resonator ProcessorをPhysical Generatorへ拡張したり、GeneratorからProcessor Chainを逆向きに参照したりしない。

---

# 2. 現在のコードベースと今回利用する既存基盤

## 2.1 Generatorの現在構造

現行`GeneratorDefinition`は次を持つ。

```text
Oscillator
Noise
Additive
Formant
Sample
Granular
WaveSequence
Wavetable
Spectral
OperatorModulation
```

`CompiledGenerator`、`LayerGeneratorTargetSpan`、`GeneratorRuntime`も同じカテゴリをそれぞれの責務で表現する。 今回、ここへ次を追加する。

```text
PhysicalString
Modal
```

## 2.2 Runtimeの既存契約

`GeneratorRuntime::render()`は現在、次を受け取る。

- `frames`
- `note_number`
- `tuning_start`
- `tuning_end`
- `sample_rate`
- `tempo_bpm`
- `LayerGeneratorTargetSpan`
- Mono / Left / Right Scratch Buffer

Physical StringとModalも同じ入口を使用する。 両GeneratorはMono Generatorとする。

```text
PhysicalString -> Mono
Modal          -> Mono
```

Stereo化は既存のLayer Pan、複数Layer、Chorus / Flanger等で行う。 物理モデル内部へ独自Stereo Spreadを追加しない。

## 2.3 現在のParameter契約

現行コードは`GeneratorParameterSpec`を正本として、Generatorの連続Parameterについて次を定義している。

- suffix
- unit
- scale
- min / max
- smoothing_seconds

今回の連続Parameterも全て同じ方法で登録する。 また、Schema 2ではModulation DepthがTargetの意味のある単位で表現される。 例えばPhysical StringのDecayが`Seconds + Log2`なら、Modulation Unitは既存契約により`Octaves`となる。

```text
base decay = 2.0 seconds
mod depth  = +1 octave
source = 1
→ 4.0 seconds
```

この契約を変更しない。

## 2.4 現在の決定的Random基盤

Runtimeには既に、`splitmix64_finalizer()`、`unit_f32()`、`bipolar_f32()`が存在する。 Physical ExciterのNoiseは、この既存基盤を利用する。 C標準`rand()`、OS乱数、Thread Local RNG、新規Random Crateは利用しない。

## 2.5 現在のFractional Delay

Processor Expansionで、Rust実装の`FractionalDelayLine`が既に存在する。 現在はProcessor内部Moduleに置かれているが、内容はProcessor固有ではない。

- Prepare時に`Vec<f32>`を確保
- Cubic Interpolation
- `read(delay_frames)`
- `write(value)`
- `reset()`
- Finite検査

Physical Stringはこの実装を再利用する。 同じFractional DelayをGenerator側へ複製しない。

## 2.6 現在のNative境界

`sonalloy-dsp-sys`と`native/daisysp-wrapper`は既に次の方式でDaisySPを包んでいる。

```text
Rust Safe Wrapper
↓
Rust FFI Declaration
↓
C ABI Opaque Handle
↓
C++ Wrapper
↓
Pinned DaisySP
```

Native側は、Argument Validation、Prepare状態、Exception捕捉、Non-Finite検査をWrapper側で管理する。 Modalでもこの形式を維持する。

---

# 3. 今回の到達点と音としての守備範囲

## 3.1 到達させる音の種類

今回のPhysical / Modal Expansionは、特定の実在楽器名をGeneratorとして実装するものではない。 目標とする音の領域は次の通り。

| 音の方向 | 主方式 | 期待する到達点 |
|---|---|---|
| Pluck / Synthetic String | Physical String | 強い |
| Harp / Koto系 | Physical String | 近縁音を作れる |
| Kalimba系 | String + Processor / Modal | 近縁音を作れる |
| Metallic String | Physical String | 強い |
| Bell | Modal | 強い |
| Plate | Modal | 強い |
| Bar / Mallet | Modal | 強い |
| Marimba / Xylophone系 | Modal | 近縁音を作れる |
| Glass / Ceramic的共鳴 | Modal | 強い |
| Membrane / Drum-like | Modal | 抽象化した近縁音を作れる |
| Imaginary Acoustic Instrument | 両方 + Layer / Processor | 主要目標 |

## 3.2 今回の品質基準

品質を「本物のギターと聞き分けられない」では評価しない。 次の性質を評価する。

1. Note Pitchへ自然に追従する
2. 音高が変わっても物理的な振動感が維持される
3. Decay / Brightness / Stiffness / Structure変更が、単なるEQやPitch Shiftではなく共鳴そのものの変化として聞こえる
4. 低音から高音まで破綻せずFiniteである
5. 異なる設定から明確に異なる材質感・共鳴感を作れる
6. 既存Processorと組み合わせて実用的な音源へ発展できる
7. 同一入力から同一出力を生成する

## 3.3 今回は実在楽器Presetを仕様にしない

Review Definitionには「bell-like」「wood-bar-like」等の比較用音色を作ってよい。 一方、Runtime Definitionとして次のようなModel Enumは作らない。

```text
Guitar
Piano
Violin
Marimba
Bell
Flute
```

物理方式を提供し、楽器名はPreset / Sound Design側の責務とする。

---

# 4. 対象範囲と明確な境界

## 4.1 実装対象

今回実装する機能を次へ固定する。

### Generator

1. `physical_string`
   - Deterministic Exciter
   - Fractional Delay Feedback Loop
   - Loop Damping
   - Stiffness / Dispersion表現
   - Nominal Decay Time
2. `modal`
   - Deterministic Exciter
   - DaisySP Resonatorによる4〜24 Mode共鳴
   - Structure
   - Brightness
   - Decay

### 共通基盤

3. Physical Exciter
   - `impulse`
   - `noise_burst`
4. Fractional Delay LineのGenerator / Processor共通化
5. DaisySP Modal Resonator用Native Wrapper
6. Parameter / Modulation / Inspect / Trace統合
7. Review Package

## 4.2 今回扱わないもの

| 機能 | 今回外へ置く理由 |
|---|---|
| Guitar専用Model | Body、Bridge、Pickup、複数弦、奏法まで責務が広がる |
| Piano専用Model | Hammer、複数弦、Soundboard、Damper、Sympathetic Resonanceが必要 |
| Bowed String | Bow-string非線形摩擦Modelが必要 |
| Reed / Clarinet | Reed非線形性とBore Reflectionの結合が必要 |
| Flute / Jet | Jet / Edge ToneとBoreの結合が必要 |
| Brass / Lip | Lip Reedと管の非線形結合が必要 |
| Generic Tube Generator | 単純Delayだけでは方式として薄く、正しいExciter設計が先に必要 |
| 2D Membrane Solver | CPU / Memory負荷と数値安定性の設計が別規模になる |
| Coupled String | 複数弦相互作用は別機能 |
| Sympathetic Resonance | Voice間 / String間の結合が必要 |
| Body IR / Convolution | Convolution Processorの別フェーズ |
| External Audio Exciter | Audio Input Contractが先に必要 |
| Bow / Pick / Hammerの詳細接触Model | 生楽器再現領域へ進みすぎる |
| Arbitrary Waveguide Network | 自由Graphと同種のTopology問題を持つ |
| Runtime Model切替 | Static topology変更なのでRecompile対象 |

## 4.3 Tube / Windを今回入れない理由

Digital Waveguideという言葉だけなら、弦も管もDelay Lineで表現できる。 しかし、音として有用なWind / Tube Generatorには、Delayだけでなく少なくとも次が必要になる。

```text
Exciter
├─ Reed
├─ Jet
└─ Lip
↓
Nonlinear Junction
↓
Forward / Backward Wave
↓
Frequency-dependent Reflection
```

今回これを簡略化して「Tube」と名付けると、Physical Stringとの差が弱い割に、将来の正しいWind ModelとDefinition契約が衝突する。 したがって、Waveguideカテゴリは今回`physical_string`が代表して埋める。

---

# 5. 全体アーキテクチャ

## 5.1 Generator配置

今回の二方式は、既存Generatorと並列の独立Variantとする。

```rust
pub enum GeneratorDefinition {
Oscillator(OscillatorDefinition),
Noise(NoiseDefinition),
Additive(AdditiveDefinition),
Formant(FormantDefinition),
Sample(SampleDefinition),
Granular(GranularDefinition),
WaveSequence(WaveSequenceDefinition),
Wavetable(WavetableDefinition),
Spectral(SpectralDefinition),
OperatorModulation(OperatorModulationDefinition),
PhysicalString(PhysicalStringDefinition),
Modal(ModalDefinition),
}
```

Genericな`PhysicalDefinition { model: ... }`は作らない。 理由は、現在のGenerator設計が方式ごとの直接Variantで統一されており、二方式だけのために一段抽象化する価値がないため。

## 5.2 DSP所有関係

```text
VoiceRuntime
└─ LayerRuntime
└─ GeneratorRuntime
├─ PhysicalStringRuntime
│  ├─ PhysicalExciterRuntime
│  ├─ FractionalDelayLine
│  ├─ Loop Low-pass State
│  └─ Dispersion All-pass State
│
└─ ModalRuntime
├─ PhysicalExciterRuntime
└─ sonalloy_dsp_sys::ModalResonator
```

すべてVoice × Layer単位で所有する。 GlobalなPhysical Stateを作らない。

## 5.3 Output Mode

両Generatorは`GeneratorOutputMode::Mono`。

```rust
Self::PhysicalString(_) | Self::Modal(_) => GeneratorOutputMode::Mono
```

## 5.4 Intrinsic Latency

両Generatorの`intrinsic_latency_frames()`は0。 Physical StringはDelay Feedbackを内部で使うが、それは音を作る仕組みそのものであり、入力信号に対する処理Latencyではない。 Modalも同様にIntrinsic Latency 0とする。

## 5.5 Availability

Assetを必要としないため常に利用可能。

```rust
Self::PhysicalString(_) | Self::Modal(_) => true
```

---

# 6. DSP実装方式と依存ライブラリ判断

## 6.1 結論

| 対象 | 実装方式 | 新規Dependency |
|---|---|---|
| Physical String | Rust独自実装 | なし |
| Modal Resonator | 既存Pinned DaisySP `Resonator` | なし |
| Exciter | Rust独自実装 | なし |
| Fractional Delay | 既存Rust実装を共通化 | なし |
| Random | 既存SplitMix系Helper | なし |
| STK | 採用しない | 追加しない |
| DaisySP-LGPL | 採用しない | 追加しない |
| 新規Rust DSP Crate | 採用しない | 追加しない |

**本フェーズで新しい外部Dependencyは追加しない。** ただし、既に固定利用しているDaisySPのBuild対象へ`Source/PhysicalModeling/resonator.cpp`を追加する。 これは新規ライブラリ追加ではなく、既存Pinned Dependencyの利用範囲拡張である。

## 6.2 現在のDaisySP固定条件

SonalloyはDaisySPを次のCommitへ固定している。

```text
a0494a3adb67f549e18dfd71a35fa656f65b38b6
```

現状CMakeは必要なSourceだけを選択してBuildしている。

```text
oscillator.cpp
variableshapeosc.cpp
svf.cpp
wavefolder.cpp
```

今回追加するのは次だけ。

```text
PhysicalModeling/resonator.cpp
```

`modalvoice.cpp`、`stringvoice.cpp`、`KarplusString.cpp`はBuild対象へ追加しない。

## 6.3 DaisySP Modalを利用する理由

Pinned DaisySPの`Resonator`は次の性質を持つ。

- 最大24 Mode
- 4 ModeずつBatch処理
- StructureからMode間隔のStiffnessを生成
- Brightnessに応じたMode Attenuation / Q Loss
- Damping値に応じたQ
- 外部Inputで励振可能
- 内部Randomなし
- Plaits由来の実績あるModal Resonator

SonalloyがModal Filter Bankを新規にRustで再設計するより、音質面・アルゴリズム面のリスクが低い。 また、Sonalloyは既にDaisySP Native境界を持っているため、新しいBuild SystemやDependency管理方式を増やさず統合できる。

## 6.4 DaisySP `ModalVoice`をそのまま使わない理由

DaisySP `ModalVoice`は次のTopologyを内包する。

```text
Click / Dust
↓
Excitation Filter
↓
Resonator
```

一見すると今回の目的と一致するが、そのまま採用しない。 理由は次の通り。

1. ExciterがDaisySP内部へ固定される
2. Sustain時に`Dust`を使う
3. `Dust`はC標準`rand()`を使う
4. SonalloyのSeed契約を適用できない
5. ExciterのAnalysis / Review契約がNative内部へ隠れる
6. StringとModalで同じExciter契約を共有できない

したがって、低レベル`Resonator`だけを利用し、ExciterはRust側で所有する。

## 6.5 DaisySP Stringを使わない理由

Pinned DaisySPには`String` / `StringVoice`が存在するが、今回のPhysical String Backendには採用しない。

### Determinism

`StringVoice`のNoise BurstはC標準`rand()`を使う。 Low-level `String`もDispersion時に`rand()`を使う。 このRandom StateはGenerator Instanceへ所有されず、SonalloyのDefinition Seed / Note ID / Layer IDへ結びつかない。 同じInstrumentでも、先に何回Native Stringを処理したかによってRandom系列が変化し得る。 これはSonalloyの決定的Render契約に適合しない。

### Delay Line Size

Pinned `KarplusString`は次を固定している。

```text
kDelayLineSize = 1024
```

低音でDelayが足りない場合、内部で簡易Upsampleへ切り替える。 Sonalloyは44.1 / 48 / 96 kHzを明示的に扱い、低音側まで同じ品質契約で動かしたい。 Sample Rateから必要BufferをPrepare時に確保するRust実装の方が、MemoryとPitchの契約を明示できる。

### API上の不整合

Pinned Headerは`SetNonLinearity()`について負値をCurved Bridge、正値をDispersionと説明するが、実装は値を`0..1`へClampする。 固定Commitの挙動をそのまま製品契約として公開するのは避ける。

## 6.6 STKを追加しない理由

STKは非常に有力なPhysical Modeling Toolkitであり、ライセンスもPermissiveである。 しかし今回追加すると、Sonalloyには次が増える。

- 第二の大規模Native DSP Dependency
- 新しいC++ Build Contract
- 新しいWrapper / Lifetime Contract
- DaisySPと重なるDelay / Modal / Physical Primitive
- Cross-platform CI対象
- Dependency更新責務

さらにSTKの強みはPlucked、Bowed、Clarinet、Flute、Brass、ModalBar等の楽器Modelを幅広く持つ点にある。 今回は特定生楽器Modelを増やすフェーズではない。 そのため、今回の二方式だけのためにSTKを導入する費用は便益を上回る。

## 6.7 新規Rust Crateを追加しない理由

Physical Stringに必要な主要Primitiveは既にある。

```text
Vec<f32>
Fractional Delay
Cubic Interpolation
One-pole Filter
First-order All-pass
SplitMix-derived deterministic sample
```

Modalは既存DaisySPを利用する。 したがって、汎用DSP Framework、Audio Graph Library、Random Libraryを追加する必要はない。

## 6.8 License判断

既存Pinned DaisySPのLicenseはMITであり、同License File内でPlaits由来部分もMITとして収録されている。 `resonator.cpp` / `resonator.h`も同じMIT-style License Headerを持つ。 今回DaisySP-LGPL RepositoryのSourceは使わない。 `THIRD_PARTY_NOTICES.md`では新しいLibrary Sectionを作らず、既存DaisySPのUsage説明へModal Resonatorを追記する。

---

# 7. DaisySP利用範囲の固定

## 7.1 Build Source

`native/daisysp-wrapper/CMakeLists.txt`のDaisySP targetへ次を追加する。

```cmake
${daisysp_SOURCE_DIR}/Source/PhysicalModeling/resonator.cpp
```

Build対象は最終的に概ね次となる。

```cmake
add_library(DaisySP STATIC
${daisysp_SOURCE_DIR}/Source/Synthesis/oscillator.cpp
${daisysp_SOURCE_DIR}/Source/Synthesis/variableshapeosc.cpp
${daisysp_SOURCE_DIR}/Source/Filters/svf.cpp
${daisysp_SOURCE_DIR}/Source/Effects/wavefolder.cpp
${daisysp_SOURCE_DIR}/Source/PhysicalModeling/resonator.cpp
)
```

Upstream aggregate targetへ切り替えない。

## 7.2 Mode Count

DaisySP Resonatorは4 Mode単位でBatch処理する。 実装上、4の倍数でないResolutionを渡すと最後の余りModeをFlushする経路がない。 Definitionで許可する`mode_count`を次へ固定する。

```text
4
8
12
16
20
24
```

0、1、6、10、25等はDefinition Errorとする。

## 7.3 Position

DaisySP `Resonator::Init(position, resolution, sample_rate)`には`position`引数がある。 Pinned実装ではModeごとの振幅へ同一の`cos(position * 2π) * 0.25`を設定しており、一般的な意味でのMode別Strike Position / Pickup Positionにはなっていない。 Sonalloy Definitionへこの値を公開しない。 Native Wrapper内部で次へ固定する。

```text
position = 0.015
```

これはDaisySP `ModalVoice`と同じ値である。

## 7.4 DaisySP Parameter名とSonalloy Parameter名

Native APIはDaisySPの概念を扱う。

```text
frequency
structure
brightness
damping
```

Sonalloy Definitionでは最後の値を`decay`と呼ぶ。 理由は、Pinned DaisySPで値が大きいほどQが高くなり、実際にはDecayが長くなるためである。

```text
Sonalloy modal.decay = 0
→ DaisySP damping = 0
→ 短いDecay Sonalloy modal.decay = 1
→ DaisySP damping = 1
→ 長いDecay
```

Definition利用者へ「dampingを増やすと減衰が減る」という逆説的名称を持ち込まない。

---

# 8. 共通Exciter設計

## 8.1 責務

Physical StringとModalは、同じExciter Definitionを利用する。 Exciterは「何を振動体へ与えて発音を開始するか」を表す。 共鳴体のDecayやMaterial Parameterとは分離する。

## 8.2 Definition

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PhysicalExciterDefinition {
Impulse,
NoiseBurst {
duration_seconds: f32,
brightness: f32,
seed: u64,
},
}
```

### JSON: Impulse

```json
{
"type": "impulse"
}
```

### JSON: Noise Burst

```json
{
"type": "noise_burst",
"duration_seconds": 0.008,
"brightness": 0.72,
"seed": 1201
}
```

## 8.3 Range

| Field | Range | 意味 |
|---|---:|---|
| `duration_seconds` | `0.0005..0.100` | Noise Burstの-60 dBまでの長さ |
| `brightness` | `0..1` | Exciter Low-pass CutoffのLog位置 |
| `seed` | `u64` | 決定的Noise系列 |

これらはNote On時の励振形状を決めるStatic Definition値である。 Parameter Catalogへ登録しない。 理由は、Burst終了後にModulationしても意味がなく、Dynamic Parameterとして公開するとParameter契約が誤解を招くため。

## 8.4 Exciter Brightnessの正確なMapping

Process Sample Rateから最大Cutoffを求める。

```text
max_cutoff = min(18000 Hz, sample_rate * 0.45)
min_cutoff = 200 Hz
```

`brightness`をLog Domainで補間する。

```text
cutoff = min_cutoff * (max_cutoff / min_cutoff) ^ brightness
```

Endpointは次となる。

```text
brightness = 0
→ 200 Hz brightness = 1
→ max_cutoff
```

AI向け文書にはこのMappingを記載する。

## 8.5 Noise Burst Envelope

Noise BurstはDurationの最後で-60 dB相当まで下がる指数Envelopeとする。 Prepare時にFrame数を求める。

```text
duration_frames = max(1, round(duration_seconds * sample_rate))
```

Decay係数は次。

```text
envelope_coeff = 10 ^ (-3 / duration_frames)
```

開始Amplitudeを1として、各Sampleで乗算する。 Durationを超えたらExciter出力を0にする。

## 8.6 Noise生成

1 Sampleごとに次を使う。

```text
counter += 1
random_bits = splitmix64_finalizer(note_seed ^ counter)
noise = bipolar_f32(random_bits)
```

`note_seed`は後述するDefinition Seed / Layer / Note IDを混ぜて生成する。

## 8.7 Exciter Low-pass

Exciter Noiseは一段One-pole Low-passへ通す。

```text
a = exp(-2π * cutoff / sample_rate)
y = (1 - a) * x + a * state
```

係数はNote開始時に計算する。 Audio Loop内で`exp()`を呼ばない。

## 8.8 Exciter出力Level

Exciter内部にユーザー向けGain Parameterを持たせない。 固定係数を次とする。

```text
PHYSICAL_EXCITER_GAIN = 0.25
```

Instrument全体の音量設計は既存Layer Gain / Processor / Limiterへ任せる。 Reviewでこの定数自体を音量合わせのために無制限に変更しない。

---

# 9. Instrument Definition

## 9.1 Physical String

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalStringDefinition {
pub exciter: PhysicalExciterDefinition,
pub decay_seconds: f32,
pub brightness: f32,
pub stiffness: f32,
}
```

JSON例：

```json
{
"generator": {
"physical_string": {
"exciter": {
"type": "noise_burst",
"duration_seconds": 0.006,
"brightness": 0.82,
"seed": 4001
},
"decay_seconds": 2.4,
"brightness": 0.68,
"stiffness": 0.18
}
}
}
```

## 9.2 Physical String Field Range

| Field | Range | Dynamic | 説明 |
|---|---:|:---:|---|
| `exciter` | Enum | × | Note On時の励振方式 |
| `decay_seconds` | `0.05..20.0` | ○ | Feedback LoopのNominal T60 |
| `brightness` | `0..1` | ○ | Loop内高域損失 |
| `stiffness` | `0..1` | ○ | All-pass Dispersion量 |

## 9.3 Modal

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModalDefinition {
pub exciter: PhysicalExciterDefinition,
pub mode_count: u8,
pub structure: f32,
pub brightness: f32,
pub decay: f32,
}
```

JSON例：

```json
{
"generator": {
"modal": {
"exciter": {
"type": "noise_burst",
"duration_seconds": 0.010,
"brightness": 0.58,
"seed": 9102
},
"mode_count": 24,
"structure": 0.72,
"brightness": 0.76,
"decay": 0.66
}
}
}
```

## 9.4 Modal Field Range

| Field | Range | Dynamic | 説明 |
|---|---:|:---:|---|
| `exciter` | Enum | × | 共鳴体へ入力する励振 |
| `mode_count` | `4,8,12,16,20,24` | × | 同時共鳴Mode数 |
| `structure` | `0..1` | ○ | Mode間隔 / Stiffness Character |
| `brightness` | `0..1` | ○ | 高次Modeの強さとLoss |
| `decay` | `0..1` | ○ | ModeのDecay長 |

## 9.5 Schema Version

`CURRENT_SCHEMA_VERSION`は2のままとする。 今回の変更は既存Schema 2 Definitionの意味を変更せず、新しいGenerator Variantを追加する機能拡張である。 Version 3へ上げない。

## 9.6 Unknown Field

既存方針通り`deny_unknown_fields`を使う。 Alias、旧名Fallback、Migration用Fieldを追加しない。

---

# 10. Parameter / Modulation契約

## 10.1 新しいGeneratorParameterSpec

`generator_parameters.rs`へ次を追加する。

```text
PHYSICAL_STRING_DECAY
PHYSICAL_STRING_BRIGHTNESS
PHYSICAL_STRING_STIFFNESS
MODAL_STRUCTURE
MODAL_BRIGHTNESS
MODAL_DECAY
```

## 10.2 Physical String Parameter

### Decay

```rust
GeneratorParameterSpec {
suffix: "physical_string_decay_seconds",
unit: ParameterUnit::Seconds,
scale: ParameterScale::Log2,
min: 0.05,
max: 20.0,
smoothing_seconds: 0.010,
}
```

Parameter ID例：

```text
layer.string.generator.physical_string_decay_seconds
```

Modulation Unitは既存契約により`octaves`。

### Brightness

```rust
GeneratorParameterSpec {
suffix: "physical_string_brightness",
unit: ParameterUnit::Normalized,
scale: ParameterScale::Linear,
min: 0.0,
max: 1.0,
smoothing_seconds: 0.010,
}
```

### Stiffness

```rust
GeneratorParameterSpec {
suffix: "physical_string_stiffness",
unit: ParameterUnit::Normalized,
scale: ParameterScale::Linear,
min: 0.0,
max: 1.0,
smoothing_seconds: 0.010,
}
```

## 10.3 Modal Parameter

### Structure

```rust
GeneratorParameterSpec {
suffix: "modal_structure",
unit: ParameterUnit::Normalized,
scale: ParameterScale::Linear,
min: 0.0,
max: 1.0,
smoothing_seconds: 0.010,
}
```

### Brightness

```rust
GeneratorParameterSpec {
suffix: "modal_brightness",
unit: ParameterUnit::Normalized,
scale: ParameterScale::Linear,
min: 0.0,
max: 1.0,
smoothing_seconds: 0.010,
}
```

### Decay

```rust
GeneratorParameterSpec {
suffix: "modal_decay",
unit: ParameterUnit::Normalized,
scale: ParameterScale::Linear,
min: 0.0,
max: 1.0,
smoothing_seconds: 0.010,
}
```

## 10.4 `is_suffix()`

全6 Parameterを既存の`generator_parameters::is_suffix()`へ追加する。 Parameter ID判定の別実装を作らない。

## 10.5 Parameter Catalog

`parameter.rs::push_generator_descriptors()`へ二Variantを追加する。 Physical String：

```text
physical_string_decay_seconds
physical_string_brightness
physical_string_stiffness
```

Modal：

```text
modal_structure
modal_brightness
modal_decay
```

`mode_count`、ExciterのStatic FieldはCatalogへ入れない。

## 10.6 Modulation例

### 弦を時間とともに暗くする

```json
{
"source": "tone_env",
"target": "layer.string.generator.physical_string_brightness",
"depth": {
"value": -0.6,
"unit": "normalized"
},
"curve": "smooth_step"
}
```

### VelocityでModal Structureを動かす

```json
{
"source": "velocity",
"target": "layer.body.generator.modal_structure",
"depth": {
"value": 0.25,
"unit": "normalized"
},
"curve": "linear"
}
```

### Mod Wheelで弦Decayを2倍まで伸ばす

```json
{
"source": "mod_wheel",
"target": "layer.string.generator.physical_string_decay_seconds",
"depth": {
"value": 1.0,
"unit": "octaves"
},
"curve": "linear"
}
```

---

# 11. Compiler契約

## 11.1 Compiled Physical Exciter

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledPhysicalExciter {
Impulse,
NoiseBurst {
duration_seconds: f32,
brightness: f32,
seed: u64,
},
}
```

Sample Rate依存のFrame数やFilter係数は`GeneratorRuntime::new()`でPrepareする。 CompilerにRuntime DSP Stateを持ち込まない。

## 11.2 Compiled Physical String

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledPhysicalStringParameters {
pub decay_seconds: ParameterHandle,
pub brightness: ParameterHandle,
pub stiffness: ParameterHandle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledPhysicalString {
pub exciter: CompiledPhysicalExciter,
pub parameters: CompiledPhysicalStringParameters,
pub layer_hash: u64,
}
```

Delay Buffer SizeはProcess Spec Sample RateからRuntime Prepare時に求めるため、Compiled structへ大きなBufferを持たせない。

## 11.3 Compiled Modal

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledModalParameters {
pub structure: ParameterHandle,
pub brightness: ParameterHandle,
pub decay: ParameterHandle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledModal {
pub exciter: CompiledPhysicalExciter,
pub mode_count: u8,
pub parameters: CompiledModalParameters,
pub layer_hash: u64,
}
```

## 11.4 CompiledGenerator

```rust
pub enum CompiledGenerator {
...
PhysicalString(CompiledPhysicalString),
Modal(CompiledModal),
}
```

## 11.5 compile_generator

既存`compile_generator()`へ二分岐を追加する。 責務は次の通り。

### Physical String

1. Exciter DefinitionをCompiled形式へ変換
2. Parameter HandleをCatalogから取得
3. Layer IDのStable Hashを保存
4. Static Fieldをコピー

### Modal

1. ExciterをCompile
2. `mode_count`を保存
3. Parameter Handle取得
4. Layer Hash保存

Definition Validationで既に不正値を拒否するため、Compiler内でRangeを別の値として再定義しない。

## 11.6 Layer Hash

Noise / Random系の既存方式に合わせ、Layer IDからStable Hashを生成する。 新しいHash Crateを追加しない。

## 11.7 Output Mode

```rust
Self::PhysicalString(_) | Self::Modal(_) => GeneratorOutputMode::Mono
```

## 11.8 Intrinsic Latency

```rust
Self::PhysicalString(_) | Self::Modal(_) => 0
```

## 11.9 Availability

```rust
Self::PhysicalString(_) | Self::Modal(_) => true
```

## 11.10 Frequency Limit

Physical Generatorの安全上限をCore側へ一つだけ定義する。

```rust
pub(crate) const PHYSICAL_FREQUENCY_LIMIT_RATIO: f64 = 0.45;
```

```text
effective_max_frequency = sample_rate * 0.45
```

Physical StringとModalのFundamentalはこの上限を共有する。 DaisySP Modal内部の高次Modeは0.499 normalizedへClampされるが、Fundamental自体はSonalloy側で0.45 Sample Rate以下へ制限する。 Runtime、Compiler Diagnostic、Inspectで別々のLiteralを持たない。

---

# 12. Runtime共通基盤

## 12.1 Fractional Delayの移動

現在：

```text
runtime/processor/fractional_delay.rs
```

変更後：

```text
runtime/fractional_delay.rs
```

または同等のRuntime共通位置へ移す。 既存利用箇所：

```text
processor/resonator.rs
processor/chorus.rs
processor/flanger.rs
```

新規利用箇所：

```text
generator/physical_string.rs
```

Importだけを変更し、既存Processorのアルゴリズムは変更しない。

## 12.2 Physical Exciter Runtime

新規：

```text
runtime/generator/physical_exciter.rs
```

候補構造：

```rust
pub(super) struct PhysicalExciterRuntime {
kind: PreparedExciterKind,
filter_state: f32,
remaining_frames: usize,
envelope: f32,
sample_counter: u64,
note_seed: u64,
impulse_pending: bool,
}
```

`new(compiled, sample_rate)`でSample Rate依存係数をPrepareする。 `trigger(layer_hash, note_id)`でNote単位Stateを初期化する。 `next_sample()`はAllocationせず1 Sample返す。

## 12.3 LayerGeneratorTargetSpan

次を追加する。

```rust
PhysicalString {
decay_seconds: ValueSpan,
brightness: ValueSpan,
stiffness: ValueSpan,
},
Modal {
structure: ValueSpan,
brightness: ValueSpan,
decay: ValueSpan,
},
```

## 12.4 zero_target_span

`CompiledGenerator::zero_target_span()`へ両Variantを追加する。 全Dynamic ParameterへZero `ValueSpan`を割り当てる。

## 12.5 VoiceTargetScratch

既存の`VoiceTargetScratch`構造は変更しない。 新しいVariantが`LayerGeneratorTargetSpan`内へ入るため、追加Vecを持たせる必要はない。

## 12.6 Voice Target評価

`VoiceRuntime`で現在行っている`CompiledGenerator`ごとのTarget評価へ二Variantを追加する。 Physical String：

```text
decay_seconds = evaluate_target(...)
brightness    = evaluate_target(...)
stiffness     = evaluate_target(...)
```

Modal：

```text
structure  = evaluate_target(...)
brightness = evaluate_target(...)
decay      = evaluate_target(...)
```

既存`evaluate_target()`とRoute評価を必ず通す。 独自Modulation計算をGenerator内で行わない。

## 12.7 GeneratorRuntime

```rust
pub(super) enum GeneratorRuntime {
...
PhysicalString(Box<PhysicalStringRuntime>),
Modal(Box<ModalRuntime>),
}
```

Box利用はRuntime Prepare時のサイズ安定化のため許容する。 Audio Loop中のAllocationではない。

## 12.8 Lifecycle Match

`GeneratorRuntime`の次のMatchを全て更新する。

- `new()`
- `start()`
- `note_off()`
- `intrinsic_latency_frames()`
- `render()`
- `reset()`

追加漏れを`_ =>`で隠さない。

---

# 13. Physical String Generator

## 13.1 方式

Physical Stringは、Fractional DelayをFeedback Loopとして用いるExtended Karplus-Strong / Digital Waveguideとする。 基本Topology：

```text
Deterministic Exciter ───────────────┐
▼
[ Sum ]
│
▼
Fractional Delay
│
▼
Loop Low-pass
│
▼
Dispersion All-pass
│
× Feedback Gain
│
└─────────────┐
│
Direct Exciter Attack ──┐                         │
▼                         │
Output <─────────────────────┘
```

自由なFeedback Routingへしない。

## 13.2 Runtime State

```rust
pub(super) struct PhysicalStringRuntime {
sample_rate: f32,
max_delay_frames: usize,
delay: FractionalDelayLine,
exciter: PhysicalExciterRuntime,
loop_filter_state: f32,
allpass_x1: f32,
allpass_y1: f32,
pending_start: bool,
}
```

必要に応じて係数Cacheを持ってよい。

## 13.3 Delay Buffer Size

最低Frequencyを次へ固定する。

```text
MIN_PHYSICAL_FREQUENCY_HZ = 4.0
```

理由：MIDI Note 0が約8.18 Hzであり、Layer Tuningの-1200 centsを含めても約4.09 Hzまで下がるため。 Prepare時：

```text
max_delay_frames = ceil(sample_rate / 4.0) + interpolation_margin
```

Interpolation Marginは既存`FractionalDelayLine`が要求する4 Sample以上を確保する。 例：

| Sample Rate | 約Max Delay | 1 VoiceのBuffer |
|---:|---:|---:|
| 44.1 kHz | 11,025 frame | 約43 KiB |
| 48 kHz | 12,000 frame | 約47 KiB |
| 96 kHz | 24,000 frame | 約94 KiB |

64 Voice × 1 String Layerでも96 kHzで約6 MiB程度であり、Desktop向けSonalloyの上限として許容できる。 実際の`Vec`にはMarginが加わる。

## 13.4 Fundamental Frequency

`base_frequencies(note_number, tuning_start, tuning_end)`を利用する。 Generator内部でMIDI Note→Hz式を複製しない。 Fundamentalは全Sampleで正値かつ`PHYSICAL_FREQUENCY_LIMIT_RATIO`以内であることを確認する。

## 13.5 Fractional Delay

基本Delay：

```text
period_frames = sample_rate / frequency_hz
```

All-passのGroup Delay補正後にFractional Delayへ渡す。

```text
delay_frames = period_frames - allpass_group_delay
```

値がInterpolation可能範囲内にあることを確認する。

## 13.6 Loop Decay

`decay_seconds`はFeedback LoopのNominal T60とする。 1周あたりFeedback Gain：

```text
loop_period_seconds = 1 / frequency_hz
feedback = 10 ^ (-3 * loop_period_seconds / decay_seconds)
```

同値：

```text
feedback = 10 ^ (-3 / (frequency_hz * decay_seconds))
```

`decay_seconds > 0`である限り、Feedbackは0より大きく1未満となる。 Loop Low-passによる追加損失があるため、実際の高域T60はNominal値より短くなる。 `docs/instrument-definition.md`ではこの点を明記する。

## 13.7 Brightness

BrightnessはLoop内Low-pass Cutoffを制御する。 CutoffをNote Frequency基準で決定する。

```text
cutoff_octaves = 2 + brightness * 6
cutoff_hz = frequency_hz * 2 ^ cutoff_octaves
cutoff_hz = clamp(cutoff_hz, 200, min(18000, sample_rate * 0.45))
```

Endpoint：

```text
brightness = 0
→ fundamentalの約4倍を基準とする暗いLoop brightness = 1
→ fundamentalの約256倍を基準とし、実質上限まで開く
```

One-pole係数：

```text
a = exp(-2π * cutoff_hz / sample_rate)
low = (1 - a) * delayed + a * state
```

## 13.8 Stiffness / Dispersion

`stiffness`はFirst-order All-passによるFrequency-dependent Phase Delayとして実装する。 All-pass係数：

```text
allpass_a = stiffness * 0.75
```

Filter：

```text
y[n] = a * x[n] + x[n-1] - a * y[n-1]
```

`stiffness = 0`でもAll-passは1 Sample相当のDelayを持つため、Fundamental Delay計算でGroup Delayを補正する。 第一目的は、高次成分ほどPhase Delayが変化することで、完全なHarmonic Stringから硬い / 金属的なStringへ連続変化させることである。

## 13.9 All-pass Group Delay補正

First-order All-passのGroup DelayをFundamental角周波数で求める。

```text
omega = 2π * frequency_hz / sample_rate group_delay =
(1 - a^2)
/ (1 + a^2 + 2a cos(omega))
```

`period_frames - group_delay`をFractional Delayへ渡す。 これにより、Stiffness変更によるFundamental Pitch Shiftを抑える。

## 13.10 ExciterのLoop入力

Exciter Sampleを次へ加える。

```text
write = exciter + dispersed * feedback
```

安全のため、内部Feedback書込み値がFiniteであることを必ず検査する。 固定Hard Clipを通常音質処理として入れない。 異常値が発生した場合はProcess Errorとする。

## 13.11 Attack出力

Delay LineへExciterを順次投入する方式だけでは、最初の1周期が無音になる。 そのため、OutputへExciterの一部を直接加える。

```text
output = delayed + exciter * 0.25
```

この係数は内部固定値であり、Parameter化しない。 Layer Gainで最終Levelを調整する。

## 13.12 Coefficient更新最適化

各Render Spanについて、次を判定する。

```text
frequency constant?
decay constant?
brightness constant?
stiffness constant?
```

全て一定の場合、次をBlock開始時に1回だけ計算する。

- Delay Frames
- Feedback Gain
- Loop Low-pass Coefficient
- All-pass Coefficient
- Fundamental Group Delay

一部が変化する場合、その係数だけSample単位更新する。 `powf()`、`exp()`、`cos()`をParameterが静的なのに毎Sample呼ぶ実装にしない。

## 13.13 Pitch Sweep

Tuning Spanが変化する場合、FundamentalをSpan内で連続更新する。 既存GeneratorのBlock分割契約に合わせ、Start / Endから同じ位置式で補間する。 Block Sizeごとに異なるPitch軌跡を作らない。

## 13.14 Start

`start(note_id)`で次を行う。

1. Delay Line Reset
2. Loop Filter Reset
3. All-pass State Reset
4. Exciter Trigger
5. Pending Start解除

Voice再利用時に前NoteのEnergyを残さない。

## 13.15 Note Off

Physical String内部では特殊なNote Off処理を行わない。 Layer ADSRがNote Offを受け、Generator OutputへReleaseを掛ける。 String自体のNatural DecayはNote保持中も継続する。

## 13.16 Reset

次を完全に初期化する。

- Fractional Delay Buffer
- Write Position
- Loop Filter
- All-pass
- Exciter
- Counter / Seed State

Fresh RuntimeとReset後Runtimeの出力を一致させる。

---

# 14. Modal Generator

## 14.1 方式

Modal Generatorは、Rust側でExciterを生成し、DaisySPの低レベル`Resonator`へ入力する。

```text
PhysicalExciterRuntime
↓
Mono Excitation Buffer
↓
Native Modal Resonator
├─ Mode 1
├─ Mode 2
├─ Mode 3
├─ ...
└─ Mode N
↓
Mono Output
```

DaisySP `ModalVoice`は使用しない。

## 14.2 Mode Count

Definitionの`mode_count`をNative Prepareへ渡す。 許可値：

```text
4 / 8 / 12 / 16 / 20 / 24
```

目安：

| Mode Count | 用途 |
|---:|---|
| 4 | 軽量、単純な共鳴 |
| 8 | 軽量なPercussion |
| 12 | 標準的な音作り |
| 16 | 密度の高いBody |
| 20 | 高品質 |
| 24 | 最大品質 / Bell / Complex Body |

これらの用途表はGuideであり、RuntimeのPresetではない。

## 14.3 Structure

Sonalloy`structure`はDaisySP`SetStructure()`へ直接0..1で渡す。 Pinned DaisySPの`CalcStiff()`はおおむね次の領域を持つ。

```text
0.00 .. 0.25
→ 負方向のStiffness
→ 高次Mode間隔を圧縮

0.25 .. 0.30
→ ほぼHarmonic

0.30 .. 0.90
→ 正方向のStiffnessを増加
→ 高次ModeをStretch

0.90 .. 1.00
→ 非線形に強くStretch
```

AI向け文書では「高いほど単純に明るい」等と説明せず、Mode間隔を変えるParameterであることを記載する。

## 14.4 Brightness

`brightness`はDaisySP`SetBrightness()`へ直接渡す。 Pinned Resonatorでは主に次へ作用する。

- 高周波ModeのAttenuation
- ModeごとのQ Loss
- Structure / Decayとの組み合わせによる高次成分残留

`brightness = 0`は暗いBody、`1`は高次Modeが強く残るBodyとして扱う。

## 14.5 Decay

Sonalloy`decay`をDaisySP`SetDamping()`へ渡す。 Endpoint契約：

```text
decay = 0
→ 最短側 decay = 1
→ 最長側
```

Secondsではない。 DaisySP内部でFrequency、Brightness、StructureとQが相互作用するため、単一の正確なT60秒数として公開しない。 秒単位Decayが必要な音色は、Physical Stringの`decay_seconds`または既存Processor / Envelopeと使い分ける。

## 14.6 Excitation Buffer

`ModalRuntime::render()`は既存Mono ScratchへExciterを生成する。 その同じBufferをNative Modal ResonatorのIn-place Processへ渡す。 追加のBlock BufferをAudio Loop中に確保しない。

## 14.7 Frequency Ramp

Native Wrapperへ次のSpanを渡す。

```text
frequency_start / frequency_end
structure_start / structure_end
brightness_start / brightness_end
decay_start / decay_end
```

C++側はRust`ValueSpan::value_at(index, frames)`と同じ位置式を使う。

```text
position = index / frames
value = start + (end - start) * position
```

これによりBlock分割によるParameter軌跡の差を最小化する。

## 14.8 Reset

DaisySP `Resonator`には公開`Reset()`がない。 Native WrapperのResetは`Init()`を再実行する。

```text
Init(position = 0.015, mode_count, sample_rate)
```

その後のProcessで現在のParameterを再設定する。 ResetをPlacement NewやHandle再生成で実装しない。 Audio Loop中にAllocationさせない。

## 14.9 Note Off

Modal内部でNote Off処理を持たない。 Natural Ringは共鳴Stateへ残るが、Layer ADSR ReleaseがOutputへ掛かる。 Voice Reset時にはResonator Stateを完全に消す。

---

# 15. DaisySP Native Wrapper拡張

## 15.1 Opaque Handle

Headerへ追加する。

```c
typedef struct sonalloy_dsp_modal_resonator sonalloy_dsp_modal_resonator;
```

## 15.2 C++ State

```cpp
struct sonalloy_dsp_modal_resonator {
daisysp::Resonator resonator;
float sample_rate = 0.0f;
int32_t mode_count = 0;
bool prepared = false;
#ifdef SONALLOY_DSP_TEST_HOOKS
bool throw_on_process = false;
#endif
};
```

## 15.3 API

```c
sonalloy_dsp_modal_resonator* sonalloy_dsp_modal_resonator_create(void);
void sonalloy_dsp_modal_resonator_destroy(
sonalloy_dsp_modal_resonator* handle
); int32_t sonalloy_dsp_modal_resonator_prepare(
sonalloy_dsp_modal_resonator* handle,
double sample_rate,
int32_t mode_count
); int32_t sonalloy_dsp_modal_resonator_reset(
sonalloy_dsp_modal_resonator* handle
); int32_t sonalloy_dsp_modal_resonator_process_ramp(
sonalloy_dsp_modal_resonator* handle,
float start_frequency_hz,
float end_frequency_hz,
float start_structure,
float end_structure,
float start_brightness,
float end_brightness,
float start_decay,
float end_decay,
float* buffer,
uint32_t frames
);
```

In-place Bufferとする。

## 15.4 Prepare Validation

次を検査する。

```text
handle != null
sample_rate finite && > 0
mode_count ∈ {4,8,12,16,20,24}
```

成功時：

```cpp
handle->sample_rate = ...;
handle->mode_count = ...;
handle->resonator.Init(0.015f, mode_count, sample_rate);
handle->prepared = true;
```

失敗時は`prepared = false`。

## 15.5 Process Validation

全EndpointについてFiniteを確認する。

```text
frequency > 0
frequency <= sample_rate * 0.45
structure ∈ 0..1
brightness ∈ 0..1
decay ∈ 0..1
buffer != null when frames > 0
prepared == true
```

Input Bufferの全SampleもFinite確認する。

## 15.6 Process Loop

各Sample：

```text
frequency = lerp(start, end, index / frames)
structure = lerp(...)
brightness = lerp(...)
decay = lerp(...) resonator.SetFreq(frequency)
resonator.SetStructure(structure)
resonator.SetBrightness(brightness)
resonator.SetDamping(decay) output = resonator.Process(buffer[index])
```

OutputがNon-FiniteならErrorを返す。

## 15.7 Failure時Output

既存Wrapper方針に合わせ、Process Error時はOutput Bufferを0でClearする。 Native ExceptionをRust側へ伝播させない。

## 15.8 Exception Boundary

Create / Destroy / Prepare / Reset / Processを既存Wrapperと同じ`try/catch (...)`境界で保護する。

## 15.9 Fault Injection

`SONALLOY_DSP_TEST_HOOKS`有効時にModal ProcessでもNative Exceptionを注入できるようにする。 既存Fault Injection Testの構造へ統合する。 別のTest専用Native Libraryを作らない。

---

# 16. `sonalloy-dsp-sys`拡張

## 16.1 File

新規：

```text
crates/sonalloy-dsp-sys/src/modal_resonator.rs
```

## 16.2 Safe Wrapper

外部へ生Pointerを露出しない。 候補API：

```rust
pub struct ModalResonator {
handle: NonNull<ffi::sonalloy_dsp_modal_resonator>,
}
```

```rust
impl ModalResonator {
pub fn new() -> Result<Self, DspError>;
pub fn prepare(&mut self, sample_rate: f64, mode_count: u8) -> Result<(), DspError>;
pub fn reset(&mut self) -> Result<(), DspError>;
pub fn process_ramp(
&mut self,
frequency: ValueRange,
structure: ValueRange,
brightness: ValueRange,
decay: ValueRange,
buffer: &mut [f32],
) -> Result<(), DspError>;
}
```

既存Crateに`ValueRange`等の共有型がなければ、無理に新しい汎用型を作らずstart/end引数でもよい。

## 16.3 Drop

`Drop`でNative Destroyを呼ぶ。

## 16.4 Send / Sync

Native Handleへ根拠なく`Send` / `Sync`を実装しない。 現在のVoice Runtime所有範囲で必要ないなら追加しない。

## 16.5 FFI Declaration

`ffi.rs`へC ABIをそのまま宣言する。 ABI型は`c_float` / `c_double` / `c_int`等、既存Styleへ合わせる。

---

# 17. 決定性・Seed・Voice独立性

## 17.1 必須契約

次が全て同じ場合、WAVは同一でなければならない。

```text
Definition
Events
Sample Rate
Block Size
Tempo
```

Physical Generatorの追加によってこの契約を弱めない。

## 17.2 Note Seed

Noise BurstのNote Seedは、最低限次を混ぜる。

```text
Definition seed
Layer stable hash
Note ID
```

例：

```text
note_seed = splitmix64_finalizer(
definition_seed
^ layer_hash.rotate_left(17)
^ note_id
)
```

具体的なBit Mix方式は既存Random GeneratorのSeed規則と揃えられるならそちらを正本とする。 同じ意味のSeed Derivationを複数Fileへ実装しない。

## 17.3 Voice IndexをSeedへ入れない

Voice StealingやPolyphony設定によって同じNote IDの音色が変わることを避ける。 Voice Slot IndexをRandom Seedへ含めない。

## 17.4 Global Random禁止

禁止：

```text
rand()
thread_rng()
random_device
current_time
process id
address-based seed
```

## 17.5 Reset

Reset後に同じNote IDを与えた場合、Fresh Runtimeと同一のExciter系列へ戻る。

---

# 18. Pitch / Sample Rate / Tuning契約

## 18.1 Sample Rate

最低限次を正式検証対象とする。

```text
44,100 Hz
48,000 Hz
96,000 Hz
```

## 18.2 Physical String低音

MIDI Note 0 + -1200 cents程度までDelay Bufferで直接表現できるようにする。 低音だけ簡易Upsampleへ切り替えるFallbackを作らない。

## 18.3 高音

Fundamentalが`sample_rate * 0.45`を超える場合、Silent Clampではなく既存GeneratorのFrequency Error契約へ合わせる。 Compiler / Inspectで有効上限を確認できるようにする。

## 18.4 Modal高次Mode

Modal Fundamentalが安全範囲でも、高次ModeはNyquistへ近づく。 DaisySP内部はMode Frequencyを0.499 normalizedへClampする。 これはBackend Algorithmの一部として許容する。 ただし、高いFundamentalでModeが大量に同一上限へ押し付けられ音質が破綻していないかReviewする。

## 18.5 Pitch Accuracy

Physical StringはStiffness 0、Brightness中間、十分なDecayで次のPitch Accuracyを目標とする。

```text
55 Hz .. 4 kHz:
Dominant Fundamental Error <= 20 cents
```

ModalはStructureがHarmonic近傍の設定でRoot ModeがNote Pitch近傍へ存在することを確認する。

## 18.6 Tuning Modulation

既存Layer TuningをModulationした場合、Physical Generatorも他Generatorと同様に追従する。 Generator独自Pitch Parameterは追加しない。

---

# 19. Note LifecycleとLayer Envelopeの関係

## 19.1 Note On

```text
Note On
↓
Voice Allocation
↓
Layer Generator Start
├─ String: Exciter Trigger + Feedback State Reset
└─ Modal: Exciter Trigger + Resonator Reset
↓
Layer Envelope Note On
```

## 19.2 Natural Decay

Physical String / Modalは、Layer ADSRとは独立したNatural Decayを持つ。

```text
Generator Natural Decay
×
Layer ADSR
=
Layer Output
```

この二重Envelopeは意図した仕様である。

## 19.3 Sustain

Note保持中にPhysical GeneratorのNatural Energyが消えても、Generatorを自動再励振しない。 今回Continuous Excitation / Sustain Bow / Breathは実装しない。

## 19.4 Note Off Trigger Layer

既存`LayerTriggerEvent::NoteOff`もPhysical Generatorにそのまま適用可能とする。 Note Off時にLayerが開始された場合、その時点でFresh ExciterをTriggerする。

## 19.5 Voice Stealing

Steal完了後に新しいNoteが開始される際、Physical Stateは前Noteから完全にResetされる。 既存Steal Fadeの前後関係を変更しない。

---

# 20. Realtime Safety / Memory / Performance

## 20.1 Audio Loop Allocation

以下の関数経路でHeap Allocationを禁止する。

```text
InstrumentRuntime::process
VoiceRuntime::render_span
GeneratorRuntime::render
PhysicalStringRuntime::render
ModalRuntime::render
PhysicalExciterRuntime::next_sample
ModalResonator::process_ramp
```

BufferはPrepare / Runtime Construction時に確保する。

## 20.2 Physical String Memory

Delay Line BufferはVoice × Physical String Layer単位。 `max_delay_frames`はSample Rateから一度計算する。 Render中にPitchに合わせてResizeしない。

## 20.3 Modal Memory

DaisySP Resonatorは固定StateをHandle内に持つ。 Mode Count変更のためにRender中再Initしない。 `mode_count`はStatic Definition Fieldであり、変更にはRecompileが必要。

## 20.4 Transcendental Function

Physical Stringでは、Static Spanに対して`powf` / `exp` / `cos`を毎Sample再計算しない。 ModalはPinned DaisySP内部アルゴリズムが毎SampleMode係数を計算するため、その部分はBackend特性として受け入れる。

## 20.5 Performance Review

Release Buildで最低限次を測定する。

### Physical String

```text
1 voice
8 voices
16 voices
32 voices
```

### Modal

```text
mode_count 12 × 1 / 8 / 16 voices
mode_count 24 × 1 / 8 / 16 voices
```

### Sample Rate

```text
48 kHz
96 kHz
```

## 20.6 Performance Gate

CI Machineの絶対時間をHard Assertしない。 Review MetricsとしてRealtime Ratioを保存する。 開発Reference環境では、48 kHz / 16 Voiceの代表DefinitionがRealtimeより速いことを最低目標とする。

```text
realtime_ratio < 1.0
```

未達の場合は、Mode Count標準値、係数更新、FFI呼出粒度を再確認する。 音を削るだけのためにMode Countを固定4へ落とさない。

## 20.7 FFI粒度

Modalは1 SampleごとにRust→Cを呼ばない。 1 Render Spanにつき1回のNative Block Callとする。

---

# 21. Error / Diagnostic

## 21.1 Definition Diagnostic

既存Diagnostic Styleへ統合する。 必要な不正条件：

### Physical Exciter

- duration non-finite
- duration out of range
- brightness non-finite
- brightness out of range

### Physical String

- decay non-finite / out of range
- brightness non-finite / out of range
- stiffness non-finite / out of range

### Modal

- mode_count unsupported
- structure non-finite / out of range
- brightness non-finite / out of range
- decay non-finite / out of range

既存の汎用Range Diagnosticで表現可能なら、新しいCodeを乱立させない。

## 21.2 Field Path

Path例：

```text
layers[0].generator.physical_string.decay_seconds
layers[0].generator.physical_string.exciter.duration_seconds
layers[1].generator.modal.mode_count
layers[1].generator.modal.structure
```

## 21.3 Runtime Error

次は`ProcessError`へ変換する。

- Non-Finite target
- Invalid frequency
- Delay read/write error
- Native modal error
- Native non-finite output
- Runtime/Compiled Variant mismatch

## 21.4 Native Error

既存`DspError` / Result Code Contractを再利用する。 Physical専用Error Enumを別に作らない。

---

# 22. Inspect / Analysis / Trace

## 22.1 Inspect

現行AI Authoring WorkflowではInspectがParameterの意味をAIへ公開する正規Interfaceである。 新Generator追加後、Inspectから最低限次が確認できるようにする。

### Physical String

```text
kind: physical_string
output_mode: mono
exciter type
exciter static values
parameter descriptors
```

### Modal

```text
kind: modal
output_mode: mono
mode_count
exciter type
parameter descriptors
```

## 22.2 Parameter Descriptor

Inspectに既に表示される次の情報が、新Parameterでも正しく出ることを確認する。

- Native Unit
- Scale
- Min / Max
- Default
- Smoothing
- Modulation Unit
- Max Modulation Depth

## 22.3 Route Effect

新ParameterをTargetとするRouteについて、Reachable Range / Clamp表示が既存と同様に働く。 Physical固有のRoute計算をCLIに書かない。

## 22.4 Trace

次をTrace可能とする。

```text
physical_string_decay_seconds
physical_string_brightness
physical_string_stiffness
modal_structure
modal_brightness
modal_decay
```

Runtimeが実際に使用したFinal Parameter値とTraceの`final_value`が同じ正本から計算されること。

## 22.5 Audio Analysis

Physical Reviewでも既存`--analyze`を利用する。 最低限見る値：

- Finite
- Peak
- RMS
- DC
- Activity last frame
- Continuity
- Spectral peaks
- Spectral centroid
- Harmonic energy ratio（Stringの特定Test）

新しい外部Python FFTを通常確認のために作らない。 Review Packageで追加の専用計測が必要な場合のみ、既存Review Scriptへ限定して追加する。

---

# 23. CLI / Documentation / Agent Skill

## 23.1 CLI Command

新しいTop-level Commandは追加しない。 既存：

```text
instrument validate
instrument inspect
render note
render events
render midi
```

からそのまま利用できる。

## 23.2 `docs/instrument-definition.md`

新しいGenerator節を追加する。 説明順：

1. Physical Stringが何を表すか
2. JSON
3. Static Field
4. Dynamic Parameter
5. Parameter ID
6. Mapping式
7. 音作りの方向
8. 制約

Modalも同じ構造にする。

## 23.3 AI向け数値説明

AI AgentがSourceを読まなくても次を理解できる記述を追加する。

### Physical String

- `decay_seconds`はNominal Loop T60
- `brightness`のCutoff Mapping
- `stiffness`のAll-pass係数Mapping
- Exciter Brightness Mapping

### Modal

- `structure`の領域ごとのMode間隔傾向
- `brightness`の役割
- `decay` 0 / 1 Endpoint
- `mode_count`がQuality / CPU / Densityへ影響すること

## 23.4 `.agents/skills/create-instrument/SKILL.md`

Generator選択Guidanceへ追加する。

```text
Physical String
→ pluck、string、harp、koto-like、metallic string、synthetic string Modal
→ bell、bar、plate、mallet、glass、metal、wood、membrane-like
```

「リアルなGuitar / Piano / Violinを作るGenerator」と説明しない。

## 23.5 `README.md`

特長の合成方式一覧へPhysical / Modalを追加する。 詳細DSP式はREADMEへ書かない。

## 23.6 `docs/runtime-processing.md`

Generator Runtime StateとNative Modal Backendを反映する。

## 23.7 `docs/architecture.md`

DaisySP Wrapper利用範囲へModal Resonatorを追加する。 Rust Physical StringとNative Modalの責務分担を一度だけ記載する。

## 23.8 `docs/testing-and-sound-review.md`

Physical / ModalのReview観点を既存節へ統合する。 別の重複したReview規則文書を作らない。

## 23.9 `THIRD_PARTY_NOTICES.md`

既存DaisySP Usageを更新する。 例：

```text
Usage:
Basic Oscillator / Hard Sync / Filter / Wavefolder / Modal Resonator
```

固定CommitとLicenseは変更しない。

---

# 24. Unit Test戦略

## 24.1 Definition

### Physical String

- Valid Definition parses
- JSON round trip
- Unknown field reject
- decay min / max
- brightness min / max
- stiffness min / max
- non-finite reject

### Modal

- Valid Definition parses
- JSON round trip
- mode_count全許可値
- 不許可mode_count reject
- structure / brightness / decay ranges

### Exciter

- impulse parse
- noise_burst parse
- duration min / max
- brightness range
- unknown field reject

## 24.2 Parameter Catalog

Physical String：

```text
layer.<id>.generator.physical_string_decay_seconds
layer.<id>.generator.physical_string_brightness
layer.<id>.generator.physical_string_stiffness
```

Modal：

```text
layer.<id>.generator.modal_structure
layer.<id>.generator.modal_brightness
layer.<id>.generator.modal_decay
```

次を確認する。

- stable order
- owner = LayerGenerator
- unit
- scale
- min / max
- default
- smoothing
- modulation unit

`mode_count`やExciter FieldがParameter登録されていないことも一度確認する。

## 24.3 Exciter

### Impulse

- Trigger後最初のSampleのみ非Zero
- 以降0
- Reset後同じ

### Noise Burst

- 同Seed + 同Note ID = 同列
- 異Seed = 異列
- 異Note ID = 異列
- Duration後0
- Brightness変更でSpectrum / Sample列が変わる
- Output finite

## 24.4 Fractional Delay

移動後も既存Testを維持する。

- integer delay
- fractional interpolation
- reset
- invalid delay

移動だけでTestを重複させない。

## 24.5 Physical String

最低限次をUnit Testする。

### Identityではなく発音

Impulse Exciter後に一定時間内で非Zero出力が存在する。

### Decay

Long DecayのTail EnergyがShort Decayより明確に大きい。

```text
energy(long) > energy(short) * threshold
```

閾値はTest実装時に音響的意味を持つ値へ固定し、単に通るまで下げない。

### Brightness

同Note / 同DecayでHigh BrightnessのSpectral Centroidまたは高域EnergyがLow Brightnessより高い。

### Stiffness

Stiffness 0と1で高次Peakの配置が異なる。 単なるWaveform inequalityだけで終わらせず、可能なら高次成分のFrequency差を見る。

### Pitch

Stiffness 0でFundamentalが期待Pitchから20 cents以内。

## 24.6 Modal Native Wrapper

C++ / Rust境界で次を確認する。

- create / destroy
- prepare valid
- invalid sample rate
- invalid mode count
- process before prepare
- parameter range reject
- null / empty buffer contract
- reset
- finite output
- injected native exception

## 24.7 Modal Runtime

### Mode Count

4と24で出力が有限であり、Spectrum Densityが異なる。

### Structure

Harmonic近傍とStrong Stretch設定でPeak配置が変わる。

### Brightness

High設定で高域Energyが増える。

### Decay

High Decay設定のTail EnergyがLow設定より大きい。

### Reset

Fresh Runtimeと一致。

---

# 25. Integration Test戦略

## 25.1 Generator Compile

一つのInstrumentに次を含める。

```text
Layer 1 = Physical String
Layer 2 = Modal
```

Validate / Compile / Render成功を確認する。

## 25.2 Processor組み合わせ

既存Processorとの結合を最低限次で確認する。

```text
Physical String
-> Layer EQ
-> Layer Resonator Modal
-> Layer Drive Voice Compressor
Global Chorus
Global Reverb
Global Limiter
```

FiniteであることとProcessor順が維持されることを確認する。

## 25.3 Parameter Change

Event Sequenceで発音中に次を変更する。

```text
physical_string_brightness
physical_string_stiffness
modal_structure
modal_decay
```

変更後の出力がBaselineと異なり、Finiteであることを確認する。

## 25.4 Modulation Route

最低限次を通す。

```text
LFO -> physical_string_stiffness
Envelope -> physical_string_brightness
Velocity -> modal_structure
ModWheel -> modal_decay
```

TraceでFinal Valueを確認する。

## 25.5 Note Off Trigger

Physical String / Modalを`event = note_off`のLayerとして使い、Note Off時に正しくExciterがTriggerされることを確認する。

## 25.6 Polyphony

複数Note IDを重ね、各VoiceのExciter / Delay / Resonator Stateが干渉しないことを確認する。 同一PitchでもNote IDが異なればNoise Burst系列が独立する。

## 25.7 Voice Stealing

Polyphony 1または2で新Noteを連続投入し、前NoteのPhysical Stateが新Noteへ漏れないことを確認する。

---

# 26. Block Size / Sample Rate / Reset検証

## 26.1 Block Size

正式比較：

```text
32
64
257
1024
```

同じDefinition / EventsでRenderし、Reference 257と比較する。

## 26.2 Block Size許容差

Floating Point処理やNative Parameter RampによりBit Exactでない可能性がある。 目標：

```text
max_abs_difference <= 1e-4
RMS_difference <= 1e-5
```

これを超える場合は、まずParameter Ramp / Exciter Counter / Native Wrapper位置式を疑う。 音が似ているから許容する判断は行わない。

## 26.3 Sample Rate

```text
44.1 kHz
48 kHz
96 kHz
```

各Rateで：

- finite
- Pitch Accuracy
- Natural Decay
- no crash
- no invalid delay
- no Native Error

を確認する。

## 26.4 Reset Reproducibility

```text
fresh runtime render
vs
render -> reset -> same render
```

Physical String / Modalを含むFull Chainで比較する。 目標：

```text
max_abs_difference = 0
```

決定的RandomまでResetできていればBit Exactを期待する。

## 26.5 Repeat Reproducibility

別Runtimeを二つ作り、同じDefinition / Eventsから生成したWAVのHashが一致することを確認する。

---

# 27. Sound Review Package

## 27.1 Directory

新規：

```text
review/physical-modal/
```

構造は既存Review Packageへ合わせる。

```text
review/physical-modal/
├─ definitions/
├─ audio/
│  ├─ technical/
│  └─ musical/
├─ inspect/
├─ metrics.json
└─ review-summary.md
```

既存Projectの命名規則に合わせて微調整してよいが、同じ成果物を別Directoryへ重複生成しない。

## 27.2 Generate Script

新規候補：

```text
review/generate/generate_physical_modal_package.py
```

既存`review/generate/common.py`とCLI Analysisを利用する。 同じWAV計測関数をコピーしない。

## 27.3 Technical Definition

最低限次を生成する。

### Physical String

```text
string_impulse
string_noise_soft
string_noise_bright
string_short_decay
string_long_decay
string_soft
string_bright
string_low_stiffness
string_high_stiffness
```

### Modal

```text
modal_4_modes
modal_12_modes
modal_24_modes
modal_harmonic_structure
modal_stretched_structure
modal_dark
modal_bright
modal_short_decay
modal_long_decay
modal_impulse
modal_noise_burst
```

## 27.4 Musical Definition

最低限次の3音色を作る。

### `physical_pluck`

目的：

- StringのPitch感
- Natural Decay
- Brightness
- EQ / Reverbとの組み合わせ

### `modal_mallet`

目的：

- Wood / Bar的Attack
- Mode Density
- Body Decay

### `imaginary_metal_body`

目的：

- Physical String + Modal Layer
- 既存ProcessorとのHybrid
- 実在楽器再現ではない音作り

## 27.5 Review Metrics

自動計測：

- Validate status
- Inspect success
- Finite
- Peak / RMS / DC
- Activity
- Continuity
- Spectrum Peaks
- Spectral Centroid
- Block Size comparison
- Sample Rate comparison
- Reset comparison
- Repeat Hash
- Render Time

## 27.6 Physical String専用Metric

可能ならReview Scriptで、対象WAVの既知Note Frequencyに対するPeak Error centsを記録する。 既存`--analyze` Spectrum Peakから十分計算できる場合、その出力を利用する。 別FFT実装を作らない。

## 27.7 Human Listening

`review-summary.md`へ次を記録する。

| 対象 | 確認内容 |
|---|---|
| String Pitch | 音程が不自然に揺れたり外れたりしない |
| String Decay | Short / Longが自然なEnergy Lossとして聞こえる |
| String Brightness | 単なるVolume差ではなく高域Lossが変化する |
| String Stiffness | HarmonicからMetallic方向へ自然に変化する |
| Modal Structure | Mode間隔のCharacterが明確に変わる |
| Modal Brightness | 高次Modeの存在感が変わる |
| Modal Decay | 共鳴Tailの長さが変わる |
| Mode Count | 4 / 12 / 24で密度差が聞こえる |
| Musical 3種 | Instrumentとして使える音になっている |
| Block Size | Click / Timing差がない |
| Sample Rate | 大きな音色破綻がない |

Metrics合格のみでSound Review完了としない。

---

# 28. Review用Definitionと試聴観点

## 28.1 Plucked String方向

例：

```json
{
"physical_string": {
"exciter": {
"type": "noise_burst",
"duration_seconds": 0.004,
"brightness": 0.8,
"seed": 1
},
"decay_seconds": 2.5,
"brightness": 0.65,
"stiffness": 0.08
}
}
```

狙い：

- Harp / Koto / Pluck系の基準
- Stiffnessを低く保ちHarmonic感を見る

## 28.2 Metallic String方向

```text
stiffness: 0.7 .. 1.0
brightness: 0.7 .. 1.0
```

狙い：

- Piano Wireそのものではなく、硬いSynthetic String
- 高次成分の非整数化

## 28.3 Wood / Bar方向

Modal：

```text
mode_count: 12
structure: harmonic近傍から少しずらす
brightness: medium
short-to-medium decay
noise burst exciter
```

## 28.4 Bell / Plate方向

Modal：

```text
mode_count: 24
structure: high
brightness: high
long decay
impulse or short noise burst
```

## 28.5 Membrane-like方向

Modal Structureを低側へ動かし、高次Mode間隔が圧縮される領域を試す。 「Drum Model」として評価しない。

## 28.6 Hybrid

```text
Layer A: Physical String
Layer B: Modal
Layer C: Noise or Wavetable Voice EQ / Compressor
Global Chorus / Reverb / Limiter
```

Physical方式が既存SonalloyのHybrid Layerへ自然に統合できることを確認する。

---

# 29. File単位の変更計画

## 29.1 `crates/sonalloy-core/src/definition.rs`

追加：

- `GeneratorDefinition::PhysicalString`
- `GeneratorDefinition::Modal`
- `PhysicalStringDefinition`
- `ModalDefinition`
- `PhysicalExciterDefinition`
- Validation

Schema Versionは2を維持。

## 29.2 `crates/sonalloy-core/src/generator_parameters.rs`

追加：

```text
PHYSICAL_STRING_DECAY
PHYSICAL_STRING_BRIGHTNESS
PHYSICAL_STRING_STIFFNESS
MODAL_STRUCTURE
MODAL_BRIGHTNESS
MODAL_DECAY
```

`is_suffix()`更新。

## 29.3 `crates/sonalloy-core/src/parameter.rs`

`push_generator_descriptors()`へPhysical String / Modal追加。 既存Parameter Unit / Modulation Unit Enumは変更不要。

## 29.4 `crates/sonalloy-core/src/compiler.rs`

追加：

- Compiled Physical Exciter
- Compiled Physical String
- Compiled Modal
- Compiled Parameter handles
- `CompiledGenerator` variants
- output mode
- latency
- availability
- compile branches
- frequency limit正本

## 29.5 `crates/sonalloy-core/src/runtime/modulation.rs`

追加：

- `LayerGeneratorTargetSpan::PhysicalString`
- `LayerGeneratorTargetSpan::Modal`
- zero target span

## 29.6 `crates/sonalloy-core/src/runtime/voice.rs`

追加：

- Target評価
- Exhaustive Generator match更新

既存Voice State / Allocation方式は変更しない。

## 29.7 `crates/sonalloy-core/src/runtime/instrument.rs`

Note Layer SelectionのProcedural Generator一覧へPhysical String / Modal追加。 Trace / Event Systemの構造は変更しない。

## 29.8 `crates/sonalloy-core/src/runtime/generator/mod.rs`

追加：

```text
mod physical_exciter;
mod physical_string;
mod modal;
```

`GeneratorRuntime`全Match更新。

## 29.9 `crates/sonalloy-core/src/runtime/generator/physical_exciter.rs`

新規。

- Prepare
- Trigger
- deterministic Noise
- Low-pass
- exponential Burst
- reset

## 29.10 `crates/sonalloy-core/src/runtime/generator/physical_string.rs`

新規。

- Delay Feedback
- T60 feedback
- brightness loop loss
- stiffness all-pass
- coefficient cache
- reset

## 29.11 `crates/sonalloy-core/src/runtime/generator/modal.rs`

新規。

- Exciter generation
- Native Modal wrapper
- target validation
- frequency span
- reset

## 29.12 Fractional Delay

移動：

```text
runtime/processor/fractional_delay.rs
→ runtime/fractional_delay.rs
```

更新：

```text
processor/resonator.rs
processor/chorus.rs
processor/flanger.rs
```

アルゴリズム変更は含めない。

## 29.13 `native/daisysp-wrapper/CMakeLists.txt`

DaisySP targetへ`resonator.cpp`追加。

## 29.14 `native/daisysp-wrapper/include/sonalloy_dsp.h`

Modal Resonator C ABI追加。

## 29.15 `native/daisysp-wrapper/src/daisysp_wrapper.cpp`

Modal Opaque Handle / Validation / Lifecycle / Process追加。

## 29.16 `crates/sonalloy-dsp-sys/src/ffi.rs`

Modal FFI Declaration追加。

## 29.17 `crates/sonalloy-dsp-sys/src/modal_resonator.rs`

新規Safe Wrapper。

## 29.18 `crates/sonalloy-dsp-sys/src/lib.rs`

必要なModal WrapperのみExport。 公開範囲は既存Styleに合わせ最小化する。

## 29.19 Test

候補：

```text
crates/sonalloy-core/tests/physical_modal.rs
```

既存Integration Testへ自然に追加できる観点はそちらへ統合する。 同一観点をUnit / Integration両方で過剰に重複させない。

## 29.20 Review

```text
review/physical-modal/
review/generate/generate_physical_modal_package.py
```

## 29.21 Documentation

更新：

```text
README.md
docs/CONCEPT.md           # 既存要件との表現整合のみ。新要件追加ではない
docs/instrument-definition.md
docs/runtime-processing.md
docs/architecture.md
docs/testing-and-sound-review.md
.agents/skills/create-instrument/SKILL.md
THIRD_PARTY_NOTICES.md
```

`docs/CONCEPT.md`は既にPhysical / Modalを要求しているため、今回実装した具体名を必要箇所へ自然に反映するだけとする。

---

# 30. 実装順序

実装順序は依存関係に沿って進める。

## 30.1 Definition / Parameter Contract

1. `PhysicalExciterDefinition`
2. `PhysicalStringDefinition`
3. `ModalDefinition`
4. Validation
5. GeneratorParameterSpec
6. Parameter Catalog
7. Serialization / Validation Test

ここでJSON契約を確定する。

## 30.2 Compiler

1. Compiled types
2. Compile helper
3. `CompiledGenerator` branches
4. Output Mode / Latency / Availability
5. Compiler Unit Test

## 30.3 Runtime共通基盤

1. Fractional Delayを共通位置へ移動
2. Existing Processor Testを通す
3. Physical Exciter Runtime
4. Exciter Unit Test

Fractional Delay移動とPhysical DSP実装を同時に行わず、既存Processor回帰を先に切り分けられる状態にする。

## 30.4 Physical String

1. Runtime State
2. Constant Parameter path
3. Decay
4. Brightness
5. Stiffness / Group Delay補正
6. Dynamic Span
7. Reset
8. Unit Test
9. Parameter Change / Modulation Integration

## 30.5 Native Modal Primitive

1. CMake Source追加
2. C Header
3. C++ Wrapper
4. Rust FFI
5. Safe Wrapper
6. Native Unit / Fault Test

この段階でModal Generatorへまだ統合しない。

## 30.6 Modal Generator

1. Runtime State
2. Exciter接続
3. Constant Parameter path
4. Dynamic Span
5. Reset
6. Unit Test
7. Parameter Change / Modulation Integration

## 30.7 Instrument統合

1. Note Selection
2. Voice Stealing
3. Note Off Trigger
4. Polyphony
5. Inspect
6. Trace

## 30.8 Documentation / Review

1. instrument-definition
2. runtime-processing
3. architecture
4. testing-and-sound-review
5. agent skill
6. README
7. third-party notices
8. Review Package
9. Human Listening

## 30.9 最終自己レビュー

実装Agentは、Test成功だけで完了しない。 最低限次を敵対的に確認する。

- Exhaustive Match漏れ
- Hidden Allocation
- Global Random
- Duplicate Range Literal
- Duplicate Frequency Formula
- Native Error握り潰し
- Parameter登録漏れ
- Traceと実値の乖離
- Reset漏れ
- Dead Code
- Testからしか呼ばれない不要API
- Documentationの古いGenerator一覧
- Third-party Notice

---

# 31. 完了条件

## 31.1 Definition

- `physical_string`がSchema 2でValidate / Round Tripできる
- `modal`がSchema 2でValidate / Round Tripできる
- Unknown Field reject
- Static / Dynamic Fieldの区分が正しい
- `mode_count`は指定6値のみ

## 31.2 Parameter / Modulation

- 6 ParameterがCatalogへ正しいUnit / Scale / Rangeで登録される
- Parameter Changeが機能する
- Modulation Routeが機能する
- InspectがUnit / Reachable Rangeを表示する
- TraceがFinal Valueを表示する

## 31.3 Physical String

- Impulse / Noise Burstで発音
- Pitch追従
- Decay差
- Brightness差
- Stiffness差
- 44.1 / 48 / 96 kHz
- Block Size独立
- Reset再現
- Polyphony独立
- Voice Stealing安全
- Audio Loop Allocationなし

## 31.4 Modal

- 4 / 8 / 12 / 16 / 20 / 24 Mode動作
- Impulse / Noise Burstで発音
- Structure差
- Brightness差
- Decay差
- Pitch追従
- 44.1 / 48 / 96 kHz
- Block Size独立
- Reset再現
- Native Fault安全
- Audio Loop Allocationなし

## 31.5 Dependency / License

- 新規外部Dependency 0
- DaisySP fixed commit変更なし
- Build Sourceは`resonator.cpp`だけ追加
- DaisySP-LGPL不使用
- THIRD_PARTY_NOTICES更新

## 31.6 CI

最低限：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git diff --check
```

既存Native Fault Injection / Sanitizer / Windows / macOS / Linux CIも全て成功する。

## 31.7 Review Package

- 全Definition Validate
- 全WAV Finite
- Metrics生成
- Block Size比較
- Sample Rate比較
- Reset比較
- Repeat Hash
- Performance Metric
- Inspect保存
- Human Listening記録

## 31.8 Sound Completion

Technical Testが通っただけでは完了としない。 少なくとも次の3音色を人間が試聴する。

```text
physical_pluck
modal_mallet
imaginary_metal_body
```

「実在楽器にそっくりか」ではなく、本書3.2の品質基準で評価する。

---

# 32. 次フェーズへ残すもの

今回の結果を踏まえ、次に検討可能なPhysical系機能は次。 優先順位はこの計画内で固定しない。

## 32.1 Wind / Tube Waveguide

必要要素：

- Bore Delay
- Reflection Filter
- Reed / Jet / Lip Exciter
- Nonlinear Junction

単純Tube Delayとして先行実装しない。

## 32.2 Bowed String

必要要素：

- Bow Velocity
- Bow Pressure
- Nonlinear Friction
- String Waveguide Coupling

## 32.3 Coupled Body

Physical String出力を複数Modal Bodyへ内部接続する方式。 現在はLayerを分けてHybrid化できるため、必要性を実際の音作りから判断する。

## 32.4 Sympathetic Resonance

複数String / Voice間のEnergy Coupling。 Voice境界を跨ぐため別設計とする。

## 32.5 Convolution Body

IR Asset契約、Partition、Latencyが必要。 Processor Expansionの後続として別計画にする。

## 32.6 Detailed Acoustic Models

Guitar、Piano、Violin等の完全Modelは`docs/CONCEPT.md`の意図的非対象を維持する。

---

# 33. 実装Agent向け最終ルール

1. `docs/CONCEPT.md`と本書を正本として扱う。
2. 現在の`main`を読まず古いPlanのSchema 1前提を持ち込まない。
3. `schema_version = 2`を維持する。
4. Physical StringとModalを直接`GeneratorDefinition` Variantへ追加する。
5. Generic Physical Graphを作らない。
6. 新しい外部Dependencyを追加しない。
7. DaisySP固定Commitを変更しない。
8. DaisySPから追加Buildするのは`PhysicalModeling/resonator.cpp`だけとする。
9. DaisySP `ModalVoice`を使わない。
10. DaisySP `StringVoice` / `KarplusString`を使わない。
11. C標準`rand()`を使わない。
12. Exciter RandomはSonalloyの決定的Random Helperを使う。
13. Voice Slot IndexをRandom Seedへ入れない。
14. Existing Fractional Delayを複製せず共通位置へ移す。
15. Fractional Delay移動時に既存Processor Algorithmを変更しない。
16. Audio LoopでAllocationしない。
17. Physical String Delay BufferはPrepare時にSample Rateから確保する。
18. Coefficientが静的なら毎Sample再計算しない。
19. Modalは1 Blockにつき1 Native Callとする。
20. Native ExceptionをC ABI外へ出さない。
21. Native Error時は既存Result Code契約へ変換する。
22. 新Dynamic Parameterは全て`GeneratorParameterSpec`へ登録する。
23. Parameter RangeをRuntimeへLiteralで再定義しない。
24. Dynamic Parameterは既存`evaluate_target()`を通す。
25. InspectとTraceへ独自計算を追加せずCore契約を再利用する。
26. `mode_count`とExciter Fieldを誤ってDynamic Parameterにしない。
27. Physical String / ModalはMono Generatorとする。
28. Generator独自Stereo Spreadを追加しない。
29. Intrinsic Latencyは0とする。
30. Note OffのNatural Decay処理を別Envelopeとして増やさない。
31. Layer ADSRとの二重Decayを仕様として維持する。
32. ResetでRandom / Delay / Resonator / Filter Stateを全て戻す。
33. Block Size 32 / 64 / 257 / 1024を比較する。
34. Sample Rate 44.1 / 48 / 96 kHzを比較する。
35. Physical StringのPitch Accuracyを数値で確認する。
36. ModalのMode Count / Structure差をSpectrumで確認する。
37. Parameter ChangeとModulation Routeを実際のRenderで確認する。
38. Note Off Trigger Layerを確認する。
39. Voice Stealingを確認する。
40. Human Listeningを省略しない。
41. Sound Reviewの目的を「本物の楽器再現」へ変えない。
42. Dead Codeを残さない。
43. `#[allow(dead_code)]`で未使用コードを隠さない。
44. 将来用Trait / Registryを追加しない。
45. Public APIを必要以上に増やさない。
46. 新しい名前や数値MappingはAI AgentがSourceを読まず理解できるよう文書化する。
47. Testを通すためだけにRange / Thresholdを緩めない。
48. Review Scriptで製品Interfaceと同じ分析を重複実装しない。
49. 実装後にREADME / docs / Agent Skill / Third-party Noticeを一貫した現行状態へ更新する。
50. 最終自己レビューでは、本フェーズだけでなく既存Generator / Processorへの回帰も確認する。

---

# 34. 参考資料

## 34.1 Sonalloy

- `docs/CONCEPT.md`
- `docs/instrument-definition.md`
- `docs/runtime-processing.md`
- `docs/architecture.md`
- `docs/testing-and-sound-review.md`
- `docs/plan/plan-processor-expansion.md`
- `docs/plan/plan-ai-instrument-authoring.md`
- `crates/sonalloy-core/src/definition.rs`
- `crates/sonalloy-core/src/compiler.rs`
- `crates/sonalloy-core/src/generator_parameters.rs`
- `crates/sonalloy-core/src/parameter.rs`
- `crates/sonalloy-core/src/runtime/generator/mod.rs`
- `crates/sonalloy-core/src/runtime/modulation.rs`
- `crates/sonalloy-core/src/runtime/voice.rs`
- `crates/sonalloy-core/src/runtime/instrument.rs`
- `crates/sonalloy-core/src/runtime/processor/fractional_delay.rs`
- `native/daisysp-wrapper/CMakeLists.txt`
- `native/daisysp-wrapper/include/sonalloy_dsp.h`
- `native/daisysp-wrapper/src/daisysp_wrapper.cpp`
- `THIRD_PARTY_NOTICES.md`

## 34.2 DaisySP — Sonalloy固定Commit

固定Commit：

```text
a0494a3adb67f549e18dfd71a35fa656f65b38b6
```

参照対象：

- `Source/PhysicalModeling/resonator.h`
- `Source/PhysicalModeling/resonator.cpp`
- `Source/PhysicalModeling/modalvoice.h`
- `Source/PhysicalModeling/modalvoice.cpp`
- `Source/PhysicalModeling/KarplusString.h`
- `Source/PhysicalModeling/KarplusString.cpp`
- `Source/PhysicalModeling/stringvoice.h`
- `Source/PhysicalModeling/stringvoice.cpp`
- `Source/Noise/dust.h`
- `LICENSE`

## 34.3 依存判断で比較したもの

### DaisySP Resonator

採用。 理由：

- 既存Dependency
- MIT
- Internal Randomなし
- Modal Bodyとして独立
- 24 Mode
- Sonalloy Exciterと分離可能

### DaisySP ModalVoice

不採用。 理由：

- Exciterまで固定
- Dust / Global `rand()`へ依存
- Sonalloy Seed契約と不一致

### DaisySP String / StringVoice

不採用。 理由：

- Global `rand()`
- 1024固定Delay Line
- 低音Fallback
- 固定Commit API挙動の不整合
- Rust既存Primitiveでより明示的に実装可能

### STK

不採用。 理由：

- 有力でLicense上の大きな障害はない
- しかし第二の大型Native Dependencyになる
- 今回の二方式だけでは導入便益が小さい
- 生楽器個別ModelへScopeが引っ張られる

---

# Appendix A. Definition例

## A.1 Physical String単体

```json
{
"schema_version": 2,
"metadata": {
"name": "Physical Pluck",
"author": null,
"description": "Deterministic waveguide pluck"
},
"performance": {
"polyphony": 16,
"voice_stealing": "quietest_releasing_then_oldest"
},
"layers": [
{
"id": "string",
"enabled": true,
"trigger": {
"event": "note_on",
"key_min": 0,
"key_max": 120,
"velocity_min": 1,
"velocity_max": 127
},
"gain_db": -8.0,
"pan": 0.0,
"tuning_cents": 0.0,
"envelope": {
"attack_seconds": 0.0,
"decay_seconds": 0.0,
"sustain_level": 1.0,
"release_seconds": 0.1
},
"generator": {
"physical_string": {
"exciter": {
"type": "noise_burst",
"duration_seconds": 0.006,
"brightness": 0.82,
"seed": 4001
},
"decay_seconds": 2.4,
"brightness": 0.68,
"stiffness": 0.18
}
},
"processors": []
}
],
"voice_processors": [],
"global_processors": [],
"modulation": null
}
```

実装時は現行Schema 2の正確なSerialize Shapeと照合し、上記に既存Field差がある場合は現行Definition側を正とする。

## A.2 Modal単体

```json
{
"schema_version": 2,
"metadata": {
"name": "Modal Body",
"author": null,
"description": "Deterministic modal resonator instrument"
},
"performance": {
"polyphony": 16,
"voice_stealing": "quietest_releasing_then_oldest"
},
"layers": [
{
"id": "body",
"enabled": true,
"trigger": {
"event": "note_on",
"key_min": 0,
"key_max": 108,
"velocity_min": 1,
"velocity_max": 127
},
"gain_db": -12.0,
"pan": 0.0,
"tuning_cents": 0.0,
"envelope": {
"attack_seconds": 0.0,
"decay_seconds": 0.0,
"sustain_level": 1.0,
"release_seconds": 0.15
},
"generator": {
"modal": {
"exciter": {
"type": "noise_burst",
"duration_seconds": 0.010,
"brightness": 0.58,
"seed": 9102
},
"mode_count": 24,
"structure": 0.72,
"brightness": 0.76,
"decay": 0.66
}
},
"processors": []
}
],
"voice_processors": [],
"global_processors": [],
"modulation": null
}
```

---

# Appendix B. Test Matrix

| ID | Generator | Sample Rate | Block | Voices | 観点 |
|---|---|---:|---:|---:|---|
| S01 | String | 44.1k | 32 | 1 | Basic |
| S02 | String | 48k | 64 | 1 | Basic |
| S03 | String | 48k | 257 | 1 | Reference |
| S04 | String | 48k | 1024 | 1 | Block |
| S05 | String | 96k | 257 | 1 | Sample Rate |
| S06 | String | 48k | 257 | 16 | Polyphony |
| S07 | String | 48k | 257 | 32 | Performance |
| S08 | String | 96k | 257 | 16 | Memory / CPU |
| M01 | Modal 4 | 44.1k | 32 | 1 | Basic |
| M02 | Modal 12 | 48k | 64 | 1 | Basic |
| M03 | Modal 24 | 48k | 257 | 1 | Reference |
| M04 | Modal 24 | 48k | 1024 | 1 | Block |
| M05 | Modal 24 | 96k | 257 | 1 | Sample Rate |
| M06 | Modal 12 | 48k | 257 | 16 | Polyphony |
| M07 | Modal 24 | 48k | 257 | 16 | Performance |
| M08 | Modal 24 | 96k | 257 | 16 | Worst representative |
| H01 | String + Modal | 48k | 257 | 8 | Hybrid |
| H02 | Full Chain | 48k | 257 | 8 | Processor integration |
| H03 | Full Chain | 48k | 32/64/257/1024 | 8 | Block independence |
| H04 | Full Chain | 44.1/48/96k | 8 | Sample Rate |
| H05 | Full Chain | 48k | 257 | 8 | Reset / Repeat |

---

# Appendix C. Review時に禁止する誤判定

次を「実装成功」の理由にしない。

```text
WAVが生成できた
Finiteだった
CIがGreenだった
違うParameterでWaveformが違った
DaisySPだから正しい
Rustだから安全
```

Physical / Modalでは、少なくとも次を対応づけて確認する。

```text
Parameter
↓
Algorithm上の意味
↓
数値的な変化
↓
聴感上の変化
```

例：

```text
stiffness
↓
all-pass dispersion増加
↓
高次Peak位置が変化
↓
硬い / metallicな弦感が増える
```

```text
modal.structure
↓
mode spacing stiffness変化
↓
peak ratioが変化
↓
bar / plate / bell的characterが変化
```

ここまで確認して本フェーズを完了とする。
