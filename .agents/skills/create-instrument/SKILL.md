---
name: create-instrument
description: Use when creating, editing, or debugging Sonalloy instruments, or composing, previewing, auditioning, and rendering Score-based music in Sonalloy. Covers instrument definitions, custom WAV assets, Score YAML, validation, inspection, and audio rendering.
---

# Create Instrument

Sonalloyでは、JSONで音源を定義し、Score YAMLで楽曲や試奏を記述します。CLIで検証・展開・レンダリングし、結果を聴いて調整します。

## 概要と参照方法

操作方法と仕様の参照先を作業別に示します。

| 作業 | 参照先 |
|---|---|
| プリセット探索 | `sonalloy preset list` |
| CLI操作 | 該当コマンドの `--help` |
| Definition構造 | [Definition仕様](references/definition.md) |
| Generatorの設定 | [Generator仕様](references/generators.md) |
| Processorの設定 | [Processor仕様](references/processors.md) |
| Modulation | [Modulation仕様](references/modulation.md) |
| Score YAML | [Score仕様](references/score_spec.md) |
| 詳細Report・診断 | [CLI詳細仕様](references/cli.md) |

音源の種類や音響上の課題を決め、Definitionを作成・編集します。楽曲で確認する場合は適切な音源を選び、Scoreを検証してからレンダリングします。Inspect、Analysis、Traceで確認できる情報はCLIから取得し、その結果をもとに修正します。

## 音源を制作する

### 音の役割を決めて構成する

最初に、持続音、打撃音、ノイズ、共鳴音など音の役割を定めます。ひな形を作る場合は次のコマンドを使います。

```bash
sonalloy instrument init <definition.json>
```

DefinitionはLayerの集合です。各LayerはGeneratorを1つ持ち、Gain、Pan、Tuning、ADSR、Processorを設定できます。同じVoiceで複数Layerを重ねると、例えばSampleでAttackを加え、OscillatorやAdditiveで持続する芯を作れます。

```text
Note
 ├─ Layer: Sample Attack ─ Layer Processor ─┐
 ├─ Layer: Oscillator Body ─ Layer Processor ├─ Voice Processor
 └─ Layer: Noise Air ────── Layer Processor ┘        │
                                             Global Processor → Output
```

| 音の役割 | 選択肢の例 |
|---|---|
| 波形と倍音を直接作る | Oscillator、Additive、Operator Modulation |
| 母音や共鳴を作る | Formant、Modal、Physical String |
| 録音素材を使う | Sample、Granular、Wave Sequence |
| 音色を時間変化させる | Wavetable、Spectral |
| Noiseや短い成分を加える | Noise、短いSample |

複数の音源方式を使う場合は、一度に増やしすぎず、各Layerの役割を確かめてから重ねます。Layer単独の音を聴くには、元Definitionと同じDirectoryに検証用コピーを作り、対象以外のLayerを`enabled: false`にしてから`render note`を実行します。`render note`はDefinition全体の有効なLayerを鳴らすため、Layer選択Optionはありません。

| 構成 | Layerの組合せ例 |
|---|---|
| Harmonic / Formant | Formant（共鳴）+ Additive（芯）+ Sample（Attack）+ Noise（Air） |
| Spectral | Spectral + Additive + Sample + Noise |
| Digital | Wavetable（持続音）+ Operator Modulation（倍音）+ Sample（短いAttack） |
| Physical / Modal | Physical String（励振）+ Modal（Body・共鳴） |

### Definitionを組み立てる

Layerごとに発音条件とADSRを調整します。DecayとSustainは役割に応じて選び、Releaseは音の終わりが自然になる長さにします。Generator、Processor、Modulationは、前節の検証用Definitionで効果を確かめ、最後に有効なLayerを戻して全体を聴きます。

実際のParameter IDと範囲は `instrument inspect --json` で確認して設定します。

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

VelocityとKey Trackingは組み込みSourceのため、Source定義なしでRouteから参照できます。

### WAV Assetを用意する

Sample、Wavetable、Spectral、Granular、Wave Sequence、ConvolutionではWAVを参照できます。PathはDefinitionのあるDirectoryからの相対Path、または絶対Pathです。共有するDefinitionでは相対Pathを使うと移動しやすくなります。

`sha256`を指定する場合は、WAVのSHA-256を記録します。

```bash
# Linux
sha256sum <path>

# Windows
Get-FileHash -Algorithm SHA256 <path>
```

Assetの欠落、Hash不一致、Decodeや準備の失敗はValidationまたはCompile時の診断になります。診断されたPath、Hash、WAV形式を修正して再検証します。

## 楽曲を制作する

### 用途に合う音源を選ぶ

`sonalloy preset list`で収録音源を確認し、管理用IDとScoreから参照するPreset値の両方を見ます。音色名、説明、カテゴリ、タグなどで一覧を絞り込み、カテゴリを演奏上の役割として固定せず、音域や発音特性に合う音色を選びます。

一覧の説明はビルド時に埋め込まれたメタデータです。Scoreを演奏・レンダリングする環境には、Preset Definitionと必要なAssetも必要です。

### Scoreで楽曲を記述する

`score init <path>`でScore YAMLのひな形を作り、曲の構成、調性、テンポ、拍子、楽器、旋律や伴奏を編集します。音符ごとの強弱・接続と奏法、強弱の流れ、コードの食いと声部配置、楽器ごとのスウィングとタイミングを記述できます。

制作時は、旋律だけでなく、低音、和声、リズム、音色のアタックと余韻を合わせて確認します。セクション間で素材を再利用し、曲全体で音域や密度に変化をつけます。各楽器の音量や定位を調整してから、複数楽器をまとめて聴きます。

Presetの試聴Scoreは `preview.score.yaml` を使い、同じ演奏内容を詳細に試聴する場合も同じファイルを使います。別の演奏内容で奏法を試す場合は `showcase.score.yaml` を用意します。共通Scoreは `presets/common-scores/` にあり、外部Definitionを割り当てる楽器には `--bind` を指定します。

MIDIをScoreへ取り込む場合は `score import-midi`、ScoreをMIDIへ書き出す場合は `score export-midi` を使います。ScoreとMIDIの変換で保持・省略される情報は[Score仕様](references/score_spec.md)に記載しています。

## 検証と試聴

作業対象と確認したい内容に応じてコマンドを選びます。

| 対象 | コマンド | 確認する内容 |
|---|---|---|
| Definition | `instrument validate` | JSON、制約、Assetの準備 |
| Compile後の音源 | `instrument inspect` | 実行値、Parameter ID、Route、Processor、Warning |
| 音源候補の比較 | `instrument report` | 条件をそろえた音響解析、必要に応じて音量 |
| 単音 | `render note` | 音色、音域、Velocity、ADSR |
| 発音中の操作 | `render events` | Parameter Change、Ramp、Pedal、Bendなど |
| MIDIフレーズ | `render midi` | MIDI演奏とControlの反映 |
| Score | `score validate`、`score inspect` | 構文・参照・演奏内容、展開後Timeline |
| 楽曲 | `render score` | Part、Section、Mix、Stem、Master |
| Realtime試聴 | `audition`、`play` | Audio DeviceやMIDI Inputを使う演奏 |

Definitionは次の順で確認します。

```bash
sonalloy instrument validate <definition>
sonalloy instrument inspect <definition> --json
sonalloy render note <definition> --note 60 --velocity 100 --gate 0.5 --tail 0.5 \
  --analyze --output out/<name>/note.wav --json
```

発音中のParameterやControl Eventを確認するときは `render events`、Phrase全体の演奏を確認するときは `render midi` を使います。Modulationの実行中の値は `--trace` で観測します。

Scoreでは、検証後に展開結果を確認してからRenderします。

```bash
sonalloy score validate <score.yaml>
sonalloy score inspect <score.yaml> --summary --json
sonalloy render score <score.yaml> --output out/song.wav --analyze --json
```

複数楽器のScoreから特定の楽器や区間を確認するときは、`render score`や`audition score`の選択Optionをヘルプで確認します。Audio Deviceで再生する前に `device list` でIDを確認します。プリセットの試聴は次のようにDefinitionを束縛します。

```bash
sonalloy score validate presets/common-scores/lead.score.yaml --bind preview=<definition>
sonalloy render score presets/common-scores/lead.score.yaml \
  --bind preview=<definition> --output out/<name>/phrase.wav --analyze --json
sonalloy audition score presets/common-scores/lead.score.yaml --bind preview=<definition> --loop
```

Inspect、Analysis、Traceが公開する値を取得するためにRuntime Source Codeを読んだり、同じ値を再計算する外部Scriptを作ったりしません。製品Interfaceにない研究や一回限りの人向け分析が必要なときに、外部ツールを使います。

## 評価と仕上げ

音源を比べるときは、同じNote、Velocity、Gate、Tail、Sample Rateでレンダリングします。`instrument report --loudness`は複数Definitionの音響とIntegrated LUFS、True Peakを同じ条件で比較できます。楽曲では `render score --analyze --json` のPart・Section別の解析を使い、数値を音源の役割や演奏密度と合わせて判断します。必要な音量調整はScoreの楽器GainやSection Gainに反映し、再度Mixを確認します。

Modulationが意図した範囲を動くかはInspectでParameter IDと許容範囲を確認し、Traceで実際の変化を確認します。最大Velocityでの過大な出力も確認します。

```bash
sonalloy render note <definition> --note 60 --velocity 127 --analyze --output out/<name>/max-velocity.wav
```

Analysisの`level.over_full_scale`が`false`であることを確認し、最後に生成したWAVと完成Scoreを試聴します。Warningを含め診断を確認し、問題があれば該当DefinitionやScoreを修正して検証・試聴を繰り返します。
