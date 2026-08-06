---
name: create-instrument
description: Use ONLY when the user asks to create, edit, or debug a Sonalloy instrument definition (音源の作成・編集・修正), add a sample layer with a custom WAV, or render and listen to an instrument sound. Covers instrument init, JSON editing, validate / inspect, SHA-256 asset setup, and render note / midi. Not for CLI reference questions (docs/cli.md) or architecture questions (docs/architecture.md).
---

# Create Instrument

Sonalloyで音源（Instrument）を作成・編集・検証・試聴するための手順です。DefinitionはJSONの正本であり、ここでの変更は`docs/`配下の仕様と矛盾させないでください。

## 適用条件

| | 内容 |
|---|---|
| 対象 | 新規Instrumentの作成、既存Definitionの編集、Sample Layerの追加、音源の試聴・修正 |
| 対象外 | 仕様の説明（`docs/instrument-definition.md`）、CLIの全コマンド解説（`docs/cli.md`）、実行時挙動（`docs/runtime-processing.md`） |

## 実行フロー

```text
Step 1  ひな形を生成する（新規時）／編集対象のJSONを特定する（既存時）
Step 2  Definitionを編集する
Step 3  instrument validate で検証する
Step 4  Sample Layerを追加する（Sampleを使う場合）
Step 5  render note / render midi で試聴する
Step 6  仕上げる（関連docsへの反映、Git管理）
```

## Step 1: ひな形を生成する

```bash
sonalloy instrument init <path>
```

`init`はSaw Oscillatorの最小Definition（Polyphony 16、ADSR `0.005 / 0.18 / 0.65 / 0.3`、Gain `-14 dB`、Filter `12000 Hz / 0.12`）を生成します。既存Definitionを編集する場合はこのStepを省略し、対象JSONを特定します。

## Step 2: Definitionを編集する

編集対象は`metadata`、`performance`、`layers`、`voice_processors`、`global_processors`、`modulation`です。制約の要点：

- `schema_version`は`1`のみ。未知FieldはJSON Parse Errorになる
- `polyphony`は1〜64。`gain_db`は-60〜12、`pan`は-1〜1、`tuning_cents`は-1200〜1200
- ADSRは0〜30秒、Sustainは0〜1。Keyは0〜127、Velocityは1〜127
- Generatorは`oscillator`（`sine` / `saw` / `square` / `triangle` / `pulse`）、`noise`（`white` / `pink` / `brown`）、または`sample`
- Oscillatorの`waveform`は`{"type": "..."}`形式。Pulseは`pulse_width`、全Oscillatorは`phase_reset`と`phase`を持つ
- 全Fieldの意味・単位・Rangeは`docs/instrument-definition.md`が正本

```bash
sonalloy instrument validate examples/instruments/<name>.json
```

## Step 3: 検証する

```bash
sonalloy instrument validate <definition>          # JSON Parse・Validation・Compileまで実行
sonalloy instrument inspect <definition> --json    # 実行値を機械可読で表示
```

- `validate`の成功は`valid <path>`。Warningは`print_warnings`で表示されるため必ず確認する
- `inspect`でPolyphony、Layer Trigger、GeneratorのWaveform / Color / Seed / Output Mode、Gain、Pan、Tuning、Envelope、Processor Chain、Modulation、Warningを確認する
- Warningが1つでも残る場合は「ほかのLayerでRenderを継続する」設計のため、意図しない無効化がないかを確認する

## Step 4: Sample Layerを追加する（Sampleを使う場合）

1. 自作WAVを`testdata/assets/`などへ置く（PCM 16/24 bitまたはFloat 32。Mono推奨。StereoはCompile時にMonoへDownmixされる）
2. SHA-256を計算する

```powershell
# Windows
Get-FileHash -Algorithm SHA256 <path>   # 小文字のhexでJSONへ記録する
```

```bash
# Linux
sha256sum <path>
```

3. `layers`へ`generator.sample`を追加する

```json
{
  "generator": {
    "sample": {
      "asset": { "path": "<definitionからの相対Path>", "sha256": "<計算値>" },
      "root_note": 60,
      "playback_mode": "one_shot",
      "interpolation": "cubic"
    }
  }
}
```

4. `validate`でHash一致とWarningを確認する。SHA-256省略時はWarning、欠落・不一致・Decode失敗時はそのLayerだけが無効化されてRenderが継続する

## Step 5: 試聴する

単音の確認：

```bash
sonalloy render note <definition> \
  --note 60 --velocity 100 --gate 0.5 --tail 0.5 \
  --sample-rate 48000 --block-size 257 --output out/note.wav
```

フレーズの確認：

```bash
sonalloy render midi <definition> <midi-file> \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/phrase.wav
```

出力は32-bit float、2 Channel、指定Sample RateのStereo WAVです。親Directoryは事前に作成してください。生成後は`scripts/review/measure_wav.py`でFinite性・Peak / RMS / DCを確認できます。

## 失敗時の対処

| Exit Code | 意味 | 対処 |
|---:|---|---|
| `0` | 成功 | — |
| `1` | Definition / Compile Error | `--json`でDiagnosticsを取得し、Field Path付きのErrorを修正する |
| `2` | CLI入力またはRender Request Error | Option値（Sample Rate、Block Size、Tail、Frequency）を確認する |
| `3` | Core Process / Render Error | `--json`の`DSP_ERROR`等を確認する。それでも解決しない場合は`docs/runtime-processing.md`のError規則を確認する |
| `4` | WAV出力 Error | 出力先Directoryの存在と書き込み権限を確認する |

主な診断Code：`SCHEMA_UNSUPPORTED`、`JSON_INVALID`、`REQUIRED_FIELD_MISSING`、`ID_DUPLICATED`、`VALUE_OUT_OF_RANGE`、`LAYER_RANGE_INVALID`、`FILTER_CUTOFF_CLAMPED`、`ASSET_NOT_FOUND`、`ASSET_HASH_MISMATCH`、`ASSET_DECODE_FAILED`、`ASSET_RESAMPLED`、`ASSET_DOWNMIXED`。

## 参照

- `docs/creating-an-instrument.md` — 人間向けガイド（パラメータの意味、音作りの考え方）
- `docs/instrument-definition.md` — DefinitionのJSON仕様・制約
- `docs/cli.md` — Command・Option・Exit Code
- `docs/runtime-processing.md` — 実行時挙動
