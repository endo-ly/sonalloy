---
name: create-instrument
description: Use when creating, editing, or debugging Sonalloy instruments, or composing, previewing, auditioning, and rendering Score-based music in Sonalloy. Covers instrument definitions, custom WAV assets, Score YAML, validation, inspection, and audio rendering.
---

# Create Instrument

Sonalloyでは、音源をDefinition（JSON）で、楽曲と試奏をScore（YAML）で記述します。CLIで検証・レンダリングし、結果を聴いて調整します。

## 参照先

作業に応じて次の仕様を参照します。

| 作業 | 参照先 |
|---|---|
| プリセット探索 | `sonalloy preset list` |
| CLI操作 | 該当コマンドの `--help`、[CLI詳細仕様](references/cli.md) |
| Definition構造 | [Definition仕様](references/definition.md) |
| Generatorの設定 | [Generator仕様](references/generators.md) |
| Processorの設定 | [Processor仕様](references/processors.md) |
| Modulation | [Modulation仕様](references/modulation.md) |
| Score YAML | [Score仕様](references/score_spec.md) |

## 音源を制作する

### 音の役割を決めて構成する

最初に、持続音、打撃音、ノイズ、共鳴音など音の役割を定めます。ひな形は次のコマンドで作ります。

```bash
sonalloy instrument init <definition.json>
```

DefinitionはLayerの集合で、各LayerはGeneratorを1つ持ち、Gain、Pan、Tuning、ADSR、Processorを設定できます。同じVoiceで複数Layerを重ねます。

```text
Note
 ├─ Layer: Sample Attack ─ Layer Processor ─┐
 ├─ Layer: Oscillator Body ─ Layer Processor ├─ Voice Processor
 └─ Layer: Noise Air ────── Layer Processor ┘        │
                                             Global Processor → Output
```

役割に合うGeneratorの選択肢と、構成の例を次に示します。

| 音の役割 | 選択肢の例 |
|---|---|
| 波形と倍音を直接作る | Oscillator、Additive、Operator Modulation |
| 母音や共鳴を作る | Formant、Modal、Physical String |
| 録音素材を使う | Sample、Granular、Wave Sequence |
| 音色を時間変化させる | Wavetable、Spectral |
| Noiseや短い成分を加える | Noise、短いSample |

| 構成 | Layerの組合せ例 |
|---|---|
| Harmonic / Formant | Formant（共鳴）+ Additive（芯）+ Sample（Attack）+ Noise（Air） |
| Spectral | Spectral + Additive + Sample + Noise |
| Digital | Wavetable（持続音）+ Operator Modulation（倍音）+ Sample（短いAttack） |
| Physical / Modal | Physical String（励振）+ Modal（Body・共鳴） |

一度に多くのLayerを増やさず、各Layerの音を確かめてから重ねます。`render note` はDefinitionの有効なLayerすべてを鳴らすため、Layerごとの確認は、元と同じDirectoryに検証用コピーを作り、他を `enabled: false` にして行います。

### Definitionを組み立てる

Layerごとに発音条件とADSRを調整します。DecayとSustainは役割に応じて選び、Releaseは音の終わりが自然になる長さにします。Generator、Processor、Modulationの設定は各仕様を参照し、実際のParameter IDと範囲は `instrument inspect --json` で確認して設定します。

### WAV Assetを用意する

Sample、Wavetable、Spectral、Granular、Wave Sequence、ConvolutionはWAVを参照できます。PathはDefinitionのあるDirectoryからの相対Pathまたは絶対Pathで、共有するDefinitionでは相対Pathを使うと移動しやすくなります。`sha256` を記録する場合は次のコマンドで取得します。

```bash
sha256sum <path>                                    # Linux
Get-FileHash -Algorithm SHA256 <path>               # Windows
```

Assetの形式、準備規則、失敗時の診断は[Definition仕様](references/definition.md)に記載しています。診断されたPath、Hash、WAV形式を修正して再検証します。

## 楽曲を制作する

`sonalloy preset list` で収録音源を確認します。管理用IDとScoreから参照するPreset値の両方を見ながら、音域や発音特性に合う音色を選びます。

`score init <path>` でScore YAMLのひな形を作り、曲の構成、調性、テンポ、拍子、楽器、旋律や伴奏を編集します。文法と演奏意味論は[Score仕様](references/score_spec.md)に記載しています。制作時は、旋律だけでなく低音、和声、リズム、音色のアタックと余韻を合わせて確認し、各楽器の音量と定位を調整してから複数楽器をまとめて聴きます。

プリセットの試聴は、同じ演奏内容を詳しく聴く場合は `preview.score.yaml`、別の演奏内容で奏法を試す場合は `showcase.score.yaml`、複数音源で共用する試奏は `presets/common-scores/` を使います。外部Definitionを割り当てる楽器には `--bind` を指定します。MIDIの取り込みは `score import-midi`、書き出しは `score export-midi` で、変換で保持・省略される情報はScore仕様に記載しています。

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

Definitionは次の順で確認します。Modulationなど発音中の値の変化は `--trace` で観測します。

```bash
sonalloy instrument validate <definition>
sonalloy instrument inspect <definition> --json
sonalloy render note <definition> --note 60 --velocity 100 --gate 0.5 --tail 0.5 \
  --analyze --output out/<name>/note.wav --json
```

Scoreは検証後に展開結果を確認してからRenderします。

```bash
sonalloy score validate <score.yaml>
sonalloy score inspect <score.yaml> --summary --json
sonalloy render score <score.yaml> --output out/song.wav --analyze --json
```

共通Scoreでプリセットを試聴するときは、Definitionを束縛して `render score` / `audition score` を実行します。Audio Deviceで再生する前に `device list` でIDを確認します。

```bash
sonalloy render score presets/common-scores/lead.score.yaml \
  --bind preview=<definition> --output out/<name>/phrase.wav --analyze --json
```

Inspect、Analysis、Traceが公開する情報はCLIのInterfaceから取得し、ソースコードを読んだり同じ値を再計算する外部Scriptを作ったりしません。

## 評価と仕上げ

音源を比べるときは、同じNote、Velocity、Gate、Tail、Sample Rateでレンダリングします。`instrument report --loudness` は複数Definitionの音響とIntegrated LUFS、True Peakを同じ条件で比較できます。楽曲では `render score --analyze --json` のPart・Section別の解析を使い、数値を音源の役割や演奏密度と合わせて判断します。必要な音量調整はScoreの楽器GainやSection Gainに反映し、再度Mixを確認します。

Modulationが意図した範囲を動くかは、InspectでParameter IDと許容範囲、Traceで実際の変化を確認します。最大Velocityでの過大な出力は次のRenderで確認し、Analysisの結果にクリップ（フルスケール超過）がないことを確かめます。

```bash
sonalloy render note <definition> --note 60 --velocity 127 --analyze --output out/<name>/max-velocity.wav
```

最後に生成したWAVと完成Scoreを試聴し、Warningを含め診断を確認します。問題があれば該当するDefinitionやScoreを修正して、検証と試聴を繰り返します。
