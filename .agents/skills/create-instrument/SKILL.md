---
name: create-instrument
description: Use ONLY when the user asks to create, edit, or debug a Sonalloy instrument definition (音源の作成・編集・修正), add an Additive, Formant, Sample, Wavetable, Spectral, Operator Modulation, Granular, Physical String, or Modal layer with a custom WAV, or render and listen to an instrument sound. Covers instrument init, JSON editing, validate / inspect, SHA-256 asset setup, and render note / midi / pattern.
---

# Create Instrument

Sonalloyで音源（Instrument）を作成・編集・検証・試聴するための手順書です。

## 参照ファイル

より詳細な仕様はreferences配下にまとめています。必要に応じて参照してください。

| 文書 | 内容 |
|---|---|
| `references/definition.md` | Definition全体の構造・Performance・Layer・Trigger・Macro / Vector・External Audio・コンパイル時の変換 |
| `references/generators.md` | 全GeneratorのField・Range・Dynamic Parameter・制約 |
| `references/processors.md` | 全ProcessorのField・Range・Dynamic Parameter・固定Latency |
| `references/modulation.md` | Modulation Source・Routeの計算規則・Parameter IDのPrefix・MSEG |
| `references/patterns.md` | Audition PatternのSchema・Event・MIDI Interchange |
| `references/demos.md` | Demoの定義・時間軸・MIDI / Audio出力の仕様 |
| `references/cli.md` | 全コマンドのOption・出力Report・診断Code・Exit Code |

## 全体フロー

```text
init → edit → validate → inspect → pattern trial / render / analyze / trace → optional demo trial → optional realtime trial → refine
```

1. **init**：新規Definitionのひな形を生成（既存を編集する場合は省略）
2. **edit**：Generator、ADSR、Processor、Modulationを編集
3. **validate**：`instrument validate`でJSON、制約、Asset準備を検証
4. **inspect**：`instrument inspect --json`でCompile後の実行値（Parameter ID、Route Effect、Clamp範囲）を確認
5. **pattern trial / render**：単音だけで判断できない場合は用途に合うAudition Patternを作り、`render pattern`または`render note` / `render events` / `render midi`でWAVを生成する。必要な事実を`--analyze`と`--trace`で取得する
6. **demo trial**：複数Instrumentをまとめて確認する場合は、[Demo仕様](references/demos.md)の操作の流れに沿って検証・Render・MIDI Exportを行う
7. **realtime trial**：`device list`でDeviceを確認し、MIDI Keyboardがある場合は`play`、ない場合は`audition pattern`で同じDefinitionを演奏する
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

Dynamic ParameterのIDはProcessorの配置でPrefixが決まり、Modulation RouteとParameter ChangeのTargetはこの形式で書きます。上例のLayer Filterは`layer.<layer_id>.processor.attack_tone.cutoff`、Reverbは`global.processor.space.mix`、Generatorは`layer.<layer_id>.generator.<name>`形式（Operator Modulationだけ`operator.<1-4>.<parameter>`）です。Prefixの一覧は[Modulation仕様](references/modulation.md)、ProcessorのFieldは[Processor仕様](references/processors.md)を参照し、Routeを書く前に`instrument inspect --json`で実際のParameter IDと配置を確認します。

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

Sample、Wavetable、Spectral、Granular、Wave Sequenceは外部WAVをAssetとして参照します。共通する扱いをまとめます。各Generatorへ渡す`asset.path`は、DefinitionのあるDirectoryを基準とした相対Pathにします。絶対Pathは動作しますが`ASSET_ABSOLUTE_PATH`のWarning対象で、Definitionの移植性を下げます。

### 配置と形式

- 形式はPCM 16/24 bitまたはFloat 32。Mono / Stereoを使用できます
- `sha256`は起動時の検証用。省略するとWarning、欠落・不一致・Decode失敗時はそのLayerだけが無効化されてRenderが継続します

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

GeneratorはLayerの`generator` Fieldへ、いずれか1つを指定します。各GeneratorのField・Range・Dynamic Parameter・制約は[Generator仕様](references/generators.md)を参照してください。Modulation Target IDは`layer.<layer_id>.generator.<name>`形式です（Operator Modulationだけ`operator.<1-4>.<parameter>`）。

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
- Warningが残る場合、Sonalloyは「他LayerでRenderを継続する」設計のため、`inspect`で意図しない無効化（Sample欠落など）がないかを確認する
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

## Patternで用途を試奏する

単音だけでは音色の判断が難しい場合は、1つのInstrumentへ送る演奏条件をAudition Patternへ記述します。Patternは曲全体や複数InstrumentのArrangementではなく、音源の用途を確認するためのNote、Chord、Phrase、Drum Pattern、Performance Control、Parameter Changeの入力です。SchemaとEventの詳細は[Pattern仕様](references/patterns.md)を参照してください。

```bash
sonalloy pattern init out/<name>/audition.json
sonalloy pattern validate out/<name>/audition.json
sonalloy pattern inspect out/<name>/audition.json
sonalloy render pattern <definition> out/<name>/audition.json \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --analyze --output out/<name>/audition.wav
```

音源の用途に応じて、次のような最小Patternを用意します。

| 音源の用途 | 試奏条件 |
|---|---|
| Bass | 短いBass Phrase。低音域とVelocity差を含める |
| Pad | 同じTickのChord。長いGateとReleaseを含める |
| Lead | PhraseとPitch Bend、AftertouchまたはMod Wheel |
| Drum Kit | Kick、Snare、Closed / Open Hi-Hatに対応するKeyのPattern |

Audio Deviceが使える場合は、MIDI Keyboardなしでも次で試奏できます。

```bash
sonalloy audition pattern <definition> out/<name>/audition.json --loop
```

MIDI Fileを既存Phraseとして使う場合は、Note Channelを1つ選んでPatternへ保存できます。

```bash
sonalloy pattern import-midi <phrase.mid> --channel 1 \
  --output out/<name>/phrase.json
sonalloy audition midi <definition> <phrase.mid> --channel 1
```

`Parameter Change`を含むPatternはStandard MIDIへExportできません（変換規則は[Pattern仕様](references/patterns.md)）。

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
| Warningが出た | `instrument inspect`で意図しないLayer無効化（Sample欠落など）がないか確認する |
| 音が鳴らない | `enabled: true`、`trigger`の範囲に発音するNote / Velocityが含まれているか確認する |
| Sampleが無視された | Asset PathとSHA-256の一致、WAV形式（PCM 16/24、Float 32）を確認する |
