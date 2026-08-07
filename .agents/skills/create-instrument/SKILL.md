---
name: create-instrument
description: Use ONLY when the user asks to create, edit, or debug a Sonalloy instrument definition (音源の作成・編集・修正), add a sample or Wavetable layer with a custom WAV, or render and listen to an instrument sound. Covers instrument init, JSON editing, validate / inspect, SHA-256 asset setup, and render note / midi. Not for CLI reference questions (docs/cli.md) or architecture questions (docs/architecture.md).
---

# Create Instrument

Sonalloyで音源（Instrument）を作成・編集・検証・試聴するための手順です。DefinitionはJSONの正本であり、ここでの変更は`docs/`配下の仕様と矛盾させないでください。

## 適用条件

| | 内容 |
|---|---|
| 対象 | 新規Instrumentの作成、既存Definitionの編集、Sample / Wavetable / Operator Modulation Layerの追加、音源の試聴・修正 |
| 対象外 | 仕様の説明（`docs/instrument-definition.md`）、CLIの全コマンド解説（`docs/cli.md`）、実行時挙動（`docs/runtime-processing.md`） |
| 成果物 | Definition JSONと、`render`で生成した試聴用WAV（`out/<name>/`配下） |

## 実行フロー

```text
Step 1  ひな形を生成する（新規時）／編集対象のJSONを特定する（既存時）
Step 2  Definitionを編集する
Step 3  instrument validate で検証する
Step 4  Wavetable / Operator Modulation Layerを追加する（使用する場合）
Step 5  Sample Layerを追加する（Sampleを使う場合）
Step 6  render note / render midi で試聴する
Step 7  仕上げる（関連docsへの反映、差分確認）
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
- Generatorは`oscillator`（`sine` / `saw` / `square` / `triangle` / `pulse`）、`noise`（`white` / `pink` / `brown`）、`wavetable`、`operator_modulation`、または`sample`
- Oscillatorの`waveform`は`{"type": "..."}`形式。Pulseは`pulse_width`、全Oscillatorは`phase_reset`と`phase`を持つ
- Wavetableは`asset`、`frame_length`（64〜4096の2の冪）、`position`（0〜1）、`phase_reset`、`phase`を持つ。Asset全体のSample数がFrame Lengthで割り切れることを確認する
- WavetableのSource Sample RateはPitchへ使われず、Compile時にResampleされない。SHA-256を指定し、`instrument inspect --json`でPrepared状態とBandを確認する
- Operator Modulationは4 Operator固定で、`algorithm`は`stack_4`、`stack_3_plus_carrier`、`two_stacks`、`fork_to_carrier`、`two_modulators_plus_carrier`、`three_modulators`、`shared_modulator`、`parallel`から選ぶ。Carrierだけに`level`を設定し、接続元だけに`modulation_amount`を設定する
- Operator Modulationの`mode`は`phase`、`frequency`、`amplitude`、`ring`。Phase / FrequencyのAmountは0〜8、Amplitude / Ringは0〜1で、AM / RingのFeedbackは0だけを許可する。Unisonは最大4 Voice
- 全Fieldの意味・単位・Rangeは`docs/instrument-definition.md`が正本

```bash
sonalloy instrument validate examples/instruments/<name>.json
```

Complex Oscillatorのphase_distortionとfeedbackはSineだけで使用でき、hard_syncとは併用しません。wavefoldは全Waveformで使用できます。3つのAmountは0〜1で、Parameter IDはlayer.<layer_id>.generator.phase_distortion、layer.<layer_id>.generator.wavefold、layer.<layer_id>.generator.oscillator_feedbackです。

## Step 3: 検証する

```bash
sonalloy instrument validate <definition>          # JSON Parse・Validation・Compileまで実行
sonalloy instrument inspect <definition> --json    # 実行値を機械可読で表示
```

- `validate`の成功は`valid <path>`。Warningは`print_warnings`で表示されるため必ず確認する
- `inspect`でPolyphony、Layer Trigger、GeneratorのWaveform / Color / Seed / Wavetable Band / Output Mode、Gain、Pan、Tuning、Envelope、Processor Chain、Modulation、Warningを確認する
- Operator Modulationでは`inspect --json`のMode、Algorithm、Evaluation Order、Carrier、4 OperatorのParameter ID、Envelope、Unison、Effective Frequency上限を確認する
- Complex Oscillatorではphase_domain Backend、Signal Order、DC Blocker、WavefolderのParameter IDも確認する
- Warningが1つでも残る場合は「ほかのLayerでRenderを継続する」設計のため、意図しない無効化がないかを確認する

## Step 4: Wavetable Layerを追加する（Wavetableを使う場合）

1. 周期波形をFrame順に連結したPCM16、PCM24、またはFloat32のWAVを用意する。MonoまたはStereoを使用できるが、StereoはCompile時にMonoへDownmixされる
2. 一周期のSample数を`frame_length`へ記録する。64〜4096の2の冪で、WAV全体のSample数が割り切れる値を選ぶ。Source Sample RateはPitchへ使われない
3. SHA-256を計算する

```powershell
# Windows
Get-FileHash -Algorithm SHA256 <path>   # 小文字のhexでJSONへ記録する
```

```bash
# Linux
sha256sum <path>
```

4. `layers`へ`generator.wavetable`を追加する

```json
{
  "generator": {
    "wavetable": {
      "asset": { "path": "<definitionからの相対Path>", "sha256": "<計算値>" },
      "frame_length": 2048,
      "position": 0.0,
      "phase_reset": true,
      "phase": 0.0
    }
  }
}
```

5. `validate`でFrame Layout、Hash、Silent Frame / DC Warningを確認し、`inspect --json`でPrepared状態、Band、Position Parameter ID、Output Modeを確認する。Assetの欠落・Hash不一致・Decode失敗ではWavetable Layerだけが無効化されてRenderが継続する

## Step 5: Operator Modulation Layerを追加する（Operator Modulationを使う場合）

1. `examples/instruments/operator-modulation-reference.json`を基に`generator.operator_modulation`を追加する
2. `algorithm`のTopologyに合わせてCarrierの`level`、接続元の`modulation_amount`、PM / FMの`feedback`を設定する。未使用Fieldは0にする
3. `instrument validate`で4 Operator、Range、Carrier、Feedback、Unisonを検証し、`instrument inspect --json`で固定TopologyとParameter Catalogを確認する
4. `render note`または`render events`でMode、Ratio、Index、Envelope、Note Release、Unisonを確認する。人間の確認項目は`docs/testing-and-sound-review.md`にまとめる

```json
{
  "generator": {
    "operator_modulation": {
      "mode": "frequency",
      "algorithm": "stack_4",
      "operators": [
        { "ratio": 1.0, "detune_cents": 0.0, "level": 0.9, "modulation_amount": 0.0, "feedback": 0.0, "phase": 0.0, "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.2, "sustain_level": 1.0, "release_seconds": 0.1 } },
        { "ratio": 2.0, "detune_cents": 0.0, "level": 0.0, "modulation_amount": 2.5, "feedback": 0.0, "phase": 0.0, "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.1, "sustain_level": 1.0, "release_seconds": 0.1 } },
        { "ratio": 3.0, "detune_cents": 0.0, "level": 0.0, "modulation_amount": 1.5, "feedback": 0.0, "phase": 0.0, "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.08, "sustain_level": 1.0, "release_seconds": 0.08 } },
        { "ratio": 5.0, "detune_cents": 0.0, "level": 0.0, "modulation_amount": 2.0, "feedback": 0.25, "phase": 0.0, "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.05, "sustain_level": 1.0, "release_seconds": 0.05 } }
      ],
      "phase_reset": true,
      "unison": null
    }
  }
}
```

## Step 6: Sample Layerを追加する（Sampleを使う場合）

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

## Step 7: 試聴する

単音の確認：

```bash
sonalloy render note <definition> \
  --note 60 --velocity 100 --gate 0.5 --tail 0.5 \
  --sample-rate 48000 --block-size 257 --output out/<name>/note.wav
```

フレーズの確認：

```bash
sonalloy render midi <definition> <midi-file> \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/<name>/phrase.wav
```

出力は32-bit float、2 Channel、指定Sample RateのStereo WAVです。試聴WAVは音源ごとに`out/<name>/`へ分けて出力し、親Directoryは事前に作成してください。生成後は`scripts/review/measure_wav.py`でFinite性・Peak / RMS / DCを確認できます。

## 失敗時の対処

| Exit Code | 意味 | 対処 |
|---:|---|---|
| `0` | 成功 | — |
| `1` | Definition / Compile Error | `--json`でDiagnosticsを取得し、Field Path付きのErrorを修正する |
| `2` | CLI入力またはRender Request Error | Option値（Sample Rate、Block Size、Tail、Frequency）を確認する |
| `3` | Core Process / Render Error | `--json`の`DSP_ERROR`等を確認する。それでも解決しない場合は`docs/runtime-processing.md`のError規則を確認する |
| `4` | WAV出力 Error | 出力先Directoryの存在と書き込み権限を確認する |

主な診断Code：`SCHEMA_UNSUPPORTED`、`JSON_INVALID`、`REQUIRED_FIELD_MISSING`、`ID_DUPLICATED`、`VALUE_OUT_OF_RANGE`、`LAYER_RANGE_INVALID`、`FILTER_CUTOFF_CLAMPED`、`ASSET_NOT_FOUND`、`ASSET_HASH_MISMATCH`、`ASSET_DECODE_FAILED`、`ASSET_RESAMPLED`、`ASSET_DOWNMIXED`。Wavetableでは`WAVETABLE_LAYOUT_INVALID`、`WAVETABLE_PREPARATION_FAILED`、`WAVETABLE_SILENT_FRAME`、`WAVETABLE_DC_OFFSET`、`GENERATOR_RESOURCE_LIMIT_EXCEEDED`も確認します。

## 参照

- `docs/creating-an-instrument.md` — 人間向けガイド（パラメータの意味、音作りの考え方）
- `docs/instrument-definition.md` — DefinitionのJSON仕様・制約
- `docs/cli.md` — Command・Option・Exit Code
- `docs/runtime-processing.md` — 実行時挙動
