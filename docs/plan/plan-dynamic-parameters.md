# Sonalloy P3 Dynamic Parameters / Modulation 詳細設計・実装計画
- **対象Repository**：`endo-ly/sonalloy`
- **正本要件**：`docs/CONCEPT.md`
- **前提実装**：`docs/plan/plan-mvp.md`で完了した既存Core
- **用途**：実装エージェントへ渡す詳細設計・実装計画
- **文書言語**：日本語。型名、API名、Parameter ID、File Pathのみ英語を使用する
- **成果物**：Markdownのみ。HTML版は作成しない

---

## 0. この計画書の位置づけ
本書は、既存Coreの次に実装するDynamic ParametersとModulationを、一括で完成させるための計画書である。

製品全体の要件、責務、将来像は`docs/CONCEPT.md`を正本とする。

本書は`CONCEPT.md`のうち、次の要件を現在のコードベースへ実装可能な粒度へ落とす。
- 安定したParameter ID
- DefinitionからCompiled InstrumentへのParameter参照解決
- 発音中に反映できる連続Parameter Change
- Velocity、Key Tracking、LFO、Envelope、Random
- Pitch Bend、Mod Wheel、Aftertouch
- SourceからTargetへのModulation Route
- 複数Routeの加算、Clamp、Smoothing
- CLIとMIDI Fileからの再現可能なOffline Render
P3は一つの実装単位として扱う。

`plan-mvp.md`で扱った範囲が音声処理基盤からHybrid Instrumentまでを含むのに対し、P3は既存のDefinition、Compiler、Voice、Runtime、Rendererを横断して拡張する。

そのため別Phaseへ分割せず、本書の実装順に沿って一括で実装・検証する。
### 0.1 名称の扱い
`P3`はこの計画を識別するためだけの名称である。

コード、型名、関数名、Module名、Fixture名、Diagnostic、コメント、利用者向けの恒久文書へ`P3`を残さない。

既存の`docs/plan/plan-mvp.md`は完了済みの履歴文書としてそのまま残す。
### 0.2 実装判断の優先順位
判断に迷った場合は、次の順序で優先する。
1. `docs/CONCEPT.md`
2. 本書で固定した責務と不変条件
3. 現在の既存Coreの挙動を壊さないこと
4. Audio Threadでの単純さと安全性
5. 将来のGenerator、Effect、Adapterへの拡張
将来使う可能性だけを理由に、現在使わないFramework、抽象化、Crate、依存、設定項目を追加しない。

一方、Parameter ID、Compile時参照解決、Runtimeの所有関係など、後続機能が共通利用する契約は現在の段階で曖昧にしない。
### 0.3 本書で固定するもの
- P3の対象範囲と対象外
- 現行コードのどこを変更するか
- Definition上のParameter IDとModulation構造
- Compile時のTarget解決
- Runtime Parameter Stateの所有者
- Event順序とSample Accurateな反映
- Control-rate評価とBlock Size非依存
- Sourceごとの値域とLifecycle
- Targetごとの値変換とDSP適用
- Error、Warning、失敗時の状態
- CLI、MIDI、Reference Instrument
- Unit Test、結合テスト、音声確認
- 実装順序と完了条件
### 0.4 本書で固定しないもの
- Realtime Audio Device
- Realtime MIDI Device
- Riffra統合
- Public C ABI
- CLAP、VST3
- GUI Editor
- Parameter Automationの記録・編集UI
- MPE
- Note単位Aftertouch
- Tempo Sync LFO
- Sample Position Modulation
- Wavetable、FM、Granular固有Parameter
- Effect ChainとEffect Parameter
- 自由なAudio Graph
- Modulation Source同士の接続
- Route Amount自体のModulation
- Runtimeでの構成Hot Swap

---

# 1. 目的と完成像
## 1.1 現在の既存Core
現在のSonalloyは、次を実装済みである。
- JSON Instrument Definition
- Definition Validation
- Compiled Instrument
- Oscillator Layer
- Sample Layer
- 複数Layerを持つHybrid Voice
- Polyphonic Voice
- ADSR
- Voice Filter
- Gain、Pan、Tuning
- Velocity専用のGain / Filter反応
- Sample AccurateなNote On / Note Off
- Offline Render
- MIDI File変換
- CLIによるValidate、Inspect、Note Render、MIDI Render
ただし、連続ParameterはCompiled値またはVoice開始時の計算値として各Runtimeへ直接保持されている。

発音中のParameter Changeを共通の仕組みで適用する方法はない。

Velocityは`velocity_response`という専用構造と専用関数で処理されており、他のSourceと同じRouteとして扱われていない。

MIDIのPitch Bend、Mod Wheel、Aftertouchは無視される。
## 1.2 P3で成立させること
P3では、Instrument内の連続Parameterを共通の契約で識別し、Definition、CLI、将来Adapter、Runtimeで同じ意味を共有できるようにする。

そのうえで、ParameterのBase値を演奏中に変更し、複数のModulation Sourceから時間変化を加えられるようにする。

完成時には、次の流れが成立する。
```text
Instrument Definition
    │
    ├─ Layer / Voice Parameter
    ├─ Modulation Source
    └─ Source → Target Route
    │
    ▼
Compile
    │
    ├─ Stable Parameter IDを検証
    ├─ Parameter Handleへ解決
    ├─ Sourceを実行用構造へ変換
    └─ Routeを実行順へ変換
    │
    ▼
Instrument Runtime
    │
    ├─ Parameter Change Event
    ├─ Pitch Bend / Mod Wheel / Aftertouch
    ├─ Voice固有Source
    └─ Control SpanごとのEffective値
    │
    ▼
Gain / Pan / Pitch / Filterへ滑らかに反映
    │
    ▼
Offline Stereo WAV
```
## 1.3 完成状態
P3の完成状態は次である。

> Definitionに保存したModulation SourceとRoute、およびSample AccurateなParameter / External Control EventをCompileし、発音中のHybrid Instrumentへ滑らかかつ決定的に反映し、Block Sizeが変わっても同等の音声をOffline Renderできる。
## 1.4 代表成果物
P3の完了時には、次のReference InstrumentをRepositoryへ含める。
### Moving Hybrid Pad
既存のOscillator + Sample Hybridを利用し、内部Modulationの動作を確認する。
```text
Sample Layer
  ├─ Velocity → Gain
  ├─ Random → Pan
  └─ Slow LFO → Pan

Oscillator Layer
  ├─ Modulation Envelope → Tuning
  └─ Velocity → Gain

Voice Filter
  ├─ LFO → Cutoff
  ├─ Key Tracking → Cutoff
  └─ Mod Wheel → Cutoff
```
確認する価値：
- SampleとOscillatorが一つのVoice内で動的に融合する
- Noteごとに異なるRandom値を持つ
- LFOとEnvelopeがBlock境界に依存しない
- Mod Wheelで発音中の音色を操作できる
### Expressive Hybrid Lead
外部演奏Controlを確認する。
```text
Oscillator Layer
  ├─ Pitch Bend → Tuning
  ├─ Aftertouch → Gain
  └─ Mod Wheel → Pan

Voice Filter
  └─ Aftertouch → Cutoff
```
確認する価値：
- Pitch BendがOscillatorとSampleへ同じ音程変化を与える
- Aftertouchが発音中のVoiceへ反映される
- MIDI FileとCLI Event Sequenceで同じCore Eventを利用する
## 1.5 P3で証明する設計上の価値
P3は単にLFOを追加する作業ではない。

次の設計が後続機能でも使えることを証明する。
- GeneratorやEffectを追加してもParameter IDの意味が変わらない
- FrontendがCore内部のField配置を知らなくてよい
- Runtimeが文字列検索やDefinition再解釈をしない
- Active VoiceへParameter Changeを反映できる
- Voice固有SourceとInstrument共有Sourceを混同しない
- Modulation追加で音声配線を自由Graph化しない
- Realtime Adapterへ接続してもAudio Threadの責務を変えなくてよい

---

# 2. 対象範囲
## 2.1 含める機能
### Parameter基盤
- Stable Parameter ID
- Parameter Descriptor
- Parameter Unit
- Parameter Range
- Parameter Scale
- Normalized Valueとの変換
- Compile時Parameter Handle
- RuntimeのDense Parameter State
- Sample Accurate Parameter Change Event
- ParameterごとのSmoothing時間
### Initial Dynamic Targets
- Layer Gain
- Layer Pan
- Layer Tuning
- Voice Filter Cutoff
- Voice Filter Resonance
初期Targetは現在実装済みの信号経路に限定する。

ADSR時間、波形種類、Generator種類、Layer Trigger、Asset PathなどはDynamic Targetにしない。
### Modulation Sources
- Velocity
- Key Tracking
- LFO
- Modulation Envelope
- Random
- Pitch Bend
- Mod Wheel
- Channel Aftertouch
### Modulation Routes
- Source ID
- Target Parameter ID
- Amount
- Curve
- 複数Routeの加算
- Target RangeへのClamp
- Voice ScopeとInstrument Scopeの検証
### Runtime / DSP
- Control-rate評価
- Block Size非依存のControl Clock
- Effective値のSpan生成
- Gain Ramp
- Pan Ramp
- Oscillator Frequency Ramp
- Sample Playback Ratio Ramp
- Filter Cutoff / Resonance Ramp
- Voice Stealingとの統合
- Resetと決定性
### CLI / MIDI
- `instrument inspect`でParameterとRouteを表示
- Event Sequence FileによるParameter / Control Render
- MIDI Pitch Bend
- MIDI CC1 Mod Wheel
- MIDI Channel Aftertouch
- 固定条件のReference Render
### Testing / Review
- Unit Test
- Core結合テスト
- CLI結合テスト
- Block Size独立性
- Reset再現性
- Random決定性
- 音声のfinite / peak / discontinuity確認
- 人間による試聴
## 2.2 対象外
次はP3へ含めない。
- Discrete Parameterの演奏中変更
- Generator追加
- Noise
- Square / Triangleの正式Generator対応
- Wavetable
- FM
- Granular
- Sample Zone
- Loop
- Round Robin
- Sample Position Modulation
- Layer / Voice / Global Effect Chain
- Drive、EQ、Chorus、Delay、Reverb
- Sustain Pedal
- Mono、Legato、Portamento
- Per-note Aftertouch
- MPE
- Tempo Sync
- Host Transport同期
- Realtime Device
- Riffra
- CLAP / VST3
- Public C ABI
- Runtime Compiled Instrument差替え
- Routeの追加・削除をAudio Threadで行うこと
- Route Amount自体をDynamic Parameterにすること
- SourceからSourceへのModulation
- 任意のModulation Graph
## 2.3 求める品質
P3では次を優先する。
1. Existing 既存Coreの音声処理を壊さない
2. ModulationなしのInstrumentが従来と同等に鳴る
3. Parameter Changeで明確なClickを出さない
4. Pitch変化でNaN、Infinity、逆再生、範囲外参照を起こさない
5. LFO、Envelope、RandomがBlock Sizeで変化しない
6. Shared StateをVoice数分進めない
7. Voice Stealing中も新旧Voiceの値が破綻しない
8. Process中に文字列検索、HashMap探索、容量拡張を行わない
9. 同じDefinition、Event、Random Seedから同等出力を得る
10. Reference Instrumentが技術Demoだけでなく音色として成立する
## 2.4 性能方針
P3で本格的なBenchmark Frameworkは導入しない。

ただし次を守る。
- Compile時にRouteとTargetを解決する
- Runtime配列容量はPrepare前に確定する
- Process中にFile I/O、JSON、文字列生成を行わない
- VoiceごとにParameter Catalogを複製しない
- Shared Parameter StateをVoiceごとに更新しない
- Control-rateで十分なSourceをSample単位評価しない
- Native DSPはBlockまたはSpan単位で呼ぶ

---

# 3. 現行コードベースとの接続
## 3.1 Crate境界
現在の依存方向を維持する。
```text
sonalloy-cli
    ↓
sonalloy-core
    ↓
sonalloy-dsp-sys
    ↓
DaisySP
```
P3で新しいCrateを追加しない。
## 3.2 Definition
現在の`InstrumentDefinition`は、おおむね次を保持する。
- `metadata`
- `performance`
- `layers`
- `voice_filter`
- `velocity_response`
各Layerは次を直接保持する。
- `gain_db`
- `pan`
- `tuning_cents`
- `envelope`
- `generator`
P3では`velocity_response`を削除し、Velocity Routeへ統合する。

`gain_db`などのBase値は既存位置に残す。

Parameterを別の汎用Mapへ移し、Definitionの可読性を落としてはならない。
## 3.3 Compiler
現在のCompilerはDefinitionをValidationし、次の実行値へ変換する。
- dB → Linear Gain
- cent → Tuning Ratio
- ADSR秒 → Sample数
- Sample Asset → Prepared Sample
- Filter Cutoff → Sample Rate上限を適用した値
P3ではこれに次を追加する。
- Canonical Parameter ID生成
- Parameter Descriptor生成
- Parameter Handle割当
- Source ID解決
- Route Target解決
- RouteのScope検証
- Runtime容量の確定
Compile後のAudio PathでDefinition文字列を再解釈しない。
## 3.4 Process Contract
現在の`ProcessEventKind`はNote On / Note Offを扱う。

`InstrumentRuntime::process`は、Event OffsetごとにBlockをSegmentへ分割する。

P3では同じ仕組みに次を追加する。
- Parameter Change
- Pitch Bend
- Mod Wheel
- Channel Aftertouch
EventのSample Offset契約は変更しない。

`ProcessEventKind`へ`f32`値を追加するため、`ProcessEventKind`、`ProcessEvent`、`ScheduledEvent`の`Eq` deriveは外し、`PartialEq`までとする。

浮動小数点Eventへ不正な`Eq`実装を追加しない。
## 3.5 Instrument Runtime
現在のInstrument Runtimeは次を所有する。
- Compiled Instrument
- Voice Pool
- Scratch Buffer
- Process Spec
- Voice Stealing設定
- Velocity Response
P3では次を追加する。
- Shared Parameter State
- External Control State
- Control Clock
- Shared Span Scratch
Velocity Response専用Stateは削除する。
## 3.6 Voice Runtime
現在のVoice Runtimeは次を所有する。
- Voice State
- Note ID
- Note Number
- Velocity
- Started Frame
- Estimated Level
- Layer Runtime
- Filter State
- Pending Note
- Steal Fade
P3では次を追加する。
- Voice Source State
- Effective Target Scratch
- Modulation Envelope State
- LFO Phase
- Random値
VoiceごとにShared Parameter Smootherを複製しない。
## 3.7 Layer Runtime
現在のLayer Runtimeは次を保持する。
- Trigger
- ADSR
- Generator Runtime
- 固定Gain
- 固定Pan Gain
- 固定Tuning Ratio
- Gain Smoother
P3では固定Gain / Pan / TuningをHandle参照とEffective Spanへ置き換える。

発音開始時のFadeとDynamic Gain変更は別の状態として扱う。
## 3.8 Velocity専用経路
現在は`VelocityResponseDefinition`、`CompiledVelocityResponse`、`runtime::mix::velocity_gain`、`runtime::mix::velocity_cutoff`が専用経路を構成している。

`InstrumentRuntime::render_range`、`VoiceRuntime::request_note`、`VoiceRuntime::render`もVelocity Responseを引数として渡している。

P3ではこれらをVelocity SourceとRouteへ置き換える。

移行完了後は、Velocityを二重適用しないよう専用型、専用引数、専用計算関数、`lib.rs`のExportを削除する。
## 3.9 Existing Smoother
現在の`Smoother`は、Current、Target、Remaining Frameを持つ線形Smootherである。

P3ではBase Parameter Changeに再利用できる。

ただし一つのSmootherをVoiceごとに`next()`してはならない。

Instrument Runtimeが一度だけ進め、VoiceへStart / End値を渡す。
## 3.10 ADSR
現在の`AdsrRuntime`はAmplitude EnvelopeとしてLayerに属する。

P3のModulation Envelopeは同じCurve計算を再利用してよい。

ただしAmplitude ADSRのStateや出力を直接Modulation Sourceとして共有しない。

専用のSource DefinitionとVoice Stateを持つ。
## 3.11 Sample Runtime
現在のSample Runtimeは、固定`playback_ratio`でFractional Cursorを進める。

P3ではSpan内でRatioを滑らかに変更できるようにする。

Pitch変化中もPositionは単調増加し、範囲外参照しない。

Sample終端Fadeは残りOutput Frame数を固定Ratioで計算しているため、Dynamic Ratioに合わせて見直す。
## 3.12 DSP Sys
現在のOscillator APIは、一つのFrequencyでSlice全体を処理する。

現在のFilter APIは、CutoffだけをRampできる。

P3では次のNative API拡張が必要になる。
- Oscillator Frequency Ramp
- Filter Cutoff / Resonance Ramp
内部C ABIであり、Public C ABIにはしない。
## 3.13 CLI / MIDI
現在のCLIは次を提供する。
- `instrument init`
- `instrument validate`
- `instrument inspect`
- `render note`
- `render midi`
現在のMIDI AdapterはNoteとTempoを変換し、Controller、Pitch Bend、AftertouchをWarning付きで無視する。

P3では対応ControlだけをCore Eventへ変換する。

---

# 4. 設計原則
## 4.1 音声配線は変えない
P3は信号経路を変更しない。
```text
Generator
  → Layer ADSR
  → Layer Gain / Pan
  → Layer Mix
  → Voice Filter
  → Voice Sum
  → Output
```
Modulationは、この固定経路に存在するParameterの値を変える。

RouteによってProcessorを追加、削除、並べ替えしない。
## 4.2 IDとHandleを分ける
Definition、CLI、Frontendでは文字列のParameter IDを使う。

Compiled InstrumentとRuntimeでは固定長のParameter Handleを使う。

Audio Pathで文字列比較やHashMap探索をしない。
## 4.3 Base値とEffective値を分ける
Base値はDefinitionまたはParameter Change Eventで設定される値である。

Effective値は、Base値へModulation Routeの結果を加え、Clampした値である。
```text
Base Parameter
  + Route 1
  + Route 2
  + ...
  → Clamp
  → DSP適用値
```
Modulation結果をBase値へ書き戻さない。
## 4.4 Parameter ChangeとModulationを混同しない
Parameter Change EventはBase値を変更する。

Modulation SourceはBase値を変更せず、現在のEffective値だけへ影響する。

ResetではBase値をCompiled Defaultへ戻し、Source Stateも初期化する。
## 4.5 Smoothingを二重にしない
Base Parameter ChangeはParameter DescriptorのSmoothing時間で平滑化する。

LFOやEnvelopeなど時間連続SourceはControl SpanのStart / End値を補間して適用する。

同じ変化へBase Smootherと追加Smootherを重ねない。
## 4.6 Shared Stateは一度だけ進める
Parameter Smoother、Pitch Bend、Mod Wheel、Aftertouch、Control ClockはInstrument Runtimeが所有する。

一つのProcess Spanにつき一度だけ更新する。

Voiceは共有値を参照し、自分のSourceだけを進める。
## 4.7 Voice固有StateはVoiceが所有する
次はVoiceごとに独立する。
- Velocity
- Key Tracking
- LFO Phase
- Modulation Envelope State
- Random値
- Effective Target Scratch
同じNote Eventから生じたLayerは同じVoice Source Stateを利用する。
## 4.8 Compile時に容量を確定する
CompileまたはRuntime生成時に次を確定する。
- Parameter数
- Source数
- Route数
- TargetごとのRoute範囲
- Voice Source State数
- Effective Target Scratch数
Process中にRoute用`Vec`を拡張しない。
## 4.9 Determinism
同じ条件では同等の結果を得る。

決定性に含める条件：
- Definition
- Asset
- Process Spec
- Event列
- Block Sizeに依存しないAbsolute Frame
- Random Source DefinitionのSeed
- Note ID
- Source ID
Random Deviceや現在時刻をSeedにしない。
## 4.10 既存の単純なInstrumentを複雑化しない
Modulation SourceやRouteが0件の場合、Runtimeは不要なVoice Source評価を行わない。

Parameter Change Eventがない場合、Base値はDefaultのまま利用する。

Existing Basic Poly Synth / Metallic Hybridは、新形式へ更新後もModulationなしで利用できる。

---

# 5. 用語と全体フロー
## 5.1 用語
| 用語 | 意味 |
|---|---|
| Parameter ID | DefinitionとFrontendで使う安定した文字列ID |
| Parameter Handle | Compiled Instrument内だけで有効なDense Index |
| Descriptor | Unit、Range、Scale、Default、Smoothingを持つParameter契約 |
| Base Value | DefinitionまたはParameter Changeで設定された値 |
| Source | Velocity、LFOなどの正規化された変調値 |
| Route | SourceからTargetへAmountとCurveを適用する接続 |
| Effective Value | BaseへRoute結果を加算しClampした値 |
| Control Quantum | Modulation値を更新する固定Frame間隔 |
| Control Span | EventまたはQuantum境界で区切られた処理区間 |
| Shared State | Instrument全体で共有するBase値とExternal Control |
| Voice State | Noteごとに独立するSourceとEffective値 |
## 5.2 Compileまでの流れ
```text
Definition
  ├─ Layer Base Values
  ├─ Voice Filter Base Values
  ├─ Modulation Sources
  └─ Modulation Routes
        │
        ▼
Validation
  ├─ ID
  ├─ Range
  ├─ Source Type
  ├─ Target存在
  └─ Scope整合
        │
        ▼
Compile
  ├─ Parameter Catalog
  ├─ Parameter Handle
  ├─ Compiled Sources
  ├─ Compiled Routes
  └─ Route Range by Target
```
## 5.3 Runtimeの流れ
```text
Process Block
  │
  ├─ Event Validation
  ├─ Output Zero Clear
  └─ Event / Quantum境界を計算
        │
        ▼
Control Span
  ├─ Shared Base Start / End
  ├─ External Control Start / End
  └─ VoiceごとのSource Start / End
        │
        ▼
Effective Target Start / End
        │
        ▼
Layer / Voice DSP
```
## 5.4 Control Spanの境界
Control Spanは次の最短位置で区切る。
- Process Block End
- 次のProcess Event Offset
- 次のControl Quantum境界
- 次のShared Parameter / External Control Smoother完了
- Voice Stealing Fade完了
- Modulation EnvelopeのSegment完了
Voice StealingやEnvelopeの内部境界は、そのVoiceだけのSubspanとして扱ってよい。

Shared Parameter StateとControl ClockをVoiceのSubspanごとに進め直してはならない。
## 5.5 Start / End値
SpanのStart値はSpan先頭Sampleへ適用する値である。

SpanのEnd値はSpan終了位置、すなわち次Spanの先頭時点の値である。

`frames = N`のSpanでは、Sample 0からSample N-1へStartからEnd直前まで補間する。

次SpanのStartは直前SpanのEndと一致する。

この規則でBlock分割による1 Sampleのずれを防ぐ。

---

# 6. Parameter Contract
## 6.1 Canonical Parameter ID
Parameter IDはInstrument Definition内で一意な文字列とする。

初期TargetのIDは構造から決定的に生成する。
```text
layer.<layer_id>.gain
layer.<layer_id>.pan
layer.<layer_id>.tuning
voice.filter.cutoff
voice.filter.resonance
```
例：
```text
layer.body.gain
layer.attack.pan
layer.body.tuning
voice.filter.cutoff
```
Layer IDは既存Definitionの`layer.id`を利用する。

Parameter IDをDefinitionへ重複保存しない。

Base FieldとParameter IDの対応はCore側の一か所で定義する。

将来Generator Parameterを追加するときは次の形式を使用できる。
```text
layer.<layer_id>.generator.<parameter_name>
```
将来Effect Parameterを追加するときは所属位置をIDに含める。

現在は未実装IDを予約だけしない。
## 6.2 IDの制約
Layer IDとUser-defined Source IDは次に固定する。
```text
[a-z][a-z0-9_]{0,63}
```
- ASCII小文字から開始する
- 2文字目以降はASCII小文字、数字、`_`
- 長さは1〜64
- `.`は構造区切りとしてCoreだけが追加し、Component IDでは許可しない
- 大文字小文字の自動変換、Trim、空白除去を行わない
- User-defined Source IDはBuilt-in Source IDと重複できない
Canonical Parameter IDはCoreが生成する。Routeに書かれたTarget文字列は、生成済みCatalogとの完全一致で解決する。
## 6.3 Parameter Handle
Compiled InstrumentではParameter IDをDense Indexへ変換する。

推奨形：
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParameterHandle(usize);
```
HandleはCompiled Instrument内のDense Indexを表す。

Parameter数へ人工的な固定上限を追加しない。

HandleはCompiled Instrumentごとに割り当てられる。

Definitionへ保存しない。

異なるCompiled Instrument間で同じ数値Handleを同一Parameterとみなさない。

HandleへCatalog UUIDやGenerationを埋め込む仕組みはP3では追加しない。

呼び出し側はCompile後にHandleを解決し直し、同じCompiled Instrumentから生成したRuntimeへだけ渡す。
## 6.4 Parameter Descriptor
Descriptorは少なくとも次を持つ。
```rust
pub struct ParameterDescriptor {
    pub id: String,
    pub owner: ParameterOwner,
    pub unit: ParameterUnit,
    pub scale: ParameterScale,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub smoothing_seconds: f32,
}
```
P3で公開するParameterはすべて連続`f32`である。

一種類しか存在しない段階で`ParameterValueType`の一要素Enumは追加しない。

離散ParameterをCatalogへ追加する段階で型情報の表現を拡張する。

`id`と表示用文字列もP3では分けず、Frontend向けLabelが必要になった時点で追加する。
### Parameter Owner
```rust
pub enum ParameterOwner {
    Layer { definition_index: usize },
    VoiceFilter,
}
```
OwnerはDefinition上の所属を示す。

`definition_index`は元のLayer配列Indexであり、Enabled Layerだけを詰めたRuntime Layer Indexとは異なる。

Audio PathはOwnerからRuntime Layerを探索せず、Compiled Layerが保持するParameter Handleを直接使う。

Parameter ID文字列からProcess中にOwnerを解析しない。
### Parameter Unit
初期値：
```rust
pub enum ParameterUnit {
    Decibels,
    Pan,
    Cents,
    Hertz,
    Normalized,
}
```
### Parameter Scale
```rust
pub enum ParameterScale {
    Linear,
    Log2,
}
```
- Gain dB：Linear
- Pan：Linear
- Tuning cent：Linear
- Filter Cutoff Hz：Log2
- Resonance：Linear
## 6.5 Parameter Range
| Parameter | Min | Max | Default Source | Smoothing |
|---|---:|---:|---|---:|
| Layer Gain | -60 dB | +12 dB | `gain_db` | 5 ms |
| Layer Pan | -1 | +1 | `pan` | 5 ms |
| Layer Tuning | -1200 cent | +1200 cent | `tuning_cents` | 5 ms |
| Filter Cutoff | 20 Hz | 20000 Hz | `cutoff_hz` | 10 ms |
| Filter Resonance | 0 | 1 | `resonance` | 10 ms |
Parameter DescriptorのRangeはSample Rateに依存させず、20〜20000 Hzで固定する。

Filter DSPへ渡す安全上限はCompile時に`min(20000, sample_rate × 0.45)`として別に保持する。

これによりParameterのNormalized MappingをSample Rate変更で変えない。
## 6.6 Normalized Value
Runtime Parameter Change Eventは`0.0..=1.0`のNormalized Valueを受け取る。

DefinitionとCLI表示はNative Unitを使用する。

Descriptorは次を提供する。
```rust
fn normalize(native: f32) -> Result<f32, ParameterValueError>;
fn denormalize(normalized: f32) -> Result<f32, ParameterValueError>;
```
### Linear
```text
normalized = (native - min) / (max - min)
native = min + normalized × (max - min)
```
### Log2
```text
normalized = log2(native / min) / log2(max / min)
native = min × 2 ^ (normalized × log2(max / min))
```
非有限値を拒否する。

Normalized値を黙ってClampせず、Event Validation Errorとする。

DefinitionのBase値は既存方針に従いValidationまたはCompile Warningを返す。
## 6.7 Catalog
Compiled InstrumentはParameter Catalogを保持する。
```rust
pub struct ParameterCatalog {
    descriptors: Box<[ParameterDescriptor]>,
    lookup: HashMap<String, ParameterHandle>,
}
```
`lookup`はControl側でのみ利用する。

Process中は`descriptors[handle.index()]`とDense State配列を使う。

Catalog順序：
1. Layer配列順
2. 各Layer内でGain、Pan、Tuning
3. Voice Filter Cutoff、Resonance
Filterが存在しない場合、Filter ParameterをCatalogへ追加しない。

LayerがDisabledでもCatalogにはParameterを含める。

Missing Sample AssetでLayerが実行時DisabledになってもIDを消さない。

これによりAssetの有無でParameter Handle順が変わらない。
## 6.8 Public Lookup
CoreはControl側向けに次を提供する。
```rust
impl CompiledInstrument {
    pub fn parameters(&self) -> &[ParameterDescriptor];
    pub fn parameter_handle(&self, id: &str) -> Option<ParameterHandle>;
}
```
Runtimeへ文字列IDのParameter Changeを直接渡すAPIは作らない。

CLIはRender開始前にIDをHandleへ解決する。
## 6.9 Initial Targetだけを一般化する
Parameter Infrastructureは将来Target追加を可能にする。

ただしP3で未実装TargetのDescriptor、Handle、空Stateを作らない。

特に次をCatalogへ含めない。
- ADSR Attack / Decay / Sustain / Release
- Oscillator Waveform
- Phase Reset
- Sample Root Note
- Asset Path
- Layer Trigger Range
- Polyphony
- Voice Stealing Rule
- Modulation Source Rate
- Route Amount

---

# 7. DefinitionとModulation Model
## 7.1 Instrument Definition
P3では現在のDefinition構造を直接更新する。

概念構造は次とする。
```rust
pub struct InstrumentDefinition {
    pub schema_version: u32,
    pub metadata: InstrumentMetadata,
    pub performance: PerformanceDefinition,
    pub layers: Vec<LayerDefinition>,
    pub voice_filter: Option<FilterDefinition>,
    #[serde(default)]
    pub modulation: Option<ModulationDefinition>,
}
```
`velocity_response`は削除し、Velocityの反応をModulation Routeへ統合する。

Modulationを使わないInstrumentでは`modulation`を省略できる。

新規Definition構造も既存方針どおり未知Fieldを拒否し、Enum値は`snake_case`で保存する。
## 7.2 Modulation Definition
```rust
pub struct ModulationDefinition {
    pub sources: Vec<ModulationSourceDefinition>,
    pub routes: Vec<ModulationRouteDefinition>,
}
```
`sources`または`routes`が0件でもよい。

`modulation`が存在する場合、空配列を許可するが、Exampleへ意味のない空Blockを必須記載しない。
## 7.3 Built-in Source
次はDefinitionへSource Definitionを書かなくてもRouteから参照できる予約IDとする。
```text
velocity
key_tracking
pitch_bend
mod_wheel
aftertouch
```
意味はCoreが固定する。

User-defined Source IDと予約IDの重複を拒否する。
## 7.4 User-defined Source
```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModulationSourceDefinition {
    Lfo(LfoDefinition),
    Envelope(ModEnvelopeDefinition),
    Random(RandomDefinition),
}
```
各SourceはDefinition内で一意な`id`を持つ。
## 7.5 LFO Definition
```rust
pub struct LfoDefinition {
    pub id: String,
    pub waveform: LfoWaveform,
    pub rate_hz: f32,
    pub phase: f32,
}
```
```rust
#[serde(rename_all = "snake_case")]
pub enum LfoWaveform {
    Sine,
    Triangle,
}
```
Range：
- `rate_hz`：0.01〜40 Hz
- `phase`：0以上1未満
LFOはVoice Scopeとする。

Note On時にDefinitionのPhaseから開始し、Voice終了まで継続する。

Free-running Instrument LFO、Tempo Sync、Random PhaseはP3へ含めない。
## 7.6 Modulation Envelope Definition
```rust
pub struct ModEnvelopeDefinition {
    pub id: String,
    pub attack_seconds: f32,
    pub decay_seconds: f32,
    pub sustain_level: f32,
    pub release_seconds: f32,
}
```
Rangeと時間単位はAmplitude ADSRと同じとする。

Voice Scopeであり、Note On / Note Offに従う。

Amplitude ADSRの設定やStateを流用して一つにまとめず、別のEnvelope Instanceとして保持する。

Curve計算Helperは共有してよい。
## 7.7 Random Definition
```rust
pub struct RandomDefinition {
    pub id: String,
    pub seed: u64,
}
```
RandomはVoice ScopeのSample-and-holdとする。

Note On時に一度`-1..=1`の値を生成し、Voice終了まで保持する。

複数の独立Randomが必要な場合は、異なるIDとSeedを持つSourceを複数定義する。

周期更新Random、Noise Generator、現在時刻を利用するSeedはP3へ含めない。
## 7.8 Route Definition
```rust
pub struct ModulationRouteDefinition {
    pub source: String,
    pub target: String,
    pub amount: f32,
    pub curve: ModulationCurve,
}
```
```rust
#[serde(rename_all = "snake_case")]
pub enum ModulationCurve {
    Linear,
    SmoothStep,
}
```
`amount`の範囲は`-1.0..=1.0`。

Route IDは追加しない。

Definition配列IndexをDiagnostic Pathとして使用し、配列順を同一Target内の安定した加算順とする。

Routeの追加・削除はDefinition変更と再Compileで反映する。
## 7.9 Amountの意味
Source値をCurveへ通し、Target Parameterの可変範囲に対する割合として加算する。

Linear Parameter：
```text
delta = curved_source × amount × (max - min)
effective = base + sum(delta)
```
Log2 Parameter：
```text
log2_delta = curved_source × amount × log2(max / min)
effective = base × 2 ^ sum(log2_delta)
```
最後にTarget RangeへClampする。

Target固有の隠れた倍率を持たせない。

Pitch Bendの可動幅もTuning TargetへのRoute Amountで定義する。

例としてLayer TuningのRangeが-1200〜+1200 centの場合、約±200 centのPitch Bendは次で表せる。
```text
amount = 200 / 2400
```
## 7.10 Curve
### Linear
```text
curved = source
```
### SmoothStep
Unipolar Source：
```text
curved = x² × (3 - 2x)
```
Bipolar Source：
```text
curved = sign(x) × smoothstep(abs(x))
```
CurveはSource値へ適用し、その後にAmountを乗算する。
## 7.11 Definition例
```json
{
  "metadata": {
    "name": "Moving Hybrid Pad",
    "author": null,
    "description": "Hybrid instrument with internal modulation"
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
        "key_min": 0,
        "key_max": 127,
        "velocity_min": 1,
        "velocity_max": 127
      },
      "gain_db": -16.0,
      "pan": 0.0,
      "tuning_cents": 0.0,
      "envelope": {
        "attack_seconds": 0.3,
        "decay_seconds": 1.2,
        "sustain_level": 0.7,
        "release_seconds": 2.0
      },
      "generator": {
        "type": "oscillator",
        "waveform": "saw",
        "phase_reset": true
      }
    }
  ],
  "voice_filter": {
    "cutoff_hz": 1800.0,
    "resonance": 0.18
  },
  "modulation": {
    "sources": [
      {
        "type": "lfo",
        "id": "filter_motion",
        "waveform": "sine",
        "rate_hz": 0.25,
        "phase": 0.0
      },
      {
        "type": "envelope",
        "id": "pitch_motion",
        "attack_seconds": 0.08,
        "decay_seconds": 0.6,
        "sustain_level": 0.0,
        "release_seconds": 0.2
      },
      {
        "type": "random",
        "id": "voice_pan",
        "seed": 8128
      }
    ],
    "routes": [
      {
        "source": "velocity",
        "target": "layer.body.gain",
        "amount": 0.25,
        "curve": "linear"
      },
      {
        "source": "filter_motion",
        "target": "voice.filter.cutoff",
        "amount": 0.18,
        "curve": "smooth_step"
      },
      {
        "source": "pitch_motion",
        "target": "layer.body.tuning",
        "amount": 0.04,
        "curve": "linear"
      },
      {
        "source": "voice_pan",
        "target": "layer.body.pan",
        "amount": 0.10,
        "curve": "linear"
      }
    ]
  }
}
```
## 7.12 Validation
Definition Validationで次を検査する。
- InstrumentにLayerが1件以上ある
- Layer IDが一意
- Layer IDがParameter IDへ安全に組み込める文字制約を満たす
- Source IDが一意
- Source IDが文字制約を満たす
- User Source IDがReserved Source IDと重複しない
- LFO RateとPhaseがRange内
- Modulation Envelope値がRange内
- Random Seedが存在する
- Route Source ID形式が有効
- Route Target ID形式が有効
- Route AmountがRange内
- Curveが対応値
Targetの実在、Sourceの実在、Targetが連続ParameterかどうかはParameter Catalog生成後のCompilerで検証する。

Modulationが存在しない場合はSource / Route Validationを行わない。
## 7.13 Existing Definition更新
`schema_version`の運用は変更せず、Repository内のDefinition、Example、Fixture、CLI生成結果を現在の構造へ一括更新する。旧構造を読む分岐は作らない。
- `velocity_response`を削除する
- Velocity反応が必要なInstrumentだけ、対応するVelocity Routeへ置き換える
- Filter Cutoffは必要に応じてBase値とRoute Amountを調整し、従来の高Velocity時Cutoffを再現する
- Gainの旧Linear-amplitude式を隠れた特殊処理として残さず、Reference Instrumentを新しいdB Routeで再調整する
- Modulationを使わないInstrumentでは`modulation`を省略する
- `instrument init`は最小のModulationなしDefinitionを生成する
# 8. Compiler設計
## 8.1 Compile順序
1. Definition構造をValidationする
2. Definition上の全LayerからParameter Catalogを生成する
3. Enabled Layer / Generator / AssetをCompileし、Parameter HandleをBindingする
4. Built-in Source Tableを生成する
5. User SourceをCompileする
6. Source IDをSource Handleへ解決する
7. Route Target IDをParameter Handleへ解決する
8. Targetが連続Parameterであることを確認する
9. RouteをTarget Handle順へ整理する
10. TargetごとのRoute Rangeを生成する
11. Runtime生成に必要な容量を確定する
12. ErrorがなければCompiled Instrumentを返す
Asset Warningなど既存Compilerの動作は維持する。
## 8.2 Source Handle
```rust
pub struct SourceHandle(usize);
```
Source HandleはCompiled Instrument内だけで有効なDense Indexとする。

Built-in SourceとUser Sourceを実行時に同じ文字列Lookupへ入れる必要はない。

Built-in SourceはEnum、User SourceはDense Tableで保持してよい。
## 8.3 Source Scope
```rust
pub enum SourceScope {
    Voice,
    Instrument,
}
```
Voice Scope：
- Velocity
- Key Tracking
- LFO
- Modulation Envelope
- Random
Instrument Scope：
- Pitch Bend
- Mod Wheel
- Aftertouch
P3のTargetはすべてVoice内のLayerまたはVoice Filterへ適用される。

Instrument ScopeはSource Stateの所有者を表し、Global Effect Targetの先行実装を意味しない。
## 8.4 Compiled Source
```rust
pub enum CompiledVoiceSource {
    Velocity,
    KeyTracking,
    Lfo(CompiledLfo),
    Envelope(CompiledModEnvelope),
    Random(CompiledRandom),
}
```
Instrument Scope Sourceは固定3種類なのでRuntime Fieldとして保持してよい。

Source実装のためのTrait Objectは使わない。
## 8.5 Compiled Route
```rust
pub struct CompiledRoute {
    pub source: CompiledSourceRef,
    pub target: ParameterHandle,
    pub amount: f32,
    pub curve: ModulationCurve,
}
```
```rust
pub enum CompiledSourceRef {
    Voice(SourceHandle),
    PitchBend,
    ModWheel,
    Aftertouch,
}
```
RouteはTarget Handleごとに連続した領域へ並べる。

同一Target内ではDefinition配列順を維持する。

浮動小数点加算順を固定し、Block SizeやVoice数で順序を変えない。
## 8.6 Target Route Range
Parameter HandleごとにRoute範囲を持つ。
```rust
pub struct RouteRange {
    pub start: usize,
    pub len: usize,
}
```
TargetにRouteがない場合は`len = 0`。

Process中に全Routeを走査してTargetを探さない。
## 8.7 Target Validation
次をCompile Errorにする。
- 存在しないLayerのTarget
- FilterなしInstrumentのFilter Target
- 未対応Parameter名
- Discrete FieldへのTarget
- Catalogに存在しないParameterへのTarget
- 不正なReserved ID形式
- 存在しないSource
- Source Definitionが無効なRoute
Disabled LayerのParameterはCatalogに存在するためTargetとして解決できる。

Missing AssetでSample Layerが無効になってもCompile結果のParameter構造を変えない。
## 8.8 Compile Diagnostic
追加する主なDiagnostic Code：
- `PARAMETER_ID_INVALID`
- `PARAMETER_NOT_FOUND`
- `SOURCE_ID_INVALID`
- `SOURCE_ID_DUPLICATED`
- `SOURCE_NOT_FOUND`
- `SOURCE_VALUE_INVALID`
- `ROUTE_AMOUNT_INVALID`
- `ROUTE_TARGET_INVALID`
Diagnostic Path例：
```text
modulation.sources[1].rate_hz
modulation.sources[2].seed
modulation.routes[2].source
modulation.routes[2].target
layers[0].id
```
既存の`VALUE_OUT_OF_RANGE`を利用した方が一貫する単純な値Errorは、無理に専用Codeへ分けない。
## 8.9 Error収集
Definition / Compile Errorは、安全に継続できる範囲で複数収集する。

一つのRoute解決失敗でCompilerをPanicさせない。

Parameter Catalog自体が成立しない場合は、依存するRoute解決をSkipしてよい。

Errorが一つでもあればCompiled Instrumentを返さない。

WarningだけならCompiled Instrumentを返す。
## 8.10 Compiled Target Binding
Compiled LayerとCompiled Filterは、Runtimeで使用するParameter Handleを明示的に保持する。

概念形：
```rust
pub struct CompiledLayerParameters {
    pub gain: ParameterHandle,
    pub pan: ParameterHandle,
    pub tuning: ParameterHandle,
}

pub struct CompiledFilterParameters {
    pub cutoff: ParameterHandle,
    pub resonance: ParameterHandle,
    pub effective_max_cutoff_hz: f32,
}
```
既存の`gain_linear`、固定Pan Gain、固定Tuning RatioをRuntime値の正本として残さない。

Base値の正本はParameter DescriptorのDefaultとRuntime Parameter Stateである。

Compiled Layer / Filterは、自分がどのHandleを利用するかだけを保持する。

Generator、Envelope、Trigger、Prepared SampleなどParameter化しない設定は従来どおりCompiled構造へ保持する。
## 8.11 Runtime容量
Compile結果から次の容量を決定できるようにする。
- Parameter Count
- Voice Source Count
- Compiled Route Count
- Voiceが使用するDynamic Target Count
Runtime生成時にSource State、Shared Parameter State、Target Scratchを必要量だけ確保する。

人工的な最大Parameter数や最大Route数は設けない。

Process中に容量拡張しないことを要件とする。
# 9. Runtime Parameter State
## 9.1 Shared Parameter State
Instrument RuntimeがParameter Handle順のState配列を持つ。
```rust
pub struct RuntimeParameterState {
    current_normalized: f32,
    target_normalized: f32,
    remaining_frames: usize,
}
```
Base Parameter ChangeはNormalized値でStateを更新する。

Default値はDescriptorのDefault Native値をNormalizeして初期化する。
## 9.2 Parameter Change Event
```rust
ProcessEventKind::ParameterChange {
    parameter: ParameterHandle,
    normalized: f32,
}
```
Event適用時：
1. HandleがCatalog範囲内か確認
2. Valueがfiniteか確認
3. `0..=1`か確認
4. DescriptorのSmoothing Frame数を求める
5. CurrentからTargetへのRampを開始
不正EventはProcess Errorとし、Block処理前のValidationで検出する。
## 9.3 Shared Span
Instrument RuntimeはControl Spanごとに、全ParameterのStart / End Base値を一度計算する。

推奨Scratch：
```rust
pub struct ParameterSpanValue {
    pub start: f32,
    pub end: f32,
}
```
値はNormalized Domainで保持する。

Target適用時にNative Unitへ変換する。

Parameter数が小さいため、全Parameter分をSpanごとに更新してよい。

将来Parameter数が増えて問題が実測された場合にDirty Setを検討する。
## 9.4 Smoother Advance
`frames = N`のSpanについて、StateをN Frame進める。

Startは進行前のCurrent、EndはN Frame進行後のCurrentとする。

Span生成時に次のSmoother完了位置を境界へ含めるため、Ramp中のSpanがTarget到達位置を跨がない。

安定状態ではStartとEndが同じ値になる。

Voiceごとに同じSmootherを進めない。
## 9.5 External Control State
Process Eventへ次を追加する。
```rust
ProcessEventKind::PitchBend { value: f32 }
ProcessEventKind::ModWheel { value: f32 }
ProcessEventKind::Aftertouch { value: f32 }
```
Instrument Runtimeは次を保持する。
```rust
pub struct ExternalControlState {
    pub pitch_bend: Smoother,
    pub mod_wheel: Smoother,
    pub aftertouch: Smoother,
}
```
Range：
- Pitch Bend：-1〜1
- Mod Wheel：0〜1
- Aftertouch：0〜1
Event適用時に各ControlのTarget値を更新し、短いSmoothingを開始する。

初期値はPitch Bend、Mod Wheel、Aftertouchとも5 msとし、一か所の定数で管理する。

External Control StateもInstrument Runtimeが一度だけ進め、VoiceごとにSmoothingしない。
## 9.6 Reset
Reset時：
- Base ParameterをCompiled Defaultへ戻す
- Parameter Smootherを停止する
- Pitch Bendを0へ戻す
- Mod Wheelを0へ戻す
- Aftertouchを0へ戻す
- Control Clockを0へ戻す
- 全Voice Source StateをResetする
- 全VoiceのRandom値を破棄する
同じEvent列を再Renderした場合、同等出力を得る。
## 9.7 Prepare
PrepareはParameter CountとVoice数に応じてScratch容量を確保する。

Prepare失敗時は以前のPrepared Stateを利用可能なまま残さない。

既存のDSP Prepare失敗無効化方針に合わせる。

---

# 10. Control-rateとEvent順序
## 10.1 Control Quantum
内部定数として32 Frameを使用する。
```text
CONTROL_QUANTUM_FRAMES = 32
```
Definition Fieldにはしない。

48 kHzで約0.67 ms、44.1 kHzで約0.73 msである。

LFO、Envelope、Effective TargetのStart / Endをこの境界で更新する。
## 10.2 Absolute Control Clock
Quantum境界はProcess Block先頭ではなくAbsolute Frame 0を基準にする。
```text
next_boundary = ((absolute_frame / 32) + 1) × 32
```
Block Sizeが64、257、1024でも同じAbsolute FrameでSourceを評価する。
## 10.3 Offline Event Canonicalization
同一Sample Offsetでは次の順序を使用する。
1. Note Off
2. Parameter Change
3. Pitch Bend
4. Mod Wheel
5. Aftertouch
6. Note On
`ProcessEventKind::priority()`へ規則を一か所に置き、OfflineのCLI / MIDI Adapterが同一OffsetのEventを正規化するために利用する。Coreの`ProcessBlock::validate_for`は同じOffsetのEventを入力順で受け入れる。
理由：
- Existing Noteを先にReleaseへ移す
- 新しいBase / External Controlを同じSampleから反映する
- 新Noteは更新後の値で開始する
同種Eventは入力順を維持する。

同じParameterまたはExternal Controlへ同一Offsetで複数Eventがある場合、最後のEventが最終Targetになる。

CLI / MIDI Adapterはこの順序でOfflineのScheduled Eventを安定Sortする。
## 10.4 Event Validation
Process開始前にBlock内全EventをValidationする。

`ProcessBlock::validate_for`は、ProcessSpecだけで判定できる次を確認する。
- Sample Offset昇順
- Offset < frames
- 同一Offsetの入力順
- Note Number / Velocity Range
- Normalized Value Range
- External Control Range
- 全値finite

その後、`InstrumentRuntime`がCompiled Instrumentを使ってParameter Handle Rangeを確認する。

CatalogをGenericな`ProcessBlock`へ渡す設計にはしない。

二段階のValidationをどちらもState変更前に完了し、途中Eventで失敗してBlock前半だけStateが進む状態を避ける。

Validation失敗時：
- Output対象範囲を無音にする
- Parameter Stateを変更しない
- External Control Stateを変更しない
- Voice Stateを変更しない
- Process Errorを返す
## 10.5 Zero-frame Block
`frames = 0`はNo-opとして成功する。

Eventが含まれている場合はInvalidとする。
## 10.6 Span生成
Block内で次の境界までのFrame数を求める。
```text
min(
  block_end,
  next_event,
  next_quantum,
  next_shared_smoother_completion
)
```
共有Spanを生成後、Voiceは自分のEnvelope SegmentやSteal Fade境界でSubspanへ分割できる。

Shared Stateは共有Span全体で一度だけ進める。
## 10.7 Note OnがQuantum途中にある場合
Note On EventでVoice Source Stateを初期化する。

新VoiceはNote On Sample時点のShared Parameter値を使用する。

LFO PhaseはDefinition Phaseから開始する。

Modulation EnvelopeはAttack先頭から開始する。

RandomはNote On時に生成する。

次Quantumまでの残りFrameだけを最初のVoice Source Spanとして評価する。
## 10.8 Voice Stealing途中のPending Note
Steal Fade完了が共有Span途中の場合：
1. 旧VoiceをFade完了位置までRender
2. Shared Spanの該当Offset値を補間して取得
3. Pending Noteを同じAbsolute Frameで開始
4. 新Voice Sourceを初期化
5. 共有Span残り区間を新NoteでRender
Shared Parameter StateとExternal Control Stateを再Advanceしない。
## 10.9 Tempo
P3 SourceはTempoを使用しない。

`ProcessContext.tempo_bpm`は既存Contractとして保持する。

Tempo Sync LFOを先回りして実装しない。

---

# 11. Modulation Source
## 11.1 Source出力Range
| Source | Range | Scope | 更新 |
|---|---|---|---|
| Velocity | 0〜1 | Voice | Note On時固定 |
| Key Tracking | -1〜1 | Voice | Note On時固定 |
| LFO | -1〜1 | Voice | Control Span |
| Envelope | 0〜1 | Voice | Control Span / Segment境界 |
| Random | -1〜1 | Voice | Note On時固定 |
| Pitch Bend | -1〜1 | Instrument | Event |
| Mod Wheel | 0〜1 | Instrument | Event |
| Aftertouch | 0〜1 | Instrument | Event |
## 11.2 Velocity
```text
value = velocity / 127
```
Velocity 1〜127を使用する。

Note On Velocity 0はAdapter側でNote Offへ変換する。
## 11.3 Key Tracking
```text
value = note_number / 127 × 2 - 1
```
MIDI Note 0を-1、127を+1とするBipolar Sourceである。

特定のCenter Noteやoctave単位の追従率はP3で追加しない。

必要な効果量はBase値とRoute Amountで調整する。
## 11.4 LFO
LFO Phaseは`0..1`で保持する。
```text
phase_increment = rate_hz / sample_rate
```
Span Start / EndのPhaseからSource値を計算する。

PhaseはSpan経過Frame数で進め、Block境界でResetしない。
### Sine
```text
value = sin(2π × phase)
```
### Triangle
```text
value = 1 - 4 × abs(phase - 0.5)
```
Phaseは`phase.fract()`へWrapする。

一つのSpan内でTriangleの頂点`phase = 0.5`またはWrap境界`phase = 1.0`を跨ぐ場合、その位置をVoice Subspan境界へ追加する。

SineはControl SpanのStart / End値を線形補間する。

40 HzはValidation上限とするが、Reference Instrumentは20 Hz以下を使用する。
## 11.5 Modulation Envelope
State：
```text
Idle
Attack
Decay
Sustain
Release
```
Amplitude ADSRと同じ遷移規則を使用する。
- Note On：Attack
- Note Off：現在値からRelease
- 0秒Segment：即時次State
- Release完了：Idle
Envelope Runtimeは、現在Stateについて次のSegment境界までのFrame数を返せるようにする。

Envelope Segment完了位置をVoice Subspan境界へ追加し、一つのSubspan内でStateを跨がない。

Subspanについて進行前のStart値と、指定Frame進行後のEnd値を数式から計算してStateを更新する。

End値を得るために`next_sample()`をFrame数分繰り返さない。

Target適用時はStart / End間を線形補間する。

Curve計算HelperはAmplitude ADSRと共有してよいが、State Objectは共有しない。
## 11.6 Random
Random値は決定的な整数Mixから生成する。

入力：
- Random Source Definitionの`seed`
- Note ID
- Source IDのStable Hash
Layer IDやVoice Pool IndexをSeedへ使わない。

Voice Stealingで異なるVoice Slotへ割り当てられても、同じNote IDとSourceなら同じ値になる。

生成規則を次に固定する。
1. Source IDをFNV-1a 64-bitでHashする
2. Source Seed、Note ID、Source ID HashをXORでまとめる
3. SplitMix64のFinalizerを一回適用する
4. 上位24 bitを`0..1`へ変換する
5. `value × 2 - 1`でBipolar化する
使用する定数と手順はTestで固定する。

```rust
fn splitmix64_finalizer(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
```

Source ID HashはFNV-1a 64-bitのOffset Basis `0xcbf2_9ce4_8422_2325`とPrime `0x0000_0100_0000_01b3`を使用する。

この処理はCore内の小さなPure Functionとして実装する。

標準Libraryの実行ごとにSeedが変わるHasherや外部PRNG Dependencyへ依存しない。

状態を更新し続けるPRNGは不要である。
## 11.7 External Control
Instrument Runtimeが次を保持する。
- Pitch Bend：-1〜1
- Mod Wheel：0〜1
- Aftertouch：0〜1
値は通常のRoute規則でTargetへ適用する。

Pitch Bendだけを特別なTuning処理へ直結しない。

Pitch Bendの可動幅はTuning TargetへのRoute AmountでDefinitionに明示する。

これによりPitch BendをFilterやPanへ利用する場合も同じ仕組みを使える。

MIDI Channelごとに独立したControl StateはP3へ含めない。

CLIのMIDI Adapterは、読み込んだChannel Controlを一つのInstrument Scopeへ時系列順に統合する。

複数Channelで異なるPitch Bend等を同時利用するMIDIは保証対象外とし、検出した場合はWarningを返す。
# 12. Route評価
## 12.1 Targetごとの評価
各Voice、各Control Spanで、利用するTargetだけを評価する。

Target Start：
1. Shared Base StartをNative値へ変換
2. 各Route Source Startを取得
3. CurveとAmountを適用
4. 加算
5. RangeへClamp
Target Endも同じ手順で計算する。
## 12.2 Effective Scratch
VoiceはParameter Handle順の全値を持たず、実際にVoice DSPで使用するTargetだけのScratchを持ってよい。

推奨形：
```rust
pub struct VoiceTargetSpan {
    pub layer_gain: Box<[Span<f32>]>,
    pub layer_pan: Box<[Span<f32>]>,
    pub layer_tuning: Box<[Span<f32>]>,
    pub filter_cutoff: Option<Span<f32>>,
    pub filter_resonance: Option<Span<f32>>,
}
```
Layer数はCompile時に確定する。

汎用Dynamic MapをVoiceごとに持たない。
## 12.3 Clamp
複数Routeの合計後に一度Clampする。

RouteごとにClampしない。

これにより正負Routeが互いに相殺できる。

非有限値が発生した場合はProcess Errorとし、対象Spanを無音にする。

Compile済み値とSource Rangeから通常は非有限値が出ない設計にする。
## 12.4 Routeなし
RouteがないTargetはShared Base Start / Endだけを利用する。

Sourceが定義されていてもRouteがなければ評価しない。

Unused SourceはCompile Warningにしない。

将来の編集途中Definitionを不必要に拒否しないためである。
## 12.5 Disabled Layer
Definitionで`enabled = false`のLayerはRuntime Voice Layerを作らない既存方針を維持してよい。

Catalog IDを保持する場合でもDSP Target評価はSkipする。

Missing AssetによりSample GeneratorがDisabledでも、同LayerにOscillator Generatorは存在しないため発音しない。

Parameter Change EventはStateとして受け付けるが音声には影響しない。

---

# 13. Target適用
## 13.1 Layer Gain
Base値とModulationはdB Domainで計算する。

Effective Start / End dBをLinear Gainへ変換する。
```text
linear = 10 ^ (db / 20)
```
Span内はLinear Gainを線形補間する。

Amplitude ADSR、Dynamic Gain、発音開始Fadeの順で乗算する。
```text
generator
  × amplitude_envelope
  × dynamic_gain
  × note_start_fade
```
既存の`gain_smoother`を発音開始FadeとBase Parameter Smoothingの両方へ使わない。

Note Start FadeはLayer Runtime固有Stateとして残す。
## 13.2 Layer Pan
Effective Pan Start / Endを-1〜1で得る。

両端でConstant-power Gainを計算する。
```text
angle = (pan + 1) × π / 4
left = cos(angle)
right = sin(angle)
```
Span内はLeft / Right Gainを線形補間する。

端点はConstant-powerを満たす。

Span内部の厳密なPower一定のためにSampleごとの三角関数計算は導入しない。

32 Frame Spanで聴感上問題がないことをReviewする。
## 13.3 Layer Tuning
Base TuningとRoute Deltaはcentで加算する。
```text
effective_cents = clamp(base_cents + route_deltas)
ratio = 2 ^ (effective_cents / 1200)
```
### Oscillator
MIDI Note FrequencyへTuning Ratioを乗算する。

Span Start / End FrequencyをNative Oscillatorへ渡す。

内部DSP APIを次へ拡張する。
```rust
pub fn process_ramp(
    &mut self,
    start_frequency_hz: f32,
    end_frequency_hz: f32,
    output: &mut [f32],
) -> Result<(), DspError>;
```
Native側で各SampleのFrequencyをLog Domainで補間する。

Frequencyは正でfinite、Nyquist未満であることを確認する。

NoteとTuningの組み合わせで安全な上限を超えた場合は、`sample_rate × 0.45`へClampしてProcessを継続する。

Clamp規則をRust / Nativeで食い違わせず、Rust側で確定した値をNativeへ渡す。
### Sample
Playback Ratio Start / Endを求める。
```text
ratio = 2 ^ ((note - root_note + effective_cents / 100) / 12)
```
Span内はLog Domainで補間する。
```text
ratio_at_t = start_ratio × (end_ratio / start_ratio) ^ t
```
Positionは各Sampleで正のRatioを加算する。

逆再生は行わない。
## 13.4 Sample終端
Dynamic Ratioでは「残りOutput Frame数」をNote On時の固定Ratioから事前計算しない。

終端FadeはSource Position基準で行う。
```text
fade_end = max(source_len - 1, 0)
fade_length = min(fade_end, round(sample_rate × 0.005))
fade_start = fade_end - fade_length

next_position = position + current_ratio

if fade_length == 0:
  gain = 0
else:
  gain = clamp((fade_end - next_position) / fade_length, 0, 1)
```
Fade領域外ではGain 1を使用する。

`next_position`を使うことで、終端を跨ぐ最後のOutput Sampleを0へ近づける。

Ratioが変化してもPositionは単調増加し、Fade判定が前の領域へ戻らない。

Short Sampleでも範囲外参照せず、Finished後は必ず0を返す。
## 13.5 Voice Filter
CutoffはLog2 Scaleで評価する。

ResonanceはLinear Scaleで評価する。

Native APIを次へ拡張する。
```rust
pub fn process_ramp(
    &mut self,
    start_cutoff_hz: f32,
    end_cutoff_hz: f32,
    start_resonance: f32,
    end_resonance: f32,
    buffer: &mut [f32],
) -> Result<(), DspFilterError>;
```
Left / Right Filterへ同じParameter Spanを渡す。

Filter Stateは左右独立のまま維持する。

Route評価後のCutoffはParameter RangeへClampし、DSP適用直前に`effective_max_cutoff_hz`へ再度制限する。

Base値がProcess安全上限を超える場合は既存どおりCompile Warningを返す。

Modulation中の安全Clampを毎回Warningにはしない。

FilterなしInstrumentではFilter RouteをCompile Errorにする。
## 13.6 Voice開始
新VoiceはEvent時点のShared Base値とExternal Control値を使用する。

Velocity、Key Tracking、Randomを初期化する。

LFOとModulation Envelopeを開始する。

Voice開始直後からRouteを評価する。

従来のVelocity専用Gain / Cutoff計算を残して二重適用しない。
## 13.7 Note Off
Amplitude ADSRとModulation EnvelopeへNote Offを伝える。

LFOとRandomはVoice終了まで継続する。

External Controlは共有Stateとして継続する。

Release中もParameter ChangeとModulationを反映する。
## 13.8 Voice Stealing
Steal Fade中の旧Voiceも現在のParameter / Modulationを反映する。

Pending NoteのSource StateはFade完了まで作らない。

Pending NoteがNote Offされた場合は既存挙動どおりCancelする。

Fade完了後の新Noteは、そのAbsolute Frame時点のShared値で開始する。
## 13.9 Voice終了
Voice終了時に次をResetする。
- Voice Source State
- Modulation Envelope
- LFO Phase
- Random値
- Effective Target Scratch
Shared Parameter StateとExternal Control StateはResetしない。

Instrument Reset時だけ共有StateをDefaultへ戻す。

---

# 14. Diagnosticsと失敗時の挙動
## 14.1 Definition / Compile Error
次はAudio処理開始前に検出する。
- 不正なLayer ID
- 不正なSource ID
- Source ID重複
- 存在しないSource
- 存在しないTarget
- FilterなしInstrumentへのFilter Target
- Catalogに存在しないParameter
- 非有限値
- Amount、LFO Rate、Phase、Envelope値のRange違反
Errorがある場合はCompiled Instrumentを返さない。

一部Routeだけを黙って無効化して処理を継続しない。
## 14.2 Warning
既存のAsset Warningは維持する。

MIDI Adapterでは、複数Channelの異なるInstrument Scope Controlを一つへ統合した場合にWarningを返す。

Runtime Clamp回数を管理する追加機構は作らない。
## 14.3 Process Event Error
Parameter Change / External Control EventはBlock処理前に全件Validationする。

不正Eventが一つでもある場合：
- 対象Output範囲を無音にする
- Parameter Stateを変更しない
- External Control Stateを変更しない
- Voice Stateを変更しない
- Process Errorを返す
Blockの途中まで処理した後で失敗しない。
## 14.4 DSP Error
Oscillator / Filter Native処理が失敗した場合は既存方針を維持する。
- 対象Bufferを無音化
- ErrorをRustへ返す
- C++例外を境界外へ出さない
- 失敗後に古いPrepared Stateを有効として扱わない
Dynamic Parameter対応のためにNative Error処理を弱めない。

---

# 15. CLIとMIDI Adapter
## 15.1 `instrument inspect`
既存出力へ次を追加する。
### Parameter一覧
各Parameterについて表示する。
- ID
- Unit
- Min / Max
- Default
- Scale
- Smoothing
Text Mode例：
```text
parameter layer.body.gain:
  unit: decibels
  range: -60.0 .. 12.0
  default: -16.0
  scale: linear
  smoothing: 0.005 s
```
JSON Modeでは同じ情報を構造化して返す。
### Modulation一覧
- Source ID
- Source種類
- Scope
- Source設定
- Target ID
- Amount
- Curve
Inspect側でParameter IDやRouteの意味を再計算しない。

Compiled InstrumentのCatalog / Source / Route情報を利用する。
## 15.2 Event Sequence Render
Parameter Changeを再現可能に検証するため、次を追加する。
```bash
sonalloy render events \
  <definition.json> \
  <events.json> \
  --sample-rate 48000 \
  --block-size 257 \
  --duration-frames 192000 \
  --tail 1.0 \
  --output out.wav
```
Event Sequence FileはAbsolute Frameを使用する。

概念例：
```json
{
  "events": [
    {
      "absolute_frame": 0,
      "type": "parameter_change",
      "parameter": "voice.filter.cutoff",
      "normalized": 0.35
    },
    {
      "absolute_frame": 0,
      "type": "note_on",
      "note_id": 1,
      "note": 60,
      "velocity": 100
    },
    {
      "absolute_frame": 24000,
      "type": "mod_wheel",
      "value": 1.0
    },
    {
      "absolute_frame": 48000,
      "type": "note_off",
      "note_id": 1
    }
  ]
}
```
CLIはRender開始前にParameter IDをHandleへ解決する。

Coreへ文字列Parameter IDを含むEventを渡さない。

Event Fileの構造をInstrument Definitionへ混ぜない。
## 15.3 Event Sequence Validation
CLI側で次を検査する。
- JSON構造
- Absolute Frame昇順
- Render Duration内
- Parameter IDの実在
- Normalized Value Range
- Note / Velocity Range
- External Control Range
- Note IDの型
同一FrameのEventはOffline AdapterのCanonical順へ安定Sortする。

不正Eventで一部WAVを成功扱いしない。
## 15.4 MIDI Pitch Bend
`midly::MidiMessage::PitchBend`の14-bit値を-1〜1へ変換する。

```text
raw < 8192:
  value = (raw - 8192) / 8192

raw >= 8192:
  value = (raw - 8192) / 8191
```

Raw 0を-1、8192を0、16383を+1へ対応させる。

Coreへ`PitchBend` Eventとして渡す。
## 15.5 MIDI Mod Wheel
Controller 1をMod Wheelとして扱う。
```text
value = controller_value / 127
```
Coreへ`ModWheel` Eventとして渡す。

CC1以外の未対応Controllerは既存Warning方針を維持する。

Sustain PedalはP3対象外のままとする。
## 15.6 MIDI Channel Aftertouch
`ChannelAftertouch`を0〜1へ変換する。

Polyphonic AftertouchはP3対象外としWarningを返す。
## 15.7 MIDI Channelの扱い
Sonalloy CoreはP3でMIDI ChannelをDomain Modelへ追加しない。

CLIが一つのMIDI Fileを一つのInstrumentへRenderする際、Channel ControlをInstrument Scopeへ統合する。

次の条件ではWarningを返す。
- 複数ChannelでNoteが発音している
- 同じ時間帯にChannelごとに異なるPitch Bendが存在する
- Channelごとに異なるMod Wheel / Aftertouchが存在する
MPEやMulti-timbral動作として正しく再現したとは扱わない。

Channel対応を正確にするためだけにVoiceへMIDI Channelを恒久的に追加しない。
## 15.8 CLI結合テスト
- `instrument inspect`にParameter一覧
- `instrument inspect --json`がParse可能
- Source / Route表示
- `render events`でParameter Change
- `render events`でExternal Control
- 不正Parameter ID
- 不正Normalized値
- Event順序
- MIDI Pitch Bend
- MIDI CC1
- MIDI Channel Aftertouch
- Polyphonic Aftertouch Warning
- 複数Channel Control Warning
- Unsupported Controller Warning
- Output WAV生成
- Exit Code

---

# 16. テスト戦略
## 16.1 基本方針
P3は横断変更であるため、新機能のTestだけでなく既存音声処理のRegressionを必須とする。

Test Frameworkは現在のRust標準Testと既存Dev Dependencyを利用する。

新しい大規模Test Frameworkは追加しない。

Unit Testは対象Moduleと同居させる。

Public APIのEnd-to-End TestだけをCrateの`tests/`へ置く。
## 16.2 Parameter Contract Test
- Valid ID
- 空ID
- 大文字
- 空白
- `.`先頭 / 末尾
- 連続`.`
- Layer IDからCanonical ID生成
- Normalize / Denormalize往復
- Linear Scale境界
- Log2 Scale境界
- 非有限値
- Range外
- Catalog順序
- FilterなしCatalog
- Disabled Layer
- Missing Asset Layer
- IDからHandle Lookup
- Handle範囲外Event
## 16.3 Definition / Compiler Test
- Modulation省略
- Source 0件 / Route 0件
- Built-in Source Route
- LFO Source
- Envelope Source
- Random Source
- Source ID重複
- Reserved ID重複
- Source不存在
- Target不存在
- Filter不存在Target
- Discrete Field Target
- Amount境界
- Curve
- TargetごとのRoute Range
- Definition順の加算順
- Error複数収集
- Error時Compiledなし
- Warning時Compiledあり
- Existing Exampleの一括更新後Compile
## 16.4 Runtime Parameter Test
- Default初期化
- Parameter Change Offset 0
- Block途中のParameter Change
- Block末尾直前
- Smoothing 0 Frame相当
- 5 ms / 10 ms Smoothing
- Smoothing途中の再変更
- 複数Parameter同時変更
- ResetでDefault復帰
- Invalid EventでState不変
- 0 Frame Process
- Shared StateをVoice数分進めない
Shared Stateの重複更新Testでは、同じEvent列をVoice数1とVoice数8で処理し、Base Parameter Stateの進行位置が一致することを確認する。
## 16.5 Event / Span Test
- Eventなし
- Quantum境界
- Quantum途中Event
- 同一Offset Note Off / Control / Note On
- 同種Event入力順
- Smoother完了境界
- Envelope Segment境界
- Triangle折返し
- Steal Fade完了境界
- Block末尾の短いSpan
- Block Size 64 / 257 / 1024
- Absolute Frame不連続を拒否する既存規則がある場合は維持
## 16.6 Source Test
### Velocity
- 1
- 64
- 127
- Note On velocity 0のAdapter変換
### Key Tracking
- Note 0 → -1
- Note 127 → +1
- 中央付近
- Range内
### LFO
- Phase 0
- Phase Wrap
- Sine四分点
- Triangle頂点 / 谷
- 折返し跨ぎ
- Block Size独立
- 44.1 / 48 / 96 kHz
### Modulation Envelope
- 0秒Segment
- Attack / Decay / Sustain / Release
- Attack中Note Off
- Release中Parameter Change
- Voice Stealing
- Reset
- Amplitude ADSRとState非共有
### Random
- 同じSeed / Note ID / Source IDで同じ値
- Seed違い
- Note ID違い
- Source ID違い
- Voice Slot違いで同じ値
- Reset後再現
- -1〜1
### External Control
- Pitch Bend -1 / 0 / +1
- Mod Wheel 0 / 1
- Aftertouch 0 / 1
- 同一Sample Note On前のControl
- 発音中更新
- Release中更新
- Reset
## 16.7 Route評価Test
- Route 0件
- 一Route
- 同一Target複数Route
- Positive / Negative Amount
- Linear Curve
- SmoothStep
- Unipolar Source
- Bipolar Source
- Linear Parameter
- Log2 Parameter
- Clamp下限 / 上限
- Modulation結果をBaseへ書き戻さない
- 加算順固定
- Disabled Layer Target
- RouteなしTargetのFast Path
## 16.8 Dynamic Target Test
### Gain
- dBからLinear
- Parameter Change Ramp
- Velocity Route
- LFO Route
- Note Start Fadeとの乗算
- Release中変更
- 明確なClickなし
### Pan
- -1 / 0 / +1
- Constant-power
- Ramp途中のPower
- Random Pan
- LFO Pan
- Stereo finite
### Tuning / Oscillator
- 0 cent
- ±100 cent
- ±1200 cent
- Pitch Bend Route
- Frequency Ramp
- Nyquist上限
- Native Error時無音
- Phase Reset既存挙動
### Tuning / Sample
- Playback Ratio 1
- Ratio上昇 / 下降
- Ramp中Cursor単調増加
- Short Sample
- 終端Fade
- 低Ratio / 高Ratio
- 範囲外参照なし
- Finished後無音
### Filter
- Cutoff Linearなし、Log2 Ramp
- Resonance Ramp
- Left / Right同Parameter
- State独立
- FilterなしInstrument
- Cutoff上限
- Native Error
## 16.9 Native Ramp Test
### Oscillator
- Empty Buffer
- 1 Frame
- Start = Endで既存`process`と同等
- Frequency上昇 / 下降
- Phase連続
- Invalid Frequency
- Error時Buffer Clear
- Prepare失敗後NotPrepared
### Filter
- Empty Buffer
- 1 Frame
- Cutoff Start = End
- Resonance Start = End
- Cutoff Ramp
- Resonance Ramp
- Cutoff + Resonance同時Ramp
- Left / Right独立State
- Invalid値
- Error時Buffer Clear
Native Testは既存のFFI TestとSanitizer方針へ追加する。新しいC++ Test Frameworkは導入しない。
## 16.10 Core結合テスト
- ModulationなしBasic Poly Synth
- ModulationなしMetallic Hybrid
- Moving Hybrid Pad
- Expressive Hybrid Lead
- Chord
- Release重なり
- Voice Stealing
- Parameter Change中のChord
- Pitch Bend中のSample + Oscillator
- Mod Wheel Filter
- Aftertouch Gain / Filter
- Random Voice Pan
- Block Size 64 / 257 / 1024
- Reset後再Render
- Missing Asset
- WarningのみCompile
- 44.1 / 48 / 96 kHz
## 16.11 Regression基準
ModulationなしInstrumentでは、意図して変更した箇所以外の既存挙動を維持する。

比較する項目：
- Frame数
- Note開始 / Note Off Frame
- Voice Allocation
- Voice Stealing選択
- ADSR Timing
- Sample終端
- Peak / RMS / DC
- finite
- Block Size独立
- Reset再現性
Definition形式の変更自体はRegression対象ではない。

更新後Definitionを基準に、音声処理の不要な変化を検出する。

完全Bit一致が難しいDSP変更では許容誤差を明示する。
## 16.12 Realtime Safety確認
自動TestまたはCode Reviewで次を確認する。
- Process中のFile I/Oなし
- JSONなし
- String生成なし
- HashMap Lookupなし
- Route探索なし
- 継続的なVec容量拡張なし
- Blocking Mutexなし
- Native Heap Allocationなし
- Panic経路なし
- Event Error時のState不変
本格Benchmarkは完了条件にしない。

ただし明らかなVoice数倍のShared State更新はTestで検出する。

---

# 17. 音声確認
## 17.1 自動確認
固定条件のWAVについて次を測定する。
- Frame数
- Channel数
- finite
- Peak
- RMS
- DC Offset
- 大きなSample間不連続
- 基本周波数
- Pitch Bend後の周波数
- Block Size差
- Reset後差
- Sample終端
Spectrumは参考情報として利用してよい。

単一のSpectrum閾値だけで音質合格としない。
## 17.2 Review用音声
最低限、次を生成する。
### 01-parameter-cutoff.wav
発音中にFilter Cutoff Base値を変更する。

確認：
- Event位置
- Smoothing
- Click
- 変化量
### 02-lfo-filter.wav
LFOからFilter Cutoff。

確認：
- 周期
- 滑らかさ
- Block Size差
### 03-envelope-pitch.wav
Modulation EnvelopeからOscillator / Sample Tuning。

確認：
- Attack時のPitch変化
- Decay
- Note Off後のRelease
- SampleとOscillatorの一致
### 04-random-pan.wav
同時発音NoteへRandom Pan。

確認：
- Noteごとの差
- 同じ入力での再現
- 極端な偏り
### 05-external-controls.wav
- Pitch Bend
- Mod Wheel
- Aftertouch
確認：
- 発音中反映
- Smoothing
- Targetごとの意図
### 06-voice-stealing.wav
Modulation中にPolyphony上限を超える。

確認：
- Steal Fade
- Pending Note開始
- LFO / Envelope初期化
- Click
### 07-musical-phrase.wav
Moving Hybrid PadまたはExpressive Hybrid Leadで4〜8小節。

確認：
- Modulationが音色として有効か
- SampleとOscillatorの一体感
- 過度な揺れやPitch不安定がないか
- 実際に使いたい音か
## 17.3 人間の確認項目
- Parameter Changeに明確なClickがないか
- LFOが階段状に聞こえないか
- Envelope Pitchが不自然に飛ばないか
- Pitch BendでSampleとOscillatorがずれないか
- Pan変化が不自然に音量低下しないか
- Resonance変化が発散しないか
- Voice Stealing中にModulationが破綻しないか
- Randomが毎回変わるのではなく再現できるか
- Reference Instrumentが技術Demoだけになっていないか
自動Test成功だけで音質を承認しない。

---

# 18. Repository変更
## 18.1 `sonalloy-core`
主な変更対象：
```text
crates/sonalloy-core/src/
├─ lib.rs
├─ definition.rs
├─ compiler.rs
├─ process.rs
├─ diagnostics.rs
├─ render.rs
└─ runtime/
   ├─ instrument.rs
   ├─ voice.rs
   ├─ smoothing.rs
   ├─ sample.rs
   ├─ mix.rs
   └─ adsr.rs
```
現在の実ファイル構成を優先し、計画へ名前を合わせるだけの大規模移動は行わない。

Parameter / Modulation責務が一Fileへ集中しすぎる場合は、次を追加してよい。
```text
parameter.rs
modulation.rs
runtime/modulation.rs
```
細かい型ごとにFileを分割しない。
## 18.2 `sonalloy-dsp-sys`
主な変更対象：
```text
crates/sonalloy-dsp-sys/src/
├─ ffi.rs
├─ oscillator.rs
└─ filter.rs
```
追加するNative能力：
- Oscillator Frequency Ramp
- Filter Cutoff + Resonance Ramp
Public Product ABIではなく、既存の内部DSP境界だけを拡張する。
## 18.3 Native Wrapper
既存DaisySP WrapperへSpan単位Ramp処理を追加する。
- C++例外捕捉
- Null / Argument Validation
- Error時Buffer Clear
- Process中Allocationなし
DaisySP本体を改変せずWrapper側で吸収する。
## 18.4 CLI
```text
crates/sonalloy-cli/src/
├─ main.rs
└─ midi.rs
```
必要に応じてEvent Sequence Parseを独立Fileへ分けてよい。

一つのCommand実装だけのために新Crateを追加しない。
## 18.5 Example / Test Data
- Basic Poly Synth Definition
- Metallic Hybrid Definition
- Moving Hybrid Pad Definition
- Expressive Hybrid Lead Definition
- Event Sequence Fixture
- MIDI Control Fixture
- Review MIDI
- Expected Metrics
外部音声Assetを新規追加する場合は既存の権利確認方針を維持する。

---

# 19. 実装順序
P3は一つの実装単位である。

以下は別Phaseではなく、同じ変更を安全に完成させるための着手順である。

各段階の終了時にTestを通し、未完成の仮Contractを後段で全面変更する前提にしない。
## 19.1 Definition・Parameter Catalog・Compiler
### 目的
保存形式とCompile後のParameter参照を先に固定する。
### 主な変更
- `velocity_response`削除
- Modulation Definition追加
- Canonical Parameter ID
- Descriptor / Catalog
- Handle解決
- Source / Route Compile
- Diagnostics
- Example / Fixture更新
### 実装順
1. Layer ID文字制約を定義する
2. Parameter ID生成Helperを実装する
3. Unit / Scale / Descriptorを実装する
4. Catalog生成を実装する
5. IDからHandleへのControl側Lookupを実装する
6. Modulation Definitionを追加する
7. Built-in Source IDを定義する
8. LFO / Envelope / Random Definitionを追加する
9. Route Validationを実装する
10. Source / Target解決をCompilerへ追加する
11. Target別Route Rangeを作る
12. Compiled Source / Routeを追加する
13. Existing Definitionを一括更新する
14. Inspect用公開情報を追加する
15. Unit / Compiler Testを完了する
### 完了条件
- ModulationなしDefinitionをCompileできる
- ModulationありDefinitionをCompileできる
- ID / Source / Target ErrorがPath付きで返る
- Process中に文字列解決が不要
- Existing Exampleが更新済み
## 19.2 Shared Parameter State・Event・Span
### 目的
発音中のBase Parameter変更をSample Accurateに反映する。
### 主な変更
- Runtime Parameter State
- Parameter Change Event
- External Control Event
- Offline Event Canonicalization
- Shared Span
- Smoothing
- Absolute Control Clock
- Error時State不変
### 実装順
1. Existing SmootherへSpan Advanceを追加する
2. Smoother完了Frameを境界として取得できるようにする
3. Shared Parameter StateをInstrument Runtimeへ追加する
4. Default初期化を実装する
5. Parameter Change Eventを追加する
6. External Control Eventを追加する
7. Event全件事前Validationを実装する
8. Offline Event Canonicalizationを実装する
9. Absolute Quantum境界を実装する
10. Shared Parameter Spanを一度だけ生成する
11. VoiceへRead-only Spanを渡す
12. Resetを実装する
13. Block Size / Voice数独立Testを完了する
### 完了条件
- 発音中にBase Parameterを変更できる
- Smoothing開始FrameがEvent位置と一致する
- Shared StateをVoiceごとに進めない
- Invalid EventでStateが変化しない
- Reset後に同じ結果を得る
## 19.3 Dynamic Target適用
### 目的
現在の固定値DSPをParameter Spanへ置き換える。
### 主な変更
- Layer Gain
- Layer Pan
- Layer Tuning
- Oscillator Frequency Ramp
- Sample Playback Ratio Ramp
- Sample End Fade
- Filter Cutoff / Resonance Ramp
- Voice Lifecycle統合
### 実装順
1. Layer Runtimeから固定Pan Gainを外す
2. Layer GainのBase値参照をShared Spanへ移す
3. Note Start FadeをDynamic Gainと分離する
4. Pan Angle Rampを実装する
5. Tuning CentsからRatio Spanを作る
6. Oscillator Native Frequency Rampを追加する
7. Sample RuntimeへRatio Rampを追加する
8. Sample終端FadeをSource Position基準へ変更する
9. Filter Native RampをCutoff + Resonanceへ拡張する
10. Voice FilterへEffective Spanを渡す
11. Note On / Note Off / Releaseへ接続する
12. Voice Stealing Subspanへ接続する
13. Dynamic Target TestとRegressionを完了する
### 完了条件
- 5 Targetすべてが発音中に変化する
- Oscillator / Sample Pitchが一致する
- Sample Cursorが逆行しない
- PanがConstant-power
- Filter Stateを壊さない
- Modulationなし音声に不要な退行がない
## 19.4 Modulation Source・Route評価
### 目的
Voice固有SourceとInstrument共有SourceをTargetへ加算する。
### 主な変更
- Velocity / Key Tracking
- LFO
- Modulation Envelope
- Random
- Pitch Bend / Mod Wheel / Aftertouch
- Route評価
- Clamp
- Voice Source Lifecycle
### 実装順
1. Built-in Source値取得を実装する
2. Voice Source State配列を作る
3. Velocity / Key Trackingを初期化する
4. LFO PhaseとSpan評価を実装する
5. Triangle折返し境界を実装する
6. Modulation Envelopeへ`frames_until_segment_end`とSpan Advanceを実装する
7. Envelope Segment境界をVoice Subspanへ接続する
8. Random Stable Mixを実装する
9. External Control参照を接続する
10. TargetごとのRoute加算を実装する
11. Linear / Log2評価を実装する
12. Clampを実装する
13. Note On / OffへSource Lifecycleを接続する
14. Voice Stealingへ接続する
15. Source / Route Testを完了する
### 完了条件
- 全Sourceが定義Rangeを守る
- Route複数加算が安定する
- LFO / EnvelopeがBlock Size非依存
- Randomが決定的
- Release / Steal中も破綻しない
- Base値へModulationを書き戻さない
## 19.5 CLI・MIDI・Reference Instrument
### 目的
P3機能をCLIだけで理解・再現・試聴できるようにする。
### 主な変更
- Inspect
- Event Sequence Render
- MIDI Control変換
- Reference Instrument
- Review Input
### 実装順
1. InspectへParameterを追加する
2. InspectへSource / Routeを追加する
3. Event Sequence ModelをCLIへ追加する
4. Parameter IDをRender前にHandle解決する
5. `render events`を実装する
6. MIDI Pitch Bendを変換する
7. CC1を変換する
8. Channel Aftertouchを変換する
9. Channel統合Warningを追加する
10. Moving Hybrid Padを作る
11. Expressive Hybrid Leadを作る
12. CLI結合テストを完了する
13. Review MIDI / Eventを固定する
### 完了条件
- CLIでParameter / Routeを確認できる
- Parameter ChangeをEvent Fileから再現できる
- MIDI ControlをCore Eventへ変換できる
- Reference InstrumentをRenderできる
- Unsupported MIDIの扱いが明示される
## 19.6 統合・音声確認・文書
### 目的
全機能を一つのInstrument Runtimeとして完成させ、実装と文書を一致させる。
### 実装順
1. Workspace全Testを実行する
2. Windows / LinuxでBuildとTestを確認する
3. Block Size比較を実行する
4. 44.1 / 48 / 96 kHzを確認する
5. Review WAVを生成する
6. Metricsを生成する
7. 人間が試聴する
8. 問題をDefinition調整とDSP問題へ分類する
9. 必要なRegression Testを追加する
10. 同じ入力で再生成する
11. 必要な恒久文書だけ更新する
12. Scope外機能が混入していないことを確認する
### 完了条件
- 全自動Test成功
- Review Artifact完成
- 人間の音質確認完了
- Known Limitationが記録済み
- 恒久文書と実装が一致
- P3という名称が実装や恒久文書へ残っていない

---

# 20. ドキュメント更新
文書は実装契約が変わった箇所だけ更新する。

同じ説明を複数文書へ転載しない。
## 20.1 `docs/instrument-definition.md`
更新する内容：
- `velocity_response`削除
- Modulation Block
- Source
- Route
- Parameter ID
- Amount / Curve
- 完全なDefinition例
- Validation Error
## 20.2 `docs/runtime-processing.md`
更新する内容：
- Parameter Change Event
- Offline Event Canonicalization
- Shared / Voice State
- Control Span
- Effective値
- Target Ramp
- Source Lifecycle
- Reset / Determinism
## 20.3 `docs/cli.md`
更新する内容：
- InspectのParameter / Route表示
- `render events`
- Event Sequence形式
- MIDI Pitch Bend / CC1 / Aftertouch
- Channel制約
- Warning / Exit Code
## 20.4 `docs/testing-and-sound-review.md`
更新する内容：
- P3機能名ではなくDynamic Parameter / ModulationのTest
- Block Size比較
- Random決定性
- Review WAV
- 人間の確認項目
## 20.5 `docs/architecture.md`
Parameter Catalog、Compiled Route、Runtime Stateの責務境界を理解するために必要な最小限だけ更新する。

実装計画の実装順、Test一覧、進行名称を転載しない。
## 20.6 README
通常利用者のQuick Startが変わる場合だけ、Reference Instrumentまたは`render events`への短い入口を追加する。

詳細仕様をREADMEへ複製しない。

---

# 21. 実装時の注意
## 21.1 進行名称
`P3`は本計画書内だけで使用する。

次へ残さない。
- Code
- Type
- Function
- Module
- Test名
- Fixture名
- Diagnostic
- Comment
- Example Instrument名
- README
- 恒久設計文書
既存の`docs/plan/plan-mvp.md`は完了済み履歴として変更しない。
## 21.2 過剰な抽象化を避ける
次を作らない。
- Generic Audio Graph
- SourceからSourceへのGraph
- Trait Object中心のModulation Engine
- Parameter型ごとのCrate
- Frontend Framework
- Plugin Parameter Adapter
- Runtime Hot Swap
- 不要なThread / Queue
- 将来Target用の空Descriptor
- 使用しないEffect / Generator用型
新しい抽象化を追加する場合、P3内で二つ以上の実利用箇所があることを確認する。
## 21.3 計画との差分
本書の責務や不変条件を変更する必要が出た場合、実装を続ける前に次を記録する。
- 問題
- 該当する現行Code
- 本書どおり実装できない理由
- 最小の変更案
- 音声挙動への影響
- Test変更
- 将来機能への影響
名称やFile配置の小変更だけで計画改定を要求しない。
## 21.4 作業報告
実装完了時は次をまとめる。
- 実装した機能
- 主要変更File
- Contract変更
- Test結果
- Regression結果
- Review Artifact
- Known Limitation
- 更新した恒久文書
- Scope外へ残したもの
自己評価のための長いチェックリストを作らない。

---

# 22. 全体完了条件
## 22.1 機能
- Stable Parameter ID
- Parameter Catalog
- Compile時Handle解決
- Parameter Change Event
- Gain / Pan / Tuning
- Filter Cutoff / Resonance
- Velocity
- Key Tracking
- LFO
- Modulation Envelope
- Random
- Pitch Bend
- Mod Wheel
- Aftertouch
- Route加算
- Curve
- Clamp
- Smoothing
- Event Sequence Render
- MIDI Control変換
- Reference Instrument
## 22.2 正確性
- Event位置がSample Accurate
- Offlineの同一Offset Canonical順が固定
- BaseとEffectiveが分離
- Shared StateをVoice数分進めない
- Voice Sourceが独立
- Block Size非依存
- Reset再現性
- Random決定性
- Sample Cursor Bounds-safe
- Native Error時無音
- Invalid Event時State不変
- NaN / Infinityなし
## 22.3 Regression
- Basic Poly SynthをRenderできる
- Metallic HybridをRenderできる
- Note Timingを維持
- ADSR Timingを維持
- Voice Stealing優先順位を維持
- Missing Asset部分読込を維持
- Offline Renderを維持
- CLI Validate / Inspect / Note / MIDIを維持
## 22.4 品質
- Parameter Changeに明確なClickがない
- LFOが明確に階段状でない
- Pitch BendでSample / Oscillatorが乖離しない
- Panで不自然な音量落ちがない
- Filter Modulationが発散しない
- Voice Stealing中に破綻しない
- Reference Instrumentが音色として成立する
- 人間の試聴が完了している
## 22.5 構造
- Definition / Compiled / Runtimeの責務分離
- Audio Pathで文字列Lookupなし
- Audio PathでFile / JSONなし
- Process中の継続Allocationなし
- Internal DSP ABIとProduct Interfaceを混同しない
- Realtime / Riffra / Pluginを先行実装していない
- P3名称が恒久物へ残っていない
## 22.6 ドキュメント
- Instrument Definition
- Runtime Processing
- CLI
- Testing and Sound Review
- 必要最小限のArchitecture
実装と一致している。

---

# 23. 自己レビュー
CONCEPT.mdと本Planの整合を確認する。
