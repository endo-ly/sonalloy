# 音源定義

音源定義（JSONファイル）は、手で編集して保存・管理する正本です。この文書は、音源定義を正しく書くための**Fieldの制約・Range・振る舞い**を、読者の書きたい音源の該当箇所を調べられる形でまとめます。

読者のプロセスに沿って、**全体構造 → 共通の制約 → Layer → Generator → 複数Generatorの組み合わせ → Processor → Modulation → コンパイル時の変換**の順で説明します。実行時の振る舞い（Voice、ADSRの進行、Sample再生、Grain生成など）は`docs/runtime-processing.md`を、CLIの使い方は`docs/cli.md`を参照してください。

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
| `schema_version` | スキーマ版。現在は`2`。`1`はUnsupportedとして拒否 |
| `metadata` | `name`、`author`、`description` |
| `performance` | `polyphony`、`voice_stealing` |
| `layers` | 発音の単位となるLayer配列（1個以上） |
| `voice_processors` | 全LayerのMix後に適用するProcessor Chain |
| `global_processors` | 全Voiceの合計後に適用するProcessor Chain |
| `modulation` | SourceとRouteの定義（省略可）。Routeは`depth.value`と`depth.unit`でTargetに直接効く量を指定 |

全体の例（Saw Oscillatorの最小構成）：

```json
{
  "schema_version": 2,
  "metadata": { "name": "Basic Poly Synth", "author": null, "description": "..." },
  "performance": { "polyphony": 16, "voice_stealing": "quietest_releasing_then_oldest" },
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
  }
}
```

## 共通の制約

| 項目 | 制約 |
|---|---|
| `polyphony` | 1〜64 |
| `gain_db` | -60〜12 dB |
| `pan` | -1〜1 |
| `tuning_cents` | -1200〜1200 |
| Key / Velocity | Key 0〜127、Velocity 1〜127。最小値は最大値以下 |
| ADSR | Attack / Decay / Releaseは0〜30秒、Sustainは0〜1 |
| Filter | `mode`は`low_pass` / `high_pass` / `band_pass` / `notch`、`cutoff_hz` 20〜20000 Hz、`resonance` 0〜1。CutoffがSample Rateの上限を超える場合はWarningを出し、`min(20000, Sample Rate × 0.45)`へ制限します |
| Drive | `amount` / `mix`ともに0〜1 |
| EQ | Low Shelf / Mid Peaking / High Shelfの3帯域。周波数は順に20〜500、100〜12000、2000〜20000 Hz、Gainは各-24〜24 dB、Mid Qは0.25〜8 |
| Resonator | `frequency_hz` 40〜12000 Hz、`decay_seconds` 0.02〜10秒、`damping` / `mix` 0〜1 |
| Bitcrusher | `bit_depth` 2〜16、`sample_rate_ratio` 0.01〜1、`mix` 0〜1 |
| Chorus | `delay_ms` 5〜30、`rate_hz` 0.01〜8、`depth` / `width` / `mix` 0〜1、`feedback` 0〜0.85 |
| Flanger | `delay_ms` 0.5〜10、`rate_hz` 0.01〜10、`depth` / `width` / `mix` 0〜1、`feedback` -0.95〜0.95 |
| Phaser | `stages` 2 / 4 / 6 / 8、`center_hz` 100〜5000、`sweep_octaves` 0.25〜6、`rate_hz` 0.01〜8、`depth` / `width` / `mix` 0〜1、`feedback` -0.9〜0.9 |
| Delay | `time_seconds` 0.001〜2秒、`feedback` 0〜0.95、`mix` 0〜1。Globalのみ |
| Reverb | `pre_delay_seconds` 0〜0.2秒、`decay` 0〜0.98、`damping` / `width` / `mix` 0〜1。Globalのみ |
| Compressor | `threshold_db` -60〜0 dB、`ratio` 1〜20、`attack_ms` 0.1〜200、`release_ms` 5〜2000、`knee_db` 0〜24、`makeup_gain_db` -12〜24 dB、`mix` 0〜1 |
| Limiter | `ceiling_db` -12〜0 dBFS、`release_ms` 5〜1000、`input_gain_db` -24〜24 dB |
| ID（Processor / Layer / Source） | 小文字で始まり、小文字・数字・`_`を使用。`.`は使わない |
| Modulation Depth | Targetに対応するModulation Unitで指定するSigned値。Linear TargetはNative Unit、Log2 TargetはOctaves |
| LFO | Rate 0.01〜40 Hz、Phase 0以上1未満 |
| Modulation Envelope | 各時間0〜30秒、Sustain 0〜1 |
| 未知のField | JSON Parse Error |

Filterの`mode`を省略したJSONは`low_pass`として読み込まれます。新しくSerializerで出力するFilter JSONには`mode`が明示されます。

`layers`は書かれた順に同じVoiceへMixします。`enabled: false`のLayerはコンパイル対象外です。

## Layer

Layerは「Generator + Layer Processor + ADSR + Gain + Pan」のセットで、Trigger条件に合ったLayerだけが鳴ります。

| Field | 内容 |
|---|---|
| `id` | Layer識別子。一意 |
| `enabled` | 発音の有無 |
| `trigger` | 発音条件（下記） |
| `gain_db` / `pan` / `tuning_cents` | Layer音量・定位・音程 |
| `envelope` | ADSR |
| `processors` | Generator後に直列適用するProcessor Chain |
| `generator` | 音源（[Generator](#generator)参照） |

**Trigger**

| Field | 内容 |
|---|---|
| `event` | `note_on`（Note Onで発音）または`note_off`（Note Onで待機状態になり、対応するNote Offで発音）。Voice Stealingは演奏上のNote Offではないため、待機Layerを発音しません |
| `key_min` / `key_max` | 発音するMIDI Note範囲（0〜127） |
| `velocity_min` / `velocity_max` | 発音するVelocity範囲（1〜127） |

**ADSR**

ADSRは音の音量変化を形作る4区間です。Note OnでAttackから始まり、Decayを経てSustainで待機し、Note OffでReleaseへ進みます。

| Field | 内容 |
|---|---|
| `attack_seconds` | Note Onから最大音量へ達する時間 |
| `decay_seconds` | 最大音量からSustain Levelへ下がる時間 |
| `sustain_level` | Note On中の音量（0〜1） |
| `release_seconds` | Note Offから無音へ至る時間 |

## Generator

GeneratorはLayerの`generator` Fieldへ、いずれか1つを指定します。

| Generator | 用途 |
|---|---|
| [Oscillator](#oscillator) | 基本波形とComplex変形 |
| [Noise](#noise) | White / Pink / Brown Noise |
| [Additive](#additive) | Partial直接設計による倍音構成 |
| [Formant](#formant) | 母音共鳴のBand制御 |
| [Wavetable](#wavetable) | 周期波形Frame列のPosition走査 |
| [Spectral](#spectral) | WAVのSTFT再構成とA/B Morph |
| [Operator Modulation](#operator-modulation) | 4 Operator FM / PM / AM / Ring |
| [Sample](#sample) | 鍵盤範囲別のSample再生 |
| [Granular](#granular) | Grain分解によるTexture再構成 |
| [Wave Sequence](#wave-sequence) | 複数Assetの時系列切り替え |

以下では各Fieldの**制約・Range・Dynamic Parameter**を示します。実行時の振る舞い（位相の進行、Cursor、Grain生成など）は`docs/runtime-processing.md`を参照してください。Dynamic ParameterのIDは`layer.<layer_id>.generator.<name>`形式（Operator Modulationだけ`operator.<1-4>.<parameter>`）です。

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
| `waveshaping.amount` | 0〜1 | Yes | 0はBypass。`shape = 1 + amount × 3`、Wetは`tanh(shape × x) / tanh(shape)`、DryからWetへ`amount`でLinear Crossfade |
| `phase_distortion.amount` | 0〜1 | Yes | 0はIdentity。Phase Breakpointは`0.5 - amount × 0.45`（1で0.05） |
| `wavefold.amount` | 0〜1 | Yes | 0はBypass。DaisySP Driveは`1 + amount × 7`、Wavefolderへ渡すWet量は同じ`amount` |
| `feedback.amount` | 0〜1 | Yes | 0は無効。直前Sample`previous`から`(tanh(previous × amount × 2.5)) × 0.25`のPhase寄与を作る |
| `unison.voices` | 2〜8 | No | UnisonのVoice数 |
| `unison.detune_cents` | 0〜100 | Yes | 各VoiceのDetune幅 |
| `unison.stereo_spread` | 0〜1 | Yes | 左右への配置幅 |
| `unison.phase_spread` | 0〜1 | No | 各Voiceの位相ばらつき |

制約：

- Hard SyncはSineでは使えません。Hard Sync指定時の`phase`は0だけです
- Phase DistortionとFeedbackはSineだけで使え、Hard Syncとは併用できません
- Wavefoldは全Waveformで使えます
- Hard SyncとUnisonを組み合わせる場合、`phase_spread`は0だけです

Dynamic Parameter：`sync_ratio`、`waveshape`、`phase_distortion`、`wavefold`、`oscillator_feedback`、`unison_detune`、`unison_spread`

`waveshaping.amount`、`phase_distortion.amount`、`wavefold.amount`、`feedback.amount`は0〜1のAlgorithm Strengthです。Routeで動かす場合も、Inspectに表示されるParameterのNative範囲とUnitを使用します。

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
| `inharmonicity` | 0〜1 | Yes | 高域Ratioの非整数化 |
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
| `throat` | 0〜1 | Yes | 帯域幅の拡大・縮小（0.5で等倍） |
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

Assetの欠落・Hash不一致・Decode失敗、レイアウト不正、全Frame無音は、Wavetable Layerを発音候補から除外します（他Layerのコンパイルとレンダリングは継続）。

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
| `freeze` | 0〜1 | Yes | 1でFrameを固定 |
| `blur_seconds` | 0〜1秒 | Yes | 時間方向のBlur |
| `shift_hz` | -12000〜12000 Hz | Yes | 周波数をHzで移動 |
| `morph` | 0〜1 | Yes | A/BのMorph。`asset_b`なしで0以外はValidation Error |
| `phase_reset` | Boolean | No | Note Onで位相を初期状態へ戻すか |

- MIDI NoteとLayer TuningはRoot Noteに対する周波数比として適用され、Source Durationは変わりません
- 報告Latencyは他Layerへ補償されます

Dynamic Parameter：`spectral_position`、`spectral_freeze`、`spectral_blur`、`spectral_shift`、`spectral_morph`（`asset_b`指定時のみ）

A/BのChannel数不一致はCompile Error、Assetの欠落・Hash不一致・Decode失敗はSpectral Layerを発音候補から除外します（Aだけへはフォールバックしません）。

### Operator Modulation

4つのSine Operatorを固定Topologyで接続します。Carrierだけが出力を持ち、他のOperatorは変調信号を供給します。Algorithmは`stack_4`、`stack_3_plus_carrier`、`two_stacks`、`fork_to_carrier`、`two_modulators_plus_carrier`、`three_modulators`、`shared_modulator`、`parallel`から選びます。任意の接続GraphやOperator間Cycleは指定できません。

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
| `operators[].modulation_amount` | Mode依存 | Yes | Phase/Frequencyは0〜8、Amplitude/Ringは0〜1。下記のMode式で使用 |
| `operators[].feedback` | 0〜1 | Yes | 直前Sampleで自己変調する量（Phase/Frequencyのみ） |
| `operators[].phase` | 0〜1 | No | 初期位相 |
| `operators[].envelope` | Optional ADSR | No | Operator個別のADSR |
| `unison.voices` | 1〜4 | No | UnisonのVoice数 |

制約：

- Carrierの`level`だけが出力へ寄与し、Carrier以外の`level`は0です
- 接続元のOperatorだけが`modulation_amount`を持ち、出力先を持たないAmountは0です
- FeedbackはPhase/Frequencyだけで、Amplitude/Ringでは0だけを許可します
- Unisonは最大4 Voice。ADSRを全Componentで共有します

Dynamic Parameter：`operator.<1-4>.ratio`、`operator.<1-4>.detune`、`operator.<1-4>.level`（Carrierのみ）、`operator.<1-4>.modulation_amount`（接続元のみ）、`operator.<1-4>.feedback`（Phase/Frequencyのみ）、`unison_detune` / `unison_spread`（Unison指定時）

Operatorの`modulation_amount`はModeごとに次の意味です。`output`は接続元Operatorの現在出力、`signal`は出力先Carrierの現在信号です。

| Mode | Runtimeでの意味 |
|---|---|
| Phase | 接続元の`output × amount`を加算し、合計へ`0.5`を掛けた値をCarrierのPhaseへ加算 |
| Frequency | 接続元の`output × amount`を加算し、`frequency × (1 + sum + feedback_offset)`で瞬時周波数を計算してClamp |
| Amplitude | 各接続元について`1 + output × amount`を乗算し、最後に振幅倍率を`0..4`へClamp |
| Ring | `signal`を`signal × output`へ`amount`でLinear Crossfade |

Phase/FrequencyのFeedbackは、直前出力`previous`とAmountから`(tanh(previous × amount × 2.5)) × 0.25`を作り、PhaseまたはFrequencyの式へ加算します。

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

- 重なるZoneは、同じ`round_robin_group`と完全一致するKey/Velocity範囲を持ちます
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
- Assetの欠落・Hash不一致・Decode失敗はそのZoneだけを無効化し、他ZoneとLayerのコンパイルを継続します

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

RegionがPrepared Frameへ変換できない場合は`INVALID_GRAIN_REGION`、Parameter範囲違反は`INVALID_GRAIN_PARAMETER`です。Assetの欠落・Hash不一致・Decode失敗はGranular Layerを発音候補から除外します。

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
- Missing Assetや不正RegionのStepは削除せず、Durationを保持した無音Stepとして残ります（後続Stepの時間を変えません）
- 全Stepが利用できない場合はLayerだけを発音候補から除外します

Wave Sequence固有のDynamic Parameterはありません。Step構造はコンパイル時に確定します。

## 複数Generatorの組み合わせ

複数GeneratorはLayerへ同時に記述でき、DefinitionのLayer順に加算されます。各LayerのADSRとProcessor、Voice Processor、Global Processorを順に適用します。

代表的な役割分担：

| 構成 | Layer構成 |
|---|---|
| Harmonic / Formant | Formant（共鳴）+ Additive（倍音芯）+ Sample（Attack）+ Noise（Air） |
| Spectral | Spectral（持続Body）+ Additive（倍音）+ Sample（Attack）+ Noise（Air） |
| Digital | Wavetable（持続）+ Operator Modulation（倍音芯）+ Sample（短アタック） |

各GeneratorのDynamic Parameterは、単体の場合と同じModulation RouteやParameter Changeから制御できます。Spectralの報告Latencyが最大のとき、他Layerへ遅延補償を確保して、Transientの時間位置を揃えます。

## Processor

Processorは配列の順序で直列に適用されます。配置と種類は固定で、任意のGraphやRoutingは作りません。Processorの内部FeedbackやDelay Stateは許可しますが、Processor間の接続先をDefinitionから指定することはできません。

| 配置 | 適用位置 | 使える種類 |
|---|---|---|
| Layer（`processors`） | Generatorの直後 | Filter、Drive、EQ、Resonator、Bitcrusher |
| Voice（`voice_processors`） | 全LayerのMix後 | Filter、Drive、EQ、Resonator、Compressor、Limiter |
| Global（`global_processors`） | 全Voiceの合計後 | Filter、Drive、EQ、Chorus、Flanger、Phaser、Delay、Reverb、Compressor、Limiter |

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
  { "type": "delay", "id": "echo", "time_seconds": 0.28, "feedback": 0.3, "mix": 0.15 },
  { "type": "reverb", "id": "space", "pre_delay_seconds": 0.012, "decay": 0.6, "damping": 0.35, "width": 1.0, "mix": 0.2 }
]
```

### Processor FieldとDynamic Parameter

各行の`Dynamic`はModulation RouteやParameter Changeから制御でき、`Static`はCompile時に確定します。CatalogのIDはDefinition Fieldと同じ意味を持ちますが、Filterの`cutoff_hz`だけは既存のCanonical IDとして`cutoff`になります。

| Type | Dynamic Parameter（Catalog ID） | Static Field |
|---|---|---|
| Filter | `cutoff`、`resonance` | `mode` |
| Drive | `amount`、`mix` | なし |
| EQ | `low_gain_db`、`mid_gain_db`、`high_gain_db` | 3帯域の周波数、`mid_q` |
| Resonator | `frequency_hz`、`decay_seconds`、`damping`、`mix` | 最大Delay容量（Sample Rate依存） |
| Bitcrusher | `bit_depth`、`sample_rate_ratio`、`mix` | なし |
| Chorus / Flanger | `rate_hz`、`depth`、`feedback`、`width`、`mix` | `delay_ms` |
| Phaser | `rate_hz`、`depth`、`feedback`、`width`、`mix` | `stages`、`center_hz`、`sweep_octaves` |
| Delay | `feedback`、`mix` | `time_seconds` |
| Reverb | `decay`、`damping`、`width`、`mix` | `pre_delay_seconds` |
| Compressor | `threshold_db`、`ratio`、`makeup_gain_db`、`mix` | `attack_ms`、`release_ms`、`knee_db` |
| Limiter | `ceiling_db`、`input_gain_db` | `release_ms` |

Definitionの変更でProcessorの種類、ID、配置、順序、Static Fieldを変えた場合は再Compileが必要です。Dynamic Parameterの範囲外の値はRoute加算後にClampされ、Sample RateやBlock Sizeをまたいでも同じ時間軸で処理されます。

Parameter IDの形式：

- Layer Processor: `layer.<layer_id>.processor.<processor_id>.<parameter>`
- Voice Processor: `voice.processor.<processor_id>.<parameter>`
- Global Processor: `global.processor.<processor_id>.<parameter>`

## 数値の意味論

Definitionの数値は、名前だけから効果を推測せず、次のEndpointと式で解釈します。範囲内で値を変えるときは、`instrument inspect`でNative Unit・Scale・Clamp範囲を確認できます。

| Field | 0 / 中立 | 1 / 終端 | 実行時の意味 |
|---|---|---|---|
| `drive.amount` | Identity | 最大Shape | Shape係数は`amount × 4`。Wetは正規化`tanh` Saturation |
| `drive.mix` | Dry | Wet | `input + (wet - input) × mix`のLinear Crossfade |
| Additive / Spectral `morph` | A | B | AdditiveはA/B振幅、SpectralはA/B Spectral Frameを補間 |
| Wavetable / Spectral / Granular `position` | Source Domainの開始 | Source Domainの終了 | それぞれFrame列、Spectral Frame列、Granular Regionの読出位置 |
| Noise `stereo_correlation` | 左右独立 | 左右同一 | Shared NoiseとIndependent Noiseを平方根Gainで混合 |
| `unison.stereo_spread` / `grain_pan_spread` | 中央 | 最大配置幅 | 左右配置係数へ対称に適用。Unisonは中心から各Voiceを配置 |
| Spectral `freeze` | 通常走査 | Frame固定 | Scan進行量を`1 - freeze`倍する。Phaseは進む |
| Formant `throat` | — | — | `0.5`がBandwidth不変。0〜1でBandwidthを`0.5〜2`倍へ変える |
| Reverb `decay` | 最短Decay | 最長Decay | Tank Feedbackは`clamp(decay × 0.2, 0, 0.19)` |
| Granular `randomness` | 指定Position | 最大分散 | Grainごとの決定的なBipolar位置値へ係数として掛け、Region内でWrap |
| Additive `inharmonicity` | Harmonic | 最大非整数化 | 高次PartialのRatioへ実装済みの非整数化係数を適用。Fundamental Ratio 1は維持 |

`position`の範囲は対象Generatorが準備したSource Domainの範囲であり、音声Buffer全体の絶対位置ではありません。`formant.throat`の0.5はNeutral Pointで、0や1が無変化ではありません。

## Modulation

`modulation`は省略可能です。`sources`はVoiceごとのSource定義、`routes`はSourceからDynamic Parameterへの接続です。Routeは書かれた順に同じTargetへ加算され、最後にTarget範囲へClampされます。

**組み込みSource**（定義なしで使えます）：

| Source ID | 範囲 | Polarity | 動作 |
|---|---:|---|---|
| `velocity` | 0〜1 | Unipolar | Note OnのVelocity |
| `key_tracking` | -1〜1 | Bipolar | MIDI Note 0を-1、127を+1へ変換 |
| `pitch_bend` | -1〜1 | Bipolar | 共有External Control |
| `mod_wheel` | 0〜1 | Unipolar | 共有External Control |
| `aftertouch` | 0〜1 | Unipolar | 共有External Control |

**追加できるSource**：

| `type` | Field | 動作 |
|---|---|---|
| `lfo` | `waveform`、`rate_hz`（0.01〜40）、`phase`（0〜1） | Bipolarの周期信号 |
| `envelope` | ADSR | Note Lifecycleに追従 |
| `random` | `seed` | SeedとNote IDから決まる、Voiceごとの固定値 |

追加SourceのPolarityは、LFOとRandomがBipolar（-1〜1）、EnvelopeがUnipolar（0〜1）です。Depthの符号は方向を決め、Bipolar Sourceでは正負両方向へ作用します。

```json
"modulation": {
  "sources": [
    { "type": "lfo", "id": "vibrato", "waveform": "sine", "rate_hz": 5.0, "phase": 0.0 },
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

各Routeの`depth.value`はSigned値、`depth.unit`はTargetのModulation Unitです。Linear Targetは`curved_source × depth.value`をNative Domainへ加算し、Log2 TargetはOctave Domainで加算して`base × 2^sum`へ変換します。RouteはDefinition順に加算し、最後にTarget範囲へClampします。Parameter IDの解決とRouteの計算はコンパイル前に完了するため、音声処理中に文字列IDやJSONを扱いません。

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

**ErrorとWarning**

- Errorが1つでもあれば、コンパイル結果を返しません
- Warningだけなら、Warning付きのコンパイル結果を返して処理を続けます
- Zone AssetのSHA-256省略はWarningです（Assetを読み込めたZoneは有効のまま）
- Assetの欠落・Hash不一致・Decode失敗のあるSample Zoneは無効にしてWarningを残し、他の有効なZoneやLayerがあれば処理を続けます
- Parameter ID、Source ID、Source設定、Route Target、Depth Unit / 範囲のErrorはコンパイル前にまとめて返します

検証エラーには`layers[0].envelope.attack_seconds`のようなField Pathが付きます。
