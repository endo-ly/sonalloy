---
name: create-instrument
description: Use ONLY when the user asks to create, edit, or debug a Sonalloy instrument definition (音源の作成・編集・修正), add an Additive, Sample, Wavetable, Operator Modulation, or Granular layer with a custom WAV, or render and listen to an instrument sound. Covers instrument init, JSON editing, validate / inspect, SHA-256 asset setup, and render note / midi. Not for CLI reference questions (docs/cli.md) or architecture questions (docs/architecture.md).
---

# Create Instrument

Sonalloyで音源（Instrument）を作成・編集・検証・試聴するための手順です。DefinitionはJSONの正本であり、ここでの変更は`docs/`配下の仕様と矛盾させないでください。

## 適用条件

| | 内容 |
|---|---|
| 対象 | 新規Instrumentの作成、既存Definitionの編集、Additive / Sample / Wavetable / Operator Modulation / Granular / Wave Sequence Layerの追加、音源の試聴・修正 |
| 対象外 | 仕様の説明（`docs/instrument-definition.md`）、CLIの全コマンド解説（`docs/cli.md`）、実行時挙動（`docs/runtime-processing.md`） |
| 成果物 | Definition JSONと、`render`で生成した試聴用WAV（`out/<name>/`配下） |

## 実行フロー

```text
Step 1  ひな形を生成する（新規時）／編集対象のJSONを特定する（既存時）
Step 2  Definitionを編集する
Step 3  instrument validate で検証する
Step 4  Wavetable Layerを追加する（Wavetableを使う場合）
Step 5  Operator Modulation Layerを追加する（Operator Modulationを使う場合）
Step 6  Sample Layerを追加する（Sampleを使う場合）
Step 7  Granular Layerを追加する（Granularを使う場合）
Step 8  Wave Sequence Layerを追加する（Wave Sequenceを使う場合）
Step 9  Additive Layerを追加する（Additiveを使う場合）
Step 10 render note / render midi で試聴する
Step 11 仕上げる（関連docsへの反映、差分確認）
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
- Generatorは`oscillator`（`sine` / `saw` / `square` / `triangle` / `pulse`）、`noise`（`white` / `pink` / `brown`）、`wavetable`、`operator_modulation`、`sample`、`granular`、`wave_sequence`、または`additive`
- Oscillatorの`waveform`は`{"type": "..."}`形式。Pulseは`pulse_width`、全Oscillatorは`phase_reset`と`phase`を持つ
- Wavetableは`asset`、`frame_length`（64〜4096の2の冪）、`position`（0〜1）、`phase_reset`、`phase`を持つ。Asset全体のSample数がFrame Lengthで割り切れることを確認する
- WavetableのSource Sample RateはPitchへ使われず、Compile時にResampleされない。SHA-256を指定し、`instrument inspect --json`でPrepared状態とBandを確認する
- Operator Modulationは4 Operator固定で、`algorithm`は`stack_4`、`stack_3_plus_carrier`、`two_stacks`、`fork_to_carrier`、`two_modulators_plus_carrier`、`three_modulators`、`shared_modulator`、`parallel`から選ぶ。Carrierだけに`level`を設定し、接続元だけに`modulation_amount`を設定する
- Operator Modulationの`mode`は`phase`、`frequency`、`amplitude`、`ring`。Phase / FrequencyのAmountは0〜8、Amplitude / Ringは0〜1で、AM / RingのFeedbackは0だけを許可する。Unisonは最大4 Voice
- Sample Zoneは`asset`、MIDI範囲、`playback.region`、`direction`、`loop`、`time`を持つ。`time`は`{"mode":"resample"}`、`{"mode":"fixed_stretch","ratio":1.5}`、`{"mode":"tempo_sync","source_bpm":120.0}`のいずれかを指定する
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
- `inspect`でPolyphony、Layer Trigger、GeneratorのWaveform / Color / Seed / Wavetable Band / Granular Region / Wave Sequence Steps / Output Mode、Gain、Pan、Tuning、Envelope、Processor Chain、Modulation、Warningを確認する
- Operator Modulationでは`inspect --json`のMode、Algorithm、Evaluation Order、Carrier、4 OperatorのParameter ID、Envelope、Unison、Effective Frequency上限を確認する
- Complex Oscillatorではphase_domain Backend、Signal Order、DC Blocker、WavefolderのParameter IDも確認する
- Warningが1つでも残る場合は「ほかのLayerでRenderを継続する」設計のため、意図しない無効化がないかを確認する

## Step 4: Wavetable Layerを追加する（Wavetableを使う場合）

1. 周期波形をFrame順に連結したPCM16、PCM24、またはFloat32のWAVを用意する。MonoまたはStereoを使用できるが、WavetableはCompile時にMonoへDownmixされる
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

1. 自作WAVを`testdata/assets/`などへ置く（PCM 16/24 bitまたはFloat 32。MonoまたはStereoを使用できます）
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
      "interpolation": "cubic",
      "zones": [
        {
          "id": "main",
          "asset": { "path": "<definitionからの相対Path>", "sha256": "<計算値>" },
          "root_note": 60,
          "key_min": 0,
          "key_max": 127,
          "velocity_min": 1,
          "velocity_max": 127,
          "round_robin_group": null,
          "playback": {
            "region": { "start_seconds": 0.0, "end_seconds": null },
            "direction": "forward",
            "loop": null,
            "time": { "mode": "resample" }
          }
        }
      ]
    }
  }
}
```

4. `validate`でHash一致とWarningを確認する。SHA-256省略時はWarning、欠落・不一致・Decode失敗時はそのLayerだけが無効化されてRenderが継続する

## Step 7: Granular Layerを追加する（Granularを使う場合）

1. `generator.granular`へ、Sampleと同じAsset、基準Note、Region、Grain Parameter、Seedを指定する。RegionはPrepared Audio内の秒範囲です
2. `grain_size`は0.005〜0.5秒、`density`は1〜100 grains/sec、`pitch`は-2400〜2400 cents、`position`、`randomness`、`pan_spread`は0〜1で指定する
3. SHA-256を計算し、`instrument validate`でRegionとParameterの診断を確認する。`INVALID_GRAIN_REGION`または`INVALID_GRAIN_PARAMETER`があれば修正する
4. `instrument inspect --json`でPrepared状態、Source Channel、Region Frame、6つのParameter ID、Seed、Grain Pool Limitを確認する。GranularはMono AssetでもStereo Generatorとして出力する

```json
{
  "generator": {
    "granular": {
      "asset": { "path": "<definitionからの相対Path>", "sha256": "<計算値>" },
      "root_note": 60,
      "region": { "start_seconds": 0.0, "end_seconds": null },
      "position": 0.5,
      "grain_size": 0.08,
      "density": 24.0,
      "pitch": 0.0,
      "randomness": 0.35,
      "pan_spread": 0.75,
      "seed": 8128
    }
  }
}
```

`granular_position`、`grain_size`、`grain_density`、`grain_pitch`、`grain_randomness`、`grain_pan_spread`をModulation Targetへ指定できます。固定PositionでGrainを生成し続けるとFreeze、PositionをLFO等で動かすとScrubになります。

## Step 8: Wave Sequence Layerを追加する（Wave Sequenceを使う場合）

1. `generator.wave_sequence`へ1〜128個のStepをDefinition順で指定する。`direction`はStepの選択順、各Stepの`playback_direction`はAssetのRead方向です
2. `duration`は`{"mode":"seconds","value":...}`または`{"mode":"beats","value":...}`を指定する。`playback`が`one_shot`の場合、Assetが先に終わってもStepの残り時間は無音になります。`loop`の場合はRegionをStep終端まで繰り返します
3. `crossfade`を0〜0.5で指定し、隣接StepのConstant-power Overlapを設定する。Missing AssetはStepから削除されず、Durationを保持した無音として後続Stepへ進みます
4. SHA-256を指定し、`instrument validate`と`instrument inspect --json`でStep配列、Region Frame、Duration、Direction、Playback、Availability、Pitch、Gainを確認する

```json
{
  "generator": {
    "wave_sequence": {
      "root_note": 60,
      "direction": "forward",
      "loop": true,
      "crossfade": 0.25,
      "steps": [
        {
          "id": "attack",
          "asset": { "path": "<asset path>", "sha256": "<SHA-256>" },
          "region": { "start_seconds": 0.0, "end_seconds": 0.08 },
          "duration": { "mode": "seconds", "value": 0.18 },
          "playback": "loop",
          "playback_direction": "forward",
          "gain_db": -3.0,
          "pitch_cents": 0.0
        }
      ]
    }
  }
}
```

## Step 9: Additive Layerを追加する（Additiveを使う場合）

1. `examples/instruments/additive-generator-reference.json`を基に`generator.additive`を追加する
2. `partials`へ1〜64個の非負振幅を持つPartialを指定する。`id`は空でない一意の値、`ratio`は基音に対する周波数比、`phase`は初期位相です。Partialごとに任意のADSR Envelopeを指定できます
3. `morph`はA/Bの振幅スペクトルを補間し、`spectrum_tilt_db_per_octave`は倍音番号に対する傾き、`inharmonicity`は周波数比を非整数側へ曲げます。Modulation Targetには`additive_morph`、`additive_spectrum_tilt`、`additive_inharmonicity`を指定できます
4. `instrument validate`でPartial数、ID、比率、振幅、位相、Envelope、全消音を検証し、`instrument inspect --json`でPartial数、制御値、Parameter ID、Envelopeの有無を確認する

```json
{
  "generator": {
    "additive": {
      "phase_reset": true,
      "morph": 0.0,
      "spectrum_tilt_db_per_octave": -3.0,
      "inharmonicity": 0.15,
      "partials": [
        { "id": "fundamental", "ratio": 1.0, "amplitude_a": 1.0, "amplitude_b": 0.9, "phase": 0.0 },
        { "id": "second", "ratio": 2.0, "amplitude_a": 0.4, "amplitude_b": 0.7, "phase": 0.0,
          "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.2, "sustain_level": 0.55, "release_seconds": 0.12 } }
      ]
    }
  }
}
```

## Step 10: 試聴する

単音の確認：

```bash
sonalloy render note <definition> \
  --note 60 --velocity 100 --gate 0.5 --tail 0.5 --tempo 120 \
  --sample-rate 48000 --block-size 257 --output out/<name>/note.wav
```

フレーズの確認：

```bash
sonalloy render midi <definition> <midi-file> \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/<name>/phrase.wav
```

`render note`と`render events`の`--tempo`はTempo Syncの処理Tempoを指定します。`render midi`はMIDI内のTempo Meta EventからTempo Mapを作成します。出力は32-bit float、2 Channel、指定Sample RateのStereo WAVです。試聴WAVは音源ごとに`out/<name>/`へ分けて出力し、親Directoryは事前に作成してください。生成後は`scripts/review/measure_wav.py`でFinite性・Peak / RMS / DCを確認できます。

## Step 11: 仕上げる

- `metadata.name`と`metadata.description`を実際の音色に合わせる
- `validate`と`inspect --json`のWarning、Output Mode、Parameter IDを確認する
- 生成したWAVを同じ音量条件で試聴し、関連するReview結果を記録する

## 失敗時の対処

| Exit Code | 意味 | 対処 |
|---:|---|---|
| `0` | 成功 | — |
| `1` | Definition / Compile Error | `--json`でDiagnosticsを取得し、Field Path付きのErrorを修正する |
| `2` | CLI入力またはRender Request Error | Option値（Sample Rate、Block Size、Tail、Frequency）を確認する |
| `3` | Core Process / Render Error | `--json`の`DSP_ERROR`等を確認する。それでも解決しない場合は`docs/runtime-processing.md`のError規則を確認する |
| `4` | WAV出力 Error | 出力先Directoryの存在と書き込み権限を確認する |

主な診断Code：`SCHEMA_UNSUPPORTED`、`JSON_INVALID`、`REQUIRED_FIELD_MISSING`、`ID_DUPLICATED`、`VALUE_OUT_OF_RANGE`、`LAYER_RANGE_INVALID`、`FILTER_CUTOFF_CLAMPED`、`ASSET_NOT_FOUND`、`ASSET_HASH_MISMATCH`、`ASSET_DECODE_FAILED`、`ASSET_RESAMPLED`、`ASSET_DOWNMIXED`、`UNSUPPORTED_PLAYBACK_COMBINATION`、`INVALID_STRETCH_RATIO`、`INVALID_SOURCE_TEMPO`、`STRETCH_BACKEND_FAILURE`、`INVALID_GRAIN_REGION`、`INVALID_GRAIN_PARAMETER`。Wavetableでは`WAVETABLE_LAYOUT_INVALID`、`WAVETABLE_PREPARATION_FAILED`、`WAVETABLE_SILENT_FRAME`、`WAVETABLE_DC_OFFSET`、`GENERATOR_RESOURCE_LIMIT_EXCEEDED`も確認します。

## 参照

- `docs/creating-an-instrument.md` — 人間向けガイド（パラメータの意味、音作りの考え方）
- `docs/instrument-definition.md` — DefinitionのJSON仕様・制約
- `docs/cli.md` — Command・Option・Exit Code
- `docs/runtime-processing.md` — 実行時挙動
