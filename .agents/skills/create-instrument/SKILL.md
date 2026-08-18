---
name: create-instrument
description: Use ONLY when the user asks to create, edit, or debug a Sonalloy instrument definition (音源の作成・編集・修正), add an Additive, Formant, Sample, Wavetable, Spectral, Operator Modulation, Granular, Physical String, or Modal layer with a custom WAV, or render and listen to an instrument sound. Covers instrument init, JSON editing, validate / inspect, SHA-256 asset setup, and render note / midi.
---

# Create Instrument

Sonalloyで音源（Instrument）を作成・編集・検証・試聴するための手順書です。DefinitionはJSONの正本であり、ここでの変更は`docs/instrument-definition.md`の仕様と矛盾させないでください。


## 全体フロー

```text
init → edit → validate → inspect → render/analyze/trace → optional realtime trial → refine
```

1. **init**：新規Definitionのひな形を生成（既存を編集する場合は省略）
2. **edit**：Generator、ADSR、Processor、Modulationを編集
3. **validate**：`instrument validate`でJSON、制約、Asset準備を検証
4. **inspect**：`instrument inspect --json`でCompile後のUnit、Source Polarity、Route Effect、Clamp範囲を確認
5. **render/analyze/trace**：`render note` / `render events` / `render midi`でWAVを生成し、必要な事実を`--analyze`と`--trace`で取得
6. **realtime trial**：Deviceが利用できる場合は`device list`で確認し、同じDefinitionを`play`でMIDI演奏する
7. **refine**：数値・音色・`metadata`を整理し、再度InspectとRenderを実行

## Definitionを編集する

### ひな形を生成する（新規時）

```bash
sonalloy instrument init <path>
```

Saw Oscillatorの最小Definition（Polyphony 16、ADSR `0.005 / 0.18 / 0.65 / 0.3`、Gain `-14 dB`、Voice ProcessorのFilter `12000 Hz / 0.12`）が生成されます。

### トップレベルの構造

各Fieldの単位・Rangeの正本は`docs/instrument-definition.md`です。

| Field | 内容 | 主な制約 |
|---|---|---|
| `schema_version` | スキーマ版 | `2`。`1`はUnsupported。未知FieldはJSON Parse Error |
| `metadata` | `name`、`description` | — |
| `performance` | `polyphony` | 1〜64 |
| `layers` | 発音の単位となるLayer配列 | [Layerの構造](#layerの構造)参照 |
| `voice_processors` | 全LayerのMix後に直列適用するProcessor Chain | — |
| `global_processors` | 全Voiceの合計後に直列適用するProcessor Chain（Delay / ReverbはTailを保持） | — |
| `modulation` | SourceとRouteの定義 | [ProcessorとModulation](#processorとmodulation)参照 |

### Layerの構造

音源は1つ以上のLayerで構成されます。Layerは同じVoice内でMixされ、Layerごとに独立したADSR・Gain・Pan・Tuningを持ちます。

```text
Note On
  │
  ▼
Layer 1 → Layer Processor → ADSR → Layer Gain / Pan ─┐
                                                     ├→ Voice Processor → Global Processor → Stereo 出力
Layer 2 → Layer Processor → ADSR → Layer Gain / Pan ─┘
```

| Field | 内容 | 主な制約 |
|---|---|---|
| `id` | Layer識別子 | 一意 |
| `enabled` | 発音の有無 | — |
| `trigger` | 発音条件（`event`、`key_min` / `key_max`、`velocity_min` / `velocity_max`） | Key 0〜127、Velocity 1〜127。`note_off` LayerはNote Onで待機状態になり、対応するNote Offで発音 |
| `gain_db` | Layer音量 | -60〜12 |
| `pan` | 左右位置（-1 = 左、0 = 中央、1 = 右） | -1〜1。定電力で定位 |
| `tuning_cents` | 音程調整（100 = 半音） | -1200〜1200 |
| `envelope` | ADSR | [ADSRで音の輪郭を作る](#adsrで音の輪郭を作る)参照 |
| `processors` | Generator後に直列適用するFilter / Drive | 配列順 |
| `generator` | 音源 | [Generator](#generator)参照 |

Oscillator Layerの全体例：

```json
{
  "id": "main",
  "enabled": true,
  "trigger": { "event": "note_on", "key_min": 0, "key_max": 127, "velocity_min": 1, "velocity_max": 127 },
  "gain_db": -14.0,
  "pan": 0.0,
  "tuning_cents": 0.0,
  "envelope": { "attack_seconds": 0.005, "decay_seconds": 0.18, "sustain_level": 0.65, "release_seconds": 0.3 },
  "processors": [],
  "generator": { "oscillator": { "waveform": { "type": "saw" }, "phase_reset": true, "phase": 0.0 } }
}
```

### ADSRで音の輪郭を作る

```text
Level
  ▲
  │        ┌──── sustain ────┐
  │       ╱                  ╲
  │      ╱                    ╲
  │     ╱                      ╲
  │    ╱                        ╲
  └───┴──────────────────────────┴───▶ Time
    attack   decay            release
```

| Parameter | 役割 | Range / 目安 |
|---|---|---|
| `attack_seconds` | Note Onから最大音量へ達する時間 | 0〜30秒。0で瞬発、数秒でうねり |
| `decay_seconds` | 最大音量からSustain Levelへ下がる時間 | 0〜30秒。0.05〜0.3が一般的 |
| `sustain_level` | Note On中の音量 | 0〜1。0で短い音、1で伸びる音 |
| `release_seconds` | Note Offから無音へ至る時間 | 0〜30秒。0でバツンと切れる |

### ProcessorとModulation

- **Processor**：Layer / Voice / Globalの3段階で直列適用するFilter / Drive / Delay / Reverb。`cutoff_hz`、`resonance`、`amount`、`mix`などのDynamic Parameterを持ちます
- **Modulation**：Velocity、Key Tracking、LFO、Envelope、RandomなどのSourceをDynamic Parameterへ接続します

```json
"modulation": {
  "routes": [
    { "source": "velocity", "target": "layer.main.gain", "depth": { "value": 8.0, "unit": "decibels" }, "curve": "linear" },
    { "source": "lfo", "target": "voice.processor.tone.cutoff", "depth": { "value": 1.5, "unit": "octaves" }, "curve": "linear" }
  ],
  "sources": [
    { "id": "lfo", "type": "lfo", "waveform": "sine", "rate_hz": 0.5, "phase": 0.0 }
  ]
}
```

VelocityとKey Trackingは組み込みSourceのため、Source定義なしで`routes`から参照できます。Target ID・Range・Curveの正本は`docs/instrument-definition.md`です。

Routeの`depth.value`はTargetに意味のあるUnitで書きます。Linear TargetはNative Domainへ加算し、Log2 TargetはOctave Domainへ加算します。たとえばTuningの`20 cents`、Filter Cutoffの`2 octaves`、Gainの`-9 decibels`のように、旧来の全Rangeに対する割合へ換算しません。

### 数値の意味を読む

音色設計で迷いやすい値のEndpointと実装式は次のとおりです。より詳細なRangeは`docs/instrument-definition.md`で確認します。

| Field | 意味 |
|---|---|
| `waveshaping.amount` | 0はBypass。`shape = 1 + amount × 3`、正規化`tanh` WetをAmountでDryからCrossfade |
| `phase_distortion.amount` | 0はIdentity。Breakpointは`0.5 - amount × 0.45`、1で0.05 |
| `wavefold.amount` | 0はBypass。DaisySP Driveは`1 + amount × 7`、Wet量はAmount |
| `feedback.amount` | 0は無効。Phase寄与は`(tanh(previous × amount × 2.5)) × 0.25` |
| `drive.amount` / `drive.mix` | Amount 0はIdentity、Shapeは`amount × 4`。Mix 0はDry、1はWetのLinear Crossfade |
| `morph` / `position` | MorphはA→B。Positionは対象Source Domainの開始→終了 |
| `stereo_correlation` | 0は左右独立、1は同一 |
| `pan_spread` / Unison spread | 0は中央、1は設定可能な最大配置幅 |
| `freeze` | 0は通常走査、1はFrame固定（Phaseは進む） |
| `formant.throat` | 0.5がBandwidth不変。0〜1で0.5〜2倍 |
| Operator `modulation_amount` | Phaseは合計へ0.5を掛けたPhase Offset、Frequencyは`frequency × (1 + sum + feedback_offset)`、Amplitudeは`1 + output × amount`の積、RingはCarrierとProductのCrossfade |

> **重要**：Inspect、Analysis、Traceが既に公開している事実を得るために、RuntimeのSource Codeを読んだり、同じ値を再計算する外部Python解析を作ったりしないでください。製品Interfaceで不足する研究や一回限りの人間向け分析に限り、外部ツールを使えます。

## Asset（WAV）を扱う

Sample、Wavetable、Spectral、Granular、Wave Sequenceは外部WAVをAssetとして参照します。共通する扱いをまとめます。各Generatorへ渡す`asset.path`はDefinitionのあるDirectoryを基準とした相対Path（または絶対Path）です。

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

GeneratorはLayerの`generator` Fieldへ指定します。Modulation Target IDは`layer.<layer_id>.generator.<name>`形式で、各節では`<name>`のみを列挙します。

| Generator | 用途 |
|---|---|
| [Oscillator](#oscillator) | 基本波形（Sine / Saw / Square / Triangle / Pulse）とComplex変形 |
| [Noise](#noise) | White / Pink / Brown Noise |
| [Wavetable](#wavetable) | 周期波形Frame列のPosition走査 |
| [Spectral](#spectral) | WAVのSTFT再構成とA/B Morph |
| [Operator Modulation](#operator-modulation) | 4 Operator FM / PM / AM / Ring |
| [Sample](#sample) | 鍵盤範囲別のSample再生 |
| [Granular](#granular) | Grain分解によるTexture再構成 |
| [Wave Sequence](#wave-sequence) | 複数Assetの時系列切り替え |
| [Additive](#additive) | Partial直接設計による倍音構成 |
| [Formant](#formant) | 母音共鳴のBand制御 |
| [Physical String](#physical-string) | Fractional Delay Feedbackによる弦・硬質振動 |
| [Modal](#modal) | 複数Modeの共鳴によるBody・Bell・Plate |

### Oscillator

基本波形にHard Sync、Waveshaping、Phase Distortion、Wavefold、Feedback、Unisonを加えられます。WaveformはTagged Objectで、Pulseは`pulse_width`（0〜1）を持ちます。

```json
"generator": {
  "oscillator": {
    "waveform": { "type": "sine" },
    "phase_reset": true,
    "phase": 0.0,
    "hard_sync": { "ratio": 3.0 },
    "waveshaping": { "amount": 0.25 },
    "phase_distortion": { "amount": 0.55 },
    "wavefold": { "amount": 0.25 },
    "feedback": { "amount": 0.3 },
    "unison": { "voices": 5, "detune_cents": 18.0, "stereo_spread": 0.8, "phase_spread": 0.0 }
  }
}
```

| 項目 | 制約 |
|---|---|
| `phase_distortion` / `feedback` | Sineだけで使用可能。Hard Syncとは併用不可 |
| `wavefold` | 全Waveformで使用可能 |
| `hard_sync` | Sineでは使用不可。併用時の`phase`と`phase_spread`は0 |
| 3つのAmount | 0〜1 |
| `unison.voices` | 最大5 Voice |

Modulation Target：`pulse_width`、`sync_ratio`、`waveshape`、`phase_distortion`、`wavefold`、`oscillator_feedback`、`unison_detune`、`unison_spread`

`instrument inspect --json`で`phase_domain` Backend、信号順序、DC Blocker、各Parameter IDを確認します。

### Noise

White / Pink / Brown Noiseを生成します。Stereo Correlationは0で左右独立、1で左右同一です。

```json
"generator": {
  "noise": {
    "color": "pink",
    "seed": 812347,
    "stereo_correlation": 0.65
  }
}
```

### Physical String

弦を弾く、はじく、または硬い振動体を作るときに使います。Deterministic ExciterをFractional Delay Feedbackへ入力するMono Generatorです。`decay_seconds`はNominal Loop T60、`brightness`はLoopの高域Loss、`stiffness`はDispersionを表します。特定のGuitarやPianoを再現するModelではありません。

```json
"generator": {
  "physical_string": {
    "exciter": { "type": "noise_burst", "duration_seconds": 0.006, "brightness": 0.82, "seed": 4001 },
    "decay_seconds": 2.4,
    "brightness": 0.68,
    "stiffness": 0.18
  }
}
```

`exciter.type`は`impulse`または`noise_burst`です。Noise Burstの`duration_seconds`は0.0005〜0.100、Exciter `brightness`は0〜1です。Dynamic Parameterは`physical_string_decay_seconds`、`physical_string_brightness`、`physical_string_stiffness`の3つで、`decay_seconds`だけ`Seconds + Log2`です。Pitchを作るのはLayer NoteとTuningであり、Generator独自のPitch Parameterは追加しません。

### Modal

棒、板、ベル、金属、木、ガラス、膜的な共鳴を作るときに使います。Rust側のExciterをPinned DaisySPの低レベル`Resonator`へ渡すMono Generatorです。`mode_count`は4 / 8 / 12 / 16 / 20 / 24のStatic Fieldで、`structure`はMode間隔、`brightness`は高次Modeの残留、`decay`は共鳴の長さを制御します。

```json
"generator": {
  "modal": {
    "exciter": { "type": "impulse" },
    "mode_count": 24,
    "structure": 0.72,
    "brightness": 0.76,
    "decay": 0.66
  }
}
```

Dynamic Parameterは`modal_structure`、`modal_brightness`、`modal_decay`です。`mode_count`とExciterのStatic FieldはParameter ChangeやModulation RouteのTargetにしません。Mode Countを増やすと共鳴密度とCPU負荷が増えますが、実在楽器名をGeneratorのModel名として扱わず、Layer・Processor・Hybridで音色を作ります。

### Wavetable

周期波形をFrame順に連結したWAVをFrame単位で走査します。

1. 1周期のSample数を`frame_length`として、64〜4096の2の冪で、WAV全体のSample数が割り切れる値を選ぶ
2. SHA-256を計算して`asset.sha256`へ記録する
3. Position 0 / 0.5 / 1はそれぞれ最初 / 中間 / 最後のFrame側に対応する

```json
"generator": {
  "wavetable": {
    "asset": { "path": "<WAVへの相対Path>", "sha256": "<計算値>" },
    "frame_length": 2048,
    "position": 0.0,
    "phase_reset": true,
    "phase": 0.0
  }
}
```

| 確認項目 | 内容 |
|---|---|
| `validate` | Frame配置、Hash、無音Frame / DC Warning |
| `inspect --json` | 準備済み状態、Band、Position Parameter ID、実効周波数上限 |

Modulation Target：`wavetable_position`

### Spectral

WAVをSTFT解析して再構成します。Morphを使う場合は`asset_b`へ同じChannel数のWAVを指定します。

```json
"generator": {
  "spectral": {
    "asset_a": { "path": "<WAVへの相対Path>", "sha256": "<計算値>" },
    "asset_b": null,
    "root_note": 60,
    "fft_size": 2048,
    "position": 0.0,
    "freeze": 0.0,
    "blur_seconds": 0.0,
    "shift_hz": 0.0,
    "morph": 0.0,
    "phase_reset": true
  }
}
```

| Parameter | 意味 | Range |
|---|---|---|
| `fft_size` | FFT Size。Hopは1/4、報告Latencyは`fft_size - hop_size` | 1024 / 2048 / 4096 |
| `position` | Source開始位置へ自然走査を加える | 0〜1 |
| `freeze` | 走査速度を下げ、1でFrame固定 | 0〜1 |
| `blur_seconds` | 時間方向の振幅Smoothing | 0〜1秒 |
| `shift_hz` | 各Spectral成分をHz加算移動 | -12000〜12000 Hz |
| `morph` | A/B正規化タイムライン上の補間 | 0〜1 |

- `asset_b`を指定したときだけMorph Parameterが一覧へ登録され、準備失敗時はAだけへフォールバックせずLayerが無効化されます
- MIDI NoteとLayer TuningはRoot NoteからのPitch比として適用され、Source Durationは変わりません
- 元波形への再合成を確認する際は5 Parameterを0へ揃え、Latency後で元WAVと比較します
- Latencyは他Layerへも補償されるため、Transientの時間位置はHybrid全体で確認します

Modulation Target：`spectral_position`、`spectral_freeze`、`spectral_blur`、`spectral_shift`、`spectral_morph`

`inspect --json`でAsset A/Bの準備済み状態、Source Channel、Spectral Frame、準備済みSample Rate、FFT / Hop / Bin、Latency、5 Parameter IDを確認します。

### Operator Modulation

4つのSine OperatorでFM / PM / AM / Ringを作ります。Carrierだけに`level`、接続元だけに`modulation_amount`を設定します。

```json
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
```

| 項目 | 内容 |
|---|---|
| `algorithm` | `stack_4`、`stack_3_plus_carrier`、`two_stacks`、`fork_to_carrier`、`two_modulators_plus_carrier`、`three_modulators`、`shared_modulator`、`parallel` |
| `mode` | `phase` / `frequency` / `amplitude` / `ring`。Phase / FrequencyのAmountは0〜8、Amplitude / Ringは0〜1 |
| Feedback | AM / Ringでは0だけを許可 |
| `unison.voices` | 最大4 Voice |

`stack_4`では4→3→2→1の順に信号が進み、Operator 1がCarrierです。`mode`の意味：`phase` = Phase Modulation、`frequency` = Frequency Modulation、`amplitude` = Unipolar AM、`ring` = CarrierとProductのCrossfade。

Modulation Target：`operator.<1-4>.<parameter>`（`ratio`、`detune_cents`、`level`、`modulation_amount`、`feedback`など）

`inspect --json`でMode、Algorithm、評価順序、Carrier、4 OperatorのParameter ID、Unison、実効周波数上限を確認します。

### Sample

鍵盤範囲・Velocity範囲でZoneを選択して再生します。Mono / StereoのChannel構成を保持します。

```json
"generator": {
  "sample": {
    "interpolation": "cubic",
    "zones": [
      {
        "id": "main",
        "asset": { "path": "<WAVへの相対Path>", "sha256": "<計算値>" },
        "root_note": 60,
        "key_min": 0, "key_max": 127,
        "velocity_min": 1, "velocity_max": 127,
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
```

| Field | 内容 |
|---|---|
| `root_note` | Zoneの基準音程（0〜127、60 = C4） |
| `key_min` / `key_max` | 受け付けるMIDI Note範囲（0〜127、min <= max） |
| `velocity_min` / `velocity_max` | 受け付けるVelocity範囲（1〜127、min <= max） |
| `round_robin_group` | 同一条件のZoneをDefinition順に選択するGroup。不要なら`null` |
| `playback.region` | 再生領域（秒） |
| `playback.direction` | `forward` / `reverse`。Reverseは準備済みAudioを複製せずCursorを逆方向へ進める |
| `playback.loop` | 領域内Loop。`crossfade_seconds` > 0で定電力Blend |
| `playback.time` | 時間伸縮Mode（下表） |
| `interpolation` | `cubic`（4点補間） |

`playback.time`のMode：

| Mode | 動作 | 制約 |
|---|---|---|
| `resample` | NoteのPitchへ合わせてSample Rateを変える | — |
| `fixed_stretch` | Pitchを保ったままDurationを`ratio`倍する | `ratio` 0.5〜2.0。Reverseとは併用不可 |
| `tempo_sync` | 処理Tempoへ追従（`source_bpm`基準） | `source_bpm`は0より大。Reverseとは併用不可 |

Release Sampleを作る場合はLayerの`trigger.event`を`note_off`にします。Path違いやHash不一致ではそのZoneだけが無効化され、他ZoneやLayerでRenderが継続します。

### Granular

Sampleと同じAssetをGrainへ分解して再構成します。Mono AssetでもStereo Generatorとして動作します。

```json
"generator": {
  "granular": {
    "asset": { "path": "<WAVへの相対Path>", "sha256": "<計算値>" },
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
```

| Parameter | Range / 単位 | 使い方 |
|---|---|---|
| `position` | 0〜1 | 領域内の読出位置。固定でFreeze、LFO / Mod WheelでScrub |
| `grain_size` | 0.005〜0.5秒 | ハン窓を適用するGrain長 |
| `density` | 1〜100 grains/sec | 1秒あたりのGrain数 |
| `pitch` | -2400〜2400 cents | Note PitchとLayer Tuningへ加算 |
| `randomness` | 0〜1 | Positionの決定的分散幅 |
| `pan_spread` | 0〜1 | GrainごとのStereo配置幅 |
| `seed` | 整数 | Grain生成のSeed |

Note OffではGrainを破棄せずLayer EnvelopeがReleaseへ進み、ボイススティーリングまたはReset時だけPoolを初期化します。

Modulation Target：`granular_position`、`grain_size`、`grain_density`、`grain_pitch`、`grain_randomness`、`grain_pan_spread`

`inspect --json`で準備済み状態、領域Frame、6 Parameter ID、Source Channel、Seed、Grain Pool Limitを確認します。`INVALID_GRAIN_REGION` / `INVALID_GRAIN_PARAMETER`が出たら修正します。

### Wave Sequence

複数Assetを時間順に切り替えます。Sequenceの`direction`はStep選択順、Stepの`playback_direction`はAsset Read方向です。

```json
"generator": {
  "wave_sequence": {
    "root_note": 60,
    "direction": "forward",
    "loop": true,
    "crossfade": 0.25,
    "steps": [
      {
        "id": "attack",
        "asset": { "path": "<WAVへの相対Path>", "sha256": "<SHA-256>" },
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
```

| 項目 | 内容 |
|---|---|
| Steps | 1〜128個。Definition順に再生 |
| `direction` | `forward` / `backward` / `ping_pong`。`ping_pong`は終端を重複させず往復 |
| `crossfade` | 隣接Stepの定電力Overlap。0〜0.5 |
| `duration` | `{"mode":"seconds","value":...}`または`{"mode":"beats","value":...}` |
| `playback` | `one_shot`（Asset終了後はStep残り時間が無音）/ `loop`（領域をStep終端まで繰り返す） |

Missing AssetのStepは削除されず、Durationを保持した無音として後続Stepへ進みます。

`inspect --json`でStep Count、Direction、Loop、Crossfade、領域Frame、Duration、Playback、Availability、Pitch、Gainを確認します。

### Additive

Note Frequencyに対する1〜64個のPartialを直接記述します。整数Ratioで倍音構成、`2.73`のような非整数Ratioで非整数倍音Bellや金属的質感になります。

```json
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
```

| 項目 | 内容 |
|---|---|
| `partials[].id` | 空でない一意の識別子 |
| `partials[].ratio` | 基音に対する周波数比 |
| `partials[].amplitude_a` / `amplitude_b` | `morph=0` / `morph=1`の振幅。Morph中はRatioとPhase不変で不連続を回避 |
| `partials[].phase` | 初期位相 |
| `partials[].envelope` | 任意のADSR。Layer EnvelopeはPartial Sumの後に適用 |
| `morph` | A/B振幅スペクトルの補間 |
| `spectrum_tilt_db_per_octave` | 倍音番号に対する傾き |
| `inharmonicity` | 高次Ratioを非整数側へ曲げる量 |

Modulation Target：`additive_morph`、`additive_spectrum_tilt`、`additive_inharmonicity`

完全無音Spectrum、空Partial配列、重複ID、65個以上のPartialはValidation Errorです。`inspect --json`でPartial Count、Ratio、Amplitude、Phase、Envelope有無、3 Parameter IDを確認します。

### Formant

基音の整数倍Partialへ母音の共鳴を表す5本Bandを適用します。`profiles`は1〜8個、各Profileは周波数昇順に5本Bandを持ちます。母音Positionは隣接Profileを補間し（Frequency / BandwidthはGeometric、GainはdB Linear）、Formant ShiftはBandだけを移動して基音Pitchは変えません。

```json
"generator": {
  "formant": {
    "phase_reset": true,
    "partial_count": 48,
    "vowel_position": 0.0,
    "formant_shift_cents": 0.0,
    "throat": 0.5,
    "spectral_tilt_db_per_octave": -6.0,
    "profiles": [
      {
        "id": "a",
        "formants": [
          { "frequency_hz": 800.0, "bandwidth_hz": 80.0, "gain_db": 6.0 },
          { "frequency_hz": 1150.0, "bandwidth_hz": 90.0, "gain_db": 3.0 },
          { "frequency_hz": 2900.0, "bandwidth_hz": 120.0, "gain_db": -3.0 },
          { "frequency_hz": 3900.0, "bandwidth_hz": 150.0, "gain_db": -12.0 },
          { "frequency_hz": 4950.0, "bandwidth_hz": 200.0, "gain_db": -18.0 }
        ]
      }
    ]
  }
}
```

| Parameter | Range / 働き |
|---|---|
| `partial_count` | 1〜64 |
| `vowel_position` | 0〜1。隣接Profileを補間 |
| `formant_shift_cents` | -2400〜2400。Band中心周波数とBandwidthを移動 |
| `throat` | 0〜1。Bandwidthを0.5〜2倍 |
| `spectral_tilt_db_per_octave` | -24〜12。Partialの高域傾斜 |
| Band `frequency_hz` | 100〜12000 |
| Band `bandwidth_hz` | 20〜5000 |
| Band `gain_db` | -60〜12 |

Formant固有Envelopeはなく、Layer EnvelopeがPartial Sumへ適用されます。

Modulation Target：`formant_vowel_position`、`formant_shift`、`formant_throat`、`formant_spectral_tilt`

`inspect --json`でProfile Count、5 Band、4 Parameter ID、出力Modeを確認します。

## Hybrid構成

複数Generatorを同じVoiceでMixすると、役割分担で調整しやすくなります。代表的な構成：

| 構成 | Layer構成 |
|---|---|
| Harmonic / Formant Hybrid | Formant（共鳴）+ Additive（芯）+ Sample（Attack）+ Noise（Air）+ Layer / Voice / Global Processor |
| Spectral Hybrid | Spectral + Additive + Sample + Noise + Processor / Modulation |
| Digital Hybrid | Wavetable（持続）+ Operator Modulation（倍音芯）+ Sample（短アタック） |

Hybridを作る手順：

1. 各LayerのGainとEnvelopeを単独で確認する
2. Sample / Wavetable AssetのPathとSHA-256を保持したまま複製する
3. `instrument inspect --json`でLayer / Voice / Global Processorの配置・順序・Parameter IDを確認し、Route TargetがDefinitionのLayer ID / Processor IDに一致することを確かめる
4. LFO、Modulation Envelope、Velocity、Mod Wheel、AftertouchをFormant ParameterまたはProcessorへ接続する
5. `render events`でParameter ChangeとControl Eventを含むPhrase、`render midi`でNote / Velocity / MIDI Controlを含む出力を確認する

## 検証する

```bash
sonalloy instrument validate <definition>          # JSON Parse・Validation・コンパイルまで実行
sonalloy instrument inspect <definition> --json    # 実行値を機械可読で表示（--json省略で人間可読）
```

- `validate`の成功は`valid <path>`。Warningは`print_warnings`で表示されるため必ず確認する
- `inspect`でPolyphony、Layer Trigger、Generator詳細、Gain / Pan / Tuning、Envelope、Processor Chain、ParameterのNative / Modulation Unit、Source Polarity、Route Effect、Reachable Range、Warningを確認する
- Errorには`layers[0].envelope.attack_seconds`のようなField Pathが付くため、そのまま該当箇所へ反映できる
- Warningが残る場合、Sonalloyは「他LayerでRenderを継続する」設計のため、意図しない無効化がないかを確認する

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

## Deviceが利用できる場合のRealtime試聴

```bash
sonalloy device list
sonalloy device list --json
sonalloy play <definition> --midi-device <id>
```

`play`は同じDefinitionをCoreのRealtime経路で演奏します。起動前に`device list`でAudio OutputとMIDI InputのOpaque IDを確認し、複数のMIDI Inputがある場合は`--midi-device`を必ず指定します。標準入力のEnterで停止します。Realtime試聴はOffline Render、Analysis、Traceを置き換えません。

| Option | 意味 | 既定値 |
|---|---|---|
| `--note` | MIDI Note番号 | `60` |
| `--velocity` | 打鍵の強さ | `100` |
| `--gate` | Note OnからNote Offまでの時間（秒） | `0.5` |
| `--tail` | 最後のNote Off後の追加時間（秒） | note: `0.5` / midi: `1.0` |
| `--tempo` | 処理Tempo（BPM）。Tempo Sync Sampleへ適用 | `120` |
| `--sample-rate` | Sample Rate（Hz） | `48000` |
| `--block-size` | 処理最大Block Size（Frame） | `257` |
| `--output` | Stereo WAV出力先（必須） | — |
| `--duration-frames` | `render events`の長さ（Frame） | — |
| `--analyze` | 補正後WAVのLevel / DC / Activity / Continuity / Stereo / Spectrumを出力 | Off |
| `--trace <id>` | 選択したDynamic ParameterのRuntime Snapshot。複数指定可 | なし |
| `--trace-every-frames <N>` | 定期Trace間隔。Event後と最終Frameも記録 | 480 |
| `--json` | Analysis / Traceを含む成功Reportを機械可読で出力 | Off |

- 出力は32-bit float・2 ChannelのStereo WAV。親Directoryは事前に作成する
- `render note`と`render events`の`--tempo`はTempo Syncの処理Tempo。`render midi`はMIDI内のTempo Meta EventからTempo Mapを作成する
- `render events`ではNote Eventと同じ絶対Frame位置にParameter Change（`native_value`）/ Pitch Bend / Mod Wheel / Aftertouch / Sustain Pedal（`down`）を記述できる。`render midi`ではMIDI Pitch Bend / CC1 / Channel Aftertouch / CC64が同じ実行時Eventへ変換される
- Time Stretchを含む場合は報告Latencyが`inspect`と成功JSONへ表示され、CLIが前置きLatencyを除去して演奏タイムラインのFrame 0からWAVを生成する

`--analyze`のdBFSは0を`null`で返し、Activityの閾値は-80 dBFS、ContinuityのLarge Delta閾値は0.25です。`--trace`はFrame 0、既定480 Frame間隔、Event後、最終FrameをLatency補正後のTimelineで記録します。`final`はRoute加算とClamp後のNative値です。

人間の確認項目は`docs/testing-and-sound-review.md`にまとめています。RealtimeではNote、Pitch Bend、Mod Wheel、Channel Aftertouch、Sustainを含む入力、256 / 128 FrameのBuffer、10分以上の連続演奏、Xrun・Fatal Fault・Stuck Note・Queue Overflowを確認します。

## 仕上げる

- `metadata.name`と`metadata.description`を実際の音色に合わせる
- `validate` / `inspect --json`のWarning、出力Mode、Parameter Unit、Route Effect、Parameter IDを最終確認する
- `--analyze`で数値的な出力状態を確認し、`--trace`で宣言したModulationが意図した範囲を動いたか確認する
- 生成したWAVを同じ音量条件で試聴する
- 関連docs（`docs/instrument-definition.md`など）と矛盾しないか確認する

## 失敗時の対処

### Exit Code

| Exit Code | 意味 | 対処 |
|---:|---|---|
| `0` | 成功 | — |
| `1` | 音源定義 / コンパイルエラー | `--json`でDiagnosticsを取得し、Field Path付きのErrorを修正する |
| `2` | CLI入力またはレンダリングリクエストエラー | Option値（Sample Rate、Block Size、Tail、Frequency）を確認する |
| `3` | Core処理 / レンダリングエラー | `--json`の`DSP_ERROR`等を確認する。解決しない場合は`docs/runtime-processing.md`のError規則を確認する |
| `4` | WAV出力エラー | 出力先Directoryの存在と書き込み権限を確認する |

### よくある症状と対処

| 症状 | 対処 |
|---|---|
| Warningが出た | `instrument inspect`で意図しないLayer無効化（Sample欠落など）がないか確認する |
| 音が鳴らない | `enabled: true`、`trigger`の範囲に発音するNote / Velocityが含まれているか確認する |
| Sampleが無視された | Asset PathとSHA-256の一致、WAV形式（PCM 16/24、Float 32）を確認する |

### Diagnostic Code

| Code | 対象 |
|---|---|
| `SCHEMA_UNSUPPORTED` / `JSON_INVALID` / `REQUIRED_FIELD_MISSING` | Definition全体 |
| `ID_DUPLICATED` / `VALUE_OUT_OF_RANGE` / `LAYER_RANGE_INVALID` | Layer / Parameter |
| `FILTER_CUTOFF_CLAMPED` | Filter |
| `ASSET_NOT_FOUND` / `ASSET_HASH_MISMATCH` / `ASSET_DECODE_FAILED` / `ASSET_RESAMPLED` / `ASSET_DOWNMIXED` | Asset |
| `UNSUPPORTED_PLAYBACK_COMBINATION` / `INVALID_STRETCH_RATIO` / `INVALID_SOURCE_TEMPO` / `STRETCH_BACKEND_FAILURE` | Sample再生 |
| `INVALID_GRAIN_REGION` / `INVALID_GRAIN_PARAMETER` | Granular |
| `WAVETABLE_LAYOUT_INVALID` / `WAVETABLE_PREPARATION_FAILED` / `WAVETABLE_SILENT_FRAME` / `WAVETABLE_DC_OFFSET` | Wavetable |
| `GENERATOR_RESOURCE_LIMIT_EXCEEDED` | Generator資源 |
| `MIDI_ERROR` / `AUDIO_DEVICE_ERROR` | Realtime MIDI / Audio Device |

## 参照

| 文書 | 内容 |
|---|---|
| `docs/instrument-definition.md` | DefinitionのJSON仕様・全Fieldの単位・Range |
| `docs/runtime-processing.md` | 実行時の挙動・Voice・ADSR・Sample再生・Error規則 |
| `docs/cli.md` | CLIの全Command・Option・Exit Code |
| `docs/architecture.md` | システムの静的構造 |
| `docs/testing-and-sound-review.md` | 検証とReviewの手順・人間の確認項目 |
