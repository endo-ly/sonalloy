# Modulation仕様

本書はModulationのSourceの種類・Polarity・Routeの計算規則をまとめます。

## 構造とScope

`modulation`は省略可能です。`sources`はVoiceごとのSource定義、`routes`はSourceからDynamic Parameterへの接続です。Routeは書かれた順に同じTargetへ加算され、最後にTarget範囲へClampされます。

Scopeの分担：MacroとTransport Phase、Envelope FollowerはInstrument単位、LFOやEnvelopeなどの定義SourceはVoice単位です。

## 組み込みSource

定義なしで`routes`から参照できます。

| Source ID | 範囲 | Polarity | 動作 |
|---|---:|---|---|
| `velocity` | 0〜1 | Unipolar | Note OnのVelocity |
| `key_tracking` | -1〜1 | Bipolar | MIDI Note 0を-1、127を+1へ変換 |
| `pitch_bend` | -1〜1 | Bipolar | 共有External Control |
| `mod_wheel` | 0〜1 | Unipolar | 共有External Control |
| `aftertouch` | 0〜1 | Unipolar | 共有External Control |
| `transport_beat_phase` | 0〜1 | Unipolar | `beat_position`の小数部 |
| `transport_bar_phase` | 0〜1 | Unipolar | `bar_position`の小数部 |

## 追加できるSource

`sources`へ定義して使います。

| `type` | Field | 動作 |
|---|---|---|
| `lfo` | `waveform`（`sine` / `triangle`）、`rate`（`per_second`または`per_beat`）、`phase`（0以上1未満） | Bipolarの周期信号 |
| `envelope` | ADSR（各時間の範囲はLayer ADSRと同じ） | Note Lifecycleに追従 |
| `random` | `seed` | SeedとNote IDから決まる、Voiceごとの固定値 |
| `mseg` | `initial_value`、`segments`、`loop_range` | Segmentを順に進むBipolarのMotion |
| `step` | `values`、`rate` | 値を保持するBipolarのStep列 |
| `sample_hold` | `seed`、`rate` | Rateごとに更新する決定的Bipolar値 |
| `smooth_random` | `seed`、`rate` | 決定的Bipolar値をRateに合わせて補間 |
| `envelope_follower` | `attack_ms`、`release_ms`、`input_gain_db` | 外部Audioの左右リンク振幅を0〜1へ追従するInstrument Source（`external_audio`の宣言が必要） |

Polarityは、LFO、Random、MSEG、Step、Sample Hold、Smooth RandomがBipolar（-1〜1）、EnvelopeがUnipolar（0〜1）です。Depthの符号は方向を決め、Bipolar Sourceでは正負両方向へ作用します。

`rate`の範囲は`per_second`が0.01〜40、`per_beat`が1/64〜16（Quarter-note基準）です。`per_beat`はTempo変更後も拍基準の速度を保ちます。

## Routeの計算規則

各Routeは`source`、`target`、`depth`、`curve`を持ちます。

```json
{ "source": "filter_env", "target": "voice.processor.tone.cutoff", "depth": { "value": 2.0, "unit": "octaves" }, "curve": "smooth_step" }
```

- `depth.value`はSigned値、`depth.unit`はTargetのModulation Unitです（Linear TargetはNative Unit、Log2 TargetはOctaves）。TargetごとのUnitは次節の表のとおりで、実効範囲（Clamp後の値域）は`instrument inspect --json`のParameter一覧で確認できます
- `curve`は`linear`または`smooth_step`です
- `curved_source × depth.value`をNative Domainへ加算し、Log2 TargetはOctave Domainで加算して`base × 2^sum`へ変換します
- RouteはDefinition順に加算し、最後にTarget範囲へClampします
- Parameter IDの解決とRouteの計算はコンパイル前に完了するため、音声処理中に文字列IDやJSONを扱いません

## TargetのModulation Unit

RouteのTargetに指定できるDynamic Parameterと、`depth.unit`に書くUnitの対応です。同じ名前のParameterでもGeneratorとProcessorでUnitが異なることがあるため、Target IDごとに確認します。表にないField（Static Field）はModulation対象外です。

Layerの組み込みTarget:

| Target | Unit |
|---|---|
| `layer.<id>.gain` | `decibels` |
| `layer.<id>.pan` | `pan` |
| `layer.<id>.tuning` | `cents` |

GeneratorのParameter（Target IDは`layer.<id>.generator.<parameter>`。Operator Modulationだけ`operator.<1-4>.<parameter>`形式）:

| Unit | Parameter |
|---|---|
| `normalized` | `waveshape`、`phase_distortion`、`wavefold`、`oscillator_feedback`、`pulse_width`、`unison_spread`、`noise_correlation`、`additive_morph`、`additive_inharmonicity`、`modal_structure`、`modal_brightness`、`modal_decay`、`physical_string_brightness`、`physical_string_stiffness`、`formant_vowel_position`、`formant_throat`、`wavetable_position`、`spectral_position`、`spectral_freeze`、`spectral_morph`（`asset_b`指定時）、`granular_position`、`grain_randomness`、`grain_pan_spread`、`operator.<1-4>.level`、`operator.<1-4>.feedback` |
| `octaves` | `sync_ratio`、`physical_string_decay_seconds`、`grain_size`、`grain_density`、`operator.<1-4>.ratio` |
| `cents` | `unison_detune`、`formant_shift`、`grain_pitch`、`operator.<1-4>.detune` |
| `decibels_per_octave` | `additive_spectrum_tilt`、`formant_spectral_tilt` |
| `seconds` | `spectral_blur` |
| `hertz` | `spectral_shift` |
| `index` | `operator.<1-4>.modulation_amount` |

ProcessorのParameter（Target IDは配置ごとのPrefix + `<parameter>`）:

| Unit | Parameter |
|---|---|
| `normalized` | Filter / Ladder Filterの`resonance`、Ladder Filterの`drive`、Driveの`amount` / `mix`、Formantの`vowel_position` / `throat` / `mix`、Resonatorの`damping` / `mix`、Bitcrusherの`mix`、Chorus / Flanger / Phaserの`depth` / `feedback` / `width` / `mix`、Reverbの`decay` / `damping` / `width` / `mix`、Delayの`feedback` / `mix`、Transient Shaper / Compressorの`mix`、Vocoderの`mix`、Envelope Transferの`mix` |
| `octaves` | Filter / Ladder Filterの`cutoff`、Resonatorの`frequency_hz`、Compressorの`ratio`、Bitcrusherの`sample_rate_ratio` |
| `decibels` | EQの`low_gain_db` / `mid_gain_db` / `high_gain_db`、Gateの`threshold_db` / `range_db`、Compressorの`threshold_db` / `makeup_gain_db`、Limiterの`ceiling_db` / `input_gain_db`、Convolutionの`gain_db`、Vocoderの`modulator_gain_db` / `output_gain_db`、Envelope Transferの`input_gain_db` / `floor_db`、Spectral Morphの`output_gain_db` |
| `per_second` | Chorus / Flanger / Phaserの`rate_hz` |
| `hertz` | Frequency Shifterの`shift_hz` |
| `seconds` | Resonatorの`decay_seconds` |
| `index` | Bitcrusherの`bit_depth`、Transient Shaperの`attack` / `sustain` |

## MSEG

MSEGはSegmentを順に進むBipolar Sourceです。Segmentは1〜64個、Loopの終端はExclusive Indexです。ReleaseではLoopを抜けて終端へ進みます。Segmentの変化Frameは処理境界になります。

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

Segmentの`duration`は`seconds`または`beats`、`target`は-1〜1、`curve`は`linear`または`smooth_step`です。
