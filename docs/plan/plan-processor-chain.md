# Sonalloy Processor Chain / Core Effects 詳細設計・実装計画

- **対象Repository**：`endo-ly/sonalloy`
- **正本要件**：`docs/CONCEPT.md`
- **前提実装**：`docs/CONCEPT.md`で定義されたInstrument Definition、Compile、Parameter、Runtime、CLI
- **用途**：実装エージェントへ渡す詳細設計・実装計画
- **文書言語**：日本語。型名、API名、Parameter ID、File Pathのみ英語を使用する
- **成果物**：Markdownのみ。HTML版は作成しない

---

## 0. この計画書の位置づけ

本書は、`docs/CONCEPT.md`で定義された半固定パイプラインのLayer Processing、Voice Processing、Global Effectsについて、Definition、Compile、Parameter、Runtime、CLI、検証の契約を定義する。

InstrumentはOscillatorまたはSampleをLayerとして同じVoiceへ混合し、Gain、Pan、Tuning、Processor ParameterをDynamic Parameter / Modulationから変更できる。ProcessorはLayer、Voice、Globalの固定位置へ配列順に配置する。

機能範囲は次のとおりとする。

- Layer / Voice / GlobalへDefinition順のProcessor Chainを配置する
- Filterを共通ProcessorとしてLayer、Voice、Globalへ配置する
- Filter、Drive、Delay、Reverbを最初のProcessorとして提供する
- Processor Parameterを既存Parameter Catalog、Parameter Change、Modulation Routeへ統合する
- Layer、Voice、Globalで異なるRuntime Stateの所有単位を固定する
- Global Delay / ReverbのTailをVoice Lifecycleから独立させる
- 既存Reference InstrumentとReview Packageを新構造へ移行する
- Filter、Drive、Delay、Reverbの実装方式を固定する

自由なAudio Graphを導入しない。ProcessorはDefinitionへ記載された順序で直列適用し、任意分岐、Send / Return、ユーザー定義Feedbackは扱わない。

### 0.1 機能名称

恒久的な機能名は次を使用する。

- `Processor`
- `Processor Chain`
- `Layer Processor`
- `Voice Processor`
- `Global Processor`

### 0.2 実装判断の優先順位

判断に迷った場合は次の順序で優先する。

1. `docs/CONCEPT.md`
2. 本書で固定する信号順序、所有関係、配置制約
3. 現在のProcess ContractとDynamic Parameter契約
4. 音質と人間による試聴結果
5. Realtime Safetyと決定性
6. 実装の単純さ
7. 将来のProcessor追加

「将来使う可能性」だけを理由に、自由Graph、汎用Node Framework、Trait Objectによる動的登録、外部DSP Script、追加Crateを導入しない。

一方で、音質処理を安易に独自実装することも避ける。既存Dependency、同Dependency内の追加Module、別Dependency、独自実装を機能ごとに比較し、現在のLifecycle、License、音質、保守性へ最も適合する方式を選ぶ。

### 0.3 本書で固定するもの

- 実装済み機能と依存物の対応
- 対象Processorの実装方式
- Layer / Voice / Globalの信号順序
- Processor DefinitionとParameter ID
- Processorの配置制約
- Definition / Compiled Instrument / Runtimeの責務
- Filter Processorの共通Chain化
- Filter、Drive、Delay、Reverbの処理契約
- Processor ParameterとModulationのScope
- Prepare / Process / Reset / Error時の挙動
- CLI、Definition Fixture、Reference Instrumentの更新
- Unit Test、Integration Test、Sound Review
- 実装順序と完了条件

### 0.4 本書で固定しないもの

- 自由なAudio Graph
- Processor Chainの並列分岐
- Send / Return
- Sidechain
- ユーザー定義Feedback
- Layer / Voice単位のDelayまたはReverb
- High-pass、Band-pass、Notch、Ladder Filter
- EQ、Comb、Resonator、Formant Processor
- Bitcrusher、Sample-rate Reducer
- Frequency Shifter
- Chorus、Flanger、Phaser
- Convolution
- Compressor、Limiter、Gate、Transient Shaper
- Vocoder
- Delay Time Modulation
- Tempo Sync Delay
- Ping-pong、Multi-tap Delay
- Reverb Freeze
- Reverb Sizeの演奏中変更
- Processor Bypass Automation
- Processor追加・削除・順序変更のRuntime反映
- Instrument Scope LFO
- Realtime Audio Device
- Realtime MIDI Device
- Riffra統合
- Public C ABI
- CLAP、VST3
- GUI
- Preset Migration
- Deprecated Field
- Schema互換読込

---

# 1. DSP実装方針

Processor ChainはSonalloy独自のDefinition、Compile、Parameter、Runtime契約として実装する。DSP処理本体は機能ごとに既存依存の再利用と独自実装を選ぶ。

## 1.1 結論

| 機能 | 実装方式 | 判断 |
|---|---|---|
| Filter | 既存DaisySP `Svf`を継続利用 | 現在のNative Wrapper、Ramp、Reset、Error処理、Fault Injectionをそのまま共通Processorから利用できる |
| Drive | Rust独自実装 | 必要なのはAmount、Mix、Rampを持つ小さなNonlinear処理であり、Native APIを増やす利点がない |
| Delay | Rust独自実装 | Sample RateとDefinitionからPrepare時にBuffer長を決め、PositionとMemoryをRuntimeが直接所有する方が自然 |
| Reverb | Rust独自実装 | Dattorro型Plate ReverbをSonalloyのSample Rate、Reset、Parameter、Memory契約に合わせて実装する |
| 新しい外部Dependency | 追加しない | 候補LibraryはいずれもReset、Sample Rate、固定Buffer、Graph抽象、Licenseのいずれかが現在の責務と合わない |
| DaisySP Build対象 | 増やさない | 現在の`oscillator.cpp`と`svf.cpp`だけを維持する |

## 1.2 判断の境界

- DaisySPの`Overdrive`は利用可能だが、Sonalloyで必要な0でIdentityとなるAmount、Dry / Wet Mix、Block Rampを追加するとWrapper側の責務が処理本体より大きくなるため採用しない。
- DaisySPの`DelayLine`は最大長をTemplateで固定するため、Prepare時に実行条件からMemoryを確定する現在のRuntimeへ合わせにくい。
- DaisySP-LGPLの`ReverbSc`はLGPL-2.1であり、現在のMIT部分だけの利用より配布条件を複雑にするため採用しない。
- `freeverb`、`lanceverb`等の既存Rust実装は参考候補にはなるが、公開Reset、Sample Rate、Buffer所有の契約をそのまま満たさないため直接依存しない。
- FunDSP等の包括的DSP Frameworkは、Sonalloy自身のProcessor Chain、Parameter、Lifecycleと責務が重複するため導入しない。

この結論を、将来の全DSP機能へ一般化しない。Convolution、Pitch Shift、Time Stretch、FFT系などは、それぞれの計画で依存を再評価する。

## 1.3 独自実装の範囲

独自実装は次へ限定する。

- Drive：正規化Soft ClipとDry / Wet Mix
- Delay：固定時間のStereo Feedback Delay
- Reverb：Dattorro型Stereo Plate Reverb
- Processor Chain：Definition順の直列実行、State所有、Parameter接続

Reverbは公開されたAlgorithmの構造を参考に新規実装し、候補LibraryのSource Codeをコピーしない。完成判定は自動Testだけでなく、Impulse、Phrase、複数Sample Rateの試聴を含める。

---

# 2. 機能と実装・依存一覧

## 2.1 直接Dependency一覧

| Dependency | Version / Pin | License | 用途 | 変更内容 |
|---|---|---|---|---|
| DaisySP | Commit `a0494a3adb67f549e18dfd71a35fa656f65b38b6` | MIT | Sine / PolyBLEP Saw、SVF Low-pass Filter | Build対象を増やさず、Filterを共通Processorへ移動 |
| `cmake` | 0.1.58 | MIT OR Apache-2.0 | DaisySP Native Build | 変更なし |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | Definition / Diagnostic / CLI JSON Model | Processor Definitionを追加 |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 | JSON Parse / Output / Event Sequence | Processor JSONを追加 |
| `thiserror` | 2.0.19 | MIT OR Apache-2.0 | Core / DSP Error | Processor Errorを既存契約へ追加 |
| `sha2` | 0.11.0 | MIT OR Apache-2.0 | Asset SHA-256 | 変更なし |
| `symphonia` | 0.6.0 | MPL-2.0 | WAV Asset Decode | 変更なし |
| `rubato` | 4.0.0 | MIT OR Apache-2.0 | Sample Rate変換 | 変更なし |
| `clap` | 4.6.4 | MIT OR Apache-2.0 | CLI Argument Parse | 既存CommandのInspect表示を拡張 |
| `midly` | 0.5.3 | MIT | Standard MIDI File Parse | 変更なし |
| `hound` | 3.5.1 | Apache-2.0 | Stereo WAV Encode | 変更なし |
| `approx` | 0.5.1 | MIT OR Apache-2.0 | Float Unit Test | Processor Testで利用 |
| `assert_cmd` | 2.2.2 | MIT OR Apache-2.0 | CLI Integration Test | Processor CLI Testで利用 |
| `predicates` | 3.1.4 | MIT OR Apache-2.0 | CLI Output Test | Processor Inspect Testで利用 |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 | Test用一時Directory | 変更なし |

## 2.2 全機能一覧

「独自実装」は、外部Libraryへ処理本体を委譲せずSonalloyのRustまたはC++ Wrapper内で実装していることを示す。

| 状態 | 領域 | 機能 | 実装・依存 | 責務境界 |
|---|---|---|---|---|
| 実装済み | Build | Rust Workspace | Cargo / Rust標準Toolchain | 三Crate構成と依存方向を管理 |
| 実装済み | Build | Native Build | `cmake` + CMake + C++ Toolchain | DaisySPの選択SourceだけをStatic Link |
| 実装済み | Native境界 | Opaque Handle / Result Code | **独自実装** | C++ Object Layoutと例外をRustへ漏らさない |
| 実装済み | Native境界 | Buffer検証 / 無音化 / Fault Injection | **独自実装** | Native失敗を既存`DspFailure`へ変換 |
| 実装済み | Definition | JSON Serialize / Deserialize | `serde` / `serde_json` | 保存形式のParseのみ依存 |
| 実装済み | Definition | Field / Range / ID Validation | **独自実装** | Sonalloyの意味と制約を検証 |
| 実装済み | Definition | Unknown Field拒否 | `serde` + **独自Model** | 誤記をDefaultへFallbackしない |
| 実装済み | Model | Definition / Compiled / Runtime分離 | **独自実装** | 保存、準備済み構成、演奏状態を分離 |
| 実装済み | Compiler | Asset / Parameter / Route解決 | **独自実装** | Process前に文字列とFile参照を解決 |
| 実装済み | Diagnostics | 構造化Error / Warning | `thiserror` / `serde` + **独自実装** | Frontend非依存のCode、Path、Message |
| 実装済み | Process | `ProcessSpec` / `ProcessBlock` | **独自実装** | Stereo Planar `f32`、可変Block |
| 実装済み | Process | Event Validation / 同一Offset優先順位 | **独自実装** | Note OffからNote Onまでの順序を固定 |
| 実装済み | Process | Sample Accurate Event適用 | **独自実装** | Event位置でBlockを分割して処理 |
| 実装済み | Process | Absolute Frame継続性 | **独自実装** | Context不連続をErrorにする |
| 実装済み | Render | Offline Render Loop | **独自実装** | Core ProcessをBlock単位で繰り返す |
| 実装済み | Render | Stereo WAV Encode | `hound` | CoreのPlanar AudioをFileへ書き出す |
| 実装済み | Asset | Path解決 / File Read | Rust標準Library + **独自実装** | Compile時だけ実行 |
| 実装済み | Asset | SHA-256照合 | `sha2` | Asset内容の一致確認 |
| 実装済み | Asset | WAV Probe / Decode | `symphonia` | PCM WAVを`f32`へDecode |
| 実装済み | Asset | StereoからMonoへのDownmix | **独自実装** | Sample Layer内部形式へ変換 |
| 実装済み | Asset | Sample Rate変換 | `rubato` | Compile時にEngine Sample Rateへ変換 |
| 実装済み | Asset | Prepared Sample共有 | Rust `Arc` + **独自実装** | Decode済みMono DataをVoice間共有 |
| 実装済み | Generator | Sine Oscillator | DaisySP `Oscillator` | Native DSP Primitive |
| 実装済み | Generator | Band-limited Saw | DaisySP PolyBLEP Saw | Native DSP Primitive |
| 実装済み | Generator | Oscillator Phase Reset | DaisySP + **独自Lifecycle** | Note開始とResetをSonalloyが制御 |
| 実装済み | Generator | One-shot Sample Playback | **独自実装** | VoiceごとのCursorを所有 |
| 実装済み | Generator | Sample Pitch Mapping | **独自実装** | Note / Root Note / TuningからRatio計算 |
| 実装済み | Generator | 4-point Cubic Interpolation | **独自実装** | Sample Cursorの非整数位置を補間 |
| 実装済み | Generator | Sample終端Fade | **独自実装** | One-shot末尾のClickを抑制 |
| 実装済み | Layer | Key / Velocity Trigger | **独自実装** | Note On時にLayer発音可否を判定 |
| 実装済み | Layer | ADSR | **独自実装** | Sample Rate依存FrameへCompile |
| 実装済み | Layer | Gain | **独自実装** | dB Domain評価後Linear変換 |
| 実装済み | Layer | Constant-power Pan | **独自実装** | Mono LayerをStereoへ配置 |
| 実装済み | Layer | Tuning | **独自実装** | centからOscillator / Sampleへ反映 |
| 実装済み | Layer | Layer Mix | **独自実装** | 同一Voice内の複数Layerを合流 |
| 実装済み | Voice | Polyphony | **独自実装** | DefinitionのVoice数だけPrepare |
| 実装済み | Voice | Voice Allocation | **独自実装** | Idle、Releasing、Oldestの順に選択 |
| 実装済み | Voice | Voice Stealing | **独自実装** | 再利用対象Voiceを短いFade後に再利用 |
| 実装済み | Voice | Note ID追跡 | **独自実装** | Note Offを正しいVoiceへ対応 |
| 実装済み | Voice | Voice Peak推定 | **独自実装** | Stealing選択専用の簡易値 |
| 実装済み | Voice | Stereo Voice Sum | **独自実装** | 全VoiceをInstrument Outputへ加算 |
| 実装済み | Filter | Low-pass Filter | DaisySP `Svf` + **独自Wrapper** | Layer / Voice / Globalの共通Processorとして使用 |
| 実装済み | Parameter | Stable Parameter ID | **独自実装** | FrontendとDefinitionで共通の識別子 |
| 実装済み | Parameter | Dense Parameter Handle | **独自実装** | Process中の文字列検索を排除 |
| 実装済み | Parameter | Descriptor / Unit / Range / Scale | **独自実装** | Normalize / Denormalize契約 |
| 実装済み | Parameter | Base Parameter Change Event | **独自実装** | Sample AccurateにTarget更新 |
| 実装済み | Parameter | Smoothing | **独自実装** | Parameterごとの時間でRamp |
| 実装済み | Parameter | 32 Frame Control Quantum | **独自実装** | Block Sizeと独立したControl時間軸 |
| 実装済み | Modulation | Route Compile / Target別Range | **独自実装** | Source / TargetをHandleへ解決 |
| 実装済み | Modulation | Route Amount / Curve / Clamp | **独自実装** | Definition順で加算 |
| 実装済み | Modulation | Velocity | **独自実装** | Voice Scope Source |
| 実装済み | Modulation | Key Tracking | **独自実装** | Voice Scope Source |
| 実装済み | Modulation | LFO Sine / Triangle | **独自実装** | Voice Scope、決定的Phase |
| 実装済み | Modulation | Modulation Envelope | **独自ADSR再利用** | Note Lifecycleと連動 |
| 実装済み | Modulation | Note単位Random | **独自実装** | Seed、Note ID、Source IDから決定 |
| 実装済み | Modulation | Pitch Bend | **独自Runtime** | Instrument共有External Control |
| 実装済み | Modulation | Mod Wheel | **独自Runtime** | Instrument共有External Control |
| 実装済み | Modulation | Channel Aftertouch | **独自Runtime** | Instrument共有External Control |
| 実装済み | Dynamic DSP | Oscillator Frequency Ramp | DaisySP + **独自Native Wrapper** | Span内で周波数を更新 |
| 実装済み | Dynamic DSP | Filter Cutoff / Resonance Ramp | DaisySP + **独自Native Wrapper** | Span内でFilter値を更新 |
| 実装済み | Dynamic DSP | Sample Playback Ratio Ramp | **独自実装** | Log Domainで音程変化 |
| 実装済み | Dynamic DSP | Gain / Pan Ramp | **独自実装** | Span内を補間 |
| 実装済み | MIDI | Standard MIDI File Parse | `midly` | Byte列からMIDI Eventを取得 |
| 実装済み | MIDI | TempoからAbsolute Frame変換 | `midly` + **独自実装** | Tempo MapをFrameへ変換 |
| 実装済み | MIDI | MIDI Note ID生成 | **独自実装** | Channel / Note / Serialから生成 |
| 実装済み | MIDI | Pitch Bend / CC1 / Aftertouch変換 | `midly` + **独自正規化** | Core Eventへ変換 |
| 実装済み | CLI | Command / Option Parse | `clap` | CLI表層だけを担当 |
| 実装済み | CLI | Init / Validate / Inspect | `clap` / `serde_json` + **独自Core API** | Definitionの作成・理解 |
| 実装済み | CLI | Note / Events / MIDI Render | `clap` / `serde_json` / `midly` / `hound` + **独自Core API** | 同じRendererへ入力を変換 |
| 実装済み | Test | Float近似比較 | `approx` | Unit / Integration Test補助 |
| 実装済み | Test | CLI Process Test | `assert_cmd` / `predicates` / `tempfile` | CLI公開経路の検証 |
| 実装済み | Review | WAV Metrics / Package生成 | Python標準Library中心の**独自Script** | 機械検査と試聴素材を生成 |
| 実装済み | Processor基盤 | Definition順のProcessor Chain | **独自実装** | 自由Graphではなく固定位置の直列処理 |
| 実装済み | Processor基盤 | Layer Processor Runtime | **独自実装** | Voice × LayerごとにStateを所有 |
| 実装済み | Processor基盤 | Voice Processor Runtime | **独自実装** | VoiceごとにStateを所有 |
| 実装済み | Processor基盤 | Global Processor Runtime | **独自実装** | Instrument Runtimeに一組だけ所有 |
| 実装済み | Processor | Filter | 既存DaisySP `Svf` + **独自Processor Wrapper** | Layer / Voice / Globalへ再配置 |
| 実装済み | Processor | Drive | **Rust独自実装** | 0でIdentity、Amount / Mix Ramp |
| 実装済み | Processor | Stereo Delay | **Rust独自実装** | Global限定Ring Buffer |
| 実装済み | Processor | Plate Reverb | **Rust独自実装** | Dattorro Figure-8 Topology、Global限定 |
| 実装済み | Processor | Global Tail | **独自実装** | Voiceが0でもDelay / Reverbを処理 |
| 実装済み | Parameter | Processor Parameter Catalog | 既存の**独自Parameter基盤を拡張** | 新しいAutomation機構を作らない |
| 実装済み | Modulation | Processor Route | 既存の**独自Modulation基盤を拡張** | PlacementごとにSource Scopeを検証 |
| 実装済み | CLI | Processor Inspect / Render | 既存CLI Dependency + **独自表示** | Chain順、Parameter、Routeを表示 |
| 実装済み | Review | Processor Chain Review Package | **独自Script / Test** | Baseline、個別Effect、Full Instrumentを比較 |

---

# 3. 目的と完成像

## 3.1 成立させること

音を発生させた後の加工を共通ProcessorとしてDefinitionへ保存し、Layer、Voice、Globalの正しい位置へ配置できるようにする。

```text
Instrument Definition
    │
    ├─ layers[].processors[]
    ├─ voice_processors[]
    ├─ global_processors[]
    └─ modulation.routes[]
    │
    ▼
Compile
    │
    ├─ Processor ID / Placement / Parameterを検証
    ├─ Processor ParameterをCatalogへ登録
    ├─ RouteをHandleへ解決
    ├─ Sample Rate依存値を計算
    └─ Definition順のCompiled Chainを生成
    │
    ▼
Prepare
    │
    ├─ Voice数分のLayer / Voice Processor State
    ├─ 一組のGlobal Processor State
    ├─ Filter Handle
    ├─ Delay / Reverb Buffer
    └─ Target Scratch
    │
    ▼
Process
    │
    ├─ Generator
    ├─ Layer Processor
    ├─ Layer Envelope / Gain / Pan
    ├─ Layer Mix
    ├─ Voice Processor
    ├─ Voice Sum
    └─ Global Processor
    │
    ▼
Stereo Output
```

## 3.2 完成状態

Processor Chainの完成状態は次である。

> Instrument Definitionに保存したFilter、Drive、Delay、ReverbをCompileし、Layer、Voice、Globalの固定位置へDefinition順に配置し、既存Dynamic ParameterとModulationを滑らかに反映しながら、決定的かつBlock Size非依存なStereo WAVとしてOffline Renderできる。

## 3.3 この実装で証明する設計上の価値

- Filterだけが専用のDefinition / Runtime経路を持たない
- Processor追加時にVoice Engine全体の信号順序を書き直さない
- Layer Processor Stateが別Layerまたは別Voiceへ漏れない
- Voice Processor Stateが別Voiceへ漏れない
- Global Processor StateがVoice数分複製されない
- Global TailがNote / Voice終了後も継続する
- Processor Parameterが既存のParameter ChangeとModulation契約を利用する
- Audio PathでJSON、文字列検索、HashMap検索、容量拡張を行わない
- Effect追加を理由に自由Graphへ拡張しない
- DSP Backendの型とParameterをDefinitionへ露出させない

## 3.4 Reference Instrument

Reference Instrumentとして`Processed Hybrid`を使用する。

```text
Attack Layer
  Sample Generator
    → Layer Filter
      → Layer Envelope
        → Layer Gain / Pan

Body Layer
  Saw Oscillator
    → Layer Drive
      → Layer Envelope
        → Layer Gain / Pan

Layer Mix
  → Filter Processor
    → Voice Drive

Voice Sum
  → Global Delay
    → Global Reverb
```

確認する内容：

- Layer FilterがAttack Layerだけへ作用する
- Layer DriveがBody Layerだけへ作用する
- Voice Processorが同じNoteのLayer Mix全体へ作用する
- Global Delay / Reverbが全Voice合流後に一度だけ作用する
- Chain順序を変えると意図どおり結果が変わる
- Mod WheelまたはParameter ChangeでProcessor Parameterを変更できる
- Note終了後もDelay / Reverb Tailが残る
- Reset後に同じ入力から同等の出力を得られる
- 曲で使用可能か人間が判断できる品質へ調整する

---

# 4. 対象範囲

## 4.1 含める機能

### Processor共通基盤

- Ordered Processor Chain
- Processor ID
- Layer / Voice / Global Placement
- Definition Validation
- Compiled Processor
- Processor Parameter Handle
- Processor Target Span
- Runtime Processor State
- Prepare / Process / Reset
- Definition順の保持
- Process中のAllocation禁止
- Error時の既存Runtime無効化契約

### Placement

- Layer Processor Chain
- Voice Processor Chain
- Global Processor Chain

### Initial Processor

- Low-pass Filter
- Drive
- Stereo Delay
- Stereo Plate Reverb

### Parameter / Modulation

- Processor ParameterのStable ID
- Parameter Change
- Parameter Smoothing
- Layer / Voice ProcessorへVoice Scope Sourceを接続
- Layer / Voice / Global ProcessorへExternal Controlを接続
- Route Amount / Curve / Clamp
- Global TargetへのSource Scope Validation

### CLI / Definition

- JSON DefinitionでProcessor Chainを保存
- `instrument init`
- `instrument validate`
- `instrument inspect`
- `render note`
- `render events`
- `render midi`

### Testing / Review

- Unit Test
- Core Integration Test
- CLI Integration Test
- Native Filter Test
- Block Size独立性
- Reset再現性
- Processor順序
- State Scope
- Tail処理
- Finite / Peak / DC / Discontinuity / Tail Metrics
- 人間による試聴

## 4.2 対象外

次は実装しない。

- Processorの並列接続
- Send / Return
- Sidechain
- Layer / Voice Delay
- Layer / Voice Reverb
- Delay Time Automation
- Fractional Time変更
- Tempo Sync
- Ping-pong Delay
- Multi-tap Delay
- Reverb Size Parameter
- Reverb Freeze
- Reverb IR
- Filter Mode選択
- Oversampling
- Latency補償
- Dynamic Bypass
- Dynamic Processor順序
- Processor Hot Swap
- Instrument Scope LFO
- 新しいCLI編集Command
- 新しいCrate
- 新しい外部DSP Dependency

## 4.3 品質要件

1. ProcessorなしのInstrumentが既存と同等に鳴る
2. Filterを共通Processorとして構成しても明確な音質回帰がない
3. Processor順序がDefinition順と一致する
4. Layer / Voice / Global Stateの所有単位が正しい
5. Parameter変更で明確なClickを出さない
6. すべてのProcessorが有限出力を維持する
7. Delay Feedbackが発散しない
8. Reverb Tailが不自然に途切れない
9. ResetでDelay / Reverb Tailが完全に消える
10. Block SizeでEvent位置、Parameter時間軸、Tail時間軸が変わらない
11. Process中にFile I/O、JSON、Allocation、Blocking Lockを行わない
12. Reference Instrumentが技術Demoではなく音色として成立する

---

# 5. 信号経路

## 5.1 全体

```text
Note / Control Event
        ↓
Voice Allocation
        ↓
Per Voice
  ├─ Layer Trigger
  ├─ Generator
  ├─ Layer Processor Chain
  ├─ Amplitude Envelope
  ├─ Layer Gain / Pan
  ├─ Layer Mix
  └─ Voice Processor Chain
        ↓
Voice Sum
        ↓
Global Processor Chain
        ↓
Stereo Output
```

## 5.2 Layer処理順序

```text
Generator
    ↓
Layer Processor Chain
    ↓
Amplitude ADSR / Note Start Fade
    ↓
Dynamic Gain
    ↓
Dynamic Pan
    ↓
Voice Mix
```

Layer ProcessorをAmplitude Envelopeより前へ置く。

理由：

- Filter / Driveは発音源そのものを加工する責務である
- Amplitude EnvelopeをLayerの最終Gateとして維持できる
- Layer終了時にProcessorの微小Stateが音として漏れ続けない
- Drive量がAmplitude Envelopeの音量によって意図せず変動することを避ける
- Processorなしの場合は現在の信号結果を変えない

Layerへ配置できるProcessorはFilterとDriveだけである。

Layer Processor Stateは`Voice × Layer × Processor`ごとに独立する。

## 5.3 Voice処理順序

```text
Layer Mix Stereo
    ↓
Voice Processor Chain
    ↓
Voice Steal Fade
    ↓
Voice Output
```

Voiceへ配置できるProcessorはFilterとDriveだけである。

Voice Processor StateはVoiceごとに独立する。VoiceがIdleへ戻るとき、Filter StateをResetする。

Voice Steal FadeはVoice Processorの後へ適用し、Processor出力を含む古いVoice全体を確実にFadeする。

## 5.4 Global処理順序

```text
Voice Sum Stereo
    ↓
Global Processor Chain
    ↓
Instrument Output
```

Globalへ配置できるProcessorはFilter、Drive、Delay、Reverbである。

Global Processor Stateは`InstrumentRuntime`に一組だけ存在する。

Active Voiceが0件でもGlobal Chainを実行する。入力が無音でもDelay / Reverb内部StateからTailを出力するためである。

## 5.5 Chain順序

Definitionの配列順をAudio処理順として固定する。

```text
processors: [A, B, C]
```

は必ず次を意味する。

```text
input → A → B → C → output
```

CompilerまたはRuntimeがProcessor Typeごとに並べ替えてはならない。

同じTypeを複数配置できる。Processor IDが異なれば二段Filter、二段Drive等を許可する。

## 5.6 Channel形式

| Placement | Buffer | State |
|---|---|---|
| Layer | Mono | Voice × Layer × Processor |
| Voice | Stereo Planar | Voice × Processor |
| Global | Stereo Planar | Instrument × Processor |

FilterはMonoでは一つ、StereoではLeft / Right独立のDaisySP Handleを持つ。

DriveはChannel間のMemory Stateを持たず、同じParameter Spanを各Channelへ適用する。

Delayは左右独立のRing Bufferを持つ。

ReverbはStereo InputのMid成分をTankへ入力し、Stereo Wet Outputを生成する。Dry Signalは元のLeft / Rightを維持する。

---

# 6. Instrument Definition

## 6.1 Definition構造

```text
InstrumentDefinition
├─ metadata
├─ performance
├─ layers[]
│  ├─ trigger
│  ├─ gain / pan / tuning
│  ├─ envelope
│  ├─ generator
│  └─ processors[]
├─ voice_processors[]
├─ global_processors[]
└─ modulation
```

`processors`、`voice_processors`、`global_processors`は省略時に空配列として扱う。専用Filter FieldはDefinitionのSchemaに含めず、未知Fieldとして拒否する。

次を追加しない。

- 専用Filter FieldのAlias
- Deprecated Field
- Migration Layer
- 保存形式の分岐
- `schema_version = 2`

## 6.2 Processor Definition

概念Model：

```rust
pub enum ProcessorDefinition {
    Filter(FilterProcessorDefinition),
    Drive(DriveProcessorDefinition),
    Delay(DelayProcessorDefinition),
    Reverb(ReverbProcessorDefinition),
}
```

JSONは`type`で識別するTagged Objectとする。

```json
{
  "type": "filter",
  "id": "tone",
  "cutoff_hz": 8000.0,
  "resonance": 0.15
}
```

全Processorは安定した`id`を持つ。

ID規則：

- 1〜64文字
- 小文字で開始
- 小文字、数字、`_`のみ
- `.`は禁止

## 6.3 Chain Field

Layer：

```json
{
  "id": "body",
  "processors": [
    {
      "type": "drive",
      "id": "body_drive",
      "amount": 0.55,
      "mix": 0.7
    }
  ]
}
```

Voice：

```json
{
  "voice_processors": [
    {
      "type": "filter",
      "id": "tone",
      "cutoff_hz": 12000.0,
      "resonance": 0.12
    }
  ]
}
```

Global：

```json
{
  "global_processors": [
    {
      "type": "delay",
      "id": "echo",
      "time_seconds": 0.25,
      "feedback": 0.3,
      "mix": 0.15
    },
    {
      "type": "reverb",
      "id": "space",
      "pre_delay_seconds": 0.012,
      "decay": 0.58,
      "damping": 0.35,
      "width": 1.0,
      "mix": 0.2
    }
  ]
}
```

`processors`、`voice_processors`、`global_processors`は省略時に空配列とする。

Processorを持たないInstrumentの通常表現である。

## 6.4 ID一意性

Processor IDは同じChain内で一意とする。

- 一つのLayerの`processors`
- `voice_processors`
- `global_processors`

別Layerでは同じProcessor IDを許可する。Parameter IDにLayer IDを含むため衝突しない。

Layer、Voice、Globalの異なるScopeでも同名を許可する。

## 6.5 Placement

| Processor | Layer | Voice | Global |
|---|---:|---:|---:|
| Filter | 可 | 可 | 可 |
| Drive | 可 | 可 | 可 |
| Delay | 不可 | 不可 | 可 |
| Reverb | 不可 | 不可 | 可 |

不正PlacementはDefinition Validation Errorとする。

暗黙にGlobalへ移動しない。Warningで無効化して継続しない。

## 6.6 Filter

```rust
pub struct FilterProcessorDefinition {
    pub id: String,
    pub cutoff_hz: f32,
    pub resonance: f32,
}
```

| Field | Range | Dynamic | Scale |
|---|---:|---:|---|
| `cutoff_hz` | 20〜20000 Hz | 可 | Log2 |
| `resonance` | 0〜1 | 可 | Linear |

- Low-pass固定
- DaisySP `Svf`を使用
- Safe上限は`min(20000, sample_rate × 0.45)`
- Definition値がSafe上限を超える場合は既存`FILTER_CUTOFF_CLAMPED` Warning
- Warning PathはProcessor位置まで含める

## 6.7 Drive

```rust
pub struct DriveProcessorDefinition {
    pub id: String,
    pub amount: f32,
    pub mix: f32,
}
```

| Field | Range | Dynamic | Scale |
|---|---:|---:|---|
| `amount` | 0〜1 | 可 | Linear |
| `mix` | 0〜1 | 可 | Linear |

要件：

- `amount = 0`でWet処理自体がIdentity
- `mix = 0`で完全Dry
- `mix = 1`で完全Wet
- Amount増加で連続的にSoft Clippingが強くなる
- Parameter Ramp中に不連続を出さない
- Mono / Stereoで同じ音響式
- 内部Memory Stateを持たない
- 有限入力から有限出力を生成

具体式はRuntime Module内に閉じ、次で固定する。

```text
shape = amount × 4
wet =
  amount == 0 の場合: input
  それ以外: tanh(shape × input) / tanh(shape)

output = input + (wet - input) × mix
```

この式により次を満たす。

- `amount = 0`で厳密にIdentity
- `amount → 0`でも線形処理へ連続的に収束
- `input = ±1`では`wet = ±1`
- 大入力は有限値へ滑らかに圧縮
- `amount`と`mix`をSpan内で線形補間可能

`4`はProcessor Chainの最大Drive強度を決める実装定数として一か所に置く。別のClip式や複数Modeを同時に実装しない。Sound Reviewでこの方式自体が不合格の場合は定数調整だけで済ませず、処理式を再評価して本書へ反映する。

## 6.8 Delay

```rust
pub struct DelayProcessorDefinition {
    pub id: String,
    pub time_seconds: f32,
    pub feedback: f32,
    pub mix: f32,
}
```

| Field | Range | Dynamic | Scale |
|---|---:|---:|---|
| `time_seconds` | 0.001〜2.0秒 | 不可 | Compile時固定 |
| `feedback` | 0〜0.95 | 可 | Linear |
| `mix` | 0〜1 | 可 | Linear |

- Global限定
- Stereo左右独立
- 同じDelay Time
- Cross Feedbackなし
- Ping-pongなし
- Integer Frame Delay
- `time_seconds`はCompile時にFrame数へ変換
- 演奏中変更は不可
- Feedbackを範囲外からClampせずValidation Error
- TailはVoice Lifecycleと独立

## 6.9 Reverb

```rust
pub struct ReverbProcessorDefinition {
    pub id: String,
    pub pre_delay_seconds: f32,
    pub decay: f32,
    pub damping: f32,
    pub width: f32,
    pub mix: f32,
}
```

| Field | Range | Dynamic | Scale |
|---|---:|---:|---|
| `pre_delay_seconds` | 0〜0.2秒 | 不可 | Compile時固定 |
| `decay` | 0〜0.98 | 可 | Linear |
| `damping` | 0〜1 | 可 | Linear |
| `width` | 0〜1 | 可 | Linear |
| `mix` | 0〜1 | 可 | Linear |

- Global限定
- Dattorro Figure-8 Plate Topology
- Sample Rateに合わせてDelay / Tap長を再計算
- Stereo Dry Inputを保持
- Wet TankへのExcitationは`0.5 × (left + right)`
- Stereo Wet Output Tap
- Input Diffusion、Tank Diffusion、Delay / Tap配置はDattorroのReference Topologyを初期値として固定
- 内部BandwidthとTank ModulationはTopology内部の固定値とし、実装時に数値・単位・Sample Rate換算を同じModuleへ記録する
- 内部値はDefinitionへ保存しない
- `pre_delay_seconds`変更には再Compileが必要
- Decay上限を1未満にし、無限Feedbackを許可しない
- Width 0でWetをMonoへ近づけ、1で元のStereo Wet
- MixはDry / Wetの線形Mix

## 6.10 Unknown Field

すべてのProcessor Definitionへ`deny_unknown_fields`を適用する。

誤記を無視してDefaultへFallbackしない。

---

# 7. Parameter IDとModulation

## 7.1 Parameter ID

既存Layer Parameter：

```text
layer.<layer_id>.gain
layer.<layer_id>.pan
layer.<layer_id>.tuning
```

Layer Processor：

```text
layer.<layer_id>.processor.<processor_id>.<parameter>
```

Voice Processor：

```text
voice.processor.<processor_id>.<parameter>
```

Global Processor：

```text
global.processor.<processor_id>.<parameter>
```

例：

```text
layer.body.processor.body_drive.amount
layer.attack.processor.tone.cutoff
voice.processor.tone.cutoff
voice.processor.glue.amount
global.processor.echo.feedback
global.processor.echo.mix
global.processor.space.decay
global.processor.space.damping
global.processor.space.width
global.processor.space.mix
```

FieldとParameter末尾の対応：

| Definition Field | Parameter |
|---|---|
| `cutoff_hz` | `cutoff` |
| `resonance` | `resonance` |
| `amount` | `amount` |
| `feedback` | `feedback` |
| `decay` | `decay` |
| `damping` | `damping` |
| `width` | `width` |
| `mix` | `mix` |

Dynamic対象外：

- `time_seconds`
- `pre_delay_seconds`
- Processor Type
- Processor ID
- Placement
- Chain順序

## 7.2 Catalog順序

Parameter Catalog順序を次で固定する。

1. Definition順に各Layer
2. Layer Gain / Pan / Tuning
3. 同LayerのProcessor Chain順
4. Processor内の固定Parameter順
5. Voice Processor Chain順
6. Global Processor Chain順

Processor内順序：

- Filter：Cutoff、Resonance
- Drive：Amount、Mix
- Delay：Feedback、Mix
- Reverb：Decay、Damping、Width、Mix

Processor Typeごとに並べ替えてはならない。

## 7.3 Parameter Owner

`ParameterOwner`を次へ拡張する。

- Layer Base Parameter
- Layer Processor Parameter
- Voice Processor Parameter
- Global Processor Parameter

OwnerはDefinition上のIndexを保持し、Compiler BindingとCLI Inspectへ使用する。

Audio PathではOwnerを検索に利用しない。Compiled ProcessorがParameter Handleを直接保持する。

## 7.4 Descriptor

| Parameter | Unit | Scale | Smoothing |
|---|---|---|---:|
| Filter Cutoff | Hertz | Log2 | 10ms |
| Filter Resonance | Normalized | Linear | 10ms |
| Drive Amount | Normalized | Linear | 5ms |
| Drive Mix | Normalized | Linear | 5ms |
| Delay Feedback | Normalized | Linear | 10ms |
| Delay Mix | Normalized | Linear | 10ms |
| Reverb Decay | Normalized | Linear | 20ms |
| Reverb Damping | Normalized | Linear | 20ms |
| Reverb Width | Normalized | Linear | 20ms |
| Reverb Mix | Normalized | Linear | 20ms |

Smoothing時間はParameter Moduleの定数として一か所で管理する。Definitionへ保存しない。

## 7.5 Source Scope

Layer / Voice Processor Targetへ許可するSource：

- Velocity
- Key Tracking
- LFO
- Modulation Envelope
- Random
- Pitch Bend
- Mod Wheel
- Aftertouch

Global Processor Targetへ許可するSource：

- Pitch Bend
- Mod Wheel
- Aftertouch

Global Targetへ不許可：

- Velocity
- Key Tracking
- User LFO
- Modulation Envelope
- Random

理由は、これらがVoiceごとに異なる値を持ち、Voice Sum後の一つのGlobal Parameterへ一意に集約できないためである。

Instrument Scope LFOは新設しない。

不正ScopeのRouteはCompile Errorとする。

## 7.6 Global Parameter評価

Global Processor ParameterはInstrument RuntimeでControl Spanごとに一度だけ評価する。

入力：

- Shared Base Parameter Span
- Pitch Bend Span
- Mod Wheel Span
- Aftertouch Span
- Global Targetへ接続されたRoute

Voice数分評価しない。Active Voiceの値を平均しない。最後のVoice値を採用しない。

---

# 8. Compiled Model

## 8.1 構造

概念Model：

```rust
pub struct CompiledInstrument {
    pub layers: Box<[CompiledLayer]>,
    pub voice_processors: Box<[CompiledProcessor]>,
    pub global_processors: Box<[CompiledProcessor]>,
    pub parameter_catalog: ParameterCatalog,
    pub sources: Box<[CompiledSource]>,
    pub routes: Box<[CompiledRoute]>,
    pub route_ranges: Box<[RouteRange]>,
    // existing fields
}

pub struct CompiledLayer {
    pub processors: Box<[CompiledProcessor]>,
    // existing fields
}
```

## 8.2 Compiled Processor

```rust
pub struct CompiledProcessor {
    pub id: String,
    pub processor: CompiledProcessorKind,
}

pub enum CompiledProcessorKind {
    Filter(CompiledFilterProcessor),
    Drive(CompiledDriveProcessor),
    Delay(CompiledDelayProcessor),
    Reverb(CompiledReverbProcessor),
}
```

### Filter

保持するもの：

- Cutoff Handle
- Resonance Handle
- Effective Max Cutoff

### Drive

保持するもの：

- Amount Handle
- Mix Handle

### Delay

保持するもの：

- Delay Frames
- Feedback Handle
- Mix Handle

### Reverb

保持するもの：

- Pre-delay Frames
- Decay Handle
- Damping Handle
- Width Handle
- Mix Handle
- Sample Rateへ変換済みの内部Delay Length
- Sample Rateへ変換済みのOutput Tap
- Internal Modulation Increment

Compiled Modelへ次を保存しない。

- Filter内部State
- Delay Buffer
- Delay Write Position
- Reverb Buffer
- Reverb Filter State
- Reverb LFO Phase
- Processor Scratch Buffer

## 8.3 Compile順序

1. Definition全体をValidation
2. Layer / Processor IDをValidation
3. PlacementをValidation
4. Processor Parameter RangeをValidation
5. Process SpecをValidation
6. Parameter CatalogをDefinition順に構築
7. Layer Generator / AssetをCompile
8. Layer Processor ChainをCompile
9. Voice Processor ChainをCompile
10. Global Processor ChainをCompile
11. Delay / ReverbのSample Rate依存値を計算
12. Modulation SourceをCompile
13. Route Sourceを解決
14. Route TargetをParameter Handleへ解決
15. Route Scopeを検証
16. Target別Route Rangeを構築
17. ErrorがなければCompiled Instrumentを返す

## 8.4 Filter Warning

Filter Cutoff Warning Path：

```text
layers[0].processors[1].cutoff_hz
voice_processors[0].cutoff_hz
global_processors[2].cutoff_hz
```

同じWarningをDefinition ValidationとCompilerで二重に追加しない。

## 8.5 Processor Error

Processor構造または値の不正はInstrument Compile Errorとする。

対象Processorだけを無効化して継続しない。

Sample Asset不足の既存Partial Compile規則は維持する。ProcessorはAssetを参照しないため、Processor用Partial Compile経路を新設しない。

---

# 9. RuntimeとState所有

## 9.1 Runtime Enum

Trait Objectや動的Plugin登録を使用しない。

```text
LayerProcessorRuntime
├─ Filter
└─ Drive

StereoProcessorRuntime
├─ Filter
├─ Drive
├─ Delay
└─ Reverb
```

Voice ChainはStereo ProcessorのうちFilter / Driveだけを生成する。

Global Chainは全Typeを生成できる。

## 9.2 Module構成

```text
crates/sonalloy-core/src/runtime/
├─ processor/
│  ├─ mod.rs
│  ├─ drive.rs
│  ├─ delay.rs
│  └─ reverb.rs
├─ instrument.rs
├─ voice.rs
├─ modulation.rs
├─ smoothing.rs
└─ ...
```

`processor/mod.rs`：

- Runtime Enum
- Filter Processor Wrapper
- Chain生成
- Target Span
- Process / Reset Dispatch

`drive.rs`：

- Soft Clip式
- Mono / Stereo In-place処理
- Amount / Mix Ramp

`delay.rs`：

- Ring Buffer
- Stereo Delay
- Reset
- Feedback / Mix Ramp

`reverb.rs`：

- Integer / Fractional Delay Primitive
- All-pass
- One-pole Damping
- Input Diffusion
- Figure-8 Tank
- Internal deterministic modulation
- Stereo Output Tap
- Reset

ProcessorごとにCrateを分けない。

## 9.3 Target Span

```rust
pub enum ProcessorTargetSpan {
    Filter {
        cutoff: ValueSpan,
        resonance: ValueSpan,
    },
    Drive {
        amount: ValueSpan,
        mix: ValueSpan,
    },
    Delay {
        feedback: ValueSpan,
        mix: ValueSpan,
    },
    Reverb {
        decay: ValueSpan,
        damping: ValueSpan,
        width: ValueSpan,
        mix: ValueSpan,
    },
}
```

Runtime ChainとTarget Scratchは同じ順序と同じ長さを持つ。

Process中にTarget Vecを生成しない。

## 9.4 Layer Runtime

`LayerRuntime`へ追加：

- Layer Processor Runtime Chain
- Processor Target Scratch

処理：

1. GeneratorをMono ScratchへRender
2. Layer Processor Targetを評価
3. Layer Processor ChainをIn-place適用
4. ADSR / Note Start Fadeを適用
5. Gain Rampを適用
6. Pan RampでVoice Stereo Bufferへ加算
7. Generator / Envelope終了を判定

Layerを新しいNoteへ再利用するとき、すべてのLayer Processor StateをResetする。

## 9.5 Voice Runtime

保持しないもの：

- 固定`filter_left`
- 固定`filter_right`
- OptionalなCompiled Filter
- 専用Filter Render経路

追加：

- Voice Stereo Processor Runtime Chain
- Voice Processor Target Scratch

処理：

1. Active LayerをStereoへMix
2. Voice Processor ChainをIn-place適用
3. Voice Steal Fadeを適用
4. Processor後のPeakを`estimated_level`へ反映

VoiceがIdleへ戻るときVoice Processor StateをResetする。

## 9.6 Instrument Runtime

追加：

- Global Stereo Processor Runtime Chain
- Global Processor Target Scratch

Control Spanごとの処理：

1. Shared Parameter / External Controlを進める
2. Active VoiceをRender
3. Output SpanへVoiceを加算
4. Global Targetを一度評価
5. Global Processor ChainをIn-place適用
6. 次のEvent / Quantum / Smoother境界へ進む

Active Voiceが0でもGlobal Chainを実行する。

## 9.7 Prepare

Prepare時：

- Layer / Voice Processor RuntimeをPolyphony数分生成
- Global Processor Runtimeを一組生成
- Filter Native Handleを必要数生成
- Delay Bufferを左右分確保
- Reverbの全Delay Bufferを確保
- Target Scratchを固定長で確保
- 全Stateを初期状態にする

途中失敗時：

- 部分Stateを破棄
- Runtimeを未準備状態にする
- 利用には再Prepareを必要とする

## 9.8 Reset

Reset対象：

- Layer / Voice / Global Filter
- Delay Buffer / Position
- Reverb Delay Buffer
- Reverb All-pass State
- Reverb Damping Filter
- Reverb LFO Phase
- Processor Target Scratch
- Global Tail
- 既存Voice / Source / Parameter / External Control

Resetは既存BufferをZero Clearし、再Allocationしない。

同じDefinition、Event、Process Spec、Block Sizeから初回と同等の出力を得る。

---

# 10. Processor処理契約

## 10.1 Filter

既存`DspFilter`を利用する。

Mono：

- 一つのHandle
- Layer BufferをIn-place処理

Stereo：

- Left / Right独立Handle
- 同じTarget Spanを両Channelへ適用

処理Mode：

- Cutoff / Resonance一定：通常Process
- Cutoffだけ変化：Cutoff Ramp
- Cutoff / Resonance変化：Cutoff + Resonance Ramp

Native Errorは既存`DspFailure`へ変換する。

## 10.2 Drive

DriveはRustで実装する。

必要な数学的性質：

- `amount = 0`で厳密に`output = input`
- Amountに対して連続
- 奇関数で正負対称
- 大入力を滑らかに圧縮
- 不連続Clampを主処理にしない
- 非有限入力を検出
- 同じ入力から同じ出力

処理：

```text
dry = input
wet = soft_clip(input, amount)
output = lerp(dry, wet, mix)
```

AmountとMixはSpan内でSampleごとに線形補間する。

Oversamplingは実装しない。高Drive時のAliasingはSound Review対象とし、明確に使用不能な場合は完了扱いにしない。

## 10.3 Delay

ChannelごとのRing Bufferを持つ。

概念処理：

```text
delayed = buffer[position]
write = input + delayed × feedback
buffer[position] = write
output = input × (1 - mix) + delayed × mix
position = next(position)
```

要件：

- Feedback / MixをSampleごとに補間
- 1 Frame Delayを正しく扱う
- Buffer外参照なし
- Block境界でPositionが変化しない
- Left / Right独立
- Resetで全BufferをZero Clear
- 入力0でもTailを出力
- Feedback 0.95で有限
- 非有限検出

Delay Timeは整数Frame固定とする。Fractional DelayはReverb内部Primitiveだけに限定する。

## 10.4 Reverb

### Topology

```text
Stereo Input
    ↓
Mid Excitation
    ↓
Static Pre-delay
    ↓
Input Bandwidth Filter
    ↓
4-stage Input Diffusion
    ↓
Cross-coupled Figure-8 Tank
    ├─ Modulated Tank All-pass
    ├─ Long Delay
    ├─ Damping Filter
    ├─ Decay
    ├─ Tank All-pass
    └─ Long Delay
    ↓
Stereo Output Taps
    ↓
Width
    ↓
Dry / Wet Mix
```

### Delay Length

- Dattorro Reference Sample Rate 29,761 HzのSample Lengthを秒へ変換
- `round(seconds × process_sample_rate)`で実行Lengthを算出
- 最低1 Frame
- Checked Arithmeticを使用
- Buffer確保はPrepare時
- Output Tapも同じ比率で変換

### Fractional Delay / Modulation

Tankの指定All-pass Delayへ低速Modulationを適用する。

- Internal LFOは固定Rate
- PhaseはReset時に固定初期値
- Block Sizeではなく処理Sample数でPhaseを進める
- ExcursionはReference Sample Rateから実行Sample Rateへ変換
- Fractional Readは線形補間
- Definition Parameterへ公開しない

### Stability

- Decayは最大0.98
- DefinitionのDecayは内部Feedback係数へ変換し、内部係数を0.19未満へ制限する
- Diffusion係数は固定で絶対値1未満
- Feedback Loop内で非有限値を検出
- 極小Stateを0へ戻しDenormal蓄積を防止
- Input 0でTail Energyが最終的に減衰
- Freezeは実装しない

### Stereo

- Tank ExcitationはMid
- Dryは元のStereo
- Wet Left / Rightは異なるTap組合せ
- Width 0はWet Midへ近づける
- Width 1はFull Stereo Wet
- Left / Rightが完全一致しない

## 10.5 Mix

Drive、Delay、ReverbのMixは線形Dry / Wetとする。

- 0：Dry
- 1：Wet

Constant-power Dry / Wetは実装しない。

Filterは100% WetでありMixを持たない。

---

# 11. Process ContractとError

## 11.1 Process中に行わないこと

- JSON Parse
- File I/O
- Asset Decode
- Sample Rate変換
- Hash計算
- Processor構築
- Buffer Resize
- `Vec::push`
- `String`生成
- `HashMap`検索
- Blocking Mutex
- Network
- Device操作
- Panic
- C++例外の越境

## 11.2 Processor Failure

Filter Native Error：

- `DspFailure`へ変換
- 対象Block全体を無音化
- Runtimeを未準備状態へ移行
- 再利用にはPrepareが必要

Rust Processor Error：

- Non-finite Input / Output
- Internal Index不変条件違反
- Compiled値の不整合

を`ProcessError`のProcessor Failureへ変換し、同じ無音化・未準備契約を適用する。

## 11.3 Tail

Global Delay / Reverb TailはVoice Stateへ含めない。

Offline Renderは既存`tail_frames`でTail長を明示する。

Tailの減衰をRuntimeが自動判定してRenderを延長する機能は追加しない。

## 11.4 Zero Frame

既存契約を維持し、0 Frame BlockではEventを許可しない。

Processor Stateも進めない。

---

# 12. CLIとInspection

## 12.1 `instrument init`

出力：

- 一つのOscillator Layer
- `processors: []`
- Voice Processorに一つのLow-pass Filter
- `global_processors: []`
- 既存Velocity Route
- 既存Metadata / Performance

専用Filter Fieldを出力しない。

## 12.2 `instrument validate`

追加Diagnostics：

- Processor ID形式不正
- Processor ID重複
- Placement不正
- Parameter Range不正
- Processor Target不正
- Global Route Scope不正
- Filter Cutoff Clamp Warning

## 12.3 `instrument inspect`

Human-readable / JSONの両方で表示：

- Placement
- Chain Index
- Processor ID
- Processor Type
- Static Field
- Dynamic Parameter ID
- Unit
- Range
- Default
- Scale
- Smoothing
- 接続Route

例：

```text
layer body processor[0] body_drive (drive)
  parameter layer.body.processor.body_drive.amount
  parameter layer.body.processor.body_drive.mix

voice processor[0] tone (filter)
  parameter voice.processor.tone.cutoff
  parameter voice.processor.tone.resonance

global processor[0] echo (delay)
  time_seconds: 0.25
  parameter global.processor.echo.feedback
  parameter global.processor.echo.mix
```

InspectはPlacementごとのProcessor情報を表示し、専用Filter表示を持たない。

## 12.4 Render

既存Commandを維持する。

- `render note`
- `render events`
- `render midi`

Processor Parameter Changeは既存Event SequenceのCanonical Parameter IDを利用する。

Effect専用Commandを追加しない。

---

# 13. 現行DefinitionとReview Package

## 13.1 対象ファイル

- `examples/instruments/*.json`
- `testdata/definitions/*.json`
- `testdata/events/*.json`
- Review Script内Definition
- `review-output/*/definitions/*.json`
- CLI Test内Definition
- DocumentationのJSON例

専用Filter FieldをActive Code、Current Definition、Fixture、Reference Instrument、現在仕様のDocumentへ含めない。

## 13.2 Basic Poly Synth

Voice ProcessorへFilterを記述する。

```json
"voice_processors": [
  {
    "type": "filter",
    "id": "tone",
    "cutoff_hz": 12000.0,
    "resonance": 0.12
  }
]
```

音響値を維持し、Baseline差分を検査する。

## 13.3 Metallic Hybrid

Voice Processorへ同じ`tone` Filterを記述する。

Layer、Asset、Envelope、Triggerは既存定義をそのまま使用する。

## 13.4 Dynamic Parameter Reference

```text
voice.processor.tone.cutoff
voice.processor.tone.resonance
```

Moving Hybrid Pad、Expressive Hybrid Lead、Event Sequence、MIDI Reviewを同時更新する。

## 13.5 Schema境界

専用Filter Fieldを含むJSONは未知Fieldとして失敗させる。

Migration Command、変換Script、Aliasを提供しない。

---

# 14. Repository変更範囲

主な変更対象：

```text
crates/sonalloy-core/src/
├─ definition.rs
├─ parameter.rs
├─ compiler.rs
├─ diagnostics.rs
├─ process.rs
├─ lib.rs
└─ runtime/
   ├─ instrument.rs
   ├─ voice.rs
   ├─ modulation.rs
   └─ processor/
      ├─ mod.rs
      ├─ drive.rs
      ├─ delay.rs
      └─ reverb.rs

crates/sonalloy-core/tests/
└─ core_process.rs

crates/sonalloy-cli/src/
└─ main.rs

crates/sonalloy-cli/tests/
└─ cli.rs

.agents/skills/create-instrument/
└─ SKILL.md

docs/
├─ instrument-definition.md
├─ runtime-processing.md
├─ cli.md
├─ architecture.md
├─ creating-an-instrument.md
└─ testing-and-sound-review.md

docs/plan/plan-processor-chain.md

THIRD_PARTY_NOTICES.md

examples/instruments/
└─ *.json

scripts/review/
├─ generate_basic_poly_synth_package.py
├─ generate_metallic_hybrid_package.py
├─ generate_dynamic_parameters_package.py
└─ generate_processor_chain_package.py

review-output/
├─ basic-poly-synth/
├─ metallic-hybrid/
├─ dynamic-parameters/
└─ processor-chain/

testdata/
├─ definitions/
├─ events/
└─ expected/
```

実際に存在しないFile名を計画だけを理由に作らない。現在のRepository構成へ合わせる。

既存範囲として維持する。

- `native/daisysp-wrapper/CMakeLists.txt`のDaisySP Source一覧
- DaisySP固定Commit
- `sonalloy-dsp-sys`の外部Module数

Filterの既存APIを共通Processorから再利用するためのRust側構造変更とTest追加だけを行う。

---

# 15. Test計画

## 15.1 Definition Unit Test

- 空Chain
- Processor ID形式
- 同一Chain ID重複
- 別Layerの同名ID
- Filter / Driveの全Placement
- Delay / ReverbのLayer配置拒否
- Delay / ReverbのVoice配置拒否
- 全Parameter最小 / 最大
- Range外
- Unknown Field
- 専用Filter Field拒否

## 15.2 Parameter Unit Test

- Canonical Processor Parameter ID
- ID Grammar
- Catalog順序
- Owner
- Unit / Scale / Range / Default / Smoothing
- Static FieldがCatalogへ入らない
- 同名ProcessorのScope分離
- Normalize / Denormalize

## 15.3 Compiler Unit Test

- Processor順序保持
- Parameter Handle Binding
- Filter Warning Path
- Delay Frame変換
- Reverb Delay / Tap変換
- Reverb Internal LFO Increment
- Global RouteのVoice Source拒否
- Global RouteのExternal Control許可
- Layer / Voice RouteのVoice Source許可
- Error時にCompiled Instrumentなし
- Existing Sample Asset Warning維持

## 15.4 Drive Unit Test

- Amount 0で完全Identity
- Mix 0で完全Identity
- Mix 1でWet
- 正負対称
- Amount増加でPeak圧縮
- Parameter Ramp連続性
- Mono / Stereo一致
- Finite
- Deterministic

## 15.5 Delay Unit Test

Impulse：

- 指定Frameに最初のEcho
- Feedback 0で一回
- Feedbackありで減衰
- Mix 0でDry
- Mix 1でWet
- Block分割でもEcho位置一致
- Reset後Tailなし
- Left / Right独立
- Feedback 0.95でFinite
- 1 Frame Delay

## 15.6 Reverb Unit Test

- Impulse後にTail
- Tail Energyが長期減衰
- ResetでTail消失
- Left / Right差分
- Sample RateごとのDelay時間整合
- Block Sizeごとの時間軸一致
- Decay増加でTailが長くなる
- Damping増加で高域Energyが減る
- Width 0 / 1の差
- Mix 0でDry
- Internal Modulationの決定性
- Reset後LFO Phase一致
- Finite
- DC蓄積なし
- Denormal相当の極小Stateが残り続けない

Unit Testだけで音質合格としない。

## 15.7 Runtime Unit Test

- Layer Processorが対象Layerだけへ作用
- Layer Filter StateがVoice間で独立
- Voice ProcessorがLayer Mix後へ作用
- Filter Processor StateがVoice間で独立
- Global ProcessorがVoice Sum後に一回だけ作用
- Global TailがVoice終了後も続く
- Chain順序で結果が変わる
- 空Chainが既存処理と同等
- Processor Parameter ChangeがSample Accurate
- Voice ProcessorへLFO
- Global ProcessorへMod Wheel
- Voice Stealing時のState分離
- Reset再現性
- Prepare失敗後NotPrepared
- Filter Native失敗時の無音化

## 15.8 Core Integration Test

- Processed Hybrid Compile / Prepare / Process
- 44.1kHz
- 48kHz
- 96kHz
- Block Size 32
- Block Size 64
- Block Size 257
- Block Size 1024
- Note Event
- Parameter Event
- Polyphony
- Global Tail
- Reset
- ProcessorなしBaseline

## 15.9 CLI Integration Test

- `instrument init`の新Schema
- `instrument validate`
- `instrument inspect`
- JSON InspectのPlacement / 順序
- 不正PlacementのExit Code
- Processor Parameter Event
- 専用Filter Field拒否
- Processed Hybrid WAV出力

## 15.10 Dependency Regression

- `Cargo.toml`に新しいProduct Dependencyが追加されていない
- `Cargo.lock`にProcessor Chain由来の新Packageが追加されていない
- DaisySP固定Commitが変わっていない
- CMakeのDaisySP Build対象が`oscillator.cpp`と`svf.cpp`のまま
- `THIRD_PARTY_NOTICES.md`と実際のDependencyが一致する

---

# 16. Sound Review

## 16.1 Review Package

```text
review-output/processor-chain/
├─ audio/
├─ definitions/
├─ events/
├─ midi/
├─ assets/
├─ metrics.json
└─ review-summary.md
```

生成Script：

```text
scripts/review/generate_processor_chain_package.py
```

## 16.2 Scenario

1. ProcessorなしBaseline
2. Layer Filter
3. Layer Drive
4. Voice Filter
5. Voice Drive
6. Global Filter
7. Global Drive
8. Delay Impulse
9. Reverb Impulse
10. Full Processed Hybrid
11. Processor Parameter Change
12. Global Mod Wheel
13. Voice Stealing
14. Reset前後
15. Block Size比較
16. Sample Rate比較

## 16.3 Metrics

- Finite
- Peak
- RMS
- DC
- Positive Zero Crossing
- 最大隣接Frame差
- Large Discontinuity Count
- Block Size差分
- Sample Rate別Metrics
- Processed Hybrid Tail RMS
- Reset差分
- Render出力のSHA-256

Metricsを手入力しない。

## 16.4 人間の試聴

### Filter

- Sweep
- Resonance
- Layer Isolation
- 共通Filterの回帰

### Drive

- Amount 0付近の自然さ
- 低Amountの質感
- 高AmountのAliasing / 耳障りさ
- Level変化
- Parameter ChangeのClick
- Layer / Voice / Globalでの用途差

### Delay

- Echo間隔
- Feedback減衰
- Stereo定位
- Dry / Wet
- Tail終端
- Phraseでの実用性

### Reverb

- 初期反射の密度
- 金属的Ring
- Tailの滑らかさ
- Damping
- Stereo Width
- 短音
- Pad
- Lead
- Sample Attack
- Mix Balance

### Full Instrument

- Attack SampleとOscillator Bodyの一体感
- Voice Processingが音を潰しすぎない
- Global Effectが原音を覆いすぎない
- 曲で利用したい品質か

独自ReverbがこのReviewを満たさない場合、Processor Chainを完了扱いにしない。機能を残したまま「後で改善」として完了させない。

## 16.5 Existing Review再実行

- Basic Poly Synth
- Metallic Hybrid
- Dynamic Parameters

新Schemaで再生成し、既存確認項目が維持されることを記録する。

---

# 17. Documentationと正本

現在仕様のDocumentationは、Definition、Runtime、CLI、Architecture、Review手順の責務ごとに記述する。正本要件は`docs/CONCEPT.md`とする。

## 17.1 必ず更新する文書

### `docs/instrument-definition.md`

現在仕様と一致させるため、必要な箇所だけを更新する。

- 専用Filter Fieldを含めない現行Schema
- Layer / Voice / GlobalのProcessor Chain
- Processor Definitionと配置制約
- Processor Parameter ID
- 値域とDynamic対象
- Route ScopeとValidation Error
- 現行JSON例

### `docs/runtime-processing.md`

Runtime契約として必要な箇所だけを更新する。

- GeneratorからGlobal Processorまでの現在の信号順序
- Layer / Voice / GlobalのState所有
- Prepare / Process / Reset
- Global Tail
- Processor失敗時の既存Error契約

## 17.2 実装と整合させる文書

### `docs/architecture.md`

Module責務とNative境界を実装と一致させる。依存選定の経緯や進行管理上の名称は記載しない。

### `docs/cli.md`

`instrument init`、`instrument inspect`、JSON出力などの公開CLI仕様を記載する。

## 17.3 参照文書

- `docs/testing-and-sound-review.md`：Review Packageの生成物、機械検査、試聴項目を記載する。
- `docs/CONCEPT.md`：Processor Chain、配置、Parameter、Lifecycleの正本要件を記載する。
- `README.md`：公開Commandと導入手順を記載する。Processor Chainでは既存範囲を維持する。
- `THIRD_PARTY_NOTICES.md`：実際に利用するDependencyとLicenseを記載する。

---

# 18. 実装順序

## 18.1 DSP Primitive

目的：

- Processor Chainへ統合する前に各DSPの処理契約を確定する

作業：

- Rust Drive
- Rust Stereo Delay
- Rust Plate Reverb
- Reusable Delay / Fractional Delay Primitive
- Unit Test
- Impulse Render用のTest Helper

完了条件：

- 単体Test成功
- Sample Rate / Block Size / Reset成立
- Reverbの初回試聴で致命的なRingまたは発散がない
- 新Dependencyなし

## 18.2 Definition

目的：

- Processorの保存形式とPlacementを固定する

作業：

- Processor Definition
- Layer / Voice / Global Chain Field
- Validation
- 専用Filter FieldをSchemaへ含めない
- Existing Fixture更新

完了条件：

- 新Definition Parse
- 不正Definition拒否
- 専用Filter Fieldなし

## 18.3 Parameter Catalog

目的：

- Processor Parameterを既存のParameter ChangeとModulation契約へ接続する

作業：

- Canonical ID
- Owner
- Descriptor
- Catalog順序
- Route Target
- Global Scope Validation

完了条件：

- Dynamic FieldがHandleへ解決
- Static FieldがCatalogへ入らない
- Scope不正をCompile前に検出

## 18.4 Compiled Processor

目的：

- RuntimeがDefinitionや文字列を扱わずChainを生成できるようにする

作業：

- Compiled Processor
- Chain Compile
- Sample Rate依存値
- Warning Path
- 専用Compiled Filter経路を持たない

完了条件：

- 順序保持
- Handle Binding
- Error時Compiledなし

## 18.5 Layer / Voice Processor

目的：

- FilterとDriveを共通Chainへ統合する

作業：

- Layer Runtime Chain
- Voice Runtime Chain
- Filter Wrapper再配置
- Drive統合
- Signal順序変更
- Voice Stealing統合

完了条件：

- Layer / Voice処理
- State分離
- 既存Filter回帰なし

## 18.6 Global Processor

目的：

- Voice Sum後のFilter / Drive / Delay / Reverbを成立させる

作業：

- Global Runtime Chain
- Global Target評価
- Tail処理
- Active Voice 0でのProcess
- Delay / Reverb Reset

完了条件：

- Global State一組
- Tail継続
- Reset
- Block Size独立

## 18.7 CLI、Fixture、Documentation

目的：

- Headlessで構成を理解・検証・Renderできるようにする

作業：

- Init
- Validate
- Inspect
- Existing Definition移行
- Processed Hybrid
- Event Sequence
- `docs/instrument-definition.md`と`docs/runtime-processing.md`
- 公開仕様に差分がある場合だけ`docs/cli.md`または`docs/architecture.md`

完了条件：

- CLI公開経路で新機能を利用可能
- Active Code、現行Definition、Fixture、現在仕様の文書に専用Filter Fieldが残らない

## 18.8 回帰とSound Review

目的：

- 構造、回帰、音質を完了判定する

作業：

- 全Test
- Existing Review再生成
- Processor Review生成
- Metrics
- 人間試聴
- Dependency Regression

完了条件：

- 全自動検査成功
- Existing機能回帰なし
- Processed Hybrid承認
- Drive / Delay / Reverb承認
- 新外部Dependencyなし

---

# 19. 完了条件

## Dependency

- [x] DaisySP固定Commitが維持されている
- [x] DaisySP Build対象がOscillator / SVFのまま
- [x] 新しいProduct Dependencyがない
- [x] Drive、Delay、ReverbがRust独自実装
- [x] Third-party Noticeと実態が一致

## Definition / Compile

- [x] 専用Filter FieldをSchemaへ含めない
- [x] Layer / Voice / Global Chain保存
- [x] ID / Placement / Range Validation
- [x] Processor Parameter ID
- [x] Handle解決
- [x] Definition順保持
- [x] Global Route Scope検証
- [x] Schema互換読込なし

## Runtime

- [x] Layer Processor動作
- [x] Voice Processor動作
- [x] Global Processor動作
- [x] Filter / Drive / Delay / Reverb動作
- [x] State所有単位が正しい
- [x] Global Tail
- [x] Reset
- [x] Process中Allocationなし
- [x] Error時無音化 / Runtime無効化
- [x] Block Size非依存

## Parameter / Modulation

- [x] Processor Parameter Change
- [x] Layer / VoiceへVoice Source
- [x] GlobalへExternal Control
- [x] GlobalへのVoice Source拒否
- [x] Amount / Curve / Clamp / Smoothing
- [x] Static FieldはDynamic Target外

## CLI / Current Specification

- [x] Init
- [x] Validate
- [x] Inspect
- [x] Note / Events / MIDI Render
- [x] `docs/instrument-definition.md`と`docs/runtime-processing.md`が現在実装と一致
- [x] 公開仕様差分のある`docs/cli.md`と`docs/architecture.md`が実装と一致
- [x] `docs/testing-and-sound-review.md`、`docs/CONCEPT.md`、`README.md`の責務が保たれている
- [x] Active Code、Current Definition、Fixture、Reference Instrument、現在仕様の文書に専用Filter Fieldの記述なし

## Test / Review

- [x] Unit Test
- [x] Core Integration Test
- [x] CLI Integration Test
- [x] Native Filter Test
- [x] 44.1 / 48 / 96 kHz
- [x] Block Size 32 / 64 / 257 / 1024
- [x] Reset再現性
- [x] Existing Review再生成
- [x] Processor Review生成
- [ ] 人間による音質承認

---

# 20. 提供範囲

利用可能：

- Oscillator / Sample Hybrid Instrument
- Polyphonic Voice
- Dynamic Parameter / Modulation
- Layer Processor Chain
- Voice Processor Chain
- Global Processor Chain
- Filter
- Drive
- Delay
- Plate Reverb
- CLI Validation / Inspection / Offline Render

未実装：

- Noise
- Square / Pulse / Triangle
- PWM
- Hard Sync
- Waveshaping Generator
- Unison
- Multi Sample
- Loop
- Slice
- Round Robin
- Wavetable
- FM / PM / AM / Ring Mod
- Granular
- EQ
- Chorus
- Convolution
- Dynamics
- Realtime Device
- Riffra
- CLAP
- VST3

Processor基盤は固定位置の直列Chain、明示的なParameter ID、Prepare / Process / Reset契約を備え、GeneratorまたはSample機能から利用できる。
