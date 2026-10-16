---
name: create-instrument
description: Use when creating, editing, or debugging Sonalloy instruments, or composing, previewing, auditioning, and rendering Score-based music in Sonalloy. Covers instrument definitions, custom WAV assets, Score YAML, validation, inspection, and audio rendering.
---

# Create Instrument

Sonalloyで音源（Instrument）を制作・検証・試聴し、Scoreを使った楽曲・試奏を作成・編集・確認する手順書です。

## 参照ファイル

より詳細な仕様はreferences配下にまとめています。必要に応じて参照してください。

| 文書 | 内容 |
|---|---|
| `references/definition.md` | Definition全体の構造・Performance・Layer・Trigger・Macro / Vector・External Audio・コンパイル時の変換 |
| `references/generators.md` | 全GeneratorのField・Range・Dynamic Parameter・制約 |
| `references/processors.md` | 全ProcessorのField・Range・Dynamic Parameter・固定Latency |
| `references/modulation.md` | Modulation Source・Routeの計算規則・Parameter IDのPrefix・MSEG |
| `references/score_spec.md` | 楽曲・試奏・プレビューのScore YAML文法と演奏仕様 |
| `references/cli.md` | 全コマンドのOption・出力Report・診断Code・Exit Code |

## 全体フロー

```text
init → edit → validate → inspect → Score trial / render / analyze / trace → optional realtime audition → refine
```

1. **init**：新規Definitionのひな形を生成（既存を編集する場合は省略）
2. **edit**：Generator、ADSR、Processor、Modulationを編集
3. **validate**：`instrument validate`でJSON、制約、Asset準備を検証
4. **inspect**：`instrument inspect --json`でCompile後の実行値（Parameter ID、Route Effect、Clamp範囲）を確認
5. **score trial / render**：用途に合うScore YAMLで演奏を定義し、`score validate`から`render score`へ進む。単音や診断には`render note` / `render events` / `render midi`を使い、必要な事実を`--analyze`と`--trace`で取得する
6. **楽曲の確認**：複数Instrumentを同じScoreからレンダリングし、必要に応じてStem・Master・MIDIを出力する
7. **realtime trial**：`device list`でDeviceを確認し、MIDI Keyboardがある場合は`play`、ない場合は`audition score`で同じDefinitionを演奏する
8. **refine**：数値・音色・`metadata`を整理し、再度InspectとRenderを実行

## Definitionを編集する

### ひな形を生成する（新規時）

```bash
sonalloy instrument init <path>
```

Saw Oscillator・同時発音数16の最小Definitionが生成されます。

### 構造の基本

音源は1つ以上のLayerで構成されます。Layerは同じVoice内でMixされ、Layerごとに独立したADSR・Gain・Pan・Tuningを持ちます。

```text
Note On
  │
  ▼
Layer 1 → Layer Processor → ADSR → Layer Gain / Pan ─┐
                                                      ├→ Voice Processor → Global Processor → Stereo 出力
Layer 2 → Layer Processor → ADSR → Layer Gain / Pan ─┘
```

### ADSRで音の輪郭を作る

| Parameter | 役割 | Range / 目安 |
|---|---|---|
| `attack_seconds` | Note Onから最大音量へ達する時間 | 0〜30秒。0で瞬発、数秒でうねり |
| `decay_seconds` | 最大音量からSustain Levelへ下がる時間 | 0〜30秒。0.05〜0.3が一般的 |
| `sustain_level` | Note On中の音量 | 0〜1。0で短い音、1で伸びる音 |
| `release_seconds` | Note Offから無音へ至る時間 | 0〜30秒。0でバツンと切れる |

### ProcessorとModulation

- **Processor**：Layer / Voice / Globalの3段階で直列適用します。`cutoff`、`threshold_db`、`mix`などのDynamic Parameterを持ち、Wet / Dry比は`mix` 1つで表現します
- **Dynamics**：Gate / Compressorは`detector: "self_signal"`またはGlobal専用の`"external_audio"`を指定します。外部Detectorを使うときは`external_audio`を宣言します
- **Modulation**：Velocity、Key Tracking、LFO、Envelope、Random、MSEG、Step、MacroなどのSourceをDynamic Parameterへ接続します

Processorはどの配置でもObjectのトップレベルに`type`（種類）と`id`（Parameter IDの一部になる識別子）を持ち、残りのFieldは種類ごとに異なります。`processors`はLayerのField、`voice_processors` / `global_processors`はトップレベルのFieldです。

```json
"processors": [
  { "type": "filter", "id": "attack_tone", "mode": "low_pass", "cutoff_hz": 9000.0, "resonance": 0.1 }
],
"voice_processors": [],
"global_processors": [
  { "type": "reverb", "id": "space", "pre_delay_seconds": 0.012, "decay": 0.6, "damping": 0.35, "width": 1.0, "mix": 0.2 }
]
```

Dynamic ParameterのIDはProcessorの配置でPrefixが決まり、Modulation RouteとParameter ChangeのTargetはこの形式で書きます。上例のLayer Filterは`layer.<layer_id>.processor.attack_tone.cutoff`、Reverbは`global.processor.space.mix`、Generatorは`layer.<layer_id>.generator.<name>`形式（Operator Modulationだけ`layer.<layer_id>.generator.operator.<1-4>.<parameter>`）です。Prefixの一覧は[Modulation仕様](references/modulation.md)、ProcessorのFieldは[Processor仕様](references/processors.md)を参照し、Routeを書く前に`instrument inspect --json`で実際のParameter IDと配置を確認します。

```json
"modulation": {
  "routes": [
    { "source": "velocity", "target": "layer.main.gain", "depth": { "value": 8.0, "unit": "decibels" }, "curve": "linear" },
    { "source": "lfo", "target": "voice.processor.tone.cutoff", "depth": { "value": 1.5, "unit": "octaves" }, "curve": "linear" }
  ],
  "sources": [
    { "id": "lfo", "type": "lfo", "waveform": "sine", "rate": { "value": 0.5, "unit": "per_second" }, "phase": 0.0 }
  ]
}
```

VelocityとKey Trackingは組み込みSourceのため、Source定義なしで`routes`から参照できます。Sourceの種類・Polarity・`depth`のUnit対応・使い分けは[Modulation仕様](references/modulation.md)を参照してください。

## Asset（WAV）を扱う

Sample、Wavetable、Spectral、Granular、Wave Sequenceは外部WAVをAssetとして参照します。共通する扱いをまとめます。各Generatorへ渡す`asset.path`は、DefinitionのあるDirectoryを基準とした相対Path、または絶対Pathを指定できます。共有する音源では相対Pathが適しています。

### 配置と形式

- 形式はPCM 16/24 bitまたはFloat 32。Mono / Stereoを使用できます
- `sha256`は任意の起動時検証用です。必要なAssetの欠落・Hash不一致・Decode失敗はCompile Errorとなり、Path・形式・Hashを修正してからRenderします

### SHA-256の計算

```bash
# Linux
sha256sum <path>

# Windows
Get-FileHash -Algorithm SHA256 <path>   # 小文字のhexでJSONへ記録する
```

### Sample RateとChannelの扱い（Generator別）

| Generator | Sample Rate | Channel |
|---|---|---|
| Sample / Granular / Wave Sequence | 処理Sample RateへResampleされる | Mono / Stereoを保持（GranularはMonoでもStereo出力） |
| Wavetable | Pitchへ使われず、コンパイル時にResampleされない | MonoへDownmixされる |
| Spectral | コンパイル時にSample Rate変換とSTFT解析を行い、処理Sample Rate依存のFrameになる | A/BでChannel数を一致させる |

## Generator

GeneratorはLayerの`generator` Fieldへ、いずれか1つを指定します。各GeneratorのField・Range・Dynamic Parameter・制約は[Generator仕様](references/generators.md)を参照してください。Modulation Target IDは`layer.<layer_id>.generator.<name>`形式です（Operator Modulationだけ`layer.<layer_id>.generator.operator.<1-4>.<parameter>`）。

## Hybrid構成

複数Generatorを同じVoiceでMixすると、役割分担で調整しやすくなります。代表的な構成：

| 構成 | Layer構成 |
|---|---|
| Harmonic / Formant Hybrid | Formant（共鳴）+ Additive（芯）+ Sample（Attack）+ Noise（Air）+ Layer / Voice / Global Processor |
| Spectral Hybrid | Spectral + Additive + Sample + Noise + Processor / Modulation |
| Digital Hybrid | Wavetable（持続）+ Operator Modulation（倍音芯）+ Sample（短アタック） |
| Physical / Modal Hybrid | Physical String（撥弦・振動）+ Modal（Body・共鳴）+ Layer / Voice / Global Processor |

Hybridを作る手順：

1. 各LayerのGainとEnvelopeを単独で確認する
2. Sample / Wavetable AssetのPathとSHA-256を保持したまま複製する
3. `instrument inspect --json`でLayer / Voice / Global Processorの配置・順序・Parameter IDを確認し、Route TargetがDefinitionのLayer ID / Processor IDに一致することを確かめる
4. LFO、Modulation Envelope、Velocity、Mod Wheel、AftertouchをFormant ParameterまたはProcessorへ接続し、`render events` / `render midi`でPhraseの振る舞いを確認する

## 検証する

```bash
sonalloy instrument validate <definition>          # JSON Parse・Validation・コンパイルまで実行
sonalloy instrument inspect <definition> --json    # 実行値を機械可読で表示（--json省略で人間可読）
```

- `validate`のWarningも表示されるため必ず確認する。ErrorにはField Pathが付くため、そのまま該当箇所へ反映できる
- Asset Errorの場合は、診断のField Pathを確認して参照WAVを修正する
- `inspect`の表示項目は[CLIリファレンス](references/cli.md)を参照する
- Inspect、Analysis、Traceが既に公開している事実を得るために、RuntimeのSource Codeを読んだり、同じ値を再計算する外部スクリプトを作ったりしないでください。製品Interfaceで不足する研究や一回限りの人間向け分析に限り、外部ツールを使えます

## 試聴する

```bash
# 単音
sonalloy render note <definition> \
  --note 60 --velocity 100 --gate 0.5 --tail 0.5 --tempo 120 \
  --sample-rate 48000 --block-size 257 --analyze \
  --trace layer.main.tuning --trace-every-frames 480 \
  --output out/<name>/note.wav --json

# 発音中のParameter / Control Event
sonalloy render events <definition> <events.json> \
  --duration-frames 96000 --output out/<name>/events.wav

# MIDI Phrase
sonalloy render midi <definition> <midi-file> \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/<name>/phrase.wav
```

## Scoreで用途を試奏する

単音だけでは判断できない場合は、[Score YAML](references/score_spec.md)に演奏を記述します。プリセットの短い試聴は `preview.score.yaml` を使い、別の演奏内容で詳細な奏法を試す場合だけ `showcase.score.yaml` を用意します。同じ演奏の詳細試奏にも `preview.score.yaml` を使います。共通の試奏素材は `presets/common-scores/` にあり、未束縛の楽器を `--bind` で指定します。

```bash
sonalloy score validate presets/common-scores/lead.score.yaml --bind preview=<definition>
sonalloy render score presets/common-scores/lead.score.yaml \
  --bind preview=<definition> --output out/<name>/phrase.wav --analyze --json
sonalloy audition score presets/common-scores/lead.score.yaml --bind preview=<definition> --loop
```

自作の楽曲は `score init <path>` から編集し、`score inspect --json` で展開結果を確かめます。複数楽器のリアルタイム試聴には `--part <id>` を指定し、区間は `--section <name>` で選びます。楽曲全体の音は `render score` で確認します。

MIDIは `score import-midi <midi> --instrument <definition> --output <score.yaml>` で取り込み、`score export-midi <score.yaml> --output <song.mid>` で書き出します。17楽器以上はMIDI Portを分けて出力します。音源固有のParameterやゲインレーン間で異なる制御列は省略され、対象楽器と内容がwarningになります。`--json`では成功Reportの`diagnostics`を確認します。音符ごとの強弱・接続、コードの修飾・声部配置、スウィングは[Score仕様](references/score_spec.md)に従って記述します。

## Deviceが利用できる場合のRealtime試聴

```bash
sonalloy device list
sonalloy device list --json
sonalloy play <definition> --midi-device <id>
```

`play`は同じDefinitionをCoreのRealtime経路で演奏します。起動前に`device list`でAudio Input / OutputとMIDI InputのIDを確認し、外部Audioを使うDefinitionでは必要なChannel数とSample Rateに対応するInputを`--audio-input-device`で選びます。複数のMIDI Inputがある場合は`--midi-device`を必ず指定します。標準入力のEnterで停止します。Realtime試聴はOffline Render、Analysis、Traceを置き換えません。

## 仕上げる

- `metadata.name`と`metadata.description`を実際の音色に合わせる
- `validate` / `inspect --json`のWarning、出力Mode、Parameter Unit、Route Effect、Parameter IDを最終確認する
- `--analyze`で数値的な出力状態を確認し、`--trace`で宣言したModulationが意図した範囲を動いたか確認する
- 最大Velocity（127）でもクリップしないことを確認する。`render note --velocity 127 --analyze`の`analysis.level.over_full_scale`が`false`であること
- 生成したWAVを同じ音量条件で試聴する

## 失敗時の対処

Exit Codeの意味と対処は[CLIリファレンス](references/cli.md)を参照してください。

### よくある症状と対処

| 症状 | 対処 |
|---|---|
| Asset Errorが出た | 診断のField Pathに対応するPath・Hash・WAV形式を確認する |
| 音が鳴らない | `enabled: true`、`trigger`の範囲に発音するNote / Velocityが含まれているか確認する |
| Sampleが無視された | Asset PathとSHA-256の一致、WAV形式（PCM 16/24、Float 32）を確認する |
