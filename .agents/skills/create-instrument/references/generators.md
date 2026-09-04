# Generator仕様

GeneratorはLayerの`generator` Fieldへ、いずれか1つを指定します。ここでは各GeneratorのField・Range・Dynamic Parameter・制約を扱います。

| Generator | 用途 |
|---|---|
| Oscillator | 基本波形（Sine / Saw / Square / Triangle / Pulse）とComplex変形 |
| Noise | White / Pink / Brown Noise |
| Physical String | Fractional Delay Feedbackによる弦・硬質振動 |
| Modal | 複数Modeの共鳴によるBody・Bell・Plate |
| Additive | Partial直接設計による倍音構成 |
| Formant | 母音共鳴のBand制御 |
| Wavetable | 周期波形Frame列のPosition走査 |
| Spectral | WAVのSTFT再構成とA/B Morph |
| Operator Modulation | 4 Operator FM / PM / AM / Ring |
| Sample | 鍵盤範囲別のSample再生 |
| Granular | Grain分解によるTexture再構成 |
| Wave Sequence | 複数Assetの時系列切り替え |

各GeneratorのField表で**Dynamic**がYesの項目は、Modulation RouteやParameter Changeから動かせます。Dynamic ParameterのIDは`layer.<layer_id>.generator.<name>`形式です（Operator Modulationだけ`operator.<1-4>.<parameter>`）。`position`のような走査位置は、各Generatorが準備したSource領域内の位置を表します。値を変えるときは`instrument inspect`でNative Unit・Scale・Clamp範囲を確認できます。

## Oscillator

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

`instrument inspect --json`で`phase_domain` Backend、信号順序、DC Blocker、各Parameter IDを確認します。

## Noise

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

## Physical String

Deterministic ExciterをFractional DelayのFeedback Loopへ入力し、弦や硬質な振動体の撥弦・金属的な振動を作ります。出力はMonoです。特定の楽器を再現するModelではなく、Layer Processorや他のGeneratorと組み合わせるための固定Topologyです。`decay_seconds`はLoopの高域Lossを含まないNominal T60、`brightness`はLoopの高域Loss、`stiffness`はDispersionを表します。

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

- ExciterとLoopのLow-pass Cutoffには、処理Sample Rateから決まる共通の上限があり、`brightness`はその上限までの対数位置として扱われます
- Fundamentalは4 Hz以上、処理Sample Rateの0.45倍以下で、Layer Tuningを含めてこの範囲を外れる場合はRender Errorになります
- Pitchを作るのはLayer NoteとTuningであり、Generator独自のPitch Parameterはありません
- 出力レベルは控えめなため、想定より小さいときはLayerの`gain_db`（上限+12 dB）で補正します

Note OffではGenerator固有のEnvelopeを追加せず、Layer ADSRのReleaseを適用します。

Dynamic Parameter：`physical_string_decay_seconds`、`physical_string_brightness`、`physical_string_stiffness`。`decay_seconds`は`Seconds + Log2`で、Modulation DepthのUnitは`octaves`です。Parameter ID例：`layer.string.generator.physical_string_decay_seconds`

## Modal

Rust側のDeterministic ExciterをPinned DaisySPの低レベル`Resonator`へ入力し、複数Modeの共鳴で棒・板・ベル・金属・木質・ガラス的なBodyを作ります。出力はMonoです。

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

- `structure`は単純な明るさではなくMode配置を変える値です。`decay`はNative ResonatorのDampingへ渡しますが、周波数・Structure・Brightnessとの相互作用があるため、一定秒数のDecayとして解釈しません
- Fundamentalの安全周波数範囲とNote Off時のLayer ADSRはPhysical Stringと同じです
- 実在する楽器名をGeneratorのModel名のように扱わず、Layer・Processor・Modulationの組み合わせで目指す音色を作ります

Dynamic Parameter：`modal_structure`、`modal_brightness`、`modal_decay`。`mode_count`とExciterのStatic FieldはParameter Catalogへ登録されません。Parameter ID例：`layer.body.generator.modal_structure`

## Additive

Note Frequencyを基準にした1〜64個のPartialのSineを加算します。整数比で倍音構成、非整数比（例：`2.73`）で非整数倍音Bellや金属的質感を作れます。出力はMonoです。

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

完全無音Spectrum、空のPartial配列、重複ID、65個以上のPartialはValidation Errorです。`instrument inspect --json`でPartial Count、Ratio、Amplitude、Phase、Envelope有無、3 Parameter IDを確認します。

## Formant

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
| Band `frequency_hz` | 100〜12000 Hz | No | Bandの中心周波数 |
| Band `bandwidth_hz` | 20〜5000 Hz | No | Bandの帯域幅 |
| Band `gain_db` | -60〜12 dB | No | Bandの相対強度 |

- 隣接Profileの補間は、周波数・帯域幅が幾何平均、GainがdB線形です
- Formant固有ADSRはなく、Layer ADSRをPartial合計の後に適用します

Dynamic Parameter：`formant_vowel_position`、`formant_shift`、`formant_throat`、`formant_spectral_tilt`

Profileが0個または9個以上、Partial数が0または65以上、Band数が5以外、ID重複、周波数非昇順、各Range違反はValidation Errorです。`instrument inspect --json`でProfile Count、5 Band、4 Parameter ID、出力Modeを確認します。

## Wavetable

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

- WAV全体のSample数は`frame_length`で割り切れ、Frame数が1〜256である必要があります
- Source Sample RateはPitchへ使わず、WavetableをResampleしません
- Unison 1はMono、2 Voice以上はStereoです

Dynamic Parameter：`wavetable_position`、`unison_detune` / `unison_spread`（Unison指定時）

`instrument validate`はFrame配置、Hash、無音Frame / DCを検査し、`inspect --json`は準備済み状態、Band、Position Parameter ID、実効周波数上限を返します。

## Spectral

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
| `asset_a` | — | No | 解析と時間軸の基準になるWAV。Mono / Stereoを保持 |
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
- 報告Latencyは他Layerへ補償されるため、Transientの時間位置はHybrid全体で確認します
- 元波形への再合成を確認する際は5 Parameterを0へ揃え、Latency後で元WAVと比較します
- `asset_b`の準備に失敗したときはAだけへフォールバックせず、Layerが無効化されます

Dynamic Parameter：`spectral_position`、`spectral_freeze`、`spectral_blur`、`spectral_shift`、`spectral_morph`（`asset_b`指定時のみ）

A/BのChannel数不一致はCompile Errorです。`inspect --json`でAsset A/Bの準備済み状態、Source Channel、Spectral Frame、準備済みSample Rate、FFT / Hop / Bin、Latency、5 Parameter IDを確認します。

## Operator Modulation

4つのSine Operatorを固定Topologyで接続します。Carrierだけが出力を持ち、他のOperatorは変調信号を供給します。

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
| `algorithm` | `stack_4` / `stack_3_plus_carrier` / `two_stacks` / `fork_to_carrier` / `two_modulators_plus_carrier` / `three_modulators` / `shared_modulator` / `parallel` | No | Operatorの接続Topology |
| `operators[].ratio` | 0.25〜32 | Yes | Note Frequencyに対する周波数比 |
| `operators[].detune_cents` | -100〜100 | Yes | 周波数の微調整 |
| `operators[].level` | 0〜1 | Yes | Carrierの出力音量（Carrierのみ） |
| `operators[].modulation_amount` | Mode依存 | Yes | Phase / Frequencyは0〜8、Amplitude / Ringは0〜1 |
| `operators[].feedback` | 0〜1 | Yes | 直前Sampleで自己変調する量（Phase / Frequencyのみ） |
| `operators[].phase` | 0〜1 | No | 初期位相 |
| `operators[].envelope` | Optional ADSR | No | Operator個別のADSR |
| `unison.voices` | 1〜4 | No | UnisonのVoice数 |

制約：

- Carrierの`level`だけが出力へ寄与し、Carrier以外の`level`は0です
- 接続元のOperatorだけが`modulation_amount`を持ち、出力先を持たないAmountは0です
- FeedbackはPhase / Frequency Modeだけで使え、Amplitude / Ring Modeでは0にします
- Unisonは最大4 Voice。ADSRを全Componentで共有します

AlgorithmごとのOperator接続は次のとおりです。`A→B`はAの出力でBを変調することを表し、出力を持つCarrier列のOperatorだけが`level`で出力音量を持ちます。Modulatorは`modulation_amount`だけを持ち、Carrier（変調を受けない単独Carrierを含む）の`modulation_amount`は0にします。

| Algorithm | 信号の流れ | 出力を持つCarrier |
|---|---|---|
| `stack_4` | 4→3→2→1 | 1 |
| `stack_3_plus_carrier` | 4→3→2と、変調を受けない単独Carrierの1 | 1、2 |
| `two_stacks` | 2→1と4→3 | 1、3 |
| `fork_to_carrier` | 4→2と4→3を経て、両方とも1を変調 | 1 |
| `two_modulators_plus_carrier` | 3→1と4→1と、変調を受けない単独Carrierの2 | 1、2 |
| `three_modulators` | 2→1、3→1、4→1 | 1 |
| `shared_modulator` | 4→1、4→2、4→3 | 1、2、3 |
| `parallel` | 変調なし。4 Operatorすべてが単独Carrier | 1、2、3、4 |

`modulation_amount`はModeごとに次の意味を持ちます。

| Mode | `modulation_amount`の働き |
|---|---|
| Phase | 接続元Operatorの出力でCarrierの位相を前後させる |
| Frequency | 接続元Operatorの出力でCarrierの瞬時周波数を増減させる |
| Amplitude | 接続元Operatorの出力でCarrierの振幅を変調する |
| Ring | Carrier信号と接続元Operator出力の積へ、Amountの割合で近づける |

Dynamic Parameter：`operator.<1-4>.ratio`、`operator.<1-4>.detune`、`operator.<1-4>.level`（Carrierのみ）、`operator.<1-4>.modulation_amount`（接続元のみ）、`operator.<1-4>.feedback`（Phase / Frequencyのみ）、`unison_detune` / `unison_spread`（Unison指定時）

`inspect --json`でMode、Algorithm、評価順序、Carrier、4 OperatorのParameter ID、Unison、実効周波数上限を確認します。

## Sample

鍵盤範囲・Velocity範囲でZoneを選んで再生します。Mono / StereoのChannel構成を保持します。

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
| `asset` | — | Mono / StereoのWAV |
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
| `time` | 時間伸縮Mode（下表）。省略不可 |

`loop.crossfade_seconds`が0より大きいと、境界を定電力でBlendします。CrossfadeはLoop長の半分以下にします。

| `time.mode` | 動作 | 制約 |
|---|---|---|
| `resample` | Pitch変更へ合わせてDurationも変える | — |
| `fixed_stretch` | `ratio`（0.5〜2.0）でDurationだけを変える | Pitch不変。Reverseとは併用不可 |
| `tempo_sync` | `source_bpm`とProcess TempoからDuration比を決める | Pitch不変。Reverseとは併用不可 |

`fixed_stretch`と`tempo_sync`のDuration比が0.5〜2.0の範囲外だとProcess Errorです。

Release Sampleを作る場合はLayerの`trigger.event`を`note_off`にします。Path違いやHash不一致ではそのZoneだけが無効化され、他ZoneやLayerでRenderが継続します。

## Granular

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
| `asset` | — | No | Mono / StereoのWAV。SampleとPrepared Audioを共有 |
| `root_note` | 0〜127 | No | 基準MIDI Note |
| `region` | 0以上の秒 | No | Grainが参照するPrepared Region。`end`省略時はAsset終端 |
| `position` | 0〜1 | Yes | Region内の基本位置。0がStart、1がGrain長を考慮したEnd側 |
| `grain_size` | 0.005〜0.5秒 | Yes | Grain長 |
| `density` | 1〜100 grains/sec | Yes | Grainの生成密度 |
| `pitch` | -2400〜2400 cents | Yes | Note PitchとLayer Tuningへ加算 |
| `randomness` | 0〜1 | Yes | Positionの分散幅 |
| `pan_spread` | 0〜1 | Yes | GrainごとのStereo配置幅 |
| `seed` | 整数 | No | Position・Panの決定的Seed |

Note OffではGrainを破棄せずLayer EnvelopeがReleaseへ進み、Voice StealingまたはReset時だけPoolを初期化します。

Dynamic Parameter：`granular_position`、`grain_size`、`grain_density`、`grain_pitch`、`grain_randomness`、`grain_pan_spread`

RegionがPrepared Frameへ変換できない場合は`INVALID_GRAIN_REGION`、Parameter範囲違反は`INVALID_GRAIN_PARAMETER`です。`inspect --json`で準備済み状態、領域Frame、6 Parameter ID、Source Channel、Seed、Grain Pool Limitを確認します。

## Wave Sequence

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
| `steps[].asset` | — | Mono / StereoのWAV |
| `steps[].region` | 0以上の秒 | Assetから読む`[start, end)`領域 |
| `steps[].duration` | `seconds` / `beats`、正の値 | Stepを保持する時間。`beats`はProcess Tempoへ追従 |
| `steps[].playback` | `one_shot` / `loop` | Assetを一度だけ読むか繰り返すか。`one_shot`はSource終了後、Step終端まで無音を保持する |
| `steps[].playback_direction` | `forward` / `reverse` | AssetのRead方向（Sequence方向とは独立） |
| `steps[].gain_db` | -60〜12 dB | Step固有のGain |
| `steps[].pitch_cents` | -2400〜2400 cents | Root Noteへ加算するPitch |

- 利用できないAsset・不正なRegionのStepは削除せず、Durationを保持した無音Stepとして残ります（後続Stepの時間を変えません）
- 全Stepが利用できない場合はLayerだけを発音候補から除外します

Wave Sequence固有のDynamic Parameterはありません。Step構造はコンパイル時に確定します。`inspect --json`でStep Count、Direction、Loop、Crossfade、領域Frame、Duration、Playback、Availability、Pitch、Gainを確認します。
