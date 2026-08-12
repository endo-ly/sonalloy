# Sonalloy Spectral / Resynthesis Expansion 詳細設計・実装計画

* **対象Repository**：`endo-ly/sonalloy`
* **調査基準Main**：Harmonic / Formant Synthesis Expansionマージ後の最新Main
* **正本要件**：`docs/CONCEPT.md`
* **前提実装**：Instrument Definition、Dynamic Parameter / Modulation、Processor Chain、Essential Synthesis / Sampling、Digital Synthesis、Advanced Sampling / Granular / Wave Sequence、Additive / Formant
* **ロードマップ上の扱い**：次の開発Phase（P9）
* **恒久名称**：`Spectral / Resynthesis Expansion`
* **主機能**：Spectral Analysis、Phase-aware Resynthesis、Position、Freeze、Blur、Frequency Shift、Spectral Morph
* **実装単位**：四単位。Branch / Pull Requestは一つとし、各単位をDefinition → Compile → Prepare → Runtime → Test → Sound Reviewまで縦に完成させる
* **成果物**：Markdownのみ
* **想定計画書Path**：`docs/plan/plan-spectral-resynthesis-expansion.md`

---

# 0. この計画書の位置づけ

本書は、Sonalloyへ新しいGeneratorとして、

**Spectral Generator**

を追加するための詳細設計・実装計画である。

P8までのGeneratorは主に、

```text
Oscillator
Noise
Wavetable
Operator Modulation
Sample
Granular
Wave Sequence
Additive
Formant
```

という形で、

> 「波形・Sample・Partialなどから音を直接作る」

方式だった。

P9では初めて、

```text
Audio Asset
    ↓
STFT Analysis
    ↓
Spectral Frames
    ↓
Spectrum加工
    ↓
Phase-aware Resynthesis
    ↓
Inverse FFT
    ↓
Overlap-add
    ↓
Audio
```

という、

> **一度Audioを周波数成分へ分解し、その周波数表現から音を再構築する方式**

をSonalloyへ追加する。

P9は単なる「FFTを使ったEffect」ではない。

Spectral GeneratorはNoteごとの状態を持つ独立Generatorであり、既存のLayer / Voice / Instrument Pipeline内へ入れる。

```text
Spectral Generator
       ↓
Layer Envelope
       ↓
Layer Processor
       ↓
Layer Mix
       ↓
Voice Processor
       ↓
Global Processor
```

自由なSpectral Graphは作らない。

---

# 0.1 恒久名称

コード、Definition、CLI、恒久Documentでは次の名称を使用する。

* `Spectral Generator`
* `Prepared Spectral Asset`
* `Spectral Frame`
* `Spectral Bin`
* `Spectral Position`
* `Spectral Freeze`
* `Spectral Blur`
* `Spectral Shift`
* `Spectral Morph`
* `Phase Accumulator`
* `Instantaneous Frequency`
* `Overlap-add`
* `Analysis Window`
* `Synthesis Window`

`P9`という名称は開発上だけで使用する。

次には残さない。

* Type名
* API名
* JSON Field
* Parameter ID
* Diagnostic
* CLI出力
* Review Package

---

# 0.2 実装判断の優先順位

判断に迷った場合は次の順序を優先する。

1. `docs/CONCEPT.md`
2. 本計画書で固定したSpectral Generatorの意味
3. 現在のDefinition → Compile → Runtime構造
4. Root NoteでのResynthesis品質
5. Phase Continuity
6. Realtime Safety
7. Block Size非依存性
8. Determinism
9. CPU / Memory量を予測できること
10. 実装の単純さ
11. 将来拡張

将来の、

* Vocoder
* Spectral Processor
* External Audio Input
* Physical Modeling

を理由に汎用FFT Graphを先行実装しない。

---

# 1. 外部依存の調査と採用判断

## 1.1 現在の状態

現在の`sonalloy-core`には、

```toml
rustfft = "6.4.1"
```

が既に存在する。

RustFFTはPure RustのFFT Libraryで、CPU Featureに応じAVX / SSE / NEON等を選択できるPlannerを持つ。MIT / Apache-2.0。

P9ではFFTを正式なProduction Runtimeへ使用する。

---

# 1.2 RustFFTだけを直接使用する案

RustFFTだけでもP9は実装可能。

ただしAudioは実数信号なので、

```text
Real Audio
    ↓
Complex変換
    ↓
Complex FFT
```

をそのまま行うと、

* Bufferが大きい
* 不要な負周波数側も処理する
* Runtime IFFT負荷も増える

という問題がある。

P9では各Voiceが一定間隔でInverse FFTを実行するため、単なるCompile時処理よりFFT効率の重要度が高い。

---

# 1.3 RealFFT

RealFFTはRustFFTを利用した、

* Real → Complex
* Complex → Real

専用FFT Libraryである。

偶数長では半サイズComplex FFTを利用できる。

さらに、

```text
process_with_scratch()
```

を利用すれば、事前確保済みScratchを使用してFFT実行中のHeap Allocationを避けられる。

P9の、

```text
Audio
↓
Forward Real FFT

Spectrum
↓
Inverse Real FFT
↓
Audio
```

という処理と直接一致する。

**採用する。**

---

# 1.4 Signalsmith Stretch / Signalsmith Linear

Signalsmith Stretchは既にSonalloyへ入っており、MIT Licenseである。

ただし提供する抽象度は、

```text
Audio
↓
Pitch / Time Stretch
↓
Audio
```

である。

P9で必要なのは、

```text
Magnitude
Phase
Instantaneous Frequency
Position
Freeze
Blur
Morph
Frequency Bin Remapping
```

をSonalloy自身が操作できること。

Signalsmith StretchをBackendにすると、これらのSpectral ModelをSonalloyから直接制御できない。

Signalsmith Linear側のFFT / STFTを直接C++ Wrapper越しに使うことも可能だが、

* C ABI追加
* Native Lifecycle追加
* Runtime Buffer所有追加
* Rust ↔ C++ Spectrum転送
* Native Failure Handling追加

が必要になる。

P9はPure Rustで十分実装可能なので採用しない。

---

# 1.5 FFTW

FFTWは高性能だがGPLであり、非GPL条件で配布する場合は別ライセンスが必要になる。

P9のためにこの制約を追加する理由はない。

**採用しない。**

---

# 1.6 Rubber Band

Rubber BandはPitch / Time Stretch Libraryとして成熟しているが、GPLまたは商用ライセンスである。

またSpectral FrameそのものをSonalloyの音色Modelとして扱う用途とも異なる。

**採用しない。**

---

# 1.7 最終依存方針

P9では、

```toml
realfft = "3.5.0"
```

を`sonalloy-core`の直接依存とする。

現在の直接`rustfft`依存について、P9実装がRustFFT APIを直接必要としない場合は削除する。

Complex型等はRealFFTのRe-exportを利用する。

```text
Sonalloy
   ↓
RealFFT
   ↓
RustFFT
```

という依存関係とし、

```text
Sonalloy
├─ RealFFT
└─ RustFFT
```

という不要な直接二重依存を残さない。

### P9では変更しない

```text
native/
crates/sonalloy-dsp-sys/
Signalsmith Stretch Wrapper
DaisySP Wrapper
```

Spectral Generatorは`sonalloy-core`内で実装する。

---

# 2. P9の目的

P9の目的は、

> **既存Audioを時間ごとのSpectrumへ分解し、そのSpectrumを直接加工しながら新しい音として再合成できるようにすること。**

である。

具体的には、

```text
Recorded Voice
Field Recording
Synth Sample
Metal Hit
Noise Texture
```

などを、

```text
Freeze
Spectral Blur
Frequency Shift
Pitch Shift
Spectral Morph
Position Scrub
```

によって元音とは異なるInstrumentへ変換できるようにする。

---

# 3. P9完了後に作れる音

最低限、次の種類の音が作れること。

## Resynthesis

* 元AudioをSpectrum経由で再構築した音
* Pitchを変えてもDurationを変えないResynthesis
* Stereo Spectral Resynthesis

## Freeze

* Vocal Freeze Pad
* Metal Freeze Drone
* Noise Freeze Texture
* Static Harmonic Cloud

## Blur

* Smearing Pad
* Ambient Texture
* Slowly evolving Spectrum
* AttackをぼかしたSpectral Drone

## Shift

* Metallic Frequency Shift
* Inharmonic Texture
* Robot-like Tone
* Spectral Detune

## Morph

* Voice → Synth
* Noise → Harmonic Tone
* Metal → Vocal Texture
* 2つのSpectrum間を連続移動するPad

## Hybrid

```text
Layer A: Spectral Freeze
Layer B: Additive
Layer C: Sample Attack
Layer D: Noise
        ↓
Filter / Drive
        ↓
Delay / Reverb
```

---

# 4. P9の対象範囲

## 4.1 Spectral Analysis

含める。

* Mono WAV
* Stereo WAV
* Existing Asset Decode
* Existing Sample Rate Conversion
* STFT
* Magnitude
* Phase
* Instantaneous Frequency
* Prepared Spectral Asset
* Analysis Window
* Spectral Asset Cache
* Memory Limit

---

# 4.2 Spectral Resynthesis

含める。

* Inverse FFT
* Phase Accumulation
* Overlap-add
* Root Note
* MIDI Pitch Tracking
* Layer Tuning
* Pitch Bend
* Position
* Freeze
* Blur
* Frequency Shift
* Spectral Morph
* Stereo
* Phase Reset
* One-shot Source Lifecycle
* Reported Latency
* Voice Stealing
* Reset

---

# 4.3 P9で含めないもの

明示的に対象外とする。

* Realtime External Audio Input
* Vocoder
* Live Cross-synthesis
* Spectral Processor
* FFTを既存Layerへ掛けるEffect
* Spectral Gate
* Spectral Denoise
* Spectral Compressor
* Spectral EQ
* Spectral Painting
* Bin単位のDefinition編集
* 1 BinごとのModulation Parameter
* Sinusoidal Peak Tracking
* Partial自動抽出
* Pitch Detection
* Fundamental Tracking
* Transient Detection
* Transient Preservation
* Phase Locking
* Peak Phase Locking
* Harmonic / Percussive Separation
* Spectral Grain
* FFT Grain
* Arbitrary Window選択
* Arbitrary Hop Size
* Arbitrary FFT Size
* Reverse Auto Scan
* Spectral Loop Crossfade
* Disk Streaming
* Long-file Streaming
* More than Stereo
* More than two Morph Sources
* MSEG
* Macro
* Vector Synthesis
* Physical Modeling
* CLAP / VST3
* Riffra Integration

既存SampleのTime Stretch Backendも置き換えない。

Signalsmith StretchはそのままSample用に残す。

---

# 5. Spectral Generator Definition

概念構造：

```rust
pub struct SpectralDefinition {
    pub asset_a: AssetReference,
    pub asset_b: Option<AssetReference>,
    pub root_note: u8,
    pub fft_size: u16,
    pub position: f32,
    pub freeze: f32,
    pub blur_seconds: f32,
    pub shift_hz: f32,
    pub morph: f32,
    pub phase_reset: bool,
}
```

---

# 5.1 JSON例

```json
{
  "spectral": {
    "asset_a": {
      "path": "assets/voice.wav",
      "sha256": "..."
    },
    "asset_b": {
      "path": "assets/metal.wav",
      "sha256": "..."
    },
    "root_note": 60,
    "fft_size": 2048,
    "position": 0.0,
    "freeze": 0.0,
    "blur_seconds": 0.0,
    "shift_hz": 0.0,
    "morph": 0.0,
    "phase_reset": true
  }
}
```

---

# 6. `asset_a`

必須。

Spectral GeneratorのTiming Masterでもある。

Asset Aの元Durationを基準として、

* Natural Scan
* Source終了
* Morph Source BのNormalized Timeline

を決める。

Asset AがPrepareできない場合、Spectral Generatorは利用不可とする。

---

# 7. `asset_b`

Optional。

存在する場合だけSpectral Morphを使用できる。

Asset Bは、

* MonoならAもMono
* StereoならAもStereo

を要求する。

Channel数が異なる場合はCompile Error。

Durationは同じでなくてよい。

Morph時はAとBのNormalized Positionを対応させる。

```text
A 0%   ↔ B 0%
A 50%  ↔ B 50%
A 100% ↔ B 100%
```

したがってSource Bは必要に応じ、Aの時間軸へSpectral上で圧縮・伸長された形になる。

Audio Resamplerを追加実行するわけではない。

---

# 8. `root_note`

```text
0 ～ 127
```

Asset A / Bが表す基準Pitch。

```text
played_note = root_note
shift_hz = 0
```

の場合、元SpectrumのPitchを維持する。

MIDI Noteを変更した場合は、Spectral Binを周波数方向へ再配置する。

重要：

**Pitch変更でSource Scan速度は変えない。**

つまり、

```text
Pitch +12 semitone
```

にしてもDurationは半分にならない。

ここが通常Sample Resamplingと異なる。

---

# 9. FFT Size

P9では次だけを許可する。

```text
1024
2048
4096
```

その他はDefinition Error。

### 1024

* Low Latency
* 高い時間分解能
* 粗い周波数分解能

### 2048

* Balanced
* Reference InstrumentのDefault

### 4096

* 高い周波数分解能
* Freeze / Drone向け
* Latency / FFT負荷増加

任意の2の累乗を受け付けない。

FFT Size上限をDefinitionから無制限にしない。

---

# 10. Hop Size

Definitionには公開しない。

固定：

```text
hop_size = fft_size / 4
```

つまり75% Overlap。

```text
FFT 1024 → Hop 256
FFT 2048 → Hop 512
FFT 4096 → Hop 1024
```

Host Block Sizeとは独立。

---

# 11. Window

Window種類もDefinitionへ公開しない。

P9ではPeriodic Hannを使用する。

Analysis WindowとSynthesis Windowは別にPreparationする。

概念：

```text
analysis_window = Hann

synthesis_window[n]
=
analysis_window[n]
/
Σ overlap analysis_window²
```

として、選択したHop SizeでOverlap-addのGainが1になるよう正規化する。

単に、

```text
Hann × Hann
```

を使って経験的Gainを掛けない。

Window ContractはUnit Testで検証する。

---

# 12. Prepared Spectral Asset

新規内部型：

```rust
pub struct PreparedSpectralAsset {
    pub sample_rate: f64,
    pub channels: usize,
    pub source_frames: usize,

    pub fft_size: usize,
    pub hop_size: usize,
    pub bin_count: usize,
    pub spectral_frame_count: usize,

    pub latency_frames: usize,

    pub magnitudes: Arc<[f32]>,
    pub phases: Arc<[f32]>,
    pub instantaneous_frequencies_hz: Arc<[f32]>,

    pub prepared_bytes: usize,
}
```

実際の公開範囲は既存Architectureへ合わせる。

---

# 12.1 Memory Layout

Nested Vec：

```text
Vec<Frame<Vec<Bin>>>
```

にはしない。

Contiguous：

```text
channel
  ↓
frame
  ↓
bin
```

の一次元配列とする。

Index Helperだけを内部に持つ。

```text
index(channel, frame, bin)
```

Process中にIterator CollectionやSlice作成を行わない。

---

# 13. Spectral Frame内容

各Channel / Frame / Binについて、

```text
Magnitude
Absolute Phase
Instantaneous Frequency
```

を保存する。

Complex Spectrumそのものを保存しない。

---

# 13.1 Magnitude

FFT Complex値：

```text
re + j im
```

から、

```text
sqrt(re² + im²)
```

を保存する。

---

# 13.2 Phase

```text
atan2(im, re)
```

を、

```text
-π ～ +π
```

で保持する。

主用途：

* Note On初期Phase
* Position開始時Phase
* Phase Reset

Runtimeでは毎HopこのAbsolute Phaseへ戻さない。

---

# 13.3 Instantaneous Frequency

単純な、

```text
bin_index × sample_rate / fft_size
```

だけでは、FFT Binの中央からずれた実際のSinusoid周波数を表せない。

そこで隣接Analysis FrameのPhase差からInstantaneous Frequencyを推定する。

概念：

```text
expected_phase_advance
=
2π × bin × hop / fft_size
```

```text
phase_delta
=
wrap(
    current_phase
    - previous_phase
    - expected_phase_advance
)
```

```text
true_phase_advance
=
expected_phase_advance
+ phase_delta
```

```text
instantaneous_frequency_hz
=
true_phase_advance
× sample_rate
/
(2π × hop)
```

第一FrameはNominal Bin Frequencyを使用する。

---

# 14. Spectral Preparation

新規：

```text
crates/sonalloy-core/src/spectral.rs
```

を想定する。

責務：

```text
PreparedAudio
     ↓
Zero Padding
     ↓
Analysis Window
     ↓
Real FFT
     ↓
Magnitude / Phase
     ↓
Instantaneous Frequency
     ↓
PreparedSpectralAsset
```

Runtime DSPとは分離する。

---

# 15. Audio Preparationとの関係

Spectral Assetでも、

* File I/O
* SHA-256
* WAV Decode
* Channel Validation
* Sample Rate Conversion

を新規実装しない。

既存：

```text
prepare_asset()
PreparedAudio
```

を使用する。

```text
Asset
 ↓
PreparedAudio
 ↓
PreparedSpectralAsset
```

とする。

Decode / Resampleの正本を増やさない。

---

# 16. Spectral Asset Cache

Spectral Analysisは高価なのでCompileごとに同じAssetを何度も解析しない。

新規Key：

```rust
struct SpectralAssetCacheKey {
    path: PathBuf,
    sha256: Option<String>,
    sample_rate_bits: u64,
    fft_size: usize,
}
```

同じ、

```text
Path
SHA
Process Sample Rate
FFT Size
```

なら同じPrepared Spectral Assetを共有する。

`Arc<PreparedSpectralAsset>`をVoice間でも共有する。

---

# 17. Boundary Padding

Analysis前にSourceへZero Paddingを追加する。

Leading / Trailing：

```text
fft_size - hop_size
```

frames。

これにより、

* WindowがSource先頭より前へ掛かる部分
* Source末尾より後へ掛かる部分

をゼロとして明示的に表現する。

Original SourceのSample 0はPrepared Timeline上で、

```text
fft_size - hop_size
```

だけ後ろへ移動する。

これをSpectral GeneratorのAlgorithmic Latencyとして扱う。

---

# 18. Latency

初期Contract：

```text
spectral_latency_frames
=
fft_size - hop_size
```

例：

```text
1024 → 768 frames
2048 → 1536 frames
4096 → 3072 frames
```

ただし実装時にImpulse / Identity Testで実効Latencyを測定し、この値と一致することを確認する。

ズレた場合に「だいたいこの値」で済ませない。

Compiled Generatorは、

```text
intrinsic_latency_frames()
```

へSpectral Latencyを返す。

現在のInstrument Latency Compensationをそのまま利用する。

Spectral Layerが1536 frames Latentなら、

```text
Sample Layer
Additive Layer
Noise Layer
```

などの非Latent Layer側を既存のLayer Delay Compensationで1536 frames遅らせる。

P9でOffline Rendererへ別のPre-roll / Trim機構を追加しない。

---

# 19. Runtime構造

新規：

```text
crates/sonalloy-core/src/runtime/generator/spectral.rs
```

概念：

```rust
struct SpectralRuntime {
    source_a: Arc<PreparedSpectralAsset>,
    source_b: Option<Arc<PreparedSpectralAsset>>,

    phase_accumulators: Vec<f32>,
    blurred_magnitudes: Vec<f32>,

    inverse_input: Vec<Complex<f32>>,
    inverse_output: Vec<f32>,
    inverse_scratch: Vec<Complex<f32>>,

    ola_left: Vec<f32>,
    ola_right: Vec<f32>,

    synthesis_window: Arc<[f32]>,

    scan_progress: f64,
    hop_phase: usize,

    ...
}
```

これらはVoice生成 / Prepare時に確保する。

Audio Process中には容量を変更しない。

---

# 20. FFT Plan

Inverse FFT PlanはVoiceごとに再Planningしない。

Compiled Spectral側で、

```text
Spectral Synthesis Plan
```

を作り、`Arc`でVoice間共有する。

内部的にはRealFFTの、

```text
Arc<dyn ComplexToReal<f32>>
```

等を保持する。

FFT Plan WrapperのEqualityは、

```text
fft_size
```

を意味の正本とし、内部Pointer Identityを比較しない。

---

# 21. Runtime FFT Buffer

RealFFTの、

```text
process()
```

は使用しない。

Runtimeでは必ず、

```text
process_with_scratch()
```

を使用する。

ScratchはPrepare時に確保。

Process中Allocationなし。

Stereoの場合も、

```text
Left IFFT
Right IFFT
```

を順に実行し、

```text
inverse_input
inverse_output
inverse_scratch
```

自体は一組を再利用する。

Phase / Blur StateとOLA BufferだけChannel別に持つ。

---

# 22. Spectral Synthesis Scheduler

Host Blockではなく、

```text
synthesis hop
```

を時間軸の正本とする。

```text
FFT 2048
Hop 512
```

の場合、

```text
frame 0
frame 512
frame 1024
frame 1536
...
```

でSpectral Frameを生成する。

Hostから、

```text
Block 32
Block 64
Block 257
Block 1024
```

のどれで呼ばれても、このHop位置は変わらない。

Runtimeが、

```text
samples_until_next_hop
```

等のStateをVoiceごとに保持する。

---

# 23. OLA Ring Buffer

各Synthesis Hopで生成したFFT Size分のAudioを、

```text
Overlap-add Ring
```

へ加算する。

その後Process側はRingから1 Sampleずつ読み、

読んだSlotを0へ戻す。

Ring CapacityはPrepare時に固定。

Process中、

```text
Vec::push()
resize()
reserve()
```

を行わない。

---

# 24. `position`

Definition：

```text
0.0 ～ 1.0
```

Dynamic Parameter。

Parameter ID：

```text
spectral_position
```

Positionは、

> Source Aに対するNormalized Base Position

と定義する。

---

# 24.1 Natural Scan

Spectral Generatorは何もModulationしなくてもSourceを自然速度で前進する。

概念：

```text
read_position
=
position
+
scan_progress
```

```text
scan_progress
+=
hop_size
/
source_a_frames
```

`position = 0`ならSource先頭から開始。

`position = 0.5`ならSource中央から開始。

---

# 24.2 Position Modulation

PositionがDynamicに変化した場合、

```text
Base Position
+
現在までのScan Progress
```

へ反映する。

Phase AccumulatorはResetしない。

つまりPositionを動かしても、

```text
Source PhaseをFrameごとにそのままコピーする
```

動作にはしない。

これによりScrub時のPhase Jumpを減らす。

---

# 25. `freeze`

```text
0.0 ～ 1.0
```

Dynamic Parameter。

FreezeはPhaseを停止させない。

停止させるのは**Source Scan**だけ。

```text
scan_speed
=
1 - freeze
```

したがって、

```text
freeze = 0
→ Natural Scan

freeze = 0.5
→ Half-speed Spectral Scan

freeze = 1
→ Source Position固定
```

となる。

---

# 25.1 Freeze時のPhase

重要：

Freeze中も、

```text
Phase Accumulator
```

はInstantaneous Frequencyに応じて前進する。

つまり、

```text
同じComplex FFT Frameを
何度も固定PhaseでIFFTする
```

方式にはしない。

Freeze対象：

```text
Magnitude
Instantaneous Frequency
Source Position
```

Phaseは連続進行する。

これによりSpectral Freezeを、

```text
短いFFT Frameの周期Repeat
```

にしない。

---

# 26. `blur_seconds`

```text
0.0 ～ 1.0 seconds
```

Dynamic Parameter。

Parameter Unit：

```text
Seconds
```

既存Unitを使用する。

新Unitは追加しない。

---

# 26.1 Blurの意味

P9でのBlurは、

> **時間方向のMagnitude Smoothing**

と定義する。

Frequency軸方向へ隣接Binを混ぜるBlurではない。

各Synthesis Hopで得たTarget Magnitudeへ、

```text
One-pole temporal smoothing
```

を掛ける。

---

# 26.2 Blur State

概念：

```text
smoothed
+=
alpha × (target - smoothed)
```

`alpha`は、

```text
blur_seconds
hop_size
sample_rate
```

からHopごとに一度だけ算出する。

Binごとに`exp()`を計算しない。

---

# 26.3 Blur = 0

```text
blur_seconds = 0
```

では、

```text
smoothed = target
```

とし、余計なFilterを通さない。

---

# 27. `shift_hz`

Dynamic Parameter。

```text
-12,000 ～ +12,000 Hz
```

Parameter Unit：

```text
Hertz
```

これはPitch Transposeではない。

> **各Spectral成分を周波数軸上で一定Hz移動するFrequency Shift**

である。

例：

```text
500 Hz
1000 Hz
1500 Hz
```

へ、

```text
+300 Hz
```

すると、

```text
800 Hz
1300 Hz
1800 Hz
```

となる。

倍音Ratioは維持されない。

したがって金属的・Inharmonicな音を作れる。

---

# 28. MIDI PitchとSpectral Shiftの違い

MIDI Note：

```text
Multiplicative Frequency Scaling
```

Spectral Shift：

```text
Additive Frequency Translation
```

とする。

概念：

```text
target_frequency
=
source_frequency
× note_pitch_ratio
+
shift_hz
```

---

# 29. Destination Bin Remapping

SpectrumをPitch / Shiftする場合、Phase Advanceだけ変更してはいけない。

Energyの存在するFFT Bin自体も移動する。

各Source Bin `k`について、

```text
nominal_frequency
=
k × sample_rate / fft_size
```

```text
destination_frequency
=
nominal_frequency
× note_pitch_ratio
+
shift_hz
```

```text
destination_bin
=
destination_frequency
× fft_size / sample_rate
```

とする。

---

# 29.1 Fractional Bin

Destination Binは整数とは限らない。

```text
floor_bin
ceil_bin
```

の二つへ線形配分する。

```text
lower += complex_value × (1 - fraction)
upper += complex_value × fraction
```

有効範囲外のNeighborは加算しない。

これによりNyquistを超えて移動するEnergyは境界付近で自然に減少する。

全Binを最終BinへClampしない。

---

# 30. Phase Frequency

Destination Binを決めるFrequencyと、

Phaseを進めるFrequencyは区別する。

Destination位置：

```text
Nominal Bin Frequency
```

Phase Accumulation：

```text
Instantaneous Frequency
```

を使用する。

```text
phase_frequency
=
instantaneous_frequency
× note_pitch_ratio
+
shift_hz
```

```text
phase
+=
2π
× phase_frequency
× hop_size
/ sample_rate
```

これによりOff-bin SinusoidのPhase Movementを維持する。

---

# 31. DC

DC Bin：

```text
bin 0
```

をPitch / ShiftでAudio Toneへ変換しない。

DCはDCとして扱う。

移動対象から除外する。

---

# 32. Nyquist

有効Destination Bin外へ移動するEnergyは破棄する。

Nyquistを超えたFrequencyを、

```text
NyquistへClamp
```

しない。

Multiple Bin Energyが最上位Binへ集中することを防ぐ。

---

# 33. Phase Accumulator

Runtimeは各Channel / Source BinについてPhase Accumulatorを持つ。

Note On時にだけPrepared Spectral FrameのPhaseから初期化。

その後は、

```text
Phase += Phase Advance
```

で進める。

---

# 33.1 Position Jump

Position Parameterが変わってもPhase AccumulatorをHard Resetしない。

Magnitude / Instantaneous Frequencyだけ新Positionへ移る。

これにより、

```text
Position Scrub
Freeze解除
Morph
```

で毎回Phaseが飛ぶことを防ぐ。

---

# 33.2 Phase Wrap

Phaseは定期的に、

```text
-π ～ +π
```

へWrapする。

長時間Freezeで無限に値を増加させない。

---

# 34. `phase_reset`

`true`：

Note On時に現在Source PositionのPrepared Phaseから開始。

`false`：

Voiceが再利用された場合にPhase Accumulatorを維持する。

ただしRuntime `reset()`では必ず初期状態へ戻す。

Fresh Runtime Determinismを優先する。

---

# 35. Source Frame Interpolation

Normalized Positionから得られたFrame位置がFractionalの場合、

```text
frame_a
frame_b
mix
```

を求める。

### Magnitude

Linear Interpolation。

### Instantaneous Frequency

Linear Interpolation。

### Absolute Phase

Note On / Phase Reset時だけCircular Interpolation。

Steady-state RenderではAbsolute Phaseを再適用しない。

---

# 36. `morph`

```text
0.0 ～ 1.0
```

Dynamic Parameter。

`asset_b`が存在する場合だけParameter Catalogへ公開する。

Asset Bなしで、

```text
morph != 0
```

はDefinition Error。

---

# 36.1 Magnitude Morph

単純Amplitude LerpではなくConstant-energy寄りのMorphとする。

概念：

```text
magnitude
=
sqrt(
    (1 - morph) × A²
    +
    morph × B²
)
```

Endpoints：

```text
morph 0 → A
morph 1 → B
```

を正確に維持する。

---

# 36.2 Instantaneous Frequency Morph

各BinのA / B Frequencyは、Magnitude Energyを考慮して補間する。

概念：

```text
weight_a =
(1 - morph) × magnitude_a²

weight_b =
morph × magnitude_b²
```

```text
frequency
=
(
    weight_a × frequency_a
    +
    weight_b × frequency_b
)
/
(weight_a + weight_b)
```

Energyがほぼ0ならNominal Bin FrequencyへFallback。

---

# 36.3 Phase Morph

毎FramePhaseをMorphしない。

Note On / Phase Reset時のInitial Phaseだけ、

```text
Circular Interpolation
```

する。

その後はMorphされたInstantaneous FrequencyからPhase Accumulatorを進める。

---

# 37. Morph Timeline

Asset AをTiming Masterとする。

Shared Normalized Cursor：

```text
0 → 1
```

を、

```text
A Frame Count
B Frame Count
```

それぞれへ変換する。

したがってDurationが違う2 Audioでも、

```text
Beginning ↔ Beginning
Middle    ↔ Middle
End       ↔ End
```

をMorphできる。

---

# 38. 処理順序

Spectral Generator内部の順序を固定する。

```text
Source Position
      ↓
Frame Interpolation
      ↓
A / B Morph
      ↓
Magnitude Blur
      ↓
MIDI Pitch Scaling
      ↓
Frequency Shift
      ↓
Destination Bin Remapping
      ↓
Phase Accumulation
      ↓
Complex Spectrum
      ↓
Inverse Real FFT
      ↓
1 / FFT Size Normalization
      ↓
Synthesis Window
      ↓
Overlap-add
```

順序をDefinitionから変更できるようにしない。

---

# 39. FFT Normalization

Inverse FFTのScalingをLibrary任せの暗黙挙動にしない。

IFFT後、

```text
1 / fft_size
```

をSonalloy側のContractとして適用する。

Forward → InverseのRound Trip Testで確認する。

---

# 40. Stereo

Spectral GeneratorはMono / Stereoを扱う。

## Mono A

```text
Output Mode = Mono
```

## Stereo A

```text
Output Mode = Stereo
```

Asset Bがある場合、AとChannel Countを一致させる。

Stereoでは、

* Position
* Freeze
* Blur
* Shift
* Morph

は共有する。

しかし、

```text
Magnitude
Phase
Instantaneous Frequency
Phase Accumulator
OLA
```

はL/R独立。

Stereo Imageを維持する。

---

# 41. Source終了

P9ではNatural ScanはOne-shot。

Loop Parameterを追加しない。

CursorがAsset Aの末尾を超えた場合、

```text
Target Magnitude = 0
```

へ移行する。

---

# 41.1 Blur Tail

Blur > 0の場合はSource終了後もSmoothed Magnitudeが残る。

そのため、

```text
source exhausted
```

だけではGenerator Finishedにしない。

以下を両方満たすまで処理する。

```text
Smoothed Magnitudes ≈ silence
OLA Ring ≈ silence
```

Silent Thresholdは固定内部定数とする。

例：

```text
1e-5
```

実際の値はTail Regression Testで確認する。

---

# 41.2 Freeze

```text
freeze = 1
```

ならCursorはSource末尾へ進まない。

したがってGeneratorはSource側理由ではFinishedにならない。

Note Off → Layer Envelope ReleaseによってVoiceが終了する。

---

# 42. Dynamic Parameter Contract

新規Parameter：

```text
spectral_position
spectral_freeze
spectral_blur
spectral_shift
spectral_morph
```

---

# 42.1 `spectral_position`

```text
Unit: Normalized
Range: 0 ～ 1
Scale: Linear
Smoothing: 10 ms
```

---

# 42.2 `spectral_freeze`

```text
Unit: Normalized
Range: 0 ～ 1
Scale: Linear
Smoothing: 10 ms
```

---

# 42.3 `spectral_blur`

```text
Unit: Seconds
Range: 0 ～ 1
Scale: Linear
Smoothing: 20 ms
```

---

# 42.4 `spectral_shift`

```text
Unit: Hertz
Range: -12000 ～ +12000
Scale: Linear
Smoothing: 10 ms
```

---

# 42.5 `spectral_morph`

Asset Bがある場合だけ存在。

```text
Unit: Normalized
Range: 0 ～ 1
Scale: Linear
Smoothing: 10 ms
```

新しいParameter Unitは追加しない。

---

# 43. Compiled Model

概念：

```rust
pub struct CompiledSpectral {
    pub source_a: Option<Arc<PreparedSpectralAsset>>,
    pub source_b: Option<Arc<PreparedSpectralAsset>>,

    pub root_note: u8,
    pub fft_size: usize,
    pub hop_size: usize,

    pub phase_reset: bool,

    pub output_mode: GeneratorOutputMode,
    pub latency_frames: usize,

    pub parameters: CompiledSpectralParameters,

    pub synthesis_plan: Arc<SpectralSynthesisPlan>,

    pub asset_a_path: String,
    pub asset_b_path: Option<String>,

    pub asset_a_sha256_specified: bool,
    pub asset_b_sha256_specified: bool,
}
```

---

# 44. Generator Output Mode

`CompiledGenerator::Spectral`を追加する。

`output_mode()`：

Prepared Source A成功時：

```text
Mono → Mono
Stereo → Stereo
```

Prepare失敗時はFallbackとしてMonoを返してよい。

Generator自体はUnavailableなのでAudioは出ない。

---

# 45. Availability

Spectral GeneratorがAvailableになる条件：

```text
source_a prepared
AND
(
    asset_b not specified
    OR
    source_b prepared
)
```

Asset Bを指定しているのにPrepare失敗した場合、

Aだけで勝手に再生しない。

DefinitionがMorph Source Bを要求した以上、Generator全体をUnavailableとする。

---

# 46. Asset Failure

既存Asset Error Contractを利用する。

* Not Found
* Hash Mismatch
* Decode Error
* Resample Error

に加えてSpectral Preparation Errorを分類する。

必要なら新規：

```text
SpectralPreparationFailed
```

を追加する。

---

# 47. Spectral Resource Limit

Spectral DataはSampleよりMemoryを消費する。

1 Spectral Cellあたり、

```text
Magnitude      4 bytes
Phase          4 bytes
Frequency      4 bytes
              --------
              12 bytes
```

を使用する。

P9では、

```text
MAX_PREPARED_SPECTRAL_BYTES_PER_ASSET
=
64 MiB
```

を固定上限とする。

Allocation前に、

```text
channels
× spectral_frame_count
× bin_count
× 12
```

をchecked arithmeticで計算する。

64 MiB超過は、

```text
GeneratorResourceLimitExceeded
```

として拒否する。

Runtimeで黙ってFrameを削らない。

---

# 48. Morph時Memory

Asset A / Bそれぞれ最大64 MiB。

したがって1 Spectral Generator最大Prepared Spectrumは概ね、

```text
128 MiB
```

まで。

Prepared AssetはVoice間で共有するため、

```text
128 MiB × Polyphony
```

にはならない。

Voice固有なのはPhase / Blur / FFT / OLA Stateだけ。

---

# 49. Process中禁止するもの

Spectral Runtime Process中に禁止。

```text
File I/O
WAV Decode
Resampling
Forward FFT Analysis
FFT Planning
Vec creation
Vec growth
reserve
resize
HashMap mutation
Spectrum Asset creation
Window generation
Profile allocation
String allocation
Blocking Lock
process() with internal FFT scratch allocation
```

---

# 50. Process中許可されるもの

事前確保済みBufferを使った、

```text
Frame lookup
Magnitude interpolation
Frequency interpolation
Morph
Blur
Bin remap
sin_cos
Complex accumulation
Inverse FFT process_with_scratch
Window multiply
OLA
```

だけ。

---

# 51. Lifecycle

## Note On

* Scan Progressを0へ
* Positionを現在値から評価
* OLA Ring clear
* Blur StateをCurrent Targetへ初期化
* Hop Scheduler reset
* `phase_reset=true`ならPrepared PhaseからPhase Accumulator初期化
* Source Exhausted=false

Heap Allocationなし。

---

## Note Off

Spectral Generator固有のRelease Envelopeは持たない。

既存Layer Envelopeへ任せる。

内部FreezeやBlur StateをNote Offで強制Resetしない。

---

## Voice Stealing

既存Voice Stealing Lifecycleに従う。

Steal Fade中に、

* FFT Plan再作成
* Spectrum再Prepare
* Buffer再確保

しない。

---

## Reset

次を初期化。

* Scan Progress
* Phase Accumulator
* Blur State
* OLA Ring
* Hop Scheduler
* Source Exhausted
* Pending Tail State

Reset後はFresh Runtimeと同等出力。

---

# 52. Block Size非依存

最低：

```text
32
64
257
1024
```

で比較する。

対象：

* Identity Resynthesis
* Position
* Freeze Transition
* Blur
* Shift
* Morph
* Hybrid

Spectral HopがHost Block境界へ引きずられてはいけない。

---

# 53. Sample Rate

最低：

```text
44,100
48,000
96,000
```

で確認。

* Correct Pitch
* Correct Source Duration
* Finite
* Non-silent
* Phase Stability
* Morph
* Freeze
* Shift
* Latency Report

FFT Size自体はSample数で固定なので、Sample Rateによって時間・周波数Resolutionが変わることは許容する。

ただしPitchやSource Scan時間がSample Rateに依存して変化することは許容しない。

---

# 54. Definition Validation

拒否する。

### Asset

* Empty Path
* Invalid SHA

### Root Note

* > 127

### FFT

* 1024 / 2048 / 4096以外

### Position

* NaN
* Infinity
* <0
* > 1

### Freeze

* NaN
* Infinity
* <0
* > 1

### Blur

* NaN
* Infinity
* <0
* > 1 second

### Shift

* NaN
* Infinity
* <-12000
* > +12000

### Morph

* NaN
* Infinity
* <0
* > 1
* Asset Bなしで0以外

### Prepared

* > 2 channels
* A/B Channel mismatch
* Spectral Memory Limit超過
* Empty Audio

RuntimeでClampして誤魔化さない。

---

# 55. CLI Inspect

最低限：

```text
kind: spectral
output_mode

root_note

fft_size
hop_size
bin_count
latency_frames

position
freeze
blur_seconds
shift_hz
morph
phase_reset
```

Asset A：

```text
asset_path
sha256_specified
prepared
source_sample_rate
source_channels
source_frames
prepared_sample_rate
spectral_frame_count
prepared_bytes
```

Asset Bも存在する場合は同様。

---

# 55.1 Parameter Inspect

公開するParameter ID：

```text
layer.<id>.generator.spectral_position
layer.<id>.generator.spectral_freeze
layer.<id>.generator.spectral_blur
layer.<id>.generator.spectral_shift
```

Morph Sourceあり：

```text
layer.<id>.generator.spectral_morph
```

---

# 56. Module構成

想定：

```text
crates/sonalloy-core/
├─ Cargo.toml
└─ src/
   ├─ spectral.rs                    # new
   ├─ definition.rs
   ├─ compiler.rs
   ├─ diagnostics.rs
   ├─ generator_parameters.rs
   ├─ parameter.rs
   └─ runtime/
      ├─ generator/
      │  ├─ mod.rs
      │  ├─ spectral.rs             # new
      │  ├─ additive.rs
      │  ├─ formant.rs
      │  └─ ...
      ├─ modulation.rs
      └─ voice.rs

crates/sonalloy-core/tests/
└─ spectral.rs                      # new

examples/instruments/
├─ spectral-generator-reference.json
└─ spectral-hybrid-reference.json

scripts/review/
├─ generate_spectral_resynthesis_package.py
└─ README.md

review-output/
└─ spectral-resynthesis/

docs/
├─ plan/
│  └─ plan-spectral-resynthesis-expansion.md
├─ architecture.md
├─ instrument-definition.md
├─ runtime-processing.md
├─ cli.md
├─ creating-an-instrument.md
└─ testing-and-sound-review.md
```

既存Repository構造を優先する。

---

# 57. Forward / Inverse FFT Unit Test

Deterministic Test Signalを作成。

```text
Real FFT
↓
Inverse FFT
↓
1 / N
```

で元信号を復元。

FFT Size：

```text
1024
2048
4096
```

全部確認する。

目標：

```text
max absolute error <= 1e-5
```

---

# 58. Window Reconstruction Test

Analysis / Synthesis Windowについて、

全Sample Positionで、

```text
Σ analysis_window
  × synthesis_window
≈ 1
```

を確認。

Tolerance：

```text
1e-5
```

程度。

---

# 59. Instantaneous Frequency Test

FFT Bin中央に存在しないSinusoidを使用する。

例：

```text
440 Hz
48 kHz
FFT 2048
```

Magnitude PeakのNominal Binだけでなく、

Instantaneous Frequencyが440 Hz付近になることを確認する。

---

# 60. Identity Resynthesis Test

P9で最重要のTest。

条件：

```text
root note
position = 0
freeze = 0
blur = 0
shift = 0
morph = 0
```

Source AとResynthesis結果をLatency Alignment後に比較。

最低限、

* Correlation
* RMS Error
* Max Error
* SNR

を測定する。

目標：

```text
SNR >= 60 dB
```

単純Sineだけでなく、

* Multi-tone
* Noiseを少量含むSignal
* Stereo Signal

でも確認。

このTestが成立する前にFreeze / Morphへ進まない。

---

# 61. Latency Test

Impulse Sourceを分析。

Resynthesisの最初のImpulse位置を検出。

```text
detected_latency
==
compiled latency
```

を確認する。

FFT Sizeすべてで実施。

---

# 62. Position Test

時間とともに周波数が変化するSourceを使用。

```text
position 0
position 0.25
position 0.5
position 0.75
```

で、期待するSource Segmentへ移動することを確認。

---

# 63. Freeze Test

変化するSourceを使用。

途中で、

```text
freeze 0 → 1
```

へ変更。

確認：

* Cursorが止まる
* Outputは無音にならない
* Phaseは進行する
* Hop BoundaryでClickしない
* 長時間RenderしてFinite
* Pitchが不自然にHop周期へ固定されない

---

# 64. Freeze Boundary Click Regression

P7 Granularで発見されたような聴感上のClickを防ぐ。

Smooth Sine Sourceで、

```text
freeze transition frame
```

前後のAdjacent Sample Deltaを専用測定する。

Globalな、

```text
delta < 0.25
```

だけを根拠にしない。

Transition付近を狙って測る。

---

# 65. Blur Test

同一Position Sweepを、

```text
blur = 0
blur = 0.5 sec
```

で比較。

Blur側のSpectral Fluxが低くなることを自動測定する。

単にWAVが違うだけでは合格にしない。

---

# 66. Shift Test

440 Hz Sine。

```text
shift +300 Hz
```

なら約740 Hz。

```text
shift -200 Hz
```

なら約240 Hz。

Frequency Estimateで検証。

---

# 67. MIDI Pitch Test

Root Note 60の440 Hz相当Fixture等で、

```text
root
+12 semitone
-12 semitone
```

を確認。

Frequencyは、

```text
×2
×0.5
```

へ動く。

一方Source Durationは同じ。

PitchとDurationを同時にTestする。

---

# 68. Morph Test

Source A：

```text
低いHarmonic Tone
```

Source B：

```text
高いHarmonic Tone
```

を使用。

```text
morph 0
morph 0.5
morph 1
```

で、

* EndpointがA / B
* Midpointに両方の特徴
* Continuous
* No Click

を確認。

---

# 69. Identical-source Morph Regression

Asset A / Bへ同じAudioを指定。

```text
morph 0
0.25
0.5
0.75
1
```

で大きく音が変化しないこと。

Morph Algorithm自体が不要な色付けを作っていないことを確認する。

---

# 70. Stereo Test

Stereo Fixture：

```text
Left  = 440 Hz + Harmonics
Right = 660 Hz + Harmonics
```

を使用。

Resynthesis後、

* L/Rが別
* Channel Swappingなし
* Stereo Collapseなし
* Fresh Runtime一致

を確認する。

---

# 71. Source End Test

短いOne-shot Source。

Freeze 0。

NoteをSource Durationより長く保持。

確認：

* Source末尾まで鳴る
* OLA Tailが完了する
* Blur Tailが完了する
* 永遠にVoiceがActiveにならない

---

# 72. Allocation Test

Audio Thread上で、

* Spectral Note On
* Spectral Render
* Freeze change
* Position change
* Shift change
* Morph change
* Voice Stealing
* Reset後再発音

にHeap Allocationがないこと。

最低：

```text
FFT 2048
Stereo
Morph A/B
16 Voice
```

を実際に発音して検査する。

---

# 73. Performance Test

SpectralはP9最大のCPUリスク。

Release Buildで最低限：

```text
FFT:
1024
2048
4096

Voices:
1
4
8
16
```

を測定。

追加：

```text
2048
Stereo
Morph A/B
16 Voice
```

を最重Representative Caseとして測定。

---

# 73.1 Performance Metrics

保存：

```text
audio_duration_seconds
elapsed_seconds
realtime_ratio
fft_size
hop_size
voice_count
channels
morph_enabled
spectral_frames
prepared_bytes
```

固定の合格CPU時間をP9では設定しない。

ただし、

* Voice数増加に対する異常な非線形増加
* FFT Size変更に対する異常な増加
* Process Allocation
* 明確なCPU暴走

は確認する。

---

# 73.2 Performance生成物

P8の反省を踏まえ、

Performance専用の、

```text
Definition
Event
WAV
```

をRepositoryへ大量にCommitしない。

Review ScriptでTemporary Directoryへ生成する。

Repositoryに残すのは、

```text
metrics.json
```

内のPerformance結果だけ。

---

# 74. Review Package

新規：

```text
review-output/spectral-resynthesis/
```

Script：

```text
scripts/review/generate_spectral_resynthesis_package.py
```

---

# 74.1 Review Asset

著作権や外部Sourceに依存しないDeterministic Fixtureを使用する。

Scriptで、

* Harmonic Source
* Moving Harmonic Source
* Stereo Source
* Noise / Texture Source

を生成する。

必要なら既存SonalloyのFormant ReferenceをRenderし、そのOutputをSpectral Sourceとして利用する。

外部から適当なVoice SampleをDownloadしてRepositoryへ入れない。

---

# 74.2 Review Audio

最低限：

```text
01-identity-resynthesis.wav
02-position-quarter.wav
03-position-half.wav
04-freeze.wav
05-freeze-transition.wav
06-blur.wav
07-shift-up.wav
08-shift-down.wav
09-root-note.wav
10-pitch-up-octave.wav
11-pitch-down-octave.wav
12-morph-a.wav
13-morph-mid.wav
14-morph-b.wav
15-morph-sweep.wav
16-position-scrub.wav
17-stereo-resynthesis.wav
18-high-note-spectrum.wav
19-spectral-hybrid.wav
20-spectral-hybrid-midi.wav
21-spectral-polyphony.wav
```

---

# 74.3 Technical Audio

別途：

```text
block-32
block-64
block-257
block-1024

sample-rate-44100
sample-rate-48000
sample-rate-96000

fft-1024
fft-2048
fft-4096

fresh-a
fresh-b

latency-impulse
```

を生成する。

---

# 75. Automatic Review Metrics

全Audio：

* Finite
* Peak
* RMS
* DC
* Max Adjacent Sample Delta
* Stereo Correlation

Identity：

* Latency
* SNR
* RMS Error
* Max Error
* Correlation

Freeze：

* Transition Delta
* Spectral Stability

Blur：

* Spectral Flux

Shift：

* Dominant Frequency
* Spectral Centroid

Pitch：

* Fundamental Estimate
* Duration

Morph：

* Endpoint Difference
* Midpoint Difference
* Sweep Continuity

High-note：

* Nyquist近傍Energy
* Out-of-band折り返し検査

Block Size：

* Max Absolute Difference

Fresh Runtime：

* Exact / Near-exact Difference
* SHA-256

Performance：

* Release Wall Time
* Realtime Ratio
* Scaling

---

# 76. Human Sound Review

## Identity

* 元Sourceの音色が明確に維持される
* 不要なFlangingがない
* Stereo Imageが大きく崩れない

## Freeze

* 同じ短いFrameのRepeat感にならない
* Bzzz / Clickがない
* Sustained Toneとして成立する

## Position

* Source内の場所が明確に変わる
* Scrub時に破裂音がない

## Blur

* 音の変化が滑らかになる
* 単なるLow-passのようには聞こえない
* Pitch自体が崩れない

## Shift

* Frequency ShiftとしてInharmonicになる
* Pitch Transposeとの違いが明確

## Morph

* A → Bが連続
* Midpointで音量が不自然に消えない
* Phase Jumpがない

## Pitch

* ±12 semitoneで明確にPitchが変わる
* Durationは変わらない

## Hybrid

* Spectral LayerだけLatencyがズレない
* Sample Attack / AdditiveとのTimingが合う
* Processor Chainと自然に組み合わさる

---

# 77. Reference Instrument

最低2つ追加。

```text
examples/instruments/
├─ spectral-generator-reference.json
└─ spectral-hybrid-reference.json
```

---

# 77.1 Spectral Generator Reference

一つのDefinitionで、

* Position
* Freeze
* Blur
* Shift
* Morph
* Root Note
* Stereo

を確認できるもの。

---

# 77.2 Spectral Hybrid Reference

例：

```text
Layer A: Spectral
Layer B: Additive
Layer C: Sample Attack
Layer D: Noise
```

既存、

```text
Layer Processor
Voice Processor
Global Processor
Modulation
```

まで組み合わせる。

---

# 78. Existing Generator Regression

P9実装後も最低、

```text
Oscillator
Noise
Sample
Granular
Wave Sequence
Wavetable
Operator Modulation
Additive
Formant
```

の既存Testを全成功させる。

`GeneratorRuntime`へSpectral Variantを追加するとき、

* new
* start
* note_off
* render
* reset
* output_mode
* intrinsic_latency
* availability

を漏らさない。

---

# 79. Documentation更新

## `docs/instrument-definition.md`

追加：

* Spectral JSON
* Asset A/B
* Root Note
* FFT Size
* Position
* Freeze
* Blur
* Shift
* Morph
* Phase Reset

---

## `docs/runtime-processing.md`

追加：

* Spectral Preparation
* STFT
* Phase Analysis
* Instantaneous Frequency
* Phase Accumulator
* Frequency Bin Remap
* IFFT
* OLA
* Latency
* Lifecycle

---

## `docs/architecture.md`

追加：

```text
Audio Asset
↓
Prepared Audio
↓
Prepared Spectral Asset
↓
Spectral Runtime
```

外部Native Backendを使わないことも明記。

---

## `docs/cli.md`

Inspect内容追加。

---

## `docs/creating-an-instrument.md`

* Spectral Source作成
* Freeze Pad
* Morph
* Hybrid例

---

## `docs/testing-and-sound-review.md`

Spectral Review Package追加。

---

## `.agents/skills/create-instrument/SKILL.md`

AIが、

* Spectral Definition
* Asset A/B
* FFT Size
* Freeze
* Morph

を正しく生成できるよう更新。

---

# 80. 実装単位A：Spectral Preparation / Identity Resynthesis

## 目的

Spectral Generatorの土台を完成させ、

> **何も加工しない状態で元AudioをSpectrum経由で正しく復元できる**

ところまで持っていく。

P9で最も重要なUnit。

---

## Unit A 実装順

1. Cargo Dependency整理
2. RealFFT追加
3. 不要ならDirect RustFFT削除
4. Spectral Definition
5. Validation
6. Parameter Contract基本追加
7. Prepared Spectral Asset
8. Spectral Asset Cache
9. Hann Window
10. Synthesis Window Normalization
11. Zero Padding
12. Forward FFT
13. Magnitude
14. Absolute Phase
15. Instantaneous Frequency
16. Compiled Spectral
17. Shared Inverse FFT Plan
18. Runtime Buffer
19. Inverse FFT
20. OLA
21. Latency Reporting
22. Root Note Identity
23. Mono
24. Stereo
25. CLI Inspect
26. FFT Roundtrip Test
27. Window Test
28. Instantaneous Frequency Test
29. Identity Test
30. Latency Test
31. Block Size Test
32. Sound Review

---

## Unit A完了条件

```text
Source Audio
    ↓
STFT
    ↓
Prepared Spectrum
    ↓
IFFT
    ↓
OLA
    ↓
Sourceに近いAudio
```

が成立すること。

Identity Resynthesisの品質が不十分なままUnit Bへ進まない。

---

# 81. 実装単位B：Phase / Position / Freeze / Pitch / Shift

## 目的

Spectral Generatorを単なるOffline FFT Roundtripから、

**演奏可能なPhase-aware Generator**

へする。

---

## Unit B 実装順

1. Runtime Phase Accumulator
2. Hop Scheduler
3. Frame Interpolation
4. Dynamic Position
5. Natural Scan
6. Freeze
7. Freeze Phase Continuity
8. Root Note Pitch Scaling
9. Layer Tuning
10. Pitch Bend
11. Destination Bin Remapping
12. Fractional Bin Distribution
13. Frequency Shift
14. Source End
15. OLA Drain
16. Voice Stealing
17. Reset
18. Position Test
19. Freeze Test
20. Freeze Boundary Test
21. Pitch Test
22. Shift Test
23. High-note Test
24. Block-size Regression
25. Human Review

---

# 82. 実装単位C：Blur / Morph / Stereo Integration

## 目的

P9で狙うSound-design機能を完成させる。

---

## Unit C 実装順

1. Magnitude Blur State
2. Blur Seconds Parameter
3. Blur Tail
4. Asset B Preparation
5. A/B Channel Validation
6. Normalized Timeline Mapping
7. Magnitude Morph
8. Frequency Morph
9. Initial Phase Morph
10. Optional Morph Parameter
11. Morph Endpoint Test
12. Identical-source Morph Test
13. Morph Sweep Test
14. Stereo Morph Test
15. 44.1 / 48 / 96 kHz
16. Allocation Test
17. Human Review

---

# 83. 実装単位D：Hybrid / Performance / Review / Documentation

## 目的

Spectral GeneratorをSonalloy全体へ統合し、Production Featureとして完成させる。

---

## Unit D 実装順

1. Spectral Reference Instrument
2. Spectral Hybrid Reference
3. Existing Processor統合
4. Existing Modulation統合
5. MIDI Render
6. Latency Alignment
7. 16 Voice Polyphony
8. Voice Stealing
9. Fresh Runtime
10. Performance Matrix
11. Existing Generator Regression
12. Review Package生成
13. Machine Metrics
14. Human Review
15. Documentation
16. AI Skill
17. CI
18. Final Code Cleanup

---

# 84. 主なリスク

## 84.1 Identity品質

### リスク

Phase処理やWindow処理が間違うと、加工0でも音が変わる。

### 対策

Unit AでIdentityを最初に固定。

SNR / Error / Human Reviewを必須にする。

---

# 84.2 Phase Vocoder的なSmearing

Pitch / Freeze / Position変更時にはPhase-vocoder系のSmearingが発生しうる。

P9では、

* Peak Locking
* Transient Preservation

を実装しない。

まず通常のPhase AccumulationでSound Design用途として成立することを確認する。

透明なMastering用Pitch Shifterを目標にしない。

---

# 84.3 CPU

Inverse FFTがVoiceごと・Hopごとに発生する。

### 対策

* RealFFT
* Scratch事前確保
* 4x Overlap固定
* FFT Size上限4096
* 16 Voice Performance Review
* FFT Planning共有

いきなりFFT計算の独自SIMD最適化を書かない。

---

# 84.4 FFT CPU Burst

複数VoiceのHopが同じSampleに揃うとIFFTが集中する。

P9ではまず実測する。

Split Computation Scheduler等は、実際にPerformance問題が確認されてから検討する。

先回り実装しない。

---

# 84.5 Memory

Spectral DataはRaw Sampleより大きい。

### 対策

* Prepared AssetをVoice間Arc共有
* 64 MiB / Asset上限
* 最大2 Source
* Stereoまで
* Runtime StateだけVoice固有

---

# 84.6 Position Jump Click

Positionを変えるとMagnitude / Frequencyが急変する。

### 対策

* Parameter Smoothing
* Phase Accumulator継続
* Blur利用可能
* 専用Transition Regression

---

# 84.7 Freeze Bzzz

固定Complex FrameをRepeatするとFFT周期感が出る。

### 対策

Freeze中もInstantaneous FrequencyからPhaseを進行させる。

---

# 84.8 Morph Phase Jump

A/BのAbsolute Phaseを毎Hop LerpするとPhaseが不連続になる。

### 対策

Absolute Phaseは初期化時だけ使用。

Steady-stateはMorph Frequencyから共通Phase Accumulatorを進める。

---

# 84.9 Latency

Spectral Generatorには大きなAlgorithmic Latencyがある。

### 対策

既存Instrument Latency Compensationを使用。

別のLatency Systemを作らない。

---

# 85. P9で行わない先回り設計

作らない。

```text
Generic FFT Engine API
Spectral Node Graph
Spectral Processor Framework
External Input Framework
Vocoder Framework
Frequency-domain Plugin API
Generic Phase Vocoder Framework
```

必要なのは、

```text
Spectral Generator
```

だけ。

Forward / Inverse FFT HelperはSpectral内部Primitiveとして実装する。

---

# 86. Code Quality完了条件

P9終了時、

* `todo!()`
* `unimplemented!()`
* `dbg!()`
* 仮実装
* unused field
* unused helper
* `allow(dead_code)`
* 不要な直接Dependency
* 重複FFT Wrapper
* Review専用Production Code
* Performance用大量Generated File

を残さない。

`#[allow(clippy::...)]`は理由が明確な数値Cast等だけ。

---

# 87. Realtime Safety完了条件

Audio Thread上で、

```text
Heap Allocation = 0
```

を実測する。

少なくとも、

```text
Stereo
FFT 2048
Morph enabled
16 Voice
```

で確認。

FFTのScratch Allocationも0。

---

# 88. Determinism完了条件

同じ、

* Definition
* Asset
* Process Spec
* Event

からFresh Runtime A / Bを生成。

Output一致。

Spectral GeneratorへRandomnessは追加しない。

---

# 89. P9最終完了条件

以下をすべて満たした時のみ完了。

1. Spectral GeneratorをDefinitionへ保存できる
2. Asset AをPrepareできる
3. Optional Asset BをPrepareできる
4. Monoを扱える
5. Stereoを扱える
6. A/B Channel mismatchを拒否できる
7. 1024 / 2048 / 4096 FFTを扱える
8. Hop SizeがFFT / 4で固定される
9. STFT Analysisが成立する
10. Magnitudeを保持できる
11. Absolute Phaseを保持できる
12. Instantaneous Frequencyを保持できる
13. Prepared Spectrumに64 MiB上限がある
14. Root NoteでIdentity Resynthesisできる
15. Identity SNR Testを通る
16. Exact Spectral Latencyを報告できる
17. Existing Layer Latency Compensationと統合される
18. Spectral Positionが動く
19. Natural Scanが動く
20. Freezeが動く
21. Freeze中もPhaseが連続する
22. Blurが動く
23. MIDI PitchでPitchが変化する
24. MIDI PitchでDurationが変わらない
25. Spectral Shiftが動く
26. Destination Bin Remapが正しく動く
27. Nyquist超過EnergyをClampしない
28. Optional Morphが動く
29. Morph 0 / 1がA / Bに対応する
30. Morph Midpointが連続する
31. Identical-source Morphで大きな色付けがない
32. Position JumpでPhase Hard Resetしない
33. Source終了が正しい
34. Blur Tailが正しく終了する
35. Freeze時にSource側理由で終了しない
36. Voice Stealingが正しい
37. ResetがFresh Runtimeと一致する
38. Process中Heap Allocationがない
39. FFT Process中Scratch Allocationがない
40. Block Size 32 / 64 / 257 / 1024で時間軸が一致する
41. 44.1 / 48 / 96 kHzでPitch / Timingが正しい
42. 16 Voice Performanceを計測している
43. CLI InspectでSpectral情報を確認できる
44. Parameter Catalogへ正しく公開される
45. Existing Generatorへ回帰がない
46. Spectral Reference Instrumentが存在する
47. Spectral Hybrid Referenceが存在する
48. Review Packageが存在する
49. Identityを人間が試聴して承認する
50. Freezeを人間が試聴して承認する
51. Blurを人間が試聴して承認する
52. Shiftを人間が試聴して承認する
53. Morphを人間が試聴して承認する
54. Hybridを人間が試聴して承認する
55. Windows CI成功
56. Linux CI成功
57. Sanitizer成功
58. Native Fault Injection回帰なし
59. 不要なNative変更がない
60. Performance専用生成WAVをRepositoryへ残していない

---

# 90. P9完了後の到達点

P9完了時点の主要Generator：

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
Spectral / Resynthesis
```

となる。

音の生成方式として、

```text
波形
Sample
FM / PM
Wavetable
Granular
Sequence
Partial
Formant
Spectrum
```

までカバーする。

この時点でSonalloyは、

> **Audioをそのまま再生するだけではなく、Audioを周波数構造として分解し、別の音へ再構築できる**

段階へ進む。

次の大きな未実装Generator群は、

```text
Physical
Modal
Waveguide
```

となる。

したがってP10以降では、

```text
振動体
共鳴体
Feedback
Delay Line
Exciter
```

という、Spectralとは別系統のPhysical Modeling基盤へ進む。

P9ではそこへ踏み込まず、

**Spectral Analysis → Phase-aware Resynthesis → Freeze / Blur / Shift / Morph**

を完全に仕上げることを最終目的とする。
