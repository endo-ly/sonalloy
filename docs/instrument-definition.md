# 音源定義

音源定義（JSONファイル）は、手で編集して保存・管理する正本です。この文書は、音源定義を正しく書くために必要な**Fieldの制約・Range・振る舞い**をまとめます。書きたい要素の該当章を調べる使い方を想定しているため、各章はその要素について完結します。

**全体構造 → Performance → Macro / Vector → Layer → Generator → 複数Generatorの組み合わせ → Processor → Modulation → コンパイル時の変換**の順で説明します。実行時の振る舞い（Voice、ADSRの進行、Sample再生、Grain生成など）は`docs/runtime-processing.md`を、CLIの使い方は`docs/cli.md`を参照してください。

音声処理は音源定義を直接使わず、コンパイルして変換した値だけを使います。

## 本書の範囲

| 本書で扱わない内容 | 参照先 |
|---|---|
| 実行時の動き（Voice・ADSR・Sample再生） | `docs/runtime-processing.md` |
| CLIの使い方・Option | `docs/cli.md` |

## 全体構造

音源定義は、次のトップレベルFieldを持ちます。

| Field | 内容 |
|---|---|
| `schema_version` | スキーマ版。現在は`4`。それ以外はUnsupportedとして拒否 |
| `metadata` | `name`、`author`、`description` |
| `performance` | `mode`が`polyphonic`または`monophonic`。Modeごとに必要なFieldが異なる |
| `layers` | 発音の単位となるLayer配列（1個以上） |
| `voice_processors` | 全LayerのMix後に適用するProcessor Chain |
| `global_processors` | 全Voiceの合計後に適用するProcessor Chain |
| `modulation` | SourceとRouteの定義（省略可）。Routeは`depth.value`と`depth.unit`でTargetに直接効く量を指定 |
| `macros` | 外部から変更できる0〜1のInstrument Parameter（省略可） |
| `vectors` | LayerのConstant-power Mixを制御するAxis（省略可） |

全体の例（Saw Oscillatorの最小構成）：

```json
{
  "schema_version": 4,
  "metadata": { "name": "Basic Poly Synth", "author": null, "description": "..." },
  "performance": { "mode": "polyphonic", "polyphony": 16, "voice_stealing": "quietest_releasing_then_oldest" },
  "layers": [
    {
      "id": "body",
      "enabled": true,
      "trigger": { "event": "note_on", "key_min": 0, "key_max": 127, "velocity_min": 1, "velocity_max": 127 },
      "gain_db": -14.0,
      "pan": 0.0,
      "tuning_cents": 0.0,
      "envelope": { "attack_seconds": 0.005, "decay_seconds": 0.18, "sustain_level": 0.65, "release_seconds": 0.3 },
      "generator": { "oscillator": { "waveform": { "type": "saw" }, "phase_reset": true, "phase": 0.0 } },
      "processors": []
    }
  ],
  "voice_processors": [ { "type": "filter", "id": "tone", "cutoff_hz": 12000.0, "resonance": 0.12 } ],
  "global_processors": [],
  "modulation": {
    "sources": [],
    "routes": [
      { "source": "velocity", "target": "layer.body.gain", "depth": { "value": 8.0, "unit": "decibels" }, "curve": "linear" }
    ]
  },
  "macros": [],
  "vectors": []
}
```

共通の規則：

- Layer / Processor / Sourceの識別子（ID）は、小文字で始まり、小文字・数字・`_`を使用します（`.`は使えません）
- 定義されていないFieldがあるとJSON Parse Errorになります

## Performance

`performance`はTagged Objectです。`mode`を省略したり、別ModeのFieldを混ぜたりできません。

### Polyphonic

同時に保持するVoice数を`polyphony`（1〜64）で指定し、上限到達時は`voice_stealing`で既存Voiceを選びます。

```json
"performance": {
  "mode": "polyphonic",
  "polyphony": 16,
  "voice_stealing": "quietest_releasing_then_oldest"
}
```

### Monophonic

常に1 Voiceを使い、Held NoteはLast-note priorityで切り替えます。`legato: true`では接続したNote OnでEnvelopeとGeneratorを再Triggerせず、`portamento`があれば音程だけを指定秒数で滑らかに移動します。`legato: false`ではNote Onごとに再Triggerします。

```json
"performance": {
  "mode": "monophonic",
  "legato": true,
  "portamento": { "time_seconds": 0.08 }
}
```

`portamento.time_seconds`は0より大きく10秒以下です。Monophonicでは`polyphony`と`voice_stealing`を指定しません。

## MacroとVector

### Macro

Macroは0〜1の安定したInstrument Parameterです。1つのMacroを複数RouteのSourceにできます。Parameter IDは`macro.<id>`で、PatternやEventの既存`parameter_change`から変更します。Macroを別SourceのTargetにはできません。

```json
"macros": [
  { "id": "motion", "name": "Motion", "default": 0.0 }
],
"modulation": {
  "sources": [],
  "routes": [
    {
      "source": "macro.motion",
      "target": "layer.body.tuning",
      "depth": { "value": 80.0, "unit": "cents" },
      "curve": "smooth_step"
    }
  ]
}
```

Macroは最大16個です。`default`は0〜1で、Runtimeでは5msのSmoothingを使います。

### Vector

VectorはLayerをConstant-powerで混ぜる専用機能です。2-WayのParameter IDは`vector.<id>.position`、4-Wayは`vector.<id>.x`と`vector.<id>.y`です。AxisはModulation Targetにできます。

```json
"vectors": [
  {
    "type": "two_way",
    "id": "tone",
    "name": "Tone",
    "layer_a": "body",
    "layer_b": "bright",
    "position": 0.5
  }
]
```

2-WayのWeightは`A = cos(position × π/2)`、`B = sin(position × π/2)`です。4-WayはX/YそれぞれのSine/Cosineを組み合わせます。同じLayerを複数Vectorへ所属させることはできず、Vectorは最大8個です。

## Layer

Layerは「Generator + Layer Processor + ADSR + Gain + Pan」のセットで、Trigger条件に合ったLayerだけが鳴ります。`layers`は書かれた順に同じVoiceへMixし、`enabled: false`のLayerはコンパイル対象外です。

| Field | Range | 内容 |
|---|---|---|
| `id` | — | Layer識別子。一意 |
| `enabled` | Boolean | 発音の有無 |
| `trigger` | 下記 | 発音条件 |
| `gain_db` | -60〜12 dB | Layer音量 |
| `pan` | -1〜1 | 定位 |
| `tuning_cents` | -1200〜1200 | 音程（Cent） |
| `envelope` | 下記 | ADSR |
| `processors` | — | Generator後に直列適用するProcessor Chain |
| `generator` | 下記 | 音源（[Generator](#generator)参照） |

**Trigger**

| Field | Range | 内容 |
|---|---|---|
| `event` | `note_on` / `note_off` | `note_on`はNote Onで発音。`note_off`はNote Onで待機状態になり、対応するNote Offで発音する。Voice Stealingは演奏上のNote Offではないため、待機Layerを発音しない |
| `key_min` / `key_max` | 0〜127 | 発音するMIDI Note範囲 |
| `velocity_min` / `velocity_max` | 1〜127 | 発音するVelocity範囲 |

最小値は最大値以下にします。

**ADSR**

ADSRは音の音量変化を形作る4区間です。Note OnでAttackから始まり、Decayを経てSustainで待機し、Note OffでReleaseへ進みます。

| Field | Range | 内容 |
|---|---|---|
| `attack_seconds` | 0〜30秒 | Note Onから最大音量へ達する時間 |
| `decay_seconds` | 0〜30秒 | 最大音量からSustain Levelへ下がる時間 |
| `sustain_level` | 0〜1 | Note On中の音量 |
| `release_seconds` | 0〜30秒 | Note Offから無音へ至る時間 |

## Generator

GeneratorはLayerの`generator` Fieldへ、いずれか1つを指定します。

| Generator | 用途 |
|---|---|
| [Oscillator](#oscillator) | 基本波形とComplex変形 |
| [Noise](#noise) | White / Pink / Brown Noise |
| [Physical String](#physical-string) | Fractional Delay Feedbackによる弦・硬質振動 |
| [Modal](#modal) | 複数Modeの共鳴によるBody・Bell・Plate |
| [Additive](#additive) | Partial直接設計による倍音構成 |
| [Formant](#formant) | 母音共鳴のBand制御 |
| [Wavetable](#wavetable) | 周期波形Frame列のPosition走査 |
| [Spectral](#spectral) | WAVのSTFT再構成とA/B Morph |
| [Operator Modulation](#operator-modulation) | 4 Operator FM / PM / AM / Ring |
| [Sample](#sample) | 鍵盤範囲別のSample再生 |
| [Granular](#granular) | Grain分解によるTexture再構成 |
| [Wave Sequence](#wave-sequence) | 複数Assetの時系列切り替え |

以下では各GeneratorのFieldについて、**Range・Dynamic Parameter・意味**を示します。`Dynamic`がYesの項目はModulation RouteやParameter Changeから動かせます。値を変えるときは`instrument inspect`でNative Unit・Scale・Clamp範囲を確認できます。実行時の振る舞い（位相の進行、Cursor、Grain生成など）は`docs/runtime-processing.md`を参照してください。

Dynamic ParameterのIDは`layer.<layer_id>.generator.<name>`形式です（Operator Modulationだけ`operator.<1-4>.<parameter>`）。`position`のような走査位置は、各Generatorが準備したSource領域内の位置を表します。

### Oscillator

基本波形（Sine / Saw / Square / Triangle / Pulse）を生成します。`waveform`はTagged Objectで、Pulseだけが`pulse_width`を持ちます。

```json
"generator": {
  "oscillator": {
    "waveform": { "type": "saw" },
    "phase_reset": true,
    "phase": 0.0
  }
}
```

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `waveform.type` | `sine` / `saw` / `square` / `triangle` / `pulse` | No | 波形 |
| `waveform.pulse_width` | 0.05〜0.95 | Yes | Pulseのデューティ比（Pulseのみ） |
| `phase_reset` | Boolean | No | Note Onで初期位相へ戻すか |
| `phase` | 0〜1 | No | 初期位相 |

Dynamic Parameter：`pulse_width`

**Complex Oscillator** — OscillatorへHard Sync、Waveshaping、Unison、Phase Distortion、Wavefold、Feedbackを追加できます。存在する設定だけがDynamic Parameterへ登録されます。

```json
"generator": {
  "oscillator": {
    "waveform": { "type": "sine" },
    "phase_reset": true, "phase": 0.0,
    "hard_sync": { "ratio": 3.0 },
    "waveshaping": { "amount": 0.25 },
    "phase_distortion": { "amount": 0.55 },
    "wavefold": { "amount": 0.25 },
    "feedback": { "amount": 0.3 },
    "unison": { "voices": 5, "detune_cents": 18.0, "stereo_spread": 0.85, "phase_spread": 0.0 }
  }
}
```

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `hard_sync.ratio` | 1〜16 | Yes | Masterに対するSlaveの周波数比。Log2 |
| `waveshaping.amount` | 0〜1 | Yes | Saturationの深さ。0で原形、大きいほど強く歪む。Dry / Wetを`amount`でCrossfadeする |
| `phase_distortion.amount` | 0〜1 | Yes | 位相曲線の歪み。0で原形、1で最大 |
| `wavefold.amount` | 0〜1 | Yes | 波形の折り返し量。0で原形、大きいほど折り返しが増える |
| `feedback.amount` | 0〜1 | Yes | 直前Sampleを入力側へ戻す量。0で無効 |
| `unison.voices` | 2〜8 | No | UnisonのVoice数 |
| `unison.detune_cents` | 0〜100 | Yes | 各VoiceのDetune幅 |
| `unison.stereo_spread` | 0〜1 | Yes | 左右への配置幅 |
| `unison.phase_spread` | 0〜1 | No | 各Voiceの位相ばらつき |

制約：

- Hard SyncはSine以外のWaveformで使えます。Hard Syncを使うときは`phase`を0にします
- Phase DistortionとFeedbackはSine専用で、Hard Syncとは併用できません
- Wavefoldは全Waveformで使えます
- Hard SyncとUnisonを組み合わせるときは、`unison.phase_spread`を0にします

Dynamic Parameter：`sync_ratio`、`waveshape`、`phase_distortion`、`wavefold`、`oscillator_feedback`、`unison_detune`、`unison_spread`

### Noise

White / Pink / Brown Noiseを生成します。常にStereo出力します。

```json
"generator": {
  "noise": { "color": "pink", "seed": 812347, "stereo_correlation": 0.65 }
}
```

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `color` | `white` / `pink` / `brown` | No | Noiseの種類 |
| `seed` | 整数 | No | 決定的Noise StreamのSeed |
| `stereo_correlation` | 0〜1 | Yes | 左右の相関。0で左右独立、1で左右同一 |

Dynamic Parameter：`noise_correlation`

### Physical String

Deterministic ExciterをFractional DelayのFeedback Loopへ入力し、弦や硬質な振動体の撥弦・金属的な振動を作ります。出力はMonoです。これは特定の楽器を再現するModelではなく、Layer Processorや他のGeneratorと組み合わせるための固定Topologyです。

```json
"generator": {
  "physical_string": {
    "exciter": {
      "type": "noise_burst",
      "duration_seconds": 0.006,
      "brightness": 0.82,
      "seed": 4001
    },
    "decay_seconds": 2.4,
    "brightness": 0.68,
    "stiffness": 0.18
  }
}
```

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `exciter.type` | `impulse` / `noise_burst` | No | Note On時の励振方式 |
| `exciter.duration_seconds` | 0.0005〜0.100秒 | No | Noise Burstの励振長。最後は指数Envelopeで-60 dB相当まで下がる |
| `exciter.brightness` | 0〜1 | No | Exciter Low-passの明るさ。対数目盛で、0が最も暗く1で上限まで開く |
| `exciter.seed` | 整数 | No | Note IDとLayer IDへ結び付いた決定的Noise Seed |
| `decay_seconds` | 0.05〜20秒 | Yes | Feedback LoopのNominal T60。Loopの高域損失により高域の実測Decayは短くなる |
| `brightness` | 0〜1 | Yes | Loop Low-passの明るさ。0は暗く、1は高域を残す |
| `stiffness` | 0〜1 | Yes | First-order All-passによるDispersion。高いほど高次成分の遅れが増える |

ExciterとLoopのLow-pass Cutoffには、処理Sample Rateから決まる共通の上限があり、`brightness`はその上限までの対数位置として扱われます。Fundamentalは4 Hz以上、処理Sample Rateの0.45倍以下で、Layer Tuningを含めてこの範囲を外れる場合はRender Errorになります。

Note OffではGenerator固有のEnvelopeを追加せず、Layer ADSRのReleaseを適用します。

Dynamic Parameter：`physical_string_decay_seconds`、`physical_string_brightness`、`physical_string_stiffness`

Parameter ID例：`layer.string.generator.physical_string_decay_seconds`。`decay_seconds`は`Seconds + Log2`で、Modulation DepthのUnitは`octaves`です。

### Modal

Rust側のDeterministic ExciterをPinned DaisySPの低レベル`Resonator`へ入力し、複数Modeの共鳴で棒・板・ベル・金属・木質・ガラス的なBodyを作ります。出力はMonoです。Mode数と3つのDynamic ParameterはCompile時に固定します。

```json
"generator": {
  "modal": {
    "exciter": {
      "type": "noise_burst",
      "duration_seconds": 0.010,
      "brightness": 0.58,
      "seed": 9102
    },
    "mode_count": 24,
    "structure": 0.72,
    "brightness": 0.76,
    "decay": 0.66
  }
}
```

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `exciter` | Physical Stringと同じ | No | 共鳴体へ与えるDeterministic Exciter |
| `mode_count` | `4` / `8` / `12` / `16` / `20` / `24` | No | 同時に計算するMode数。多いほど密度とCPU負荷が増える |
| `structure` | 0〜1 | Yes | Mode間隔の硬さ。値の領域に応じて、高次Modeの間隔が圧縮・ほぼ整数比・引き伸ばしに変わる |
| `brightness` | 0〜1 | Yes | 高次Modeの強さと高域Loss |
| `decay` | 0〜1 | Yes | 共鳴のDecay。0が短く、1が長い。秒単位のT60ではない |

`structure`は単純な明るさではなくMode配置を変える値です。`decay`はNative ResonatorのDampingへ渡しますが、周波数・Structure・Brightnessとの相互作用があるため、一定秒数のDecayとして解釈しません。Fundamentalの安全周波数範囲とNote Off時のLayer ADSRはPhysical Stringと同じです。

Dynamic Parameter：`modal_structure`、`modal_brightness`、`modal_decay`

Parameter ID例：`layer.body.generator.modal_structure`。`mode_count`とExciterのStatic FieldはParameter Catalogへ登録されません。

### Additive

Note Frequencyを基準にした1〜64個のPartialのSineを加算します。整数比で倍音構成、非整数比で非整数倍音Bellや金属的質感を作れます。出力はMonoです。

```json
"generator": {
  "additive": {
    "phase_reset": true,
    "morph": 0.0,
    "spectrum_tilt_db_per_octave": -3.0,
    "inharmonicity": 0.0,
    "partials": [
      { "id": "fundamental", "ratio": 1.0, "amplitude_a": 1.0, "amplitude_b": 0.7, "phase": 0.0 },
      { "id": "second", "ratio": 2.0, "amplitude_a": 0.45, "amplitude_b": 0.8, "phase": 0.0,
        "envelope": { "attack_seconds": 0.01, "decay_seconds": 0.8, "sustain_level": 0.2, "release_seconds": 0.3 } }
    ]
  }
}
```

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `morph` | 0〜1 | Yes | A/Bの振幅を補間する位置。周波数・位相は変わらない |
| `spectrum_tilt_db_per_octave` | -24〜12 dB/octave | Yes | 高域Partialの減衰傾き |
| `inharmonicity` | 0〜1 | Yes | 高次Ratioの非整数化。大きいほど高次Partialがずれ、基音のRatio 1は維持される |
| `partials` | 1〜64個 | No | Definition順の固定Partial |
| `partials[].id` | 空でない一意な文字列 | No | Partial識別子 |
| `partials[].ratio` | 0.125〜64 | No | Note Frequencyに対する周波数比 |
| `partials[].amplitude_a` / `amplitude_b` | 0〜1 | No | Morph両端の振幅 |
| `partials[].phase` | 0〜1 cycle | No | 初期位相 |
| `partials[].envelope` | Optional ADSR | No | Partial個別のADSR |

- 高域Partialは滑らかに減衰させ（NyquistへClampせず、個別の消え方を維持）、全PartialのEnergyで正規化します
- Layer ADSRはPartial合計の後に適用します

Dynamic Parameter：`additive_morph`、`additive_spectrum_tilt`、`additive_inharmonicity`

完全無音Spectrum、空のPartial配列、重複ID、65個以上のPartialはValidation Errorです。

### Formant

整数倍Partialへ母音共鳴の5本Bandを適用します。1〜8個のProfileを組み合わせ、母音Positionで補間します。出力はMonoです。

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
      { "id": "a", "formants": [
        { "frequency_hz": 800.0, "bandwidth_hz": 80.0, "gain_db": 0.0 },
        { "frequency_hz": 1150.0, "bandwidth_hz": 90.0, "gain_db": -5.0 },
        { "frequency_hz": 2900.0, "bandwidth_hz": 120.0, "gain_db": -12.0 },
        { "frequency_hz": 3900.0, "bandwidth_hz": 130.0, "gain_db": -18.0 },
        { "frequency_hz": 4950.0, "bandwidth_hz": 140.0, "gain_db": -24.0 }
      ]}
    ]
  }
}
```

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `partial_count` | 1〜64 | No | 生成する整数倍Partial数 |
| `vowel_position` | 0〜1 | Yes | Profile配列の先頭から末尾への位置 |
| `formant_shift_cents` | -2400〜2400 cents | Yes | Bandの中心周波数と帯域幅を移動（基音Pitchは不変） |
| `throat` | 0〜1 | Yes | 帯域幅の拡大・縮小。`0.5`が等倍で、端に向かって約0.5倍〜2倍へ変わる |
| `spectral_tilt_db_per_octave` | -24〜12 dB/octave | Yes | 高域Partialの減衰傾き |
| `profiles` | 1〜8個 | No | Definition順のProfile |
| `profiles[].id` | 空でない一意な文字列 | No | Profile識別子 |
| `profiles[].formants` | 5個固定 | No | 周波数昇順のBand |
| `frequency_hz` | 100〜12000 Hz | No | Bandの中心周波数 |
| `bandwidth_hz` | 20〜5000 Hz | No | Bandの帯域幅 |
| `gain_db` | -60〜12 dB | No | Bandの相対強度 |

- 隣接Profileの補間は、周波数・帯域幅が幾何平均、GainがdB線形です
- Formant固有ADSRはなく、Layer ADSRをPartial合計の後に適用します

Dynamic Parameter：`formant_vowel_position`、`formant_shift`、`formant_throat`、`formant_spectral_tilt`

Profileが0個または9個以上、Partial数が0または65以上、Band数が5以外、ID重複、周波数非昇順、各Range違反はValidation Errorです。

### Wavetable

周期波形をFrame順に連結したWAVを、Frame単位で走査します。

```json
"generator": {
  "wavetable": {
    "asset": { "path": "<WAVへの相対Path>", "sha256": "<SHA-256>" },
    "frame_length": 2048,
    "position": 0.25,
    "phase_reset": true, "phase": 0.0,
    "unison": { "voices": 5, "detune_cents": 14.0, "stereo_spread": 0.75, "phase_spread": 0.5 }
  }
}
```

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `asset` | — | No | MonoまたはStereoのWAV。StereoはMonoへDownmix |
| `frame_length` | 64〜4096（2の冪） | No | 1周期FrameのSample数 |
| `position` | 0〜1 | Yes | 最初のFrameから最後のFrameへの位置 |
| `phase_reset` / `phase` | — | No | Oscillatorと同じ |
| `unison` | — | 一部Yes | Oscillatorと同じ |

- Asset全体のSample数は`frame_length`で割り切れ、Frame数が1〜256である必要があります
- Source Sample RateはPitchへ使わず、WavetableをResampleしません
- Unison 1はMono、2 Voice以上はStereoです

Dynamic Parameter：`wavetable_position`、`unison_detune` / `unison_spread`（Unison指定時）

### Spectral

WAVをSTFT解析して再構成します。`asset_a`を必須の一次Sourceとし、`asset_b`でA/B Morphを使います。

```json
"generator": {
  "spectral": {
    "asset_a": { "path": "<WAVへの相対Path>", "sha256": "<SHA-256>" },
    "asset_b": null,
    "root_note": 60,
    "fft_size": 2048,
    "position": 0.0, "freeze": 0.0, "blur_seconds": 0.0, "shift_hz": 0.0, "morph": 0.0,
    "phase_reset": true
  }
}
```

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `asset_a` | — | No | 解析と時間軸の基準になるWAV。Mono/Stereoを保持 |
| `asset_b` | — / `null` | No | 2つ目のWAV。`asset_a`とChannel数を一致させる。指定時だけMorph Parameterが登録される |
| `root_note` | 0〜127 | No | Sourceが表すMIDI Note |
| `fft_size` | 1024 / 2048 / 4096 | No | STFT Size。Hopは`fft_size / 4`、報告Latencyは`fft_size - hop_size` |
| `position` | 0〜1 | Yes | Source Position |
| `freeze` | 0〜1 | Yes | 1でFrameを固定し、中間値で走査を遅くする。Phaseは進み続ける |
| `blur_seconds` | 0〜1秒 | Yes | 時間方向のBlur |
| `shift_hz` | -12000〜12000 Hz | Yes | 周波数をHzで移動 |
| `morph` | 0〜1 | Yes | A/BのMorph。`asset_b`なしで0以外はValidation Error |
| `phase_reset` | Boolean | No | Note Onで位相を初期状態へ戻すか |

- MIDI NoteとLayer TuningはRoot Noteに対する周波数比として適用され、Source Durationは変わりません
- 報告Latencyは他Layerへ補償されます

Dynamic Parameter：`spectral_position`、`spectral_freeze`、`spectral_blur`、`spectral_shift`、`spectral_morph`（`asset_b`指定時のみ）

A/BのChannel数不一致はCompile Errorです。

### Operator Modulation

4つのSine Operatorを固定Topologyで接続します。Carrierだけが出力を持ち、他のOperatorは変調信号を供給します。Algorithmは`stack_4`、`stack_3_plus_carrier`、`two_stacks`、`fork_to_carrier`、`two_modulators_plus_carrier`、`three_modulators`、`shared_modulator`、`parallel`から選びます。

```json
"generator": {
  "operator_modulation": {
    "mode": "phase",
    "algorithm": "stack_4",
    "operators": [
      { "ratio": 1.0, "detune_cents": 0.0, "level": 0.9, "modulation_amount": 0.0, "feedback": 0.0, "phase": 0.0,
        "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.1, "sustain_level": 1.0, "release_seconds": 0.1 } },
      { "ratio": 2.0, "detune_cents": 0.0, "level": 0.0, "modulation_amount": 2.5, "feedback": 0.0, "phase": 0.0,
        "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.08, "sustain_level": 1.0, "release_seconds": 0.08 } },
      { "ratio": 3.0, "detune_cents": 0.0, "level": 0.0, "modulation_amount": 1.5, "feedback": 0.0, "phase": 0.0,
        "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.06, "sustain_level": 1.0, "release_seconds": 0.06 } },
      { "ratio": 5.0, "detune_cents": 0.0, "level": 0.0, "modulation_amount": 2.0, "feedback": 0.25, "phase": 0.0,
        "envelope": { "attack_seconds": 0.0, "decay_seconds": 0.04, "sustain_level": 1.0, "release_seconds": 0.04 } }
    ],
    "phase_reset": true,
    "unison": null
  }
}
```

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `mode` | `phase` / `frequency` / `amplitude` / `ring` | No | PM / FM / AM / Ring |
| `algorithm` | 上記8種 | No | Operatorの接続Topology |
| `operators[].ratio` | 0.25〜32 | Yes | Note Frequencyに対する周波数比 |
| `operators[].detune_cents` | -100〜100 | Yes | 周波数の微調整 |
| `operators[].level` | 0〜1 | Yes | Carrierの出力音量（Carrierのみ） |
| `operators[].modulation_amount` | Mode依存 | Yes | Phase/Frequencyは0〜8、Amplitude/Ringは0〜1。下表のModeごとの意味で使用 |
| `operators[].feedback` | 0〜1 | Yes | 直前Sampleで自己変調する量（Phase/Frequencyのみ） |
| `operators[].phase` | 0〜1 | No | 初期位相 |
| `operators[].envelope` | Optional ADSR | No | Operator個別のADSR |
| `unison.voices` | 1〜4 | No | UnisonのVoice数 |

制約：

- Carrierの`level`だけが出力へ寄与し、Carrier以外の`level`は0です
- 接続元のOperatorだけが`modulation_amount`を持ち、出力先を持たないAmountは0です
- FeedbackはPhase / Frequency Modeだけで使え、Amplitude / Ring Modeでは0にします
- Unisonは最大4 Voice。ADSRを全Componentで共有します

`modulation_amount`はModeごとに次の意味を持ちます。

| Mode | `modulation_amount`の働き |
|---|---|
| Phase | 接続元Operatorの出力でCarrierの位相を前後させる |
| Frequency | 接続元Operatorの出力でCarrierの瞬時周波数を増減させる |
| Amplitude | 接続元Operatorの出力でCarrierの振幅を変調する |
| Ring | Carrier信号と接続元Operator出力の積へ、Amountの割合で近づける |

Dynamic Parameter：`operator.<1-4>.ratio`、`operator.<1-4>.detune`、`operator.<1-4>.level`（Carrierのみ）、`operator.<1-4>.modulation_amount`（接続元のみ）、`operator.<1-4>.feedback`（Phase/Frequencyのみ）、`unison_detune` / `unison_spread`（Unison指定時）

### Sample

鍵盤範囲・Velocity範囲でZoneを選んで再生します。Mono/StereoのChannel構成を保持します。

```json
"generator": {
  "sample": {
    "interpolation": "cubic",
    "zones": [
      {
        "id": "main",
        "asset": { "path": "<WAVへの相対Path>", "sha256": "<SHA-256>" },
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

**Zone**

| Field | Range | 意味 |
|---|---|---|
| `id` | 一意な文字列 | Zone識別子 |
| `asset` | — | Mono/StereoのWAV |
| `root_note` | 0〜127 | Zoneの基準音程 |
| `key_min` / `key_max` | 0〜127 | 発音するMIDI Note範囲 |
| `velocity_min` / `velocity_max` | 1〜127 | 発音するVelocity範囲 |
| `round_robin_group` | — / `null` | 同一条件のZoneをDefinition順に選ぶGroup |
| `interpolation` | `cubic` | 補間方式 |

- ZoneのKey / Velocity範囲が重なってよいのは、同じ`round_robin_group`を持ち、範囲が完全一致するときだけです
- 同じRound Robin Groupの選択はDefinition順で、Instrument単位のCounterで交代します
- 同一Assetを参照するZoneは、Prepared Audioを共有します

**Playback**

| Field | 意味 |
|---|---|
| `region` | 再生領域`[start, end)`（秒）。`end: null`はAsset終端 |
| `direction` | `forward` / `reverse`。ReverseはCursor方向だけを反転 |
| `loop` | Region内のLoop（`start_seconds` / `end_seconds` / `crossfade_seconds`）。`null`はOne-shot |
| `time` | 時間伸縮Mode（下記）。省略不可 |

- `loop.crossfade_seconds`が0より大きいと、境界を定電力でBlendします。CrossfadeはLoop長の半分以下です

| `time.mode` | 動作 | 制約 |
|---|---|---|
| `resample` | Pitch変更へ合わせてDurationも変える | — |
| `fixed_stretch` | `ratio`（0.5〜2.0）でDurationだけを変える | Pitch不変。Reverseとは併用不可 |
| `tempo_sync` | `source_bpm`とProcess TempoからDuration比を決める | Pitch不変。Reverseとは併用不可 |

- `fixed_stretch`と`tempo_sync`のDuration比が0.5〜2.0の範囲外だとProcess Errorです

### Granular

Sampleと同じAssetをGrainへ分解して再構成します。Mono AssetでもGrainごとに定電力PanでStereo配置するため、出力は常にStereoです。

```json
"generator": {
  "granular": {
    "asset": { "path": "<WAVへの相対Path>", "sha256": "<SHA-256>" },
    "root_note": 60,
    "region": { "start_seconds": 0.05, "end_seconds": 0.9 },
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

| Field | Range | Dynamic | 意味 |
|---|---|---|---|
| `asset` | — | No | Mono/StereoのWAV。SampleとPrepared Audioを共有 |
| `root_note` | 0〜127 | No | 基準MIDI Note |
| `region` | 0以上の秒 | No | Grainが参照するPrepared Region。`end`省略時はAsset終端 |
| `position` | 0〜1 | Yes | Region内の基本位置。0がStart、1がGrain長を考慮したEnd側 |
| `grain_size` | 0.005〜0.5秒 | Yes | Grain長 |
| `density` | 1〜100 grains/sec | Yes | Grainの生成密度 |
| `pitch` | -2400〜2400 cents | Yes | Note PitchとLayer Tuningへ加算 |
| `randomness` | 0〜1 | Yes | Positionの分散幅 |
| `pan_spread` | 0〜1 | Yes | GrainごとのStereo配置幅 |
| `seed` | 整数 | No | Position・Panの決定的Seed |

Dynamic Parameter：`granular_position`、`grain_size`、`grain_density`、`grain_pitch`、`grain_randomness`、`grain_pan_spread`

RegionがPrepared Frameへ変換できない場合は`INVALID_GRAIN_REGION`、Parameter範囲違反は`INVALID_GRAIN_PARAMETER`です。

### Wave Sequence

複数Assetを時間順に切り替えます。StepはDefinition順の不変配列で、Assetを共有します。

```json
"generator": {
  "wave_sequence": {
    "root_note": 60,
    "direction": "ping_pong",
    "loop": true,
    "crossfade": 0.25,
    "steps": [
      { "id": "attack", "asset": { "path": "<WAVへの相対Path>", "sha256": "<SHA-256>" },
        "region": { "start_seconds": 0.0, "end_seconds": 0.08 },
        "duration": { "mode": "seconds", "value": 0.18 },
        "playback": "loop", "playback_direction": "forward", "gain_db": -3.0, "pitch_cents": 0.0 },
      { "id": "body", "asset": { "path": "<WAVへの相対Path>", "sha256": "<SHA-256>" },
        "region": { "start_seconds": 0.08, "end_seconds": 0.16 },
        "duration": { "mode": "beats", "value": 0.5 },
        "playback": "one_shot", "playback_direction": "reverse", "gain_db": -6.0, "pitch_cents": 300.0 }
    ]
  }
}
```

| Field | Range | 意味 |
|---|---|---|
| `root_note` | 0〜127 | Step Assetの基準MIDI Note |
| `direction` | `forward` / `reverse` / `ping_pong` | Stepを選ぶ順序。`ping_pong`は終端を重複させず往復 |
| `loop` | Boolean | 終端後にSequenceを繰り返すか |
| `crossfade` | 0〜0.5 | 隣接Stepを重ねる割合。定電力で混合 |
| `steps` | 1〜128個 | 時間順のStep配列 |
| `steps[].id` | 一意な文字列 | Step識別子 |
| `steps[].asset` | — | Mono/StereoのWAV |
| `steps[].region` | 0以上の秒 | Assetから読む`[start, end)`領域 |
| `steps[].duration` | `seconds` / `beats`、正の値 | Stepを保持する時間。`beats`はProcess Tempoへ追従 |
| `steps[].playback` | `one_shot` / `loop` | Assetを一度だけ読むか繰り返すか |
| `steps[].playback_direction` | `forward` / `reverse` | AssetのRead方向（Sequence方向とは独立） |
| `steps[].gain_db` | -60〜12 dB | Step固有のGain |
| `steps[].pitch_cents` | -2400〜2400 cents | Root Noteへ加算するPitch |

- `one_shot`はSourceがDurationより先に終わるとStep終端まで無音を保持し、`loop`はRegionを繰り返します
- 利用できないAsset・不正なRegionのStepは削除せず、Durationを保持した無音Stepとして残ります（後続Stepの時間を変えません）
- 全Stepが利用できない場合はLayerだけを発音候補から除外します

Wave Sequence固有のDynamic Parameterはありません。Step構造はコンパイル時に確定します。

## 複数Generatorの組み合わせ

1つの音源に複数のGeneratorを鳴らすには、GeneratorごとにLayerを追加します。LayerはDefinitionの書かれた順に加算され、各LayerのADSRとProcessor、Voice Processor、Global Processorを順に通ります。

代表的な役割分担：

| 構成 | Layer構成 |
|---|---|
| Harmonic / Formant | Formant（共鳴）+ Additive（倍音芯）+ Sample（Attack）+ Noise（Air） |
| Spectral | Spectral（持続Body）+ Additive（倍音）+ Sample（Attack）+ Noise（Air） |
| Digital | Wavetable（持続）+ Operator Modulation（倍音芯）+ Sample（短アタック） |
| Physical / Modal | Physical String（撥弦・振動）+ Modal（Body・共鳴）+ Layer / Voice / Global Processor |

各GeneratorのDynamic Parameterは、単体のGeneratorと同じ方法でModulation RouteやParameter Changeから制御できます。Layer間の時間位置の調整（遅延補償）の規則は`docs/runtime-processing.md`を参照してください。

## Processor

Processorは配列の順序で直列に適用されます。配置と種類は固定で、Processor間の接続先をDefinitionから指定することはできません。

| 配置 | 適用位置 | 使える種類 |
|---|---|---|
| Layer（`processors`） | Generatorの直後 | Filter、Ladder Filter、Formant、Drive、EQ、Resonator、Bitcrusher |
| Voice（`voice_processors`） | 全LayerのMix後 | Filter、Ladder Filter、Formant、Drive、EQ、Resonator、Gate、Transient Shaper、Compressor、Limiter |
| Global（`global_processors`） | 全Voiceの合計後 | Filter、Ladder Filter、Formant、Drive、EQ、Chorus、Flanger、Phaser、Frequency Shifter、Delay、Reverb、Convolution、Gate、Transient Shaper、Compressor、Limiter |

LayerはGeneratorの出力がMonoでもStereoでも同じChainを使い、出力Channel数に応じたStateをCompile時に確保します。VoiceとGlobalのDynamicsは左右のPeakをリンクして処理します。Chorus、Flanger、PhaserはGlobal Chainに1つのStateを持ち、Voiceごとには複製しません。

```json
"processors": [
  { "type": "filter", "id": "attack_tone", "mode": "low_pass", "cutoff_hz": 9000.0, "resonance": 0.1 },
  { "type": "eq", "id": "attack_shape", "low_frequency_hz": 180.0, "low_gain_db": 2.0, "mid_frequency_hz": 1200.0, "mid_gain_db": -3.0, "mid_q": 1.1, "high_frequency_hz": 7000.0, "high_gain_db": 1.5 },
  { "type": "resonator", "id": "attack_ring", "frequency_hz": 440.0, "decay_seconds": 0.8, "damping": 0.35, "mix": 0.2 },
  { "type": "bitcrusher", "id": "attack_crush", "bit_depth": 10.0, "sample_rate_ratio": 0.5, "mix": 0.15 }
],
"voice_processors": [],
"global_processors": [
  { "type": "chorus", "id": "width", "delay_ms": 18.0, "rate_hz": 0.3, "depth": 0.7, "feedback": 0.1, "width": 0.8, "mix": 0.25 },
  { "type": "compressor", "id": "glue", "threshold_db": -18.0, "ratio": 3.0, "attack_ms": 10.0, "release_ms": 120.0, "knee_db": 6.0, "makeup_gain_db": 2.0, "mix": 0.8 },
  { "type": "limiter", "id": "ceiling", "ceiling_db": -1.0, "release_ms": 80.0, "input_gain_db": 0.0 },
  { "type": "frequency_shifter", "id": "metal_shift", "shift_hz": 420.0, "mix": 0.3 },
  { "type": "delay", "id": "echo", "time": { "value": 0.75, "unit": "beats" }, "feedback_mode": "ping_pong", "feedback": 0.3, "taps": [], "mix": 0.15 },
  { "type": "convolution", "id": "body", "ir": { "path": "assets/body-short.wav" }, "gain_db": -3.0, "mix": 0.25 },
  { "type": "reverb", "id": "space", "pre_delay_seconds": 0.012, "decay": 0.6, "damping": 0.35, "width": 1.0, "mix": 0.2 }
]
```

### FieldとRange

| Type | FieldとRange | Dynamic Parameter（Catalog ID） | Static Field |
|---|---|---|---|
| Filter | `mode`: `low_pass` / `high_pass` / `band_pass` / `notch`（省略時`low_pass`）、`cutoff_hz`: 20〜20000 Hz、`resonance`: 0〜1 | `cutoff`、`resonance` | `mode` |
| Drive | `amount`: 0〜1（大きいほど強く歪む）、`mix`: 0〜1（Dry / Wet比） | `amount`、`mix` | なし |
| EQ | Low Shelf / Mid Peaking / High Shelfの3帯域。周波数は順に20〜500、100〜12000、2000〜20000 Hz、Gainは各-24〜24 dB、Mid Qは0.25〜8 | `low_gain_db`、`mid_gain_db`、`high_gain_db` | 3帯域の周波数、`mid_q` |
| Resonator | `frequency_hz`: 40〜12000 Hz、`decay_seconds`: 0.02〜10秒、`damping` / `mix`: 0〜1 | `frequency_hz`、`decay_seconds`、`damping`、`mix` | 最大Delay容量（Sample Rate依存） |
| Bitcrusher | `bit_depth`: 2〜16、`sample_rate_ratio`: 0.01〜1、`mix`: 0〜1 | `bit_depth`、`sample_rate_ratio`、`mix` | なし |
| Chorus | `delay_ms`: 5〜30、`rate_hz`: 0.01〜8、`depth` / `width` / `mix`: 0〜1、`feedback`: 0〜0.85 | `rate_hz`、`depth`、`feedback`、`width`、`mix` | `delay_ms` |
| Flanger | `delay_ms`: 0.5〜10、`rate_hz`: 0.01〜10、`depth` / `width` / `mix`: 0〜1、`feedback`: -0.95〜0.95 | Chorusと同じ | `delay_ms` |
| Phaser | `stages`: 2 / 4 / 6 / 8、`center_hz`: 100〜5000、`sweep_octaves`: 0.25〜6、`rate_hz`: 0.01〜8、`depth` / `width` / `mix`: 0〜1、`feedback`: -0.9〜0.9 | `rate_hz`、`depth`、`feedback`、`width`、`mix` | `stages`、`center_hz`、`sweep_octaves` |
| Ladder Filter | `cutoff_hz`: 20〜20000 Hz、`resonance` / `drive`: 0〜1 | `cutoff`、`resonance`、`drive` | なし（cutoffの実効上限はSample Rate依存） |
| Formant | `vowel_position` / `throat` / `mix`: 0〜1、`formant_shift_cents`: -2400〜2400、`profiles`: 1〜8個の5帯域Profile | `vowel_position`、`formant_shift`、`throat`、`mix` | `profiles` |
| Frequency Shifter（Globalのみ） | `shift_hz`: -5000〜5000 Hz、`mix`: 0〜1 | `shift_hz`、`mix` | 127 framesの固定Latency |
| Delay（Globalのみ） | `time.value`: Secondsは0.001〜8秒、Beatsは0.015625〜2 beats、`feedback`: 0〜0.95、`taps`: 最大8個、`mix`: 0〜1 | `feedback`、`mix` | `time`、`feedback_mode`、`taps`。最大4個、Runtime bufferは16秒まで |
| Reverb（Globalのみ） | `pre_delay_seconds`: 0〜0.2秒、`decay`: 0〜0.98（大きいほど残響が長い）、`damping` / `width` / `mix`: 0〜1 | `decay`、`damping`、`width`、`mix` | `pre_delay_seconds` |
| Convolution（Globalのみ） | `ir`: Mono / Stereo WAV、`gain_db`: -24〜24 dB、`mix`: 0〜1 | `gain_db`、`mix` | IR、256 framesの固定Latency。IRは最大10秒、最大2個 |
| Gate（Voice / Global） | `threshold_db`: -80〜0 dB、`hysteresis_db`: 0〜12 dB、`attack_ms`: 0.1〜100 ms、`hold_ms`: 0〜500 ms、`release_ms`: 5〜2000 ms、`range_db`: -96〜0 dB（0 dBではGate閉時もUnity） | `threshold_db`、`range_db` | `hysteresis_db`、各Time Field |
| Transient Shaper（Voice / Global） | `attack` / `sustain`: -1〜1、`mix`: 0〜1 | `attack`、`sustain`、`mix` | Fast / Slow EnvelopeのTime Constant |
| Compressor | `threshold_db`: -60〜0 dB、`ratio`: 1〜20、`attack_ms`: 0.1〜200、`release_ms`: 5〜2000、`knee_db`: 0〜24、`makeup_gain_db`: -12〜24 dB、`mix`: 0〜1 | `threshold_db`、`ratio`、`makeup_gain_db`、`mix` | `attack_ms`、`release_ms`、`knee_db` |
| Limiter | `ceiling_db`: -12〜0 dBFS、`release_ms`: 5〜1000、`input_gain_db`: -24〜24 dB | `ceiling_db`、`input_gain_db` | `release_ms` |

`Dynamic Parameter`列はModulation RouteやParameter Changeで動かせるField、`Static Field`列はCompile時に確定するFieldです。Filterの`cutoff_hz`だけ、既存のCanonical IDとして`cutoff`をCatalog IDに用います。Processorの種類・ID・配置・順序やStatic Fieldを変えた場合は、再Compileが必要です。

Delayの`time.unit`が`beats`の場合、1 beatは4分音符1つ分で、現在のProcess Tempoから秒へ変換します。`feedback_mode`は`stereo`または`ping_pong`、TapはWet出力だけへ加算されます。Frequency ShifterとConvolutionは、それぞれ127 framesと256 framesの固定Latencyを持ちます。固定Latencyの合計はInspectの`reported_latency_frames`へ反映されます。

FilterのCutoffが処理できる上限（20 kHzとSample Rateから決まる値の小さい方）を超える定義は、Warningを出して上限へ制限します。

Parameter IDの形式：

- Layer Processor: `layer.<layer_id>.processor.<processor_id>.<parameter>`
- Voice Processor: `voice.processor.<processor_id>.<parameter>`
- Global Processor: `global.processor.<processor_id>.<parameter>`

## Modulation

`modulation`は省略可能です。`sources`はVoiceごとのSource定義、`routes`はSourceからDynamic Parameterへの接続です。Routeは書かれた順に同じTargetへ加算され、最後にTarget範囲へClampされます。MacroとTransport PhaseはInstrument Scope、LFOやEnvelopeなどの定義SourceはVoice Scopeです。

**組み込みSource**（定義なしで使えます）：

| Source ID | 範囲 | Polarity | 動作 |
|---|---:|---|---|
| `velocity` | 0〜1 | Unipolar | Note OnのVelocity |
| `key_tracking` | -1〜1 | Bipolar | MIDI Note 0を-1、127を+1へ変換 |
| `pitch_bend` | -1〜1 | Bipolar | 共有External Control |
| `mod_wheel` | 0〜1 | Unipolar | 共有External Control |
| `aftertouch` | 0〜1 | Unipolar | 共有External Control |
| `transport_beat_phase` | 0〜1 | Unipolar | `beat_position`の小数部 |
| `transport_bar_phase` | 0〜1 | Unipolar | `bar_position`の小数部 |

**追加できるSource**：

| `type` | Field | 動作 |
|---|---|---|
| `lfo` | `waveform`、`rate`（`per_second`または`per_beat`）、`phase`（0以上1未満） | Bipolarの周期信号 |
| `envelope` | ADSR（各時間の範囲はLayer ADSRと同じ） | Note Lifecycleに追従 |
| `random` | `seed` | SeedとNote IDから決まる、Voiceごとの固定値 |
| `mseg` | `initial_value`、`segments`、`loop_range` | Segmentを順に進むBipolarのMotion |
| `step` | `values`、`rate` | 値を保持するBipolarのStep列 |
| `sample_hold` | `seed`、`rate` | Rateごとに更新する決定的Bipolar値 |
| `smooth_random` | `seed`、`rate` | 決定的Bipolar値をRateに合わせて補間 |

追加SourceのPolarityは、LFO、Random、MSEG、Step、Sample Hold、Smooth RandomがBipolar（-1〜1）、EnvelopeがUnipolar（0〜1）です。Depthの符号は方向を決め、Bipolar Sourceでは正負両方向へ作用します。

```json
"modulation": {
  "sources": [
    { "type": "lfo", "id": "vibrato", "waveform": "sine", "rate": { "value": 5.0, "unit": "per_second" }, "phase": 0.0 },
    { "type": "envelope", "id": "filter_env", "attack_seconds": 0.01, "decay_seconds": 0.2, "sustain_level": 0.3, "release_seconds": 0.25 },
    { "type": "random", "id": "random_pan", "seed": 42 }
  ],
  "routes": [
    { "source": "vibrato", "target": "layer.body.tuning", "depth": { "value": 20.0, "unit": "cents" }, "curve": "linear" },
    { "source": "filter_env", "target": "voice.processor.tone.cutoff", "depth": { "value": 2.0, "unit": "octaves" }, "curve": "smooth_step" },
    { "source": "random_pan", "target": "layer.body.pan", "depth": { "value": 0.5, "unit": "pan" }, "curve": "linear" }
  ]
}
```

Rateの`per_beat`はQuarter-note beat単位で、Tempo変更後も拍基準の速度を保ちます。`per_second`の範囲は0.01〜40、`per_beat`の範囲は1/64〜16です。

MSEGのSegment `duration`は`seconds`または`beats`で、`target`は-1〜1、Curveは`linear`または`smooth_step`です。Segmentは1〜64個、Loopの終端はExclusive Indexです。ReleaseではLoopを抜けて終端へ進みます。Stepは`values`を順に保持し、Sample HoldはRateごとに決定的な値を保持し、Smooth Randomは同じ決定性を保ったまま値を補間します。これらの変化Frameは処理境界になります。

```json
{
  "id": "motion_env",
  "type": "mseg",
  "initial_value": 0.0,
  "segments": [
    { "duration": { "value": 1.0, "unit": "beats" }, "target": 1.0, "curve": "smooth_step" },
    { "duration": { "value": 0.5, "unit": "beats" }, "target": -0.5, "curve": "linear" }
  ],
  "loop_range": { "start_segment": 0, "end_segment": 2 }
}
```

各Routeの`depth.value`はSigned値、`depth.unit`はTargetのModulation Unitです（Linear TargetはNative Unit、Log2 TargetはOctaves）。`curved_source × depth.value`をNative Domainへ加算し、Log2 TargetはOctave Domainで加算して`base × 2^sum`へ変換します。RouteはDefinition順に加算し、最後にTarget範囲へClampします。Parameter IDの解決とRouteの計算はコンパイル前に完了するため、音声処理中に文字列IDやJSONを扱いません。

## コンパイル時の変換

コンパイルで一度だけ計算し、実行時はその値を使います。

| 変換 | 内容 |
|---|---|
| dB → Gain | `gain_db`を線形Gainへ |
| cent → 音程比 | `tuning_cents`を再生速度の比へ |
| ADSRの秒 → Frame数 | Sample Rateに依存するFrame数へ |
| Granular Regionの秒 → Frame数 | Prepared Audio内の固定Regionへ |
| Filter Cutoff | Sample Rateの上限へ制限 |
| Parameter一覧 | LayerとProcessorのDynamic Parameterへ、安定ID・範囲・Scale・Smoothingを割り当て |
| Modulation | SourceをTableへ、RouteのDepthをTargetのNativeまたはLog2 Domainへ解決 |

**Assetの準備**

Sample、Wavetable、Spectral、Granular、Wave Sequenceは、コンパイル時にAssetを読み込み、SHA-256を照合してWAVをDecodeします。Sample Rateが異なる場合は変換し、同一Assetを参照するZoneやGenerator間でPrepared Audioを共有します。

読み込めなかったAssetを使うUnitは無音・無効になり、Warningを残して他の部分のコンパイルとレンダリングを続けます（SampleはZone単位、Wave SequenceはStep単位、それ以外はLayer単位）。ZoneのSHA-256省略もWarningです。

**ErrorとWarning**

- Errorが1つでもあれば、コンパイル結果を返しません
- Warningだけなら、Warning付きのコンパイル結果を返して処理を続けます
- Parameter ID、Source ID、Source設定、Route Target、Depth Unit / 範囲のErrorはコンパイル前にまとめて返します
