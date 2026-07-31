# Sonalloy 要件定義・基本設計

## 1. 製品概要

### 1.1 コンセプト

**Sonalloy** は、音声素材と複数の音生成方式を組み合わせて、演奏可能な独自Instrumentを構築するハイブリッド音源エンジンである。CLIを第一級のインターフェースとして扱う。

名前の由来は `Son`（音）+ `Alloy`（合金）。性質の異なる素材や生成方式をLayerとして組み合わせ、単独では得られない音色を生み出す——この製品思想をそのまま名前にしている。

### 1.2 入力と出力

```
┌─────────────────────────────────────────────────┐
│                  入力（2系統）                    │
│                                                 │
│  A. 音声素材        B. 音響合成                   │
│  ・録音ファイル      ・Oscillator                 │
│  ・既存サンプル      ・FM / Wavetable             │
│  ・フィールド録音    ・Granular / Noise            │
└────────────────────┬────────────────────────────┘
                     ▼
              ┌─────────────┐
              │  Sonalloy   │
              │  Engine     │
              └──────┬──────┘
                     ▼
        ┌────────────────────────┐
        │  出力：Instrument       │
        │  ・MIDI/ノートで演奏可能 │
        │  ・定義ファイルとして保存 │
        │  ・複数Frontendから利用  │
        └────────────────────────┘
```

AとBは単独でも、複数のLayerとして組み合わせても使える。この「組み合わせ」がSonalloyの核となる価値である。

### 1.3 三つの中心価値

| # | 価値 | 具体例 |
|---|---|---|
| 1 | 任意の音を楽器化できる | ギター単音 → 音程展開 → Guitar Instrument／雨音 → Granular + Pitch + 長いEnv → Pad |
| 2 | ゼロから音を生成できる | FM → Bell／Wavetable → 動的Pad／Noise → Percussion |
| 3 | 複数方式を一つのInstrumentに融合できる | FM（金属倍音）+ Sine（低域の芯）+ Metal Hit Sample（アタック）+ Noise（質感）→ Metallic Bass |

**価値3が差別化ポイントである。** SamplerとSynthを同梱する製品は既に存在する。Sonalloyの違いは、各Generatorを単に並べるのではなく、発音条件・音量・定位・音程・Envelope・音響処理を持つ **Layer** として定義し、一つのVoice内で融合・個別編集・個別変調できる点にある。

### 1.4 エコシステム内の位置

Sonalloyは、部品としてのDSPライブラリと完成品としてのDAWプラグインの中間に位置する、**楽器を成立させるエンジン**である。

```
┌──────────────────────────────────────────────────────────┐
│  Host / DAW  … Riffra, Ableton, Bitwig, Reaper, ...      │
├──────────────────────────────────────────────────────────┤
│  Plugin / Frontend … CLAP, VST3, CLI, Standalone, ...    │
├──────────────────────────────────────────────────────────┤
│  ★ Sonalloy（Instrument Engine）                         │
│  Layer / Generator / Voice / Modulation / Runtime        │
├──────────────────────────────────────────────────────────┤
│  DSP Primitives … Oscillator, Filter, FFT, Resampler...  │
└──────────────────────────────────────────────────────────┘
```

| レイヤー | 責務 | Sonalloyとの関係 |
|---|---|---|
| DSP Primitives | 波形計算・信号処理の部品 | Sonalloyが**利用する** |
| **Sonalloy** | Instrument構築・発音制御・音色設計・演奏管理 | **本体** |
| Plugin / Frontend | DAWやアプリからSonalloyを呼び出す形式 | Sonalloyが**利用される** |
| Host / DAW | 楽曲制作・ミキシング・マスタリング | Sonalloyの管轄外 |

Sonalloy本体はPlugin API・Audio Device API・MIDI Device APIに依存しない。CLAP/VST3、Standalone、各Device接続はすべてAdapterとして外側に置く。

### 1.5 スコープ境界

| 扱う | 現時点で扱わない（将来の拡張は構造的に許容） |
|---|---|
| Sampling / Multi Sample / Oscillator / Subtractive / Noise / Wavetable / FM / Granular / Hybrid構成 | 完全自由なModular Graph |
| Polyphony / Voice Management / Modulation | Feedback Routing |
| 基本Effects / MIDI演奏 / Realtime & Offline Render | DAW機能、大規模Mixing・Mastering |
| CLI / CLAP・VST3展開 / Riffra組み込み | 巨大Effect Suite、Physical Modeling、Additive Synthesis |

---

## 2. アーキテクチャ設計

### 2.1 設計原則

**音の配線は固定、音の変化は自由。**

信号経路（音の配線）はSonalloy側で固定する。一方、各パラメータの時間変化（Modulation）には高い自由度を持たせる。この非対称性がアーキテクチャ全体を貫く原則である。

完全自由なモジュラーシンセ方式（任意のAudio Graph・Feedback Routing・循環接続）は採用しない。

> **採用しない理由**：Graph Scheduling・Cycle管理・計算量予測まで製品責務が膨張し、リアルタイム安全性の保証が著しく困難になるため。

### 2.2 信号経路：半固定パイプライン

```
Note Event
    │
    ▼
Voice Allocation
    │
    ▼
┌──────────────── Per Voice ────────────────┐
│                                          │
│  Layer Trigger Evaluation                │
│      │                                   │
│      ▼                                   │
│  Generators                              │
│  （Sample / Osc / FM / WT / Granular /  │
│    Noise）                               │
│      │                                   │
│      ▼                                   │
│  Layer Envelope / Layer Processing       │
│      │                                   │
│      ▼                                   │
│  Layer Mix                               │
│      │                                   │
│      ▼                                   │
│  Voice Processing / Amplifier            │
│                                          │
└──────────────────┬───────────────────────┘
                   ▼
               Voice Sum
                   │
                   ▼
        Global Effects（Instrument単位）
                   │
                   ▼
                 Output
```

段階間の接続順序は固定する。各段階の**内部パラメータ**には自由度を持たせる。処理の責務は次の3単位に分割する。

| 単位 | 責務 |
|---|---|
| **Layer** | Generator、発音条件、Layer Envelope、Layer Processing、Gain / Pan / Tuning |
| **Voice** | 同一Noteから生じる複数LayerのMix、Voice Processing、Amplifier |
| **Instrument** | 複数Voiceの合流、Global Effects、最終Output |

### 2.3 Modulationシステム

#### Source（変調源）

最低限、以下をサポートする。

LFO / Envelope / Velocity / Key Tracking / Pitch Bend / Mod Wheel / Aftertouch / Random

#### 接続例

| Source | → Target | 効果 |
|---|---|---|
| LFO | Filter Cutoff | 周期的な明るさの変化 |
| LFO | Sample Position | 素材内の揺らぎ |
| Envelope | Pitch / FM Amount | 発音時の音程・倍音変化 |
| Velocity | Layer Gain / Filter Cutoff | 演奏強度の反映 |
| Random | Layer Pan | 定位の散らばり |
| Mod Wheel | Granular Density | 演奏中の質感操作 |

Targetは、Layer共通Parameter・Generator固有Parameter・Voice Processing・Global EffectsのParameterを区別して解決する。Frontend固有のParameter表現をCoreへ持ち込まず、Instrument Definition上の対象をCompile時に実行用参照へ変換する。

#### 共通規則

**Parameter定義**

- 各Parameterは、Definition内で一意かつ安定したID、値の型、単位、最小値・最大値・初期値を持つ。
- 連続値（音量、Filter Cutoff等）は変調可能。離散値（波形種類、FM Algorithm等）は変調対象外。

**Route定義**

- Modulation Routeは、Source・Target Parameter・Amount・Curveを持つ。
- AmountはTargetの可変範囲に対する割合として扱う。
- Sourceの出力は正規化する：単方向の値は `0〜1`、正負の値は `-1〜1`。
- 一つのTargetへ複数Routeが接続された場合、各変調量を加算し、Targetの最小値・最大値へClampする。

**評価単位**

- Voice単位で評価：Velocity、Key Tracking、Voice内のEnvelope・LFO（Noteごとに決まる値）
- Instrument単位で保持：Mod Wheel等（演奏全体に作用する値）

**変更の反映**

- 連続値の変更：平滑化を適用し、クリックノイズを避ける。
- 離散値・処理構成の変更：Audio処理中に直接切り替えず、Compiled Instrumentの更新として扱う（§4.3参照）。

この規則はCLI・Riffra・CLAP・VST3・Instrument Definition・Runtimeで共通利用する。FrontendごとにParameterの意味を再定義しない。

---

## 3. 機能仕様

### 3.1 Instrument（中心概念）

Sonalloyの最上位モデル。一つのInstrumentが一つの「楽器」を表す。

```
Instrument
├── Layers[]              ← 音色を構成するLayer（複数可）
│   ├── Generator         ← 音の発生方式
│   ├── Trigger Conditions
│   ├── Gain / Pan / Tuning
│   ├── Layer Envelope
│   └── Layer Processing
├── Voice Processing      ← Layer Mix後のFilter, Drive, EQ等
├── Modulation Matrix     ← Source → Target の接続群
├── Global Effects        ← Voice Sum後のChorus, Delay, Reverb等
├── Performance Settings  ← 発音方式、同時発音数、音のつながり方等
└── Asset References      ← 外部Sampleファイルへの参照
```

**LayerとGeneratorの責務分離**：Generatorは「どの方式で音を発生させるか」を担い、Layerは「いつ・どの範囲で・どのようにInstrumentへ混ぜるか」を担う。

Instrument内のParameterは、所属先（Layer / Generator / Voice Processing / Global Effects）を明示し、安定したParameter IDで識別する。特定のFrontendやDAWに依存しないデータとして保存可能であり、Definitionから実行可能なInstrumentを再構築できることが要件である。

### 3.2 Generator（音の発生源）

各Layerは一つのGeneratorを持つ。Generatorは音の発生方式と、その方式に固有のParameterだけを担う。

| Generator | 概要 | 固有の主要パラメータ |
|---|---|---|
| **Sample** | 音声素材の再生 | Sample Zones, Playback Interpolation |
| **Oscillator** | 基本波形の生成（Sine/Saw/Square/Triangle） | Waveform, Phase, Phase Reset, Alias対策済み生成方式 |
| **Noise** | White Noise等のノイズ信号 | Type, Color |
| **Wavetable** | 複数波形間の連続移動 | Table, Position（Modulation対象）, Interpolation |
| **FM** | Oscillator間の周波数変調 | Operator構成, Ratio, Index, Algorithm（専用構造として表現） |
| **Granular** | 素材をGrain単位で分割・再構成 | Position, Grain Size, Density, Grain Pitch, Randomness, Pan Spread |

**設計判断**：

- **Subtractive Synthesis**：独立Generatorを設けず、Oscillator/Noise + Layer/Voice ProcessingのFilter・Amplifierの組み合わせで表現する。
- **FM**：一般的なAudio Graphへ無理に変換せず、Operator/Algorithm構造をそのまま保持する専用Generatorとする。

**Layer側に属するもの**（Generatorの責務外）：Key Range・Velocity Range・Note On/Note Off等のTrigger Conditions、Gain、Pan、共通Tuning、Layer Envelope、Layer Processing。

**Sample Generator内のSample Zone**：音声素材ごとにRoot Note・Key Range・Velocity Range・Round Robin Group・Loop・Playback Modeを保持する。これらはSample Layer自体の発音可否を判断するTrigger Conditionsとは別の責務である（§3.4参照）。

### 3.3 Voice Management

Voiceは、一つのNote Onから発生するLayer群と、その演奏中状態をまとめる単位である。

#### 発音方式（Performance Settingsで選択）

| 方式 | 内容 |
|---|---|
| Polyphonic | 複数のNoteを独立したVoiceとして同時に発音 |
| Monophonic | 同時に一つのVoiceのみ使用 |
| Legato | 前の鍵盤を押したまま次の鍵盤を押した場合、Envelopeを再開始せず同じVoiceの音程を移行 |
| Portamento | 設定された時間をかけて音程を滑らかに移動 |
| Sustain Pedal | 鍵盤を離しても保持を続け、ペダル解除時にReleaseへ移行 |

#### Voice管理機能

- Note On / Note Off の処理
- Note IDによるNoteとVoiceの対応付け
- Voice Allocation（空きVoiceの割り当て）
- Layer Trigger Evaluation（Note・Velocity・Trigger種別に応じた発音Layerの決定）
- Voice Stealing（上限到達時のVoice入れ替え）
- Release処理（Note Offまたはペダル解除後の減衰）
- Polyphony Limit（同時発音数の上限設定）

#### Note ID

各Note Onには、FrontendまたはAdapterが一意なNote IDを付与する。Note Off・音程変更・Note単位の表現変更は同じNote IDで対象Voiceを特定する。Note IDを持たない入力形式では、AdapterがChannel + Note Numberを基に生成・対応付けする。

#### Voice Stealing規則

Release中で出力の小さいVoiceを優先し、該当がなければ最も古いVoiceを対象とする。入れ替え時は短い減衰を入れ、波形の不連続によるクリックノイズを避ける。

#### Runtime状態（Definitionに保存しない）

各LayerのGenerator状態、Envelope状態、Voice Processing状態、鍵盤の押下状態、ペダルによる保持状態など、演奏中に変化する状態。

### 3.4 Sample Mapping

Sample Mappingは、Sample Generatorを持つLayerが発音対象になった**後**に、実際に使用するSample Zoneを選択する機能である。

```
Layer Trigger Conditions → Sample Layerを発音 → Key × Velocity × Round Robin → Sample Zone選択
```

| 概念 | 判断内容 |
|---|---|
| Layer Trigger Conditions | そのLayer自体を発音するか |
| Sample Mapping | 発音するSample Layer内で、どのSample Zoneを使用するか |

Sample Zoneの保持情報：Asset Reference・Root Note・Key Range・Velocity Range・Round Robin Group・Loop・Playback Mode。

この責務分離により、以下を同じSample Generatorモデルで表現できる：

- 単一Sampleの音域展開（Pitch Shift）
- Multi Sample Instrument（音域ごとに異なる録音）
- 強弱に応じたSample Zoneの切り替え
- Drum Kit（Keyごとに異なるOne-shot）

**Round Robin規則**：同じKey Range・Velocity Rangeに複数のSample Zoneが該当する場合、同一Round Robin Group内から順番に選択する。異なるGroupが同時に該当する構成はValidation Errorとし、選択結果が実装依存にならないようにする。

### 3.5 Effects

音源設計に直接必要な範囲のみ内蔵する：Filter / Drive・Saturation / EQ / Chorus / Delay / Reverb。

#### 適用位置

| 適用範囲 | 対象 | 使用可能なEffect |
|---|---|---|
| **Layer Processing** | 特定のLayerだけ | Filter, Drive, EQ |
| **Voice Processing** | 一つのNoteから発生したLayer Mix全体 | Filter, Drive, EQ |
| **Global Effects** | 複数Voiceを合流したInstrument全体 | Filter, Drive, EQ, Chorus, Delay, Reverb |

Delay・Reverb・Chorusは発音Voice数に比例して状態を複製しないよう、基本的にGlobal Effectsとして扱う。

#### 処理規則

- 各処理はDefinitionに記載された順序で直列に適用する。任意RoutingやFeedback接続は扱わない。
- 各Effectの連続ParameterはModulationおよび演奏中の変更対象にできる。
- Effectの追加・削除・並べ替えは処理構成の変更として扱い、Compiled Instrumentを更新して反映する（§4.3参照）。

高度なMastering処理や総合ミキシングはHost・外部Pluginの責務であり、Sonalloyが担うのは音源設計に直結する範囲までとする。

---

## 4. データモデル：Instrument Definition

### 4.1 三層構造

Instrumentの構成は、バージョン管理可能なテキストデータ（JSON）で表現する。

> **JSONを採用する理由**：バイナリ内部状態のダンプでは再現性・可読性・差分管理が担保できないため。

Definitionは編集・保存用の正本であり、Audio処理から直接利用しない。読み込み後にValidation・Asset解決・Parameter/Modulation参照解決を行い、実行用のCompiled Instrumentへ変換する。

```
Instrument Definition（JSON） + Referenced Assets（WAV等）
                │  Load / Validate / Resolve / Compile
                ▼
        Compiled Instrument
                │  Instantiate
                ▼
      Instrument Runtime Instance
```

| モデル | 責務 |
|---|---|
| **Instrument Definition** | 編集・保存・差分管理可能なInstrument構成の正本 |
| **Compiled Instrument** | Assetや参照を解決し、Audio処理前に準備された実行用の不変構造 |
| **Instrument Runtime Instance** | Voice、Envelope、Oscillator Phase、Filter・Effect状態など演奏中に変化する状態 |

同一のDefinition + Referenced Assetsから同等のCompiled Instrumentを構築し、Instrumentを再現できることが要件である。

### 4.2 保持内容

| カテゴリ | 内容 |
|---|---|
| 識別 | Schema Version, Metadata（名前、作者、説明） |
| 音源構成 | Layer一覧, 各LayerのTrigger Conditions・Mix・Processing, 各Generatorの種類と固有Parameter |
| Parameter | Parameter ID, 値の型, 単位, 最小値, 最大値, 初期値 |
| 変調 | Modulation Matrix（Source → Target Parameter ID, Amount, Curve） |
| マッピング | Sample Generator内のSample Zones（Key/Velocity/RR → Zone） |
| 演奏 | 発音方式, 同時発音数, Voice Stealing Rule, Sustain Pedal, Legato/Portamento設定 |
| 効果 | Layer Processing, Voice Processing, Global Effects |
| 再現性 | Random Seed, 参照AssetのHash |

**Compile時に解決するもの**：Asset参照、Decode済み/再生準備済みSample、Parameter参照、Modulation Target、Processor構成、実行時に必要なメモリ配置。

**保存対象外**：Compiled Instrument、ランタイム状態、Decode済み音声Buffer、Voice状態、一時計算結果。

正本形式はバージョン付きJSONとし、Rust内部ではSerdeでDefinition型と相互変換する。

### 4.3 読み込みと更新のルール

#### 読み込み時のエラー処理

| 状況 | 対応 |
|---|---|
| Definitionの構造や値に矛盾がある | Compileを失敗させ、現在利用中のCompiled Instrumentを維持 |
| 参照する音声素材が見つからない | Instrument全体の読み込みは失敗させず、該当Sample Zoneを無効化。依存Layerはその部分だけ無音、他Layer/Generatorは利用可能 |
| 不足Assetがある | 診断情報としてFrontendへ返し、参照先を指定し直して再Compile可能にする |

#### 変更時の反映規則

| 変更種別 | 反映方法 |
|---|---|
| 連続値の変更（音量、Filter Cutoff、変調量等） | Parameter Change EventとしてRuntimeへ渡し、発音中のVoiceにも平滑化して反映 |
| 構成変更（Layer/Generator/Effectの追加・削除・並べ替え、離散Parameter変更） | Control側で新しいCompiled Instrumentを生成 |

**構成変更の反映タイミング**：

- Compileに成功した構成はAudio Block境界で公開し、新しく開始するVoiceから利用する。
- 発音中のVoiceは、発音開始時に参照した構成をRelease完了まで使用する。
- Compileに失敗した場合は、現在のCompiled Instrumentを変更しない。
- Global Effectsの構成変更はAudio Block境界で切り替え、短い出力平滑化でクリックノイズを避ける。
- Effectの連続Parameter変更は構成変更を伴わず、発音中の音にも反映する。

---

## 5. インターフェース

### 5.1 CLI（第一級）

GUIやDAWがなくても、CLIだけでSonalloyの主要機能を完結できる。

| 操作カテゴリ | 具体機能 |
|---|---|
| **作る** | 新規作成, Layer追加・削除, Generator設定, Sample追加, Parameter・Modulation設定 |
| **理解する** | 内容表示, 構成解析, Validation, 不足Assetの確認 |
| **演奏する** | 単音発音, Note Sequence, MIDI File再生 |
| **書き出す** | Offline Render, WAV等へのExport |
| **リアルタイム** | MIDI Device + Audio Device による演奏（Linux含む） |
| **修復する** | 不足Assetの参照先を指定し直し、再Validation・再Compile |

CLIが操作する正本はInstrument Definitionであり、コマンド列はDefinitionを操作する手段に過ぎない。リアルタイム演奏時のMIDI Device・Audio Device管理はCLI / Standalone側のAdapterが担い、Sonalloy Coreには正規化されたEvent・Process Context・Audio Bufferを渡す。

### 5.2 Plugin（CLAP / VST3）

外部DAWからSonalloy Instrumentを利用するためのAdapter。Sonalloy CoreはPlugin APIを知らない（§1.4の原則）。CLAP / VST3 Adapterは、Host固有のEvent・Transport情報・Audio BufferをSonalloy共通のProcess Contractへ変換し、Coreの出力をHostへ返す。

### 5.3 Rust API / C ABI

Sonalloy Coreは、接続元に依存しない共通のライフサイクルとProcess Contractを公開する。

#### ライフサイクル

```
Prepare → Activate → Process（繰り返し） → Reset / Deactivate
```

| フェーズ | 内容 |
|---|---|
| **Prepare** | Sample Rate、最大Block Size、出力Channel数を受け取り、必要なBufferを事前確保 |
| **Activate** | Audio処理を開始できる状態へ移行 |
| **Process** | 現在のFrame数、Process Context、Event列、出力Bufferを受け取り、音声を生成 |
| **Reset** | 発音中Voice、Envelope、Effect状態等を初期状態へ戻す |
| **Deactivate** | Audio処理を終了 |

#### Process Contract

```
Process
├── Process Context（Current Frame Count, Tempo / Time Signature / Transport）
├── Events[]（Event Type, Note ID, Sample Offset, Payload）
└── Output Buffers
```

**仕様**：

- Sample Rate・最大Block Size・出力Channel数はPrepare時に確定。
- 一回のProcessで扱うFrame数は最大Block Size以下。呼び出しごとに変化してよい。
- Audio Sample Formatは `f32`、Buffer形式はChannelごとに分離したPlanar。少なくともMonoとStereo出力を扱う。
- EventはSample Offsetの昇順で渡す。同一Offsetでは入力順を維持。ただし同一NoteのNote OffとNote Onが同時の場合はNote Offを先に処理する。
- Audio Callback内で回復不能な例外を外部へ送出しない。処理できないEventは診断対象とし、そのProcess自体は無音または処理可能な範囲で継続する。

#### 正規化Event

Coreへ渡すEventは、生のMIDIバイト列やPlugin固有Eventではなく、以下へ正規化する：

Note On / Note Off / Sustain Pedal / Pitch Bend / Mod Wheel / Aftertouch / Parameter Change

各EventはAudio Block内のSample Offsetを持ち、Frontendの種類にかかわらず同じRuntimeでSample単位のタイミングを処理できる。

#### 接続先一覧

| 接続先 | 方式 | 用途 |
|---|---|---|
| Sonalloy CLI / Standalone | Rust API（直接） | Device・MIDI入力を共通Process Contractへ変換 |
| Riffra（C++ / JUCE） | C ABI | JUCEのAudio/MIDIを共通Process Contractへ変換 |
| 外部DAW | CLAP / VST3 | Host固有Event・Bufferを共通Process Contractへ変換 |
| オフライン外部処理 | CLI or プロセス間連携 | Event列とProcess Contextを用いたBatch処理 |

#### C ABIの境界規則

- Rust内部型を直接公開せず、不透明Handleを通じてInstrument Instanceを操作する。
- Instanceの生成・破棄はSonalloy側が行う。Audio BufferとEvent列は呼び出し側がProcess中だけ所有・貸与する。
- Rust PanicやC++例外を境界の外へ伝播させず、結果コードと診断情報で失敗を返す。

---

## 6. システム構成と技術選定

### 6.1 依存方向とレイヤー構造

```
Frontend / Adapter（CLI / Standalone / Riffra / CLAP / VST3）
        │  Device・Host固有形式を変換
        ▼
Sonalloy Public Contract（Definition API / Compile API / Lifecycle API / Process API / C ABI）
        │
        ├── Control側 ── Instrument Compiler（Validation / Asset Resolution / Parameter・Modulation Resolution）
        │                        │
        │                        ▼
        │                 Compiled Instrument ── Block境界で公開
        │                        │
        ▼                        ▼
Instrument Runtime Instance（Event処理, Voice管理, Layer処理, Modulation）
        │
        ▼
Generator Engines（Sample, Osc, FM, WT, Granular, Noise）
        │
        ▼
DSP Core（波形生成, Filter, FFT, Resampler, Envelope）
```

**禁止事項：Sonalloy CoreがCLI・Riffra・JUCE・CPAL・midir・CLAP・VST3を知ることは禁止。**

#### Control側とAudio側の責務分離

| 側 | 責務 |
|---|---|
| **Control側** | Definition編集、Validation、Asset解決、Compile、不足Assetの診断、Compiled Instrumentの公開準備 |
| **Audio側** | Process Eventの適用、Voice生成・終了、Modulation評価、音声生成、事前準備済み構成への切替 |

Compiled Instrumentの公開はAudio Block境界で行う。Audio側でJSON解析・Asset解決・構成構築を行わない。発音中Voiceが参照する旧構成は、そのVoiceの終了まで破棄しない。

### 6.2 技術候補

| 領域 | 選定 | 備考 |
|---|---|---|
| 言語 | Rust | 独立Workspace |
| Serialization | Serde / JSON | Definition正本 |
| Realtime Audio | CPAL | Standalone AdapterでAudio Deviceを管理 |
| Audio Decode | Symphonia | Compile前後のAsset準備でWAV/FLAC/OGG等をDecode |
| Resampling | Rubato | Sample Rate非依存 |
| FFT | RustFFT | Granular, 分析系 |
| Realtime MIDI | midir | Standalone AdapterでDevice入出力を管理 |
| MIDI File | midly | MIDI Fileを共通Event列へ変換 |
| CLAP | clack ecosystem | Plugin Adapter |
| Riffra統合 | C ABI / FFI | 共通Process Contractを公開する安定境界 |

### 6.3 所有と利用の境界

| 区分 | 対象 |
|---|---|
| **Sonalloyが所有**（製品の中核） | Instrument Definition, Layer, Generator, Instrument Compiler, Compiled Instrument, Voice, Modulation, Instrument Runtime, Sample Mapping, 共通Event・Process Contract |
| **Frontend / Adapterが所有**（利用環境との接続） | Audio Device, MIDI Device, Plugin Host API, JUCE / CPAL / CLAP / VST3固有のEvent・Buffer変換 |
| **既存ライブラリに委譲**（汎用基盤） | FFT, Resampling, Codec, Audio Device接続, MIDI Device接続 |

---

## 7. Riffraとの関係

**原則：コードベースは分離、製品体験は深く統合。**

```
Riffra ──利用──▶ Sonalloy
（依存の矢印は Riffra → Sonalloy の一方向のみ）
```

SonalloyはRiffraがなくても単独で全機能動作する。

| Riffra画面 | Sonalloyとの関わり |
|---|---|
| **Design** | Instrumentの視覚的エディタ（Layer追加, Generator設定, Sample選択, Env編集, Mod Routing, Effect設定, 保存）。Instrument Definitionを共通モデルとして扱い、意味の再実装はしない。 |
| **Play** | MIDI鍵盤等からのリアルタイム演奏。Performance Settingsに従いPolyphonic / Monophonic / Legato / Portamento / Sustain Pedalを利用。C++ / JUCE側でAudio/MIDIをProcess Contractへ変換し、C ABI経由で渡す。 |
| **Arrange** | 楽曲内でのInstrument利用。純粋に「使う側」であり、音源設計・Compile・発音ロジックはSonalloyが担う。 |

**所有関係**：RiffraはAudio DeviceおよびJUCE Audio Callbackを所有する。Sonalloyは渡されたProcess Context・Event・Audio Bufferに対して処理する。Sonalloy側からRiffraやJUCE固有APIを呼び出さない。

**Design画面での構成変更フロー**：

1. RiffraがControl側でCompileを要求する。
2. 成功 → Compiled InstrumentをAudio Block境界で公開。
3. 失敗 → 現在のInstrumentを維持し、診断内容を画面へ表示。
4. Asset不足 → Instrument自体は開き、不足箇所を明示。該当Sample Zoneのみ無効化された状態で他Layerは利用可能。Riffraから参照先を指定し直して再Compileできる。

---

## 8. 非機能要件

| 要件 | 内容 |
|---|---|
| **Cross-platform** | Linux / Windowsを主要対象。CoreはOS固有機能へ直接依存しない。 |
| **Headless** | GUIなしで全主要機能を利用可能（必須）。 |
| **Realtime Safety** | Audio CallbackではCompiled Instrumentと事前確保済みRuntime状態のみを使用。ファイルI/O・Asset Decode・大規模alloc・JSON解析・Blocking Lock・Network Access・Device操作は禁止。 |
| **演奏継続性** | 演奏中のParameter変更や構成変更でAudio処理を停止しない。Compile失敗時は現在の正常なCompiled Instrumentを維持。 |
| **部分的な読込** | 参照Asset不足でもInstrument全体を読込不能にせず、該当Sample Zoneのみ無効化して診断情報を返す。 |
| **再現性** | 同一のDefinition + Event + Sample Rate + Asset + Random Seed → 同一のOffline Render結果。Random SourceはInstrument Seed・Note ID・Layer IDから独立した系列を生成し、Voice処理順の違いで結果が変わらないようにする。 |
| **Sample Rate非依存** | DSP実装で固定Sample Rateを前提としない。 |
| **Frontend非依存** | Frontend固有のMIDI・Audio・Plugin形式はAdapterで共通Process Contractへ変換し、Coreへ持ち込まない。 |
| **境界安全性** | Rust API / C ABIからPanicや例外を漏らさず、所有権と寿命が明示されたHandle・Buffer・Event Contractを使用する。 |

---

## 9. 完成像（まとめ）

Sonalloyを一文で言えば：

> 任意の音を取り込み、必要ならゼロから生成し、異なる生成方式をLayerとして融合して、一つの演奏可能なInstrumentとして保存・再利用できるハイブリッド音源エンジン。

ユーザーは同じInstrument Definitionを、CLI / Riffra / CLAP Host / VST3 Host / その他アプリケーションから利用できる。Definitionは事前にCompiled Instrumentへ変換され、各Frontendは共通のEvent・Process Contractを通じて同じInstrument Runtimeを使用する。Sonalloyは特定のDAW・GUI・Plugin規格・Device APIに縛られず、独立して動作する。

製品の中心軸：

```
Sound（素材・波形）
    → Generation / Sampling（音の発生）
        → Layering / Hybridization（構成・融合）
            → Modulation（時間変化）
                → Instrument Definition（保存可能な楽器定義）
                    → Compile / Runtime（実行可能な楽器）
                        → Performance（演奏）
```

この一連の流れを、一つのエンジンとして完結させることがSonalloyの存在意義である。

