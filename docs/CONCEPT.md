# Sonalloy 要件定義・基本設計書

## 1. 製品コンセプト

### 1.1 基本思想

**Sonalloy**（Son「音」+ Alloy「合金」）は、音声素材と複数の音響合成方式をLayerとして組み合わせ、演奏可能な独自Instrumentを構築するハイブリッド音源エンジンです。CLIを第一級インターフェースとし、特定のDAWやGUIに依存しません。

**目指すもの**：一般的な電子音楽で利用される主要な音響合成方式、Sample操作、Modulation、音色処理を、一つのInstrument Engineとして安全に組み合わせられること。

> **注**：任意のAudio Graphを実行できる万能DSP環境ではなく、実用上重要な方式を専用Generator/Processorとして提供し、半固定パイプライン内で安全に扱えることを重視します。

### 1.2 四つの中心価値

| # | 価値 | 具体例 |
|---|---|---|
| 1 | 任意の音を楽器化できる | 単音Sampleの音程展開／Loop・SliceによるVocal Chop／雨音をGranular化したPad |
| 2 | ゼロから幅広い電子音を生成できる | FM Bell／PWM Pad／Wavetable Bass／Additive Drone／Modal Percussion |
| 3 | 複数方式を一つのInstrumentに融合できる | FM（金属倍音）+ Sine（低域）+ Metal Hit Sample（Attack）+ Noise（質感）+ Convolution（Body） |
| 4 | CLI-first設計でAI最適、多様な環境へ展開できる | AIによるParameter最適化／CLIスクリプトによる大量定義生成／CLAP・VST3などへの展開 |

### 1.3 入力と出力

```
┌─────────────────────────────────────────────────────────┐
│  入力                                                   │
│  ├─ A. 音声素材：録音/Field Recording/Loop/Slice/IR   │
│  ├─ B. 音響合成：Osc/FM/Granular/Additive/Spectral等  │
│  └─ C. 外部Audio入力（Vocoder/Envelope Follower等）   │
└──────────────────────┬──────────────────────────────────┘
                       ▼
              ┌────────────────┐
              │  Sonalloy Core │
              └───────┬────────┘
                      ▼
         ┌────────────────────────┐
         │  出力：Instrument      │
         │  ・MIDI演奏可能        │
         │  ・定義ファイル保存    │
         │  ・複数Frontendで利用  │
         └────────────────────────┘
```

### 1.4 エコシステム内の位置づけ

```
┌──────────────────────────────────────────────────────┐
│  Host / DAW（Riffra, Ableton, Bitwig, Reaper...）    │
├──────────────────────────────────────────────────────┤
│  Plugin / Frontend（CLAP, VST3, CLI, Standalone）    │
├──────────────────────────────────────────────────────┤
│  ★ Sonalloy Engine（本体）                           │
│  Layer / Generator / Voice / Modulation / Runtime    │
├──────────────────────────────────────────────────────┤
│  DSP Primitives（Oscillator, Filter, FFT...）        │
└──────────────────────────────────────────────────────┘
```

Sonalloy本体はPlugin API・Audio Device APIに依存せず、すべてAdapterとして外側に配置します。

### 1.5 スコープ

**扱う範囲**：
- **Sampling**：One-shot/Multi-Sample/Loop/Slice/Time Stretch/Wave Sequence
- **Oscillator**：基本波形/PWM/Hard Sync/Phase Distortion/Waveshaping/Unison
- **合成Generator**：Subtractive/Noise/Wavetable/FM/Granular/Additive/Spectral/Physical-Modal/Formant
- **Modulation**：LFO/Envelope/MSEG/Step/Random/Velocity/Key Tracking/Macro/Envelope Follower
- **Processing**：Filter/Drive/EQ/Comb/Bitcrusher/Freq Shifter/Chorus/Delay/Reverb/Convolution/Dynamics
- **Instrument機能**：Hybrid構成/Polyphony/Voice Management/MIDI演奏

**意図的に扱わない範囲**：
- 完全自由なModular Audio Graph（Cycle管理・計算量予測が困難）
- ユーザー定義の任意Feedback Routing（不安定な循環接続を防止）
- 無制限のDSP Script実行（Realtime Safety・再現性の維持）
- DAW機能（Arrangement/Recording/MixingはHostの責務）
- 生楽器の完全物理再現（電子音生成に有効なPhysical/Modal方式のみ）

---

## 2. アーキテクチャ

### 2.1 設計原則

**「音の配線は固定、音の変化は自由」**

信号経路はSonalloy側で固定し、各パラメータの時間変化（Modulation）に高い自由度を持たせます。完全自由なモジュラーシンセ方式は採用しません。

> **理由**：Graph Scheduling・Cycle管理・計算量予測まで製品責務が膨張し、Realtime Safetyの保証が著しく困難になるため。

FM、Ring Mod、Physical Modelingなど、複数信号のAudio-rate相互作用やFeedbackを必要とする方式は、**専用Generator/Processor内部の固定Topology**として実装します。

### 2.2 信号経路：半固定パイプライン

```
Note/Control Event ──┐
                     ▼
            ┌─────────────────┐
            │ Voice Allocation│
            └────────┬────────┘
                     ▼
┌───────────────── Per Voice ────────────────────┐
│                                                │
│  Layer Trigger Evaluation                      │
│         │                                      │
│         ▼                                      │
│  Generators                                    │
│  （Sample/Osc/Noise/Wavetable/FM/Granular/    │
│    Additive/Spectral/Physical/Formant）        │
│         │                                      │
│         ▼                                      │
│  Layer Envelope / Layer Processing             │
│         │                                      │
│         ▼                                      │
│  Layer Mix                                     │
│         │                                      │
│         ▼                                      │
│  Voice Processing / Amplifier                  │
│                                                │
└─────────────────┬──────────────────────────────┘
                  ▼
            Voice Sum
                  │
                  ▼
      Global Effects / Output Dynamics
                  │
                  ▼
                Output
```

| 単位 | 責務 |
|------|------|
| **Layer** | Generator、発音条件、Layer Envelope、Layer Processing、Gain/Pan/Tuning |
| **Voice** | 同一Noteから生じる複数LayerのMix、Voice Processing、Amplifier |
| **Instrument** | 複数Voiceの合流、Global Effects、最終Output |
| **Dedicated Engine** | FM Operator、Granular Scheduler、Spectral Resynthesis等の方式固有Topology |

### 2.3 Modulationシステム

**Source（変調源）**：
LFO、Envelope、MSEG、Step Modulator、Sample & Hold、Smooth Random、Velocity、Key Tracking、Pitch Bend、Mod Wheel、Aftertouch、Macro、Envelope Follower、Tempo/Transport、Generator固有Source

**接続例**：

| Source | → Target | 効果 |
|--------|----------|------|
| LFO | Filter Cutoff | 周期的な明るさの変化 |
| Envelope | Pitch/FM Index | 発音時の音程・倍音変化 |
| Velocity | Layer Gain/Filter | 演奏強度の反映 |
| Random | Layer Pan/Wavetable | Noteごとの個体差 |
| MSEG | Wavetable Position | 複雑な時間変化 |
| Macro | 複数Target | 一つの操作で音色全体を変化 |

**Control-rateとAudio-rateの分離**：
- **Control-rate**：Gain、Pan、Filter Cutoff、Wavetable Positionなどの連続Parameter
- **Audio-rate**：FM/PM/AM/Ring ModなどのSample単位信号相互作用（専用Generator内部で処理）

**共通規則**：
- 各Parameterは一意なID、型、単位、最小/最大/初期値を持つ
- Modulation RouteはSource・Target・Depth・Curveで定義する。DepthはTargetに応じた明示的な単位（dB、Pan、Cents、Hertz、Seconds、PerSecond、Index、dB/Octave、Normalized、Octaves）を持つ
- Source出力は正規化（単方向：0〜1、正負：-1〜1）
- 複数Routeが同一Targetへ接続された場合、定義順でDepthをTargetのDomainへ加算し、最後にClampする。Linear ParameterはNative Domain、Log2 ParameterはOctave Domainで評価する
- 連続値変更は平滑化を適用しクリックノイズを回避
- 波形種類、FM Algorithm、Processor種類などの離散値はModulation対象外とし、変更時は再Compile

Routeの定義例：

```json
{
  "source": "vibrato",
  "target": "layer.body.tuning",
  "depth": { "value": 20.0, "unit": "cents" },
  "curve": "linear"
}
```

Linear Targetでは`curved_source × depth`をNative値へ加算します。Log2 TargetではDepthをOctave Domainの加算値として`base × 2^sum`へ変換します。たとえば`2` octavesはNative値を最大4倍、`-1` octaveは半分にします。

**評価単位**：
- **Voice単位**：Velocity、Key Tracking、Envelope、LFO、MSEG、Random
- **Instrument単位**：Mod Wheel、Macro、Tempo/Transport同期Source
- **Input単位**：Envelope Follower、Vocoder分析値
- **Generator内部**：FM/PM/AM、Grain、Partial、Spectral FrameなどのAudio-rate処理

### 2.4 拡張規則

新しい音源方式は、次のいずれかとして追加します：

| 追加先 | 判断基準 |
|--------|----------|
| **Generator** | 音を発生させる独立方式、Noteごとの状態を持つ |
| **Generator Variant** | 既存Generatorと同じ基本責務を持つが、方式固有Parameterや処理方式が異なる |
| **Layer Processor** | 特定Layerの信号を加工 |
| **Voice Processor** | 同一NoteのLayer Mix全体を加工 |
| **Global Effect** | Voice Sum後のInstrument全体を加工 |
| **Modulation Source** | 音を直接出さず、Parameterの時間変化を作る |
| **Optional Input Processor** | 外部Audio入力を解析し、VocoderやEnvelope Followerなどへ利用する |

新機能を自由Graphへ逃がさず、最も狭い責務へ配置します。

---

## 3. 機能仕様

### 3.1 Instrument（中心概念）

Sonalloyの最上位モデル。一つのInstrumentが一つの「楽器」を表します。

```
Instrument
├── Layers[]                    ← 音色を構成するLayer
│   ├── Generator               ← 音の発生方式
│   ├── Trigger Conditions      ← 発音条件
│   ├── Gain/Pan/Tuning
│   ├── Layer Envelope
│   └── Layer Processing
├── Voice Processing            ← Layer Mix後のFilter/Drive/EQ等
├── Modulation Matrix           ← Source → Target
├── Macros                      ← 複数Parameterの演奏用操作
├── Global Effects              ← Voice Sum後のEffect
├── Performance Settings        ← 発音方式、同時発音数
├── Optional Audio Input        ← Vocoder等で使う入力
└── Asset References            ← Sample/Wavetable/IR等
```

**LayerとGeneratorの責務分離**：
- Generator：「どの方式で音を発生させるか」
- Layer：「いつ・どの範囲で・どのようにInstrumentへ混ぜるか」

### 3.2 Generator（音の発生源）

各Layerは一つのGeneratorを持ちます。

| Generator | 概要 | 主要Parameter |
|-----------|------|---------------|
| **Sample** | 音声素材の再生とMapping | Zones、Playback Mode、Loop、Slice、Time Stretch |
| **Basic Oscillator** | 基本波形生成 | Sine/Saw/Square/Pulse/Triangle、Phase |
| **Complex Oscillator** | 高度なOscillator | PWM、Hard Sync、Phase Distortion、Waveshaping、Wavefold、Oscillator Feedback、Unison、Detune Distribution、Stereo Spread、Phase Distribution |
| **Noise** | Noise信号生成 | White/Pink/Brown、Color、Stereo Correlation |
| **Wavetable** | 複数周期波形間の連続移動 | Table、Position、Interpolation、Warp、Unison |
| **Operator Modulation** | Oscillator間のFM/PM/AM/Ring Mod | Operator、Ratio、Index、Algorithm、Feedback |
| **Granular** | 素材をGrainへ分割・再構成 | Position、Grain Size、Density、Pitch、Randomness、Pan Spread |
| **Additive** | 複数Partialの合成 | Harmonic/Inharmonic Partial、Amplitude、Phase、Partial Envelope、Spectrum Tilt、Inharmonicity、Morph |
| **Spectral/Resynthesis** | Spectrum Frameから再構成 | Position、Freeze、Blur、Shift、Morph、Phase管理、Overlap-add |
| **Physical/Modal/Waveguide** | Feedback・共振系合成 | Model、Exciter、Damping、Stiffness、Dispersion |
| **Formant** | Vowel/Vocal-like Spectrum生成 | Formant Frequencies、Bandwidth、Vowel Position、Formant Shift、Throat、Spectral Tilt |
| **Wave Sequence** | Sample/Waveを時間順に切替 | Steps、Duration、Crossfade、Step Pitch/Gain、Loop、Direction、Tempo Sync、Random選択 |

**設計判断**：
- **Subtractive Synthesis**：独立Generatorを設けず、Oscillator/Noise + Layer/Voice ProcessingのFilter・Amplifierで表現
- **Vector Synthesis**：複数Layer GainをXY Vector SourceでConstant-power Crossfade
- **Vocoder**：外部Audioを必要とするInput/Voice/Global Processorとして扱う
- **Audio-rate相互作用**：通常Modulation Matrixには載せず、専用Generator内部へ閉じる

### 3.3 Voice Management

Voiceは、一つのNote Onから発生するLayer群とその演奏中状態をまとめる単位です。

**発音方式**（Performance Settingsで選択）：

| 方式 | 内容 |
|------|------|
| Polyphonic | 複数Noteを独立Voiceとして同時発音 |
| Monophonic | 同時に一Voiceだけを使用 |
| Legato | 前の鍵盤を保持したまま次の鍵盤を押した場合、Envelopeを再開始せず音程を移行 |
| Portamento | 設定時間をかけて音程を滑らかに移動 |
| Sustain Pedal | 鍵盤を離しても保持し、Pedal解除時にRelease |

**Voice管理機能**：
- Note On/Off、Note IDによる対応付け、Voice Allocation、Layer Trigger Evaluation
- Voice Stealing（Release中で出力の小さいVoiceを優先）
- Release処理、Polyphony Limit

**Note ID**：各Note OnにはFrontend/Adapterが一意なNote IDを付与。Note Off・音程変更・表現変更は同じNote IDで対象Voiceを特定します。

### 3.4 Sample MappingとProcessing

Sample Mappingは、Sample Generatorを持つLayerが発音対象になった**後**に、実際に使用するSample Zoneを選択する機能です。

```
Layer Trigger Conditions
        ▼
Sample Layerを発音
        ▼
Key × Velocity × Articulation × Round Robin
        ▼
Sample Zone選択
        ▼
Playback / Loop / Slice / Stretch
```

| 概念 | 判断内容 |
|------|----------|
| Layer Trigger Conditions | そのLayer自体を発音するか |
| Sample Mapping | 発音するSample Layer内でどのZoneを使用するか |
| Sample Processing | 選択した素材をどの位置・速度・Loop・Sliceで再生するか |

**Sample Zoneの保持情報**：
Asset Reference、Root Note、Key/Velocity Range、Round Robin Group、Articulation、Playback Mode、Start/End Position、Loop、Crossfade、Slice Map、Time Stretch Mode

**Playback Mode**：
One-shot、Gate、Loop、Crossfade Loop、Reverse、Release Trigger

**Advanced Sample Processing**：
Sample Start/End Position、Transient-based Slice、Tempo Sync、Pitch ShiftとTime Stretchの分離、Scrub、Freeze、Slice順序/Randomization、Loop Crossfade

**表現できるInstrument**：
単一Sampleの音域展開、Multi Sample Instrument、Velocity Layer、Round Robin Drum、Drum Kit、Loop Instrument、Vocal Chop、Beat Slice Instrument、Tempo同期Loop

**Round Robin規則**：
同じKey Range・Velocity Range・Articulationに複数Zoneが該当する場合、同一Round Robin Group内から決定的な順序で選択。Offline Renderの再現性を維持するため、Random選択を使う場合もInstrument Seed・Note ID・Zone IDから決定します。

### 3.5 Effects / Processor

音源設計に直接必要なProcessorを内蔵します。

**適用位置**：

| 適用範囲 | 対象 | 使用可能なProcessor |
|----------|------|---------------------|
| **Layer Processing** | 特定Layerだけ | Filter、Drive、Saturation、Waveshaper、EQ、Comb、Formant、Bitcrusher、Freq Shifter |
| **Voice Processing** | 一NoteのLayer Mix全体 | Filter、Drive、EQ、Comb/Resonator、Formant、Dynamics、Vocoder Carrier |
| **Global Effects** | Voice Sum後のInstrument全体 | Filter、Drive、EQ、Chorus、Flanger、Phaser、Delay、Reverb、Convolution、Dynamics |
| **Input Processing** | 外部Audio入力 | Gain、Filter、Envelope Follower、Vocoder Analysis |

Delay・Reverb・Chorus・Convolutionは発音Voice数に比例して状態を複製しないよう、基本的にGlobal Effectsとして扱います。

**Processorカテゴリ**：
Filter、Nonlinear（Drive/Saturation/Distortion）、Tone（EQ）、Resonance（Comb/Resonator/Formant）、Digital（Bitcrusher）、Frequency Shifter、Modulation FX（Chorus/Flanger/Phaser）、Time（Delay/Reverb）、Convolution、Dynamics、Cross Synthesis（Vocoder）

- **Filter**：Low-pass、High-pass、Band-pass、Notch、State-variable、Ladder
- **Nonlinear**：Clipper、Waveshaper、Wavefolderを含む
- **Digital**：Sample-rate Reducer、Quantizerを含む
- **Dynamics**：Compressor、Limiter、Gate、Transient Shaper
- **Time**：Multi-tap Delayを含む
- **Cross Synthesis**：Envelope Transfer、制限付きSpectral Morphを含む

**Vocoder構成**：
```
External Audio Input（Modulator）
        ▼
Analysis Filter Bank / Envelope
        ▼
Generator / Layer Mix（Carrier）
        ▼
Band Gain適用
        ▼
VoiceまたはGlobal Output
```

**処理規則**：
- ProcessorはDefinitionに記載された順序で直列適用
- 任意Routingやユーザー定義Feedback接続は扱わない
- Processor内部の固定Feedbackは許可
- 連続ParameterはModulationおよび演奏中変更の対象
- Latencyを持つProcessorはCompile時にLatencyを確定しFrontendへ報告

### 3.6 Vector / Macro / Performance Control

**Vector Synthesis**：
複数LayerのGainを連動制御して表現（2-Way/4-Way Crossfade、XY Vector Source、Constant-power Mix、Vector Envelope）。Layer Generatorの種類は問いません。

**Macro**：
演奏用に公開する安定Parameter。一Macroから複数Targetへ接続可能。TargetごとにRange/Curveを持ち、Frontendに共通名・単位・初期値を公開します。

---

## 4. データモデル：Instrument Definition

### 4.1 三層構造

Instrumentの構成は、Version管理可能なText Data（JSON）で表現します。

> **JSON採用理由**：Binary内部状態のDumpでは、再現性・可読性・差分管理が担保できないため。

```
Instrument Definition + Referenced Assets
                │
                │ Load/Validate/Resolve/Analyze/Compile
                ▼
        Compiled Instrument
                │
                │ Instantiate
                ▼
      Instrument Runtime Instance
```

| モデル | 責務 |
|--------|------|
| **Instrument Definition** | 編集・保存・差分管理可能なInstrument構成の正本 |
| **Compiled Instrument** | Asset・Parameter・Route・Processor・解析結果を解決した実行用不変構造 |
| **Instrument Runtime Instance** | Voice、Envelope、Grain、Partial、Spectral、Filter、Effect等の演奏中状態 |

同一のDefinition + Referenced Assets + Backend条件から同等のCompiled Instrumentを構築し、Instrumentを再現できることが要件です。

### 4.2 保持内容

**Definitionが保持するもの**：
- 識別：Schema Version、Metadata（名前、作者、説明）
- 音源構成：Layer一覧、Trigger、Mix、Processing、Generator種類と固有Parameter
- Parameter：Parameter ID、型、単位、最小/最大/Default、Smoothing
- 変調：Source、Route、Depth（値と単位）、Curve、Macro、Vector
- Sample Mapping：Zones、Key/Velocity/Articulation/RR、Loop、Slice、Stretch
- 合成Asset：Wavetable、Spectral Frame、IR、Modal Data、Wave Sequence Asset
- 演奏：発音方式、Polyphony、Voice Stealing、Sustain、Legato、Portamento
- 効果：Layer/Voice/Global/Input Processor
- External Input：Input Bus Binding、Channel、Role（Vocoder Modulator等）
- 再現性：Random Seed、Asset Hash、Model Hash、Analysis Version
- 実行制約：最大Voice、Unison、Grain、Partial、Spectral Frame、Latency

**Compile時に解決するもの**：
Asset参照とHash、Decode済み/Resample済みSample、Loop/Slice/Stretch準備、Wavetable Data、Spectral Analysis/Frame、IR Partition、Modal/Waveguide Data、Parameter/Modulation Target、Macro/Vector Mapping、Processor Chain、Latency、実行時Memory配置、CPU量上限値

**保存対象外**：
Compiled Instrument、Runtime状態、Decode済みBuffer、FFT一時Buffer、Voice/Grain/Partial/Filter/Delay状態、Device/Plugin Handle、一時計算結果

### 4.3 読み込みと更新のルール

**読み込み時のError処理**：

| 状況 | 対応 |
|------|------|
| Definition構造や値に矛盾 | Compile失敗。現在利用中のCompiled Instrumentを維持 |
| 参照Assetが見つからない | Instrument全体を失敗させず、依存するZone/Layer/Processorだけ無効化 |
| Hash不一致 | 対象Assetを無効化し、診断を返す |
| Optional Backendがない | 対象Optional Layerだけ無効化し、他Layerを継続 |
| CPU/Memory上限超過 | Compile Errorとして拒否 |
| External Inputがない | Input必須Processorを無音化またはBypassし、Frontendへ診断を返す |
| 事前解析に失敗 | 対象Generator/Processorを無効化し、再解析可能な診断を返す |

**変更時の反映規則**：

| 変更種別 | 反映方法 |
|----------|----------|
| 連続値変更 | Parameter Change EventとしてRuntimeへ渡し、平滑化して発音中Voiceへ反映 |
| Macro/Vector変更 | 対応する連続Parameterへ展開して反映 |
| Tempo/Transport変更 | Process Contextから同期Source/Sequence/Delayへ反映 |
| 構成変更 | Control側で新しいCompiled Instrumentを生成 |
| Asset/Wavetable/IR/Model変更 | 再Resolve/Analyze/Compile |
| Audio-rate Algorithm変更 | 離散構成変更として再Compile |

**構成変更の反映タイミング**：
- Compile成功した構成はAudio Block境界で公開
- 新しく開始するVoiceから新構成を利用
- 発音中Voiceは、原則として発音開始時の構成をRelease完了まで使用
- Compile失敗時は現在のCompiled Instrumentを変更しない
- Global Effects変更はBlock境界で切り替え、必要に応じて短いCrossfade
- Latency変更はFrontendへ通知し、安全な切替単位を要求
- Asset不足やOptional Engine不足でも、独立した他Layerを停止しない

---

## 5. インターフェース

### 5.1 CLI（第一級）

GUIやDAWがなくても、CLIだけでSonalloyの主要機能を完結できます。

| 操作カテゴリ | 具体機能 |
|--------------|----------|
| **作る** | 新規作成、Layer/Generator/Processor追加・削除、Sample/Wavetable/IR追加、Parameter/Modulation設定 |
| **理解する** | 内容表示、構成解析、Validation、依存Asset、Latency、推定計算量の確認 |
| **演奏する** | 単音、Note Sequence、MIDI File、External Control、必要ならAudio Input付き演奏 |
| **書き出す** | Offline Render、Stem/WAV等へのExport |
| **素材処理** | Sample Slice、Loop確認、Wavetable/Spectral/IR事前解析 |
| **リアルタイム** | MIDI Device + Audio Deviceによる演奏（Linux/Windows） |
| **修復する** | 不足Assetの再指定、再Validation、再Compile |
| **比較する** | 複数Definition/Parameter VariantのRender比較 |

CLIが操作する正本はInstrument Definitionであり、Command列はDefinitionを操作する手段に過ぎません。

### 5.2 Plugin（CLAP/VST3）

外部DAWからSonalloy Instrumentを利用するAdapter。Sonalloy CoreはPlugin APIを知りません。

CLAP/VST3 Adapterは次を共通Contractへ変換します：
- Note/Expression Event
- Parameter Automation
- Tempo/Time Signature/Transport
- Audio Output Buffer
- Vocoder等で必要なAudio Input Bus
- Latency報告
- State Save/Restore

### 5.3 Rust API / C ABI

Sonalloy Coreは、接続元に依存しない共通LifecycleとProcess Contractを公開します。

**Lifecycle**：
```
Prepare → Activate → Process（繰り返し） → Reset/Deactivate
```

| Phase | 内容 |
|-------|------|
| **Prepare** | Sample Rate、最大Block Size、Input/Output Channel、Context能力を受け取り、Bufferを事前確保 |
| **Activate** | Audio処理可能状態へ移行 |
| **Process** | Frame数、Context、Event列、Input/Output Bufferを受けて音声生成 |
| **Reset** | Voice、Generator、Envelope、Processor状態を初期化 |
| **Deactivate** | Audio処理終了 |

**Process Contract**：
```
Process
├── Process Context
│   ├── Absolute Frame
│   ├── Tempo/Time Signature
│   ├── Beat/Bar Position
│   └── Transport State
├── Events[]
│   ├── Note/Expression
│   ├── Sustain/Pitch Bend/Mod Wheel/Aftertouch
│   ├── Parameter Change/Macro
│   └── Transport-related Event
├── Optional Input Buffers[]
└── Output Buffers[]
```

**仕様**：
- Sample Rate・最大Block Size・Input/Output ChannelはPrepare時に確定
- 一ProcessのFrame数は最大Block Size以下で、呼び出しごとに変化可能
- Audio Sample Formatは `f32`、Planar Buffer
- 少なくともMono/Stereo Outputを扱う
- Input BufferはInstrumentが要求する場合だけ使用
- EventはSample Offset昇順。同一OffsetではAdapterが確定した順序を維持し、CoreはEvent列の順番で処理する。OfflineのEvent JSON / MIDI File Adapterは同一NoteのNote OffとNote Onが同時の場合を含め、`priority()`でCanonical順へ正規化する。Realtime MIDI AdapterはMIDI Callbackの入力Sequenceを維持する
- Audio CallbackからPanic/例外を漏らさない
- Process中の新規Asset解析・Graph構築・Heap拡張を禁止
- Latencyを持つ構成はPrepare/Compile時に報告可能

**正規化Event**：
FrontendのAuthoring Eventは、生MIDI ByteやPlugin固有Eventではなく、Parameter CatalogのNative Unit（例：CutoffはHz、TuningはCents）で受け取ります。その後、Descriptorで検証してCoreの共有Event表現へ正規化します。
Note On/Off、Sustain Pedal、Pitch Bend、Mod Wheel、Aftertouch、Note単位Expression、Parameter Change、Macro Change、Transport/Tempo Context

各EventはAudio Block内のSample Offsetを持ち、Frontendに関係なく同じRuntimeで処理します。

**C ABI境界規則**：
- Rust内部型を直接公開せず、不透明Handleを使用
- Instanceの生成・破棄はSonalloy側が行う
- Input/Output BufferとEvent列は呼び出し側がProcess中だけ所有・貸与
- Rust PanicやC++例外を境界外へ伝播させない
- 結果Codeと診断情報で失敗を返す
- Optional Generator Backendの有無をCapabilityとして取得可能

---

## 6. システム構成と技術選定

### 6.1 依存方向とレイヤー構造

```
Frontend/Adapter（CLI/Standalone/Riffra/CLAP/VST3）
        │  Device・Host固有形式を変換
        ▼
Sonalloy Public Contract
（Definition/Compile/Lifecycle/Process/C ABI）
        │
        ├── Control側 ── Instrument Compiler
        │                 ├── Validation/Asset Resolution
        │                 ├── Parameter・Modulation Resolution
        │                 ├── Sample/Wavetable/Spectral/IR Preparation
        │                 └── Latency/Resource Budget計算
        │                              │
        │                              ▼
        │                      Compiled Instrument
        │                              │ Block境界で公開
        ▼                              ▼
Instrument Runtime Instance
（Event, Voice, Layer, Modulation, Processor）
        │
        ▼
Generator Engines
（Sample/Osc/Noise/WT/Operator/Granular/Additive/
 Spectral/Physical/Formant/Wave Sequence）
        │
        ▼
DSP Core
（Oscillator, Filter, FFT, Resampler, Envelope,
 Delay, Convolution, Waveguide, Dynamics）
```

**禁止事項**：Sonalloy CoreがCLI・Riffra・JUCE・CPAL・midir・CLAP・VST3を知ることは禁止。

**Control側とAudio側の責務分離**：

| 側 | 責務 |
|----|------|
| **Control側** | Definition編集、Validation、Asset解決、Decode、Resample、FFT解析、IR Partition、Compile、Resource Budget確認 |
| **Audio側** | Event適用、Voice生成・終了、Modulation評価、音声生成、事前準備済み構成への切替 |

Compiled Instrumentの公開はAudio Block境界で行います。Audio側でJSON解析・Asset解決・FFT事前解析・構成構築を行いません。発音中Voiceが参照する旧構成は、そのVoiceの終了まで破棄しません。

### 6.2 技術候補

| 領域 | 選定 | 備考 |
|------|------|------|
| 言語 | Rust | 独立Workspace |
| Serialization | Serde/JSON | Definition正本 |
| Native DSP | DaisySP等 | Basic DSP Primitive。製品ModelはSonalloy側が所有 |
| Realtime Audio | CPAL | Standalone Adapter |
| Audio Decode | Symphonia | WAV/FLAC/OGG等 |
| Resampling | Rubato | Sample Rate変換 |
| FFT | RustFFT | Spectral/Convolution/Analysis |
| Time Stretch | 専用Engineまたは評価済みLibrary | Resamplingとは分離 |
| Realtime MIDI | midir | Standalone Adapter |
| MIDI File | midly | MIDI Fileを共通Eventへ変換 |
| CLAP | clack ecosystem | Plugin Adapter |
| Riffra統合 | C ABI/FFI | 安定境界 |
| Neural Optional | Backendを選択可能 | Core必須依存にしない |

具体Libraryは品質、License、Realtime Safety、Cross-platform性を評価して決定します。CONCEPT段階ではLibrary名より、責務と境界を優先します。

Neural OptionalにはDDSP、Timbre Transfer、Neural Codec Generator、Latent Morph、Model-based Resynthesisを含めます。標準Engineの完成条件には含めず、導入時はOffline専用とRealtime対応を区別し、Model Hash、Backend/Device、Latency、CPU/GPU要件を診断可能にします。

### 6.3 所有と利用の境界

| 区分 | 対象 |
|------|------|
| **Sonalloyが所有** | Definition、Layer、Generator Model、Compiler、Compiled Instrument、Voice、Modulation、Runtime、Sample Mapping、Processor Chain、共通Event/Process Contract |
| **Frontend/Adapterが所有** | Audio/MIDI Device、Plugin Host API、Input Bus、JUCE/CPAL/CLAP/VST3固有変換 |
| **既存Libraryに委譲** | FFT、Resampling、Codec、Device接続、MIDI接続等の汎用基盤 |
| **Optional Backendが所有** | Neural推論Runtime、GPU固有処理等。SonalloyからCapability経由で利用 |

---

## 7. Riffraとの関係

**原則：Codebaseは分離、製品体験は深く統合。**

```
Riffra ──利用──▶ Sonalloy
（依存は Riffra → Sonalloy の一方向）
```

SonalloyはRiffraがなくても単独で主要機能が動作します。

| Riffra画面 | Sonalloyとの関わり |
|------------|-------------------|
| **Design** | Layer、Generator、Sample Mapping、Wavetable、FM Operator、Granular、Modulation、Processor、Macroを視覚編集。Definitionの意味を再実装しない |
| **Play** | MIDI鍵盤等からRealtime演奏。Performance Settings、Macro、Expression、External Audio InputをProcess Contractへ変換 |
| **Arrange** | 楽曲内でInstrumentを利用。Automation、Tempo、Transport、Audio Input RoutingをSonalloy Contractへ渡す |

**所有関係**：RiffraはAudio Device、JUCE Audio Callback、Host Input/Output Routingを所有します。Sonalloyは渡されたContext・Event・Input/Output Bufferを処理します。Sonalloy側からRiffraやJUCE固有APIを呼びません。

**Design画面での構成変更Flow**：
1. RiffraがControl側でCompileを要求
2. 成功 → Compiled InstrumentをAudio Block境界で公開
3. 失敗 → 現在のInstrumentを維持し、診断を表示
4. Asset不足 → 不足Zone/Layer/Processorを明示し、他部分は利用可能
5. Resource Budget超過 → 原因となるVoice/Grain/Partial/Unison/Effectを表示
6. Latency変更 → Audio Engine側へ通知し、安全なタイミングで反映

---

## 8. 非機能要件

| 要件 | 内容 |
|------|------|
| **Cross-platform** | Linux/Windowsを主要対象。CoreはOS固有機能へ直接依存しない |
| **Headless** | GUIなしで全主要機能を利用可能 |
| **Realtime Safety** | Audio Callbackでは事前準備済み構成のみ使用。File I/O、Decode、FFT事前解析、大規模alloc、JSON、Blocking Lock、Network、Device操作は禁止 |
| **計算量の予測可能性** | Voice/Unison/Grain/Partial/Spectral/Delay/Convolution等に上限を設け、Compile時に検証 |
| **演奏継続性** | Parameter変更や構成変更でAudio処理を停止しない。Compile失敗時は現在構成を維持 |
| **部分的な読込** | Asset/Optional Backend不足でも、依存部分だけ無効化して他Layerを利用可能 |
| **再現性** | 同一Definition + Event + Context + Asset + Seed + Backend条件から同等のOffline Renderを生成 |
| **Sample Rate非依存** | 固定Sample Rateを前提としない |
| **Block Size独立** | 合理的な許容誤差内でBlock Sizeに依存しない |
| **Latencyの明示** | Spectral、Convolution、Lookahead Dynamics等のLatencyをFrontendへ報告 |
| **Frontend非依存** | Host固有形式はAdapterで共通Contractへ変換 |
| **境界安全性** | Rust API/C ABIからPanicや例外を漏らさない |
| **決定的Random** | Random、Round Robin、Grain配置等は明示Seedと安定IDから再現可能 |
| **品質検証可能性** | Generator/Processorごとに自動計測と人間の試聴手順を持つ |
| **後方互換性より明確性** | 未成熟段階では誤った抽象化を固定せず、Definitionの意味を明確に保つ |

---

## 9. 完成像

Sonalloyを一文で言えば：

> **任意の音を取り込み、主要な電子音生成方式でゼロから音を作り、それらをLayerとして融合して、一つの演奏可能なInstrumentとして保存・再利用できるハイブリッド音源エンジン。**

ユーザーは同じInstrument Definitionを、CLI / Riffra / CLAP Host / VST3 Host / その他Applicationから利用できます。Definitionは事前にCompiled Instrumentへ変換され、各Frontendは共通Event・Process Contractを通じて同じInstrument Runtimeを使用します。

Sonalloyは特定のDAW・GUI・Plugin規格・Device APIに縛られず、独立して動作します。

### 電子音の網羅方針

Sonalloyは、無限に自由なDSP環境を目指しません。代わりに、音楽制作で重要な方式を専用Generator/Processorとして順次網羅します。

```
Sound Material
    ├── Sample/Loop/Slice/IR
    └── Audio Input
        ▼
Generation
    ├── Basic/Complex Oscillator
    ├── Noise/Wavetable
    ├── FM/PM/AM/Ring Mod
    ├── Granular/Additive/Spectral
    ├── Physical/Modal/Waveguide
    ├── Formant/Wave Sequence
    └── Optional Neural
        ▼
Layering/Hybridization
        ▼
Modulation
    ├── LFO/Envelope/MSEG/Step
    ├── Random/S&H/Macro/Vector
    └── Performance/Tempo/Input Analysis
        ▼
Processing
    ├── Filter/Nonlinear/Resonator
    ├── Digital/Frequency/Modulation FX
    ├── Delay/Reverb/Convolution
    └── Dynamics/Cross Synthesis
        ▼
Instrument Definition
        ▼
Compile/Runtime
        ▼
Performance
```

### 製品の中心軸

```
Sound（素材・波形・入力）
    → Generation/Sampling（音の発生）
        → Layering/Hybridization（構成・融合）
            → Modulation（時間変化）
                → Processing（音色変形・空間・質感）
                    → Instrument Definition（保存可能な楽器定義）
                        → Compile/Runtime（実行可能な楽器）
                            → Performance（演奏）
```

この一連の流れを、一つの予測可能で再現可能なEngineとして完結させることがSonalloyの存在意義です。

自由Graphを採用せずとも、主要な電子音方式を専用Engineとして広く揃えることで、Sonalloyは**生楽器の完全再現を除く、一般的な電子音楽で必要とされる音色をほぼすべて構築可能なInstrument Engine**を目指します。
