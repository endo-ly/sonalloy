# Sonalloy Harmonic / Formant Synthesis Expansion 詳細設計・実装計画

* **対象Repository**：`endo-ly/sonalloy`
* **調査基準Main**：P7 / Advanced Sampling・Granular・Wave Sequence マージ後
* **正本要件**：`docs/CONCEPT.md`
* **前提実装**：Instrument Definition、Dynamic Parameter / Modulation、Processor Chain、Essential Synthesis / Sampling、Digital Synthesis、Advanced Sampling / Granular / Wave Sequence
* **恒久名称**：`Harmonic / Formant Synthesis Expansion`
* **実装単位**：三単位。Branch / Pull Requestは一つとし、単位ごとにDefinition → Compile 

---

# 0. この計画書の位置づけ

本書は、現在のSonalloyへ次の二つの音生成方式を追加するための詳細設計・実装計画である。

1. **Additive Generator**
2. **Formant Generator**

現在の`docs/CONCEPT.md`では、Additiveを「複数Partialの合成」、Formantを「Vowel / Vocal-like Spectrumの生成」として独立Generatorに位置付けている。Sonalloy全体としても、Additive Drone等をゼロから生成できることを中心価値の一つとしている。

現在のMainでは、

* Oscillator
* Noise
* Sample
* Granular
* Wave Sequence
* Wavetable
* Operator Modulation

が`GeneratorRuntime`として存在している。AdditiveとFormantは、このGenerator境界へ追加する。既存Generatorを自由Graphへ一般化して実装しない。

P8では、AdditiveとFormantに共通する「多数のSine PartialをRealtimeで安全に合成する処理」だけを、**非公開の内部Primitive**として共有する。

これは新しい公開Generatorでも、汎用DSP Graphでも、Plugin機構でもない。

```text
Additive Generator ─┐
                    ├─ Private Partial Bank
Formant Generator ──┘
```

AdditiveとFormant以外を将来何でも接続できる汎用Node Frameworkへ発展させない。

---

## 0.1 恒久的な名称

コード、Definition、CLI、恒久Documentでは次の名称を使用する。

* `Additive Generator`
* `Partial`
* `Partial Bank`
* `Spectrum Morph`
* `Spectrum Tilt`
* `Inharmonicity`
* `Formant Generator`
* `Formant Profile`
* `Formant Band`
* `Vowel Position`
* `Formant Shift`
* `Throat`
* `Spectral Tilt`

`P8`という番号は開発上の識別子だけに使用する。

次には残さない。

* 型名
* API名
* Parameter ID
* JSON Field
* Diagnostic
* Reference Instrument
* Review Package内の恒久名称

---

## 0.2 実装判断の優先順位

判断に迷った場合は次の順序を優先する。

1. `docs/CONCEPT.md`
2. 本計画書で固定したAdditive / Formantの意味
3. 現在のDefinition → Compile → Runtime構造
4. 音質
5. Realtime Safety
6. Block Size非依存性
7. Deterministic Render
8. CPU量を予測できること
9. 実装の単純さ
10. 将来拡張

将来のSpectral / Physical Modelingを理由に、P8でFFT Engine、Spectral Graph、Resonator Framework等を先行実装しない。

---

# 1. 外部依存の調査と採用判断

P8では、既存依存をそのまま使うか、新しいDSP Libraryを追加するかを事前に検討した。

## 1.1 現在のNative依存

現在のSonalloyでは主に、

* DaisySP
* Signalsmith Stretch
* Signalsmith Linear

をNative依存として使用している。DaisySPは現在、Basic Oscillator、Hard Sync、Filter等で利用されている。

P8で新しいNative依存を追加すると、

```text
Build
C ABI
Rust Wrapper
Fault Injection
Sanitizer
Lifetime
Error Mapping
THIRD_PARTY_NOTICES
```

まで新たに責務が増える。

したがって「DSPコードを書かなくて済む」という理由だけでは追加しない。

---

## 1.2 DaisySP HarmonicOscillator

DaisySPには`HarmonicOscillator<num_harmonics>`が存在する。

これはChebyshev Polynomialを利用したHarmonic Oscillatorであり、公式Documentationでも「少数のharmonicでよく機能する」方式と説明されている。Amplitude指定も基本的にはrootに対する整数倍harmonicの列である。

P8 Additiveで必要なのは、

* 1〜64 Partial
* 任意の非整数Ratio
* PartialごとのPhase
* PartialごとのEnvelope
* Spectrum A / B間Morph
* Spectrum Tilt
* Inharmonicity
* 高音Partialの個別Alias管理

である。

したがって`HarmonicOscillator`を利用すると、Sonalloy側のAdditive DefinitionをDaisySPの方式へ歪めることになる。

**採用しない。**

---

## 1.3 DaisySP OscillatorBank

DaisySPには`OscillatorBank`も存在するが、これは7種類のSaw / Squareを組み合わせたDivide-down Organ系Oscillatorである。一般的な多数Partial Additive Generatorではない。

**採用しない。**

---

## 1.4 DaisySP FormantOscillator / VOSIM

DaisySPの`FormantOscillator`は、

* Carrier Frequency
* Formant Frequency
* Phase Shift

を持つ単一Formant中心の専用Oscillatorである。

`VosimOscillator`も、

* Carrier
* Formant 1
* Formant 2
* Shape

という構造である。

一方、SonalloyのFormant Generatorでは、

* 複数Formant
* 各FormantのBandwidth
* 各FormantのGain
* Vowel Profile
* Profile間Morph
* Formant Shift
* Throat
* Spectral Tilt

を一つの一貫したModelとして扱いたい。

DaisySPのFormant系Oscillatorを採用すると、このDefinitionとの対応が悪く、結局Sonalloy側で別の処理を大量に追加することになる。

**P8のFormant Backendとしては採用しない。**

---

## 1.5 Soundpipe

Soundpipeも候補として確認した。

SoundpipeはC製のDSP Libraryで、多数のSynthesis / DSP Moduleを持つ。一方、Repositoryは2024年1月からArchive状態であり、標準Buildではlibsndfileも必要とする。またModule LifecycleはCreate → Initialize → Compute → Destroyという独自所有形を持つ。

既にSonalloy側には、

* Asset Decode
* Oscillator
* Filter
* Runtime Lifecycle
* Native Wrapper

が成立している。

P8のAdditive / Formantだけのために新しいC Library境界を増やす利点は小さい。

**採用しない。**

---

## 1.6 最終判断

| 候補                         | 長所                     | P8との不一致                                       | 判断     |
| -------------------------- | ---------------------- | --------------------------------------------- | ------ |
| DaisySP HarmonicOscillator | 既存依存、軽量                | 任意Inharmonic Partial、Phase、Envelope、Morphに不向き | 不採用    |
| DaisySP OscillatorBank     | 既存依存                   | 7固定OscillatorのOrgan用途                         | 不採用    |
| DaisySP FormantOscillator  | 既存依存、Alias対策済み         | 単一Formant中心、Bandwidth/Profile Morphなし         | 不採用    |
| DaisySP VOSIM              | 既存依存                   | 2 Formant固定に近い                                | 不採用    |
| Soundpipe                  | DSP種類が豊富               | 新C依存、独自Lifecycle、Archive済み                    | 不採用    |
| **Sonalloy Core内部実装**      | Definitionと完全一致、依存追加なし | 自前実装が必要                                       | **採用** |

### P8の依存方針

**新しい外部依存は追加しない。**

また、Additive / Formantのために`sonalloy-dsp-sys`や`native/daisysp-wrapper`を拡張しない。

P8の主要DSPは`sonalloy-core`に実装する。

---

# 2. P8の目的

P8の目的は、

> 「既存波形を変調する」のではなく、**倍音構成そのものを直接設計して音を作る能力**をSonalloyへ追加すること。

である。

P7まででSonalloyには、

```text
基本波形
Noise
Sample
Wavetable
FM / PM / AM / Ring
Phase Distortion
Wavefold
Granular
Wave Sequence
```

が存在する。

P8ではこれに、

```text
多数Partialを直接構成する
        ↓
Additive

母音・声道に似た周波数分布を作る
        ↓
Formant
```

を追加する。

---

# 3. 完成後に作れる代表的な音

P8完了後、少なくとも次をSonalloy単体で構築できること。

### Additive

* Drawbar Organ系Tone
* Harmonic Pad
* Additive Drone
* Inharmonic Bell
* Metallic Texture
* Slowly Morphing Spectrum
* Partial Envelopeによる時間変化するBell / Pad
* WavetableやFMとは異なる静的・動的倍音設計

### Formant

* A / I / U / E / O系Vowel Tone
* Vocal Pad
* Talking Synth系Tone
* Robot Voice的Synth
* Formant Sweep Lead
* Choir-like Texture
* Noise Layerと組み合わせたBreathy Vocal Texture

### Hybrid

```text
Layer A: Additive Harmonics
Layer B: Formant Vowel
Layer C: Sample Attack
Layer D: Noise Breath
            │
            ▼
       Voice Filter
            │
            ▼
       Delay / Reverb
```

一つのInstrument内で既存Generatorと融合できることも完成条件とする。

---

# 4. 対象範囲

## 4.1 Additive Generator

含める。

* 1〜64 Partial
* Harmonic Partial
* Inharmonic Partial
* PartialごとのFrequency Ratio
* PartialごとのAmplitude
* PartialごとのInitial Phase
* Optional Partial Envelope
* Spectrum A / B
* Spectrum Morph
* Spectrum Tilt
* Global Inharmonicity
* Phase Reset
* 高域PartialのAlias抑制
* Dynamic Parameter / Modulation
* Voice Stealing / Reset
* CLI Inspect
* Offline Render
* Sound Review

---

## 4.2 Formant Generator

含める。

* Harmonic Excitation
* 最大64 Partial
* 1〜8 Vowel / Formant Profile
* Profileごと5 Formant Band
* Formant Frequency
* Bandwidth
* Gain
* Vowel Position
* Profile間Morph
* Formant Shift
* Throat
* Spectral Tilt
* Phase Reset
* Dynamic Parameter / Modulation
* 高域Alias抑制
* CLI Inspect
* Offline Render
* Sound Review

---

## 4.3 P8で含めないもの

次は明示的に対象外とする。

* Spectral / Resynthesis
* FFT / STFT Asset Analysis
* WAVからPartialを自動抽出
* WAVからFormantを自動解析
* Spectral Freeze
* Spectral Blur
* Cross-synthesis
* Physical / Modal / Waveguide
* Vocal Tract Physical Model
* Vocoder
* External Audio Input
* Envelope Follower
* Speech Synthesis
* Text to Speech
* Consonant Model
* Phoneme Sequencer
* Formant自動発音辞書
* MSEG
* Step Modulator
* Macro
* Vector Synthesis
* AdditiveのPartialごとのPan
* Additive内部Unison
* PartialごとのRealtime Ratio Parameter
* PartialごとのRealtime Amplitude Parameter
* Runtime中のPartial追加 / 削除
* Runtime中のFormant Profile追加 / 削除
* 任意DSP Graph
* 新しいSchema Version
* Migration
* Deprecated Field
* Legacy Alias

Breath / Sibilance等が必要な場合は既存Noise Layerを組み合わせる。

---

# 5. 共通内部Primitive：Partial Bank

## 5.1 目的

AdditiveとFormantは最終的に、

```text
Frequency
Amplitude
Phase
```

を持つ多数のSine Partialを加算する。

これを二重実装しない。

新しく、

```text
crates/sonalloy-core/src/runtime/generator/partial_bank.rs
```

を追加する。

ただし`PartialBank`は**非公開の実装Primitive**とする。

Definitionへ、

```json
{
  "partial_bank": {}
}
```

のようなGeneratorは公開しない。

---

## 5.2 Partial上限

```text
MAX_PARTIALS = 64
```

とする。

理由：

* CPU上限を予測可能にする
* Voice × Partial数の最大負荷を固定する
* Fixed Runtime Storageを使用できる
* P8の音色表現には十分広い
* 無制限PartialはSpectral / Resynthesisの責務へ近付く

Definition Validation時に65以上を拒否する。

Runtimeで黙って64へ切り捨てない。

---

## 5.3 Sine生成

4 Operator程度であれば現在のOperator Runtimeのような`sin()`直接評価でも成立するが、P8では最大64 PartialをVoiceごとに処理するため、同じ方法をそのまま64倍へ拡大しない。現在のOperator RuntimeがSampleごとにSineを評価していることは既存実装から確認できる。

P8では固定長Sine Tableを使用する。

```text
SINE_TABLE_LENGTH = 4096
```

* 一周期
* `[0, 1)` Phase
* Linear Interpolation
* Table端はWrap
* Compile / Prepare側で生成
* Process中に初期化しない

Runtime開始時のLazy Initializationは禁止する。

---

## 5.4 Sine Table精度

Unit Testで、

```text
lookup_sine(phase)
vs
sin(TAU * phase)
```

をPhase全域で比較する。

最大誤差を定量化し、Review Packageへ残す。

目標：

```text
max absolute error <= 1e-5
```

達成困難な場合はTable Lengthを8192へ増やす。

補間方式をCubicへすることを最初の選択肢にしない。

---

## 5.5 Runtime State

概念上：

```rust
struct PartialBankRuntime {
    phases: [f32; MAX_PARTIALS],
    gains: [f32; MAX_PARTIALS],
    gain_steps: [f32; MAX_PARTIALS],
    ratio_factors: [f32; MAX_PARTIALS],
    ratio_steps: [f32; MAX_PARTIALS],
    active_count: usize,
    control_frames_remaining: usize,
}
```

実際の型構成は既存Styleへ合わせる。

重要なのは、

* Process中にVecを作らない
* Partial数を変更しない
* Phase StateをVoiceごとに持つ

ことである。

---

# 6. Spectral Control Update

Additive / Formantは、ParameterからPartialごとのGainを導出する際に、

* `log2`
* `pow`
* `exp`
* dB → Linear

等を使用する。

これを64 Partial × 全Sampleで実行しない。

一方で、Block先頭だけで算出するとBlock Size依存になる。

そのためGenerator内部に**固定時間間隔のSpectral Control Update**を持つ。

---

## 6.1 Update間隔

```text
SPECTRAL_CONTROL_INTERVAL ≈ 1 ms
```

Process SpecのSample RateからPrepare時にFrame数へ変換する。

例：

```text
44.1 kHz → 44 frames
48 kHz   → 48 frames
96 kHz   → 96 frames
```

Host Block Sizeでは決めない。

---

## 6.2 Updateの動作

Control Tick到達時に、

1. 現在Sample位置のParameter値を取得
2. Partialごとの次Target Gain / Ratio Factorを算出
3. 現在値から次TargetへControl Interval長でRamp
4. その後のSampleでは軽量な加算だけを行う

```text
Parameter
    │
    ▼
Spectral Control Tick
    │
    ├─ expensive coefficient calculation
    │
    ▼
Gain / Ratio Ramp
    │
    ▼
Per-sample Partial Bank
```

Control TickのPhaseはGenerator Runtimeが保持する。

Block境界でResetしない。

したがって、

```text
32
64
257
1024
```

等のHost Block Sizeを変えてもControl Tick位置は変わらない。

---

# 7. Additive Generator Definition

## 7.1 構造

概念構造：

```rust
pub struct AdditiveDefinition {
    pub phase_reset: bool,
    pub morph: f32,
    pub spectrum_tilt_db_per_octave: f32,
    pub inharmonicity: f32,
    pub partials: Vec<AdditivePartialDefinition>,
}
```

```rust
pub struct AdditivePartialDefinition {
    pub id: String,
    pub ratio: f32,
    pub amplitude_a: f32,
    pub amplitude_b: f32,
    pub phase: f32,
    pub envelope: Option<AdsrDefinition>,
}
```

Definitionでは`Vec`を使用してよい。

Compile後はPartial数が固定された構造へ変換する。

---

## 7.2 Additive JSON例

```json
{
  "additive": {
    "phase_reset": true,
    "morph": 0.0,
    "spectrum_tilt_db_per_octave": -3.0,
    "inharmonicity": 0.0,
    "partials": [
      {
        "id": "fundamental",
        "ratio": 1.0,
        "amplitude_a": 1.0,
        "amplitude_b": 0.7,
        "phase": 0.0
      },
      {
        "id": "second",
        "ratio": 2.0,
        "amplitude_a": 0.45,
        "amplitude_b": 0.8,
        "phase": 0.0,
        "envelope": {
          "attack_seconds": 0.01,
          "decay_seconds": 0.8,
          "sustain_level": 0.2,
          "release_seconds": 0.3
        }
      },
      {
        "id": "inharmonic",
        "ratio": 2.73,
        "amplitude_a": 0.15,
        "amplitude_b": 0.5,
        "phase": 0.25
      }
    ]
  }
}
```

---

# 8. Additive Partial Contract

## 8.1 `id`

* 必須
* Layer内Additive Generatorで一意
* 空文字禁止
* Parameter IDには直接展開しない
* CLI Inspect / Diagnostic識別用

PartialごとのRealtime ParameterをP8では公開しないため、`id`をParameter IDとして利用する必要はない。

---

## 8.2 `ratio`

```text
0.125 ～ 64.0
```

Note Frequencyに対する倍率。

整数に限定しない。

例：

```text
1.0
2.0
3.0
```

ならHarmonic。

```text
1.0
1.414
2.73
4.12
```

のような非整数値も許可する。

これにより、Global Inharmonicityを使わなくても任意のInharmonic Spectrumを明示できる。

---

## 8.3 `amplitude_a` / `amplitude_b`

```text
0.0 ～ 1.0
```

Spectrum Morphの両端。

```text
morph = 0
→ amplitude_a

morph = 1
→ amplitude_b
```

中間：

```text
amp = A + (B - A) * morph
```

とする。

AとBでPartial数を別々に持たない。

同じPartial Slotを共有し、一方に存在しないPartialはAmplitudeを0にする。

これによりMorph時に、

* Partial追加
* Partial削除
* Phase State追加
* Runtime Allocation

を発生させない。

---

## 8.4 `phase`

```text
0.0 ～ 1.0
```

一周期内のInitial Phase。

Dynamic Parameterにはしない。

---

## 8.5 Partial Envelope

各PartialはOptionalで既存ADSRを持てる。

```text
Partial Base Amplitude
        │
        ▼
Partial ADSR
        │
        ▼
Spectrum Gain
```

Envelopeが存在しないPartialは常に1.0。

Layer Envelopeはその後に別途適用される。

```text
Partial Envelope
       ↓
Partial Sum
       ↓
Generator Output
       ↓
Layer Envelope
```

両者を統合しない。

---

# 9. Additive Spectrum Morph

`morph`はDynamic Parameterとする。

用途：

* Organ → Bell
* Hollow → Bright
* Odd Harmonic → Full Harmonic
* Static → Metallic

等。

P8ではMorphする対象をAmplitudeだけに限定する。

Morphしない：

* Ratio
* Phase
* Envelope Definition
* Partial ID

Ratio Morphまで含めると、Spectrum構造自体が移動するためSpectral / Resynthesisとの境界が曖昧になる。

---

# 10. Spectrum Tilt

Additiveの`Spectrum Tilt`は、

> 高次Partialほどどの程度減衰・増幅するか

を一つの値で制御する。

範囲：

```text
-24 ～ +12 dB / octave
```

Ratio 1.0を基準とする。

概念式：

```text
octaves = log2(ratio)

tilt_db =
    spectrum_tilt_db_per_octave
    * octaves

tilt_gain =
    db_to_linear(tilt_db)
```

Control Tickで計算する。

Per-sampleで`log2`や`pow`を呼ばない。

---

# 11. Inharmonicity

Additiveでは二種類のInharmonic表現を持つ。

### 明示的

Partialの`ratio`へ非整数値を直接書く。

### Global Inharmonicity

全Partialを高次ほど少しずつStretchする。

Dynamic Parameter：

```text
0.0 ～ 1.0
```

0ならDefinition Ratioを完全に維持する。

概念的にはStiff-string系のProgressive Stretchを使用する。

```text
B = inharmonicity * B_MAX

effective_ratio =
    ratio *
    sqrt(
        (1 + B * ratio²)
        /
        (1 + B)
    )
```

`ratio = 1`は常に1を維持する。

`B_MAX`はP8の固定定数として小さく設定し、高次Partialほど明確にStretchするが、通常範囲で極端なPitch崩壊を起こさない値とする。

初期候補：

```text
B_MAX = 0.0005
```

Sound Reviewで、

* 0
* 0.25
* 0.5
* 1.0

を確認する。

この値を音を聞かずに大きくしない。

---

# 12. Additive Output Normalization

多数Partialの単純加算では、Partial数によって音量が急激に増える。

P8ではSpectrum Gain算出時にEnergy Normalizationを行う。

概念：

```text
energy = Σ gain_i²

normalization =
    1 / max(1, sqrt(energy))
```

各Control TickでTarget Gainへ適用する。

重要：

**現在ActiveなPartial Envelope数を使ってNormalizationしない。**

Partial Envelopeが終了した瞬間に他PartialのGainを上げる方式は禁止する。

Granularで既に確認したような、

```text
active count変化
    ↓
全体Gain段差
    ↓
Click / Bzzz
```

をAdditiveへ再導入しない。

Partial Envelopeが減衰した場合、全体Energyも自然に減衰してよい。

---

# 13. Formant Generatorの基本方式

P8のFormant Generatorは、外部AudioをFilterする方式ではなく、

> **Harmonic Partial BankへVocal-likeなSpectral Envelopeを掛ける方式**

とする。

```text
Note Frequency
      │
      ▼
1f / 2f / 3f / 4f ... Harmonics
      │
      ▼
Formant Spectral Envelope
      │
      ▼
Spectral Tilt
      │
      ▼
Partial Bank
      │
      ▼
Formant Generator Output
```

これにより、

* External Audio不要
* Vocoderと明確に分離
* Formant Frequency / Bandwidth / Gainを直接Model化
* Additiveと同じAlias / Partial合成基盤を利用
* 新Native Library不要

となる。

---

# 14. Formant Definition

## 14.1 概念構造

```rust
pub struct FormantDefinition {
    pub phase_reset: bool,
    pub partial_count: u8,
    pub vowel_position: f32,
    pub formant_shift_cents: f32,
    pub throat: f32,
    pub spectral_tilt_db_per_octave: f32,
    pub profiles: Vec<FormantProfileDefinition>,
}
```

```rust
pub struct FormantProfileDefinition {
    pub id: String,
    pub formants: Vec<FormantBandDefinition>,
}
```

```rust
pub struct FormantBandDefinition {
    pub frequency_hz: f32,
    pub bandwidth_hz: f32,
    pub gain_db: f32,
}
```

---

## 14.2 JSON例

```json
{
  "formant": {
    "phase_reset": true,
    "partial_count": 48,
    "vowel_position": 0.0,
    "formant_shift_cents": 0.0,
    "throat": 0.5,
    "spectral_tilt_db_per_octave": -6.0,
    "profiles": [
      {
        "id": "a",
        "formants": [
          {
            "frequency_hz": 800.0,
            "bandwidth_hz": 80.0,
            "gain_db": 0.0
          },
          {
            "frequency_hz": 1150.0,
            "bandwidth_hz": 90.0,
            "gain_db": -5.0
          },
          {
            "frequency_hz": 2900.0,
            "bandwidth_hz": 120.0,
            "gain_db": -12.0
          },
          {
            "frequency_hz": 3900.0,
            "bandwidth_hz": 130.0,
            "gain_db": -18.0
          },
          {
            "frequency_hz": 4950.0,
            "bandwidth_hz": 140.0,
            "gain_db": -24.0
          }
        ]
      },
      {
        "id": "i",
        "formants": [
          {
            "frequency_hz": 270.0,
            "bandwidth_hz": 60.0,
            "gain_db": 0.0
          },
          {
            "frequency_hz": 2290.0,
            "bandwidth_hz": 100.0,
            "gain_db": -6.0
          },
          {
            "frequency_hz": 3010.0,
            "bandwidth_hz": 120.0,
            "gain_db": -12.0
          },
          {
            "frequency_hz": 3900.0,
            "bandwidth_hz": 130.0,
            "gain_db": -18.0
          },
          {
            "frequency_hz": 4950.0,
            "bandwidth_hz": 140.0,
            "gain_db": -24.0
          }
        ]
      }
    ]
  }
}
```

数値はReference Instrument用の例であり、Sonalloyが固定Vowel Tableとして内部に持つ値ではない。

Definition側が正本である。

---

# 15. Formant Profile

## 15.1 Profile数

```text
1 ～ 8
```

1 ProfileならStatic Formant Tone。

複数ProfileならVowel Morph可能。

---

## 15.2 Formant数

P8では各Profileを、

```text
5 Formant固定
```

とする。

理由：

* Vowel Toneとして十分な表現力
* CPUが予測可能
* Profile間でBand対応が曖昧にならない
* 「Aでは4 Formant、Iでは6 Formant」のようなMorph規則を作らずに済む

Definitionでは`Vec`を使用してもよいが、Validationで必ず5件要求し、Compiled側では、

```rust
[CompiledFormantBand; 5]
```

へ変換する。

---

# 16. Formant Band Contract

## 16.1 Frequency

```text
100 ～ 12,000 Hz
```

Profile内ではStrictly Ascendingとする。

```text
F1 < F2 < F3 < F4 < F5
```

順序が違う場合に自動Sortしない。

Definition Errorとする。

---

## 16.2 Bandwidth

```text
20 ～ 5,000 Hz
```

0は禁止。

---

## 16.3 Gain

```text
-60 ～ +12 dB
```

Formant間の相対強度を定義する。

---

# 17. Vowel Position

`vowel_position`：

```text
0.0 ～ 1.0
```

Dynamic Parameter。

Profile配列の先頭から末尾までを連続移動する。

例えば5 Profile：

```text
0.00 → A
0.25 → E
0.50 → I
0.75 → O
1.00 → U
```

Profile名自体にVowelとしての意味を強制しない。

```text
bright
dark
nasal
metallic
```

のような任意Profileも使える。

---

## 17.1 Profile補間

隣接Profile間では対応するFormant Band同士を補間する。

### Frequency

Log / Geometric Interpolation。

```text
f = exp(
    lerp(
        ln(f_a),
        ln(f_b),
        t
    )
)
```

### Bandwidth

同じくGeometric Interpolation。

### Gain

dB値をLinear Interpolation。

```text
gain_db =
    gain_a +
    (gain_b - gain_a) * t
```

---

# 18. Formant Shift

`formant_shift_cents`：

```text
-2400 ～ +2400 cents
```

Dynamic Parameter。

Formant中心周波数へRatioとして適用する。

```text
shift_ratio =
    cents_to_ratio(formant_shift_cents)

center *= shift_ratio
bandwidth *= shift_ratio
```

Centerだけを移動してBandwidthを固定しない。

Bandwidthも同じ比率で動かし、Formant Qの大きな変化を避ける。

重要：

**NoteのFundamental Pitchは変えない。**

これにより、

```text
音程は同じ
声道サイズだけ変化したように聞こえる
```

動作を狙う。

---

# 19. Throat

`throat`：

```text
0.0 ～ 1.0
```

Dynamic Parameter。

P8では曖昧なTone Controlにせず、

> Formant Bandwidth全体の幅を制御するParameter

と固定する。

```text
throat = 0.0
→ profile bandwidth × 0.5

throat = 0.5
→ profile bandwidth × 1.0

throat = 1.0
→ profile bandwidth × 2.0
```

概念式：

```text
bandwidth_multiplier =
    2 ^ (2 * (throat - 0.5))
```

狭い側：

* 尖った
* Nasal
* Resonant

広い側：

* 柔らかい
* Open
* Smeared

という方向の音色変化を担う。

---

# 20. Formant Spectral Envelope

各Harmonic Partialの周波数を`f`とする。

各Formant BandはGaussian-likeなPeakとして計算する。

BandwidthはFWHMとして扱う。

```text
sigma =
    bandwidth_hz / 2.35482
```

各Band：

```text
distance =
    (f - center) / sigma

band_gain =
    db_to_linear(gain_db)
    *
    exp(-0.5 * distance²)
```

5 Bandを加算する。

```text
formant_gain =
    Σ band_gain
```

その後、

```text
Formant Gain
    ×
Spectral Tilt
    ×
Alias Fade
```

をPartial Gainとする。

この`exp()`計算はSpectral Control Tickでだけ行う。

Sampleごとには行わない。

---

# 21. Formant Spectral Tilt

範囲：

```text
-24 ～ +12 dB / octave
```

Dynamic Parameter。

Additiveと同じ意味を使用する。

ただしParameter IDはGeneratorごとに分離する。

---

# 22. Partial FrequencyとAlias対策

P8では高音域で、

```text
Partial Frequency > Nyquist
```

となるPartialを同じ周波数へClampしてはいけない。

例えば、

```text
20 kHz
21 kHz
22 kHz
25 kHz
```

を一律20 kHzへClampすると、そこへEnergyが集中する。

P8では**Gain Fade Out**を使用する。

---

## 22.1 Alias Fade

基準：

```text
0.40 × Sample Rate以下
    → gain 1

0.40 ～ 0.45 × Sample Rate
    → smooth fade

0.45 × Sample Rate以上
    → gain 0
```

既存CoreもOscillatorの有効周波数上限を共通契約として管理しているため、P8の上限も同じ場所で意味を固定する。

ただし既存OscillatorのFrequency Clampをそのまま使うのではなく、Partial専用の**Amplitude Fade Contract**として定義する。

---

## 22.2 Phase

Alias FadeでGainが0になったPartialもPhaseは継続する。

Pitch Bend等で再び有効帯域へ戻った際、

```text
Phase Reset
↓
Click
```

を起こさないためである。

---

# 23. Parameter Contract

現在の`GeneratorParameterSpec`をそのまま利用する。Range / Unit / Scale / Smoothingの正本をGenerator Definition、Parameter Catalog、Runtimeで別々に定義しない。

---

## 23.1 新しいParameter Unit

現在のParameter Unitには、

* Decibels
* Cents
* Hertz
* Ratio
* Seconds
* PerSecond
* Index
* Normalized

等が存在するが、dB / octaveは存在しない。

P8で、

```rust
ParameterUnit::DecibelsPerOctave
```

を追加する。

`Decibels`へ意味を押し込まない。

---

## 23.2 Additive Parameter

### `additive_morph`

```text
Unit: Normalized
Range: 0 ～ 1
Scale: Linear
Smoothing: 10 ms
```

### `additive_spectrum_tilt`

```text
Unit: DecibelsPerOctave
Range: -24 ～ +12
Scale: Linear
Smoothing: 10 ms
```

### `additive_inharmonicity`

```text
Unit: Normalized
Range: 0 ～ 1
Scale: Linear
Smoothing: 10 ms
```

---

## 23.3 Formant Parameter

### `formant_vowel_position`

```text
Unit: Normalized
Range: 0 ～ 1
Scale: Linear
Smoothing: 10 ms
```

### `formant_shift`

```text
Unit: Cents
Range: -2400 ～ +2400
Scale: Linear
Smoothing: 10 ms
```

### `formant_throat`

```text
Unit: Normalized
Range: 0 ～ 1
Scale: Linear
Smoothing: 10 ms
```

### `formant_spectral_tilt`

```text
Unit: DecibelsPerOctave
Range: -24 ～ +12
Scale: Linear
Smoothing: 10 ms
```

---

# 24. P8でPartial単位Parameterを公開しない理由

P8では例えば、

```text
partial.0.amplitude
partial.1.amplitude
...
partial.63.amplitude
```

のような64個以上のRealtime ParameterをCatalogへ公開しない。

理由：

1. Parameter数がInstrument構造へ強く依存する
2. Spectrum A / Bと二重管理になる
3. Modulation Matrixが極端に巨大になる
4. Runtime Target Scratchが膨張する
5. AIが音色を作る場合、Definitionを書き換えてCompileする方が自然
6. P8で必要なRealtime表現はMorph / Tilt / Inharmonicityで十分広い

Partial単位の編集自体はDefinitionで可能。

Realtime MotionはGlobal Spectrum Parameterで行う。

---

# 25. Compiled Model

## 25.1 Additive

概念：

```rust
pub struct CompiledAdditive {
    pub partials: Box<[CompiledAdditivePartial]>,
    pub phase_reset: bool,
    pub parameters: CompiledAdditiveParameters,
    pub sine_table: Arc<[f32]>,
}
```

```rust
pub struct CompiledAdditivePartial {
    pub id: String,
    pub ratio: f32,
    pub amplitude_a: f32,
    pub amplitude_b: f32,
    pub phase: f32,
    pub envelope: Option<CompiledAdsr>,
}
```

---

## 25.2 Formant

概念：

```rust
pub struct CompiledFormant {
    pub partial_count: usize,
    pub phase_reset: bool,
    pub profiles: Box<[CompiledFormantProfile]>,
    pub parameters: CompiledFormantParameters,
    pub sine_table: Arc<[f32]>,
}
```

```rust
pub struct CompiledFormantProfile {
    pub id: String,
    pub formants: [CompiledFormantBand; 5],
}
```

---

# 26. Compile時に行う処理

Additive：

* Partial数検証
* Partial ID重複検証
* Ratio検証
* Amplitude検証
* Phase検証
* ADSR検証
* Dynamic Parameter Handle解決
* Sine Table生成
* Runtime固定容量確認
* Spectrumが完全無音でないことを確認

Formant：

* Partial Count検証
* Profile数検証
* ID重複検証
* 各Profileが5 Formantであること
* Frequency昇順確認
* Frequency / Bandwidth / Gain検証
* Log補間用値の事前計算
* dB Gainの事前計算可能部分を準備
* Dynamic Parameter Handle解決
* Sine Table生成
* Runtime容量確定

---

# 27. Process中に禁止するもの

Additive / Formant共通：

* Heap Allocation
* Vec容量拡張
* File I/O
* JSON処理
* Asset Decode
* Sine Table作成
* Partial追加 / 削除
* Profile追加 / 削除
* Profile Sort
* Lock待ち
* External Library Initialization
* `sin()`を64 Partialすべてで直接評価
* Formantの`exp()`を毎Sample × Partial × Bandで実行
* Audio ThreadからDiagnostic Stringを大量生成

Realtime Pathでは、事前確保済み状態だけを使う。

---

# 28. Generator Lifecycle

現在`GeneratorRuntime::note_off()`はOperator Modulationの専用Lifecycleを扱っている。AdditiveでPartial Envelopeを追加するため、ここをAdditiveにも接続する必要がある。

---

## 28.1 Additive Note On

* `phase_reset=true`ならInitial Phaseへ戻す
* Partial Envelopeを`note_on`
* Spectral Control Stateを初期Targetへ設定
* Audio Thread Allocationなし

---

## 28.2 Additive Note Off

* Partial Envelopeを`note_off`
* Layer Envelopeも既存LifecycleどおりRelease
* Partial EnvelopeをVoice終了判定の独立正本にしない

---

## 28.3 Formant Note On

* `phase_reset=true`ならPartial PhaseをReset
* Spectral Control State初期化
* Profile Index / Vowel Parameterから最初のGainを生成

Formant固有Envelopeは持たない。

Layer Envelopeを使用する。

---

## 28.4 Reset

Additive：

* Phase
* Partial Envelope
* Gain Ramp
* Ratio Ramp
* Spectral Control Phase

をReset。

Formant：

* Phase
* Gain Ramp
* Spectral Control Phase

をReset。

Reset後、同じCompiled InstrumentとEvent列からFresh Runtimeと同等出力を得る。

---

# 29. Generator Output Mode

Additive：

```text
Mono
```

Formant：

```text
Mono
```

Stereo化は既存Layer Pan、Layer Mix、Processor Chainで行う。

P8内部へPartial PanやStereo Spreadを追加しない。

64 Partial × Stereo × Unisonまで一度に実装してCPU上限を不透明にしない。

---

# 30. Dynamic Parameter評価

既存`ValueSpan`とParameter Smoothingを使用する。

ただしDerived SpectrumはSpectral Control Tickで評価する。

例：

```text
vowel_position
      │
      ▼
Existing Parameter Span
      │
      ▼
Absolute Spectral Tick
      │
      ▼
Formant Profile interpolation
      │
      ▼
Partial target gains
      │
      ▼
1 ms gain ramp
```

Block SizeをTick単位として使用しない。

---

# 31. Validation / Diagnostic

## 31.1 Additive Error

拒否する：

* Partial 0件
* Partial 65件以上
* Duplicate Partial ID
* 空ID
* NaN / Infinity
* Ratio範囲外
* Amplitude範囲外
* Phase範囲外
* Invalid ADSR
* Morph範囲外
* Tilt範囲外
* Inharmonicity範囲外
* Spectrum A / Bともに全Partial 0

RuntimeでSilentに修正しない。

---

## 31.2 Formant Error

拒否する：

* Profile 0件
* Profile 9件以上
* Duplicate Profile ID
* 空ID
* Partial Count 0
* Partial Count 65以上
* Profile Formantが5件でない
* Frequency範囲外
* Bandwidth範囲外
* Gain範囲外
* Formant Frequencyが昇順でない
* Vowel Position範囲外
* Shift範囲外
* Throat範囲外
* Tilt範囲外
* NaN / Infinity

---

# 32. CLI Inspect

## 32.1 Additive

最低限表示する。

```text
kind: additive
output_mode: mono
partial_count
phase_reset
morph
spectrum_tilt_db_per_octave
inharmonicity
max_partial_count
partials:
  id
  ratio
  amplitude_a
  amplitude_b
  phase
  has_envelope
```

---

## 32.2 Formant

表示：

```text
kind: formant
output_mode: mono
partial_count
phase_reset
profile_count
vowel_position
formant_shift_cents
throat
spectral_tilt_db_per_octave

profiles:
  id
  formants:
    frequency_hz
    bandwidth_hz
    gain_db
```

Parameter Catalogにも新Parameterを通常どおり公開する。

---

# 33. Reference Instrument

最低3つ追加する。

```text
examples/instruments/
├─ additive-generator-reference.json
├─ formant-generator-reference.json
└─ harmonic-formant-hybrid-reference.json
```

---

## 33.1 Additive Reference

目的：

* Harmonic / Inharmonic
* Spectrum Morph
* Partial Envelope
* Tilt
* Inharmonicity

を一つのDefinitionから確認する。

---

## 33.2 Formant Reference

Profile例：

```text
A
E
I
O
U
```

を用意する。

ただし内部Built-in Presetにはしない。

Reference Definitionのデータとしてだけ置く。

---

## 33.3 Hybrid Reference

```text
Layer A: Additive
Layer B: Formant
Layer C: Sample Attack
Layer D: Noise
```

既存Processor Chainまで通す。

新Generatorだけ単独で鳴ることではなく、Sonalloy本来のHybrid Instrumentとして成立することを確認する。

---

# 34. Module構成

想定変更：

```text
crates/sonalloy-core/src/
├─ definition.rs
├─ compiler.rs
├─ diagnostics.rs
├─ generator_parameters.rs
├─ parameter.rs
├─ runtime/
│  ├─ generator/
│  │  ├─ mod.rs
│  │  ├─ partial_bank.rs       # new
│  │  ├─ additive.rs           # new
│  │  ├─ formant.rs            # new
│  │  ├─ oscillator.rs
│  │  ├─ operator.rs
│  │  ├─ wavetable.rs
│  │  ├─ granular.rs
│  │  └─ wave_sequence.rs
│  ├─ modulation.rs
│  └─ voice.rs

crates/sonalloy-core/tests/
├─ additive.rs                  # new
├─ formant.rs                   # new
└─ existing tests

crates/sonalloy-cli/
├─ src/main.rs
└─ tests/cli.rs

docs/
├─ plan/
│  └─ plan-harmonic-formant-synthesis-expansion.md
├─ architecture.md
├─ instrument-definition.md
├─ runtime-processing.md
├─ cli.md
├─ creating-an-instrument.md
└─ testing-and-sound-review.md

examples/instruments/
├─ additive-generator-reference.json
├─ formant-generator-reference.json
└─ harmonic-formant-hybrid-reference.json

scripts/review/
├─ generate_harmonic_formant_package.py
└─ README.md

review-output/
└─ harmonic-formant-synthesis/

.agents/skills/create-instrument/
└─ SKILL.md
```

実際のRepository構造を優先する。

計画書の見た目だけのために既存Fileを分割しない。

---

# 35. Native側変更

原則として、

```text
native/
crates/sonalloy-dsp-sys/
```

はP8で変更しない。

Additive / Formant実装の途中でNative変更が必要になった場合、

> なぜCore Rustで成立しないのか

を先に説明できない限り追加しない。

外部依存調査は本計画で完了済みとして扱い、「実装しやすそうだからDaisySPへ切り替える」という現場判断は禁止する。

---

# 36. Definition Unit Test

## 36.1 Additive

* 1 Partial
* 64 Partial
* 0 Partial拒否
* 65 Partial拒否
* Integer Ratio
* Fractional Ratio
* Ratio最小
* Ratio最大
* Ratio範囲外
* Amplitude A 0 / 1
* Amplitude B 0 / 1
* Amplitude範囲外
* Phase 0 / 1境界
* Phase範囲外
* Duplicate ID
* Empty ID
* Partial Envelopeあり
* Partial Envelopeなし
* Invalid ADSR
* Morph 0 / 1
* Tilt最小 / 最大
* Inharmonicity 0 / 1
* 全無音Definition拒否
* Unknown Field拒否

---

## 36.2 Formant

* 1 Profile
* 8 Profile
* 0 Profile拒否
* 9 Profile拒否
* Partial Count 1 / 64
* Partial Count 0 / 65拒否
* 5 Formant
* 4 / 6 Formant拒否
* Frequency最小 / 最大
* Frequency範囲外
* Bandwidth最小 / 最大
* Bandwidth範囲外
* Gain最小 / 最大
* Gain範囲外
* 非昇順Frequency拒否
* Duplicate Profile ID
* Empty Profile ID
* Vowel Position 0 / 1
* Shift端点
* Throat端点
* Tilt端点
* NaN / Infinity
* Unknown Field拒否

---

# 37. Parameter Test

新しい`DecibelsPerOctave`を含める。

* Normalize min → 0
* Normalize max → 1
* Denormalize 0 → min
* Denormalize 1 → max
* Round Trip
* Out of Range
* NaN
* Infinity

Additive / Formantの各Parameterについて、

* Catalog ID
* Owner
* Unit
* Scale
* Default
* Range
* Smoothing

を確認する。

---

# 38. Partial Bank Unit Test

* Sine Phase 0
* 0.25
* 0.5
* 0.75
* Wrap
* Lookup Table Error
* Phase Advance
* Phase Reset
* Phase Continue
* 1 Partial
* 64 Partial
* Gain 0
* Gain 1
* Gain Ramp
* Ratio Ramp
* Alias Fade 0.40境界
* Alias Fade中間
* Alias Fade 0.45境界
* Out-of-band Partial 0 Gain
* Finite Output
* Zero-frame
* Fixed Capacity

---

# 39. Additive Runtime Test

* Single Sine Partial
* Fundamental + Octave
* 32 Harmonics
* 64 Harmonics
* Fractional Ratio
* Spectrum A
* Spectrum B
* Morph 0
* Morph 0.5
* Morph 1
* Tilt Negative
* Tilt Positive
* Inharmonicity 0
* Inharmonicity 1
* FundamentalをInharmonicityで動かさない
* Partial Envelope Attack
* Partial Envelope Decay
* Partial Envelope Release
* Note Off
* Phase Reset
* Phase Continue
* Voice Stealing
* Reset
* High Note
* Pitch Bend
* Parameter Change途中
* Zero-frame
* No Allocation

---

# 40. Formant Runtime Test

* Single Profile
* Multiple Profile
* Vowel Position 0
* Position 1
* Adjacent Profile 0.5
* Frequency Geometric Morph
* Bandwidth Geometric Morph
* Gain dB Morph
* Formant Shift -1200 / 0 / +1200
* ShiftしてFundamental Pitchを変えない
* Throat 0 / 0.5 / 1
* Spectral Tilt
* Five Formant Sum
* 1 Partial
* 64 Partial
* High Note
* Alias Fade
* Pitch Bend
* Vowel Position LFO
* Parameter Change
* Phase Reset
* Voice Stealing
* Reset
* Zero-frame
* No Allocation

---

# 41. Block Size Test

最低：

```text
32
64
257
1024
```

Additive：

* Spectrum Morph Sweep
* Inharmonicity Sweep
* Partial Envelope

Formant：

* Vowel Position Sweep
* Formant Shift Sweep

について比較する。

Spectral Control TickをHost Block境界から独立させるため、Block Size違いによる明確な音声差を許容しない。

既存Review Packageと同様に数値比較を残す。

---

# 42. Sample Rate Test

```text
44,100
48,000
96,000
```

で、

* Finite
* Non-silent
* Alias Fade位置
* Pitch
* Morph
* Reset

を確認する。

Control Updateの時間間隔も約1msで維持されること。

---

# 43. Allocation Test

Audio Thread上で、

* Additive Note On
* Additive Note Off
* Additive Render
* Additive Voice Stealing
* Formant Note On
* Formant Render
* Formant Voice Stealing

にHeap Allocationが発生しないことをTestする。

特に、

```text
64 Partial
×
16 Voice
```

を実際に発音した状態で検査する。

---

# 44. Performance Test

P8ではPartial数によるCPU増加が主要リスクである。

最低ケース：

```text
Additive:
1 Partial
16 Partial
32 Partial
64 Partial

Polyphony:
1
4
8
16
```

Formant：

```text
32 Partial
64 Partial

Profiles:
1
5
8
```

を測定する。

Testの目的は絶対的なRealtime保証値を固定することではなく、

* Partial数に対して異常な非線形増加がない
* Process中Allocationがない
* FormantのControl計算がSample Inner Loopへ漏れていない

ことの検出。

---

# 45. Regression：Grain境界問題を繰り返さない

P7 Granularでは、Active Grain数でNormalizationしていたため、Grain追加 / 終了の瞬間に他GrainのGainが変化し、聴感上のビリビリ / ブツブツが発生した。

P8では同種の問題を専用Testで防止する。

Additive：

* Constant Spectrum
* Morph固定
* Partial Envelopeなし

で出力の隣接Sample Jumpを測定。

Formant：

* Vowel固定
* Shift固定
* Throat固定

で測定。

さらにParameter Sweep時にも専用Fixtureを作る。

単に、

```text
large discontinuity < 0.25
```

のような緩い共通Thresholdだけを根拠に「Clickなし」と判定しない。

---

# 46. Sound Review Package

新規：

```text
review-output/harmonic-formant-synthesis/
```

生成Script：

```text
scripts/review/generate_harmonic_formant_package.py
```

---

## 46.1 Additive Review Audio

最低限：

1. `01-additive-fundamental.wav`
2. `02-harmonic-organ.wav`
3. `03-inharmonic-bell.wav`
4. `04-spectrum-a.wav`
5. `05-spectrum-b.wav`
6. `06-spectrum-morph-sweep.wav`
7. `07-spectrum-tilt-sweep.wav`
8. `08-inharmonicity-sweep.wav`
9. `09-partial-envelope-bell.wav`
10. `10-high-note-alias-check.wav`
11. `11-additive-polyphony.wav`

---

## 46.2 Formant Review Audio

12. `12-vowel-a.wav`
13. `13-vowel-i.wav`
14. `14-vowel-u.wav`
15. `15-vowel-e.wav`
16. `16-vowel-o.wav`
17. `17-vowel-morph.wav`
18. `18-formant-shift-sweep.wav`
19. `19-throat-sweep.wav`
20. `20-formant-tilt-sweep.wav`
21. `21-vowel-position-lfo.wav`
22. `22-high-note-formant.wav`
23. `23-formant-noise-texture.wav`

---

## 46.3 Hybrid / Regression

24. `24-harmonic-formant-hybrid.wav`
25. `25-harmonic-formant-hybrid-midi.wav`

加えて、

```text
block-32
block-64
block-257
block-1024

sample-rate-44100
sample-rate-48000
sample-rate-96000

fresh-a
fresh-b
```

等のTechnical Fixtureを生成する。

---

# 47. Automatic Review Metrics

最低限記録する。

全Audio：

* Sample Rate
* Channel Count
* Duration
* Peak
* RMS
* DC
* Finite
* Max Adjacent Sample Delta

Additive：

* Partial Count
* Fundamental Frequency
* Harmonic / Non-harmonic Energy
* Morph A/Bとの差分
* Inharmonicity差分
* High-frequency energy
* Block Size comparison
* Fresh Runtime comparison

Formant：

* Profile Count
* Formant Parameter values
* Vowel Position差分
* Shift差分
* Throat差分
* Tilt差分
* Block Size comparison
* Fresh Runtime comparison

Review Scriptが「Parameterを変えたらSampleが違った」だけで機能確認を終えない。

---

# 48. Human Sound Review

## 48.1 Additive

人間が確認する。

### Harmonic Organ

* 倍音が明確
* 不自然なBzzz / Clickなし
* 基音のPitchが正しい

### Inharmonic Bell

* Integer Harmonicとは明確に違う
* 金属的なSpectrumになる
* 高音Aliasが主音として聞こえない

### Spectrum Morph

* A → Bが連続
* 中間で音量が急落 / 急増しない
* Zipper Noiseなし

### Partial Envelope

* 高次Partialだけ先に消える等の音色変化が自然
* Partial終了時に残りPartialが突然大きくならない

---

## 48.2 Formant

### Static Vowel

* A / I / U / E / Oが少なくとも相対的に聞き分けられる
* 単なるEQ Sweepに聞こえるだけではない

### Vowel Morph

* Profile境界でClickなし
* 位置が連続して変わる
* Profile境界で急激に別音へ飛ばない

### Formant Shift

* Note Pitchは維持
* Vocal Character / Sizeだけが変わる

### Throat

* Resonance幅の違いが聞き取れる
* 端点で発振・急増しない

### High Note

* 高次Aliasが支配的にならない
* Partialが減ることによる自然な音色変化の範囲に収まる

---

# 49. Documentation更新

最低限：

### `docs/instrument-definition.md`

* Additive JSON
* Partial Contract
* Spectrum Morph
* Formant JSON
* Profile / Band Contract
* Vowel Position

### `docs/runtime-processing.md`

* Partial Bank
* Spectral Control Tick
* Alias Fade
* Partial Envelope Lifecycle
* Formant Spectrum生成

### `docs/architecture.md`

* Additive / Formant専用Generator
* Shared Private Partial Bank
* Native依存なし

### `docs/cli.md`

* Inspect Output

### `docs/creating-an-instrument.md`

* Additiveの使い方
* Formantの使い方
* Noise LayerとFormantの組み合わせ

### `docs/testing-and-sound-review.md`

* Harmonic / Formant Review Package

### `.agents/skills/create-instrument/SKILL.md`

AIがAdditive / Formant Definitionを正しく生成できるよう更新する。

---

# 50. 実装単位

P8は一つのPRで進める。

ただし三つのVertical Unitに分ける。

---

# 50.1 Unit A：Partial Bank + Additive Generator

## 目的

多数PartialをRealtime Safeに生成する共通基盤を完成させ、Additive GeneratorをDefinitionからReviewまで通す。

## 実装順

1. Additive Definition
2. Additive Validation
3. Parameter Unit追加
4. Additive Parameter Contract
5. Compiled Additive
6. Partial Bank
7. Sine Table
8. Alias Fade
9. Spectral Control Tick
10. Spectrum Morph
11. Spectrum Tilt
12. Inharmonicity
13. Partial Envelope
14. Note Off統合
15. Reset / Voice Stealing
16. CLI Inspect
17. Unit Test
18. Integration Test
19. Additive Reference Instrument
20. Additive Review Audio
21. Human Review

### Unit A完了条件

* 1〜64 Partial
* Harmonic / Inharmonic
* Morph
* Tilt
* Inharmonicity
* Partial Envelope
* Block Size非依存
* No Allocation
* Sound Review合格

まで成立すること。

Formantへ進む前にAdditive単独で完成させる。

---

# 50.2 Unit B：Formant Generator

## 目的

Unit AのPartial Bankを利用し、Vowel / Vocal-like Spectrumを生成する専用Generatorを完成させる。

## 実装順

1. Formant Definition
2. Profile / Band Validation
3. Formant Parameter Contract
4. Compiled Formant
5. Harmonic Partial配置
6. Vowel Position
7. Profile Morph
8. Formant Shift
9. Throat
10. Spectral Tilt
11. Gaussian Spectrum Envelope
12. Spectral Control Tick接続
13. Alias Fade
14. Reset / Voice Stealing
15. CLI Inspect
16. Unit Test
17. Integration Test
18. Formant Reference Instrument
19. Formant Review Audio
20. Human Review

### Unit B完了条件

* Static Vowel
* Vowel Morph
* Shift
* Throat
* Tilt
* Block Size非依存
* No Allocation
* Sound Review合格

まで成立すること。

---

# 50.3 Unit C：Hybrid / Regression / Documentation

## 目的

新Generatorを既存Sonalloy全体へ統合し、P8単体のDemoではなく製品機能として完成させる。

## 実装順

1. Harmonic / Formant Hybrid Instrument
2. Existing Sample / NoiseとのLayer Mix
3. Existing Modulationとの統合
4. Filter / Drive / Delay / Reverbとの統合
5. MIDI Render
6. 16 Voice Performance
7. Block Size Regression
8. Sample Rate Regression
9. Reset / Fresh Runtime
10. Existing Review Package Regression
11. CLI Regression
12. Documentation更新
13. AI Skill更新
14. Final Review Package生成
15. Final Human Review
16. CI

---

# 51. 既存機能への回帰確認

P8実装後も最低限、

* Oscillator
* Noise
* Sample
* Wavetable
* Operator Modulation
* Granular
* Wave Sequence

の既存Testをすべて通す。

Additive / Formant追加のために`GeneratorRuntime` Matchを更新する際、

* `start`
* `note_off`
* `render`
* `reset`
* `intrinsic_latency`
* `output_mode`

のどれかを漏らさない。

現在のGenerator Runtime境界へ正規に追加する。

---

# 52. Realtime Safety完了条件

P8完了時、Audio処理中に次がないこと。

```text
malloc / new
Vec growth
HashMap insertion
File I/O
Asset decoding
Sine Table generation
Profile sorting
JSON serialization
Blocking Lock
Native lifecycle creation
```

また、

```text
64 Partial
×
16 Voice
```

を実際にProcessしたAllocation Testを持つこと。

---

# 53. Determinism完了条件

同じ、

* Definition
* Event
* Process Spec

からFresh Runtimeを2回作り、

```text
Fresh A
Fresh B
```

が同等出力になること。

Additive / FormantにはRandomnessを導入しない。

---

# 54. Audio Quality完了条件

自動Testだけで完成扱いにしない。

最低限、

* Spectrum Morph
* Partial Envelope
* Inharmonic Bell
* Vowel Morph
* Formant Shift
* High Note

を人間が試聴する。

特に、

> 「FiniteだからOK」

> 「0.25以上のSample JumpがないからOK」

とは判定しない。

P7 Granularで実際に聴感上の問題が先に発見されたことを踏まえ、音色Generatorは人間の最終試聴を必須とする。

---

# 55. 主なリスクと対策

## 55.1 CPU負荷

### リスク

64 Partial × Polyphonyで単純Sine計算が増える。

### 対策

* 64上限
* Sine Table
* Linear Interpolation
* 高価なSpectrum計算は約1ms Control Tick
* P8内部Unisonなし
* Process Allocationなし
* 16 Voice Performance Test

---

## 55.2 高音Alias

### リスク

高次PartialがNyquistを超える。

### 対策

* Frequency ClampではなくAmplitude Fade
* 0.40〜0.45 Sample RateでFade
* High Note Sound Review
* Spectrum Metrics

---

## 55.3 Normalization Pumping

### リスク

Partial数やEnvelope状態にNormalizationを追従させると、残ったPartialのGainが急変する。

### 対策

Spectrum Targetだけを基準にEnergy Normalizeし、Active Envelope Countを使用しない。

---

## 55.4 Formant MorphのZipper Noise

### 対策

* Existing Parameter Smoothing
* Spectral Control Tick
* Partial Gain Ramp
* Block Size非依存Tick
* Vowel Sweep Review

---

## 55.5 Formant音が「声っぽくない」

### 対策

まず、

* 5 Formant
* Gain
* Bandwidth
* Spectral Tilt
* Harmonic Source

のReference Profileを調整する。

この問題だけを理由に即座にSoundpipeや別Native Libraryを追加しない。

方式そのものが不足していることをReviewで確認してから再設計する。

---

## 55.6 Additive Definitionが大きい

64 PartialをJSONで明示するとDefinitionが長くなる。

これは許容する。

SonalloyはCLI / AI-firstであり、

```text
Partial Spectrum生成
Definition生成
Parameter最適化
```

は人間の手入力だけを前提としない。

無理に圧縮DSLを追加しない。

---

# 56. P8で行わない先回り実装

次のためのFrameworkを作らない。

### P9 Spectral / Resynthesis向け

* FFT Engine
* Spectrum Frame
* OLA
* Phase Vocoder
* Spectral Asset Format

### Physical Modeling向け

* Resonator Graph
* Delay Network
* Modal Bank
* Waveguide Framework

### Modulation Expansion向け

* MSEG
* Step
* Macro
* Vector

P8で必要なのはAdditiveとFormantだけ。

---

# 57. 最終成果物

コード：

* Additive Definition / Compile / Runtime
* Formant Definition / Compile / Runtime
* Private Partial Bank
* Parameter Contract
* Diagnostics
* CLI Inspect

Reference：

* Additive Instrument
* Formant Instrument
* Harmonic / Formant Hybrid

Test：

* Definition
* Parameter
* Compile
* Runtime
* Lifecycle
* Block Size
* Sample Rate
* Allocation
* Performance
* Regression

Review：

* `review-output/harmonic-formant-synthesis/`
* Technical WAV
* Metrics
* Human Review Summary

Document：

* 本計画書
* Instrument Definition
* Runtime Processing
* Architecture
* CLI
* Creating Instrument
* Testing / Sound Review
* AI Skill

---

# 58. P8完了条件

以下を**すべて満たした時だけP8完了**とする。

1. Additive GeneratorをDefinitionへ保存できる
2. 1〜64 Partialを扱える
3. Integer / Noninteger Ratioを扱える
4. Partial Amplitude / Phase / Optional Envelopeを保持できる
5. Spectrum MorphがDynamicに動く
6. Spectrum TiltがDynamicに動く
7. InharmonicityがDynamicに動く
8. Partial EnvelopeがNote Off / Reset / Voice Stealingと正しく連動する
9. Formant GeneratorをDefinitionへ保存できる
10. 1〜8 Profileを扱える
11. 各Profileが5 Formant Bandを持つ
12. Frequency / Bandwidth / GainをDefinitionで指定できる
13. Vowel PositionでProfile間を連続Morphできる
14. Formant ShiftがDynamicに動く
15. ThroatがDynamicに動く
16. Spectral TiltがDynamicに動く
17. Formant ShiftでNote Fundamentalを変更しない
18. 高域PartialをFrequency Clampせず安全にFadeできる
19. Process中Heap Allocationがない
20. Block Size 32 / 64 / 257 / 1024で時間軸が変わらない
21. 44.1 / 48 / 96 kHzでFiniteかつ非無音
22. Reset後にFresh Runtimeと同等の出力になる
23. 16 Voice × 64 PartialのPerformanceを検証している
24. CLI Inspectで構造を確認できる
25. Existing Generatorの回帰がない
26. Reference Instrumentが存在する
27. Review Packageが自動検証を通る
28. Additiveを人間が試聴して承認する
29. Formantを人間が試聴して承認する
30. Hybrid Instrumentを人間が試聴して承認する
31. Windows / Linux CIが成功する
32. 新しい外部依存を追加していない
33. `sonalloy-dsp-sys` / Native Wrapperへ不要な責務を増やしていない

---

# 59. P8完了後の到達点

P8終了時点の主要Generator群は、

```text
Sample
Basic Oscillator
Complex Oscillator
Noise
Wavetable
Operator Modulation
Granular
Wave Sequence
Additive
Formant
```

となる。

ここまでで、

```text
波形
変調
Sample
粒子
時間Sequence
倍音構造
Vocal-like Spectrum
```

という主要な電子音生成アプローチを広く持つ。

その次のP9では、これまでとは異なり、

```text
既存Audio
    ↓
周波数分析
    ↓
Spectral Frame
    ↓
再合成
```

という新しいPrepared Spectral Data基盤が必要になる。

したがってP8ではそこへ踏み込まず、**Additive / Formantを現在のGenerator Architecture上で完全に完成させること**を最終目的とする。
