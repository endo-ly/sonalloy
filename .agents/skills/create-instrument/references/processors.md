# Processor仕様

本書は全ProcessorのField・Range・Dynamic Parameterと、種類ごとの補足をまとめます。

## 適用位置と種類

Processorは配列の順序で直列に適用されます。配置と種類は固定で、Processor間の接続先をDefinitionから指定することはできません。

| 配置 | 適用位置 | 使える種類 |
|---|---|---|
| Layer（`processors`） | Generatorの直後 | Filter、Ladder Filter、Formant、Drive、EQ、Resonator、Bitcrusher |
| Voice（`voice_processors`） | 全LayerのMix後 | Filter、Ladder Filter、Formant、Drive、EQ、Resonator、Gate、Transient Shaper、Compressor、Limiter（Gate / CompressorはSelf Signalのみ） |
| Global（`global_processors`） | 全Voiceの合計後 | Filter、Ladder Filter、Formant、Drive、EQ、Chorus、Flanger、Phaser、Frequency Shifter、Delay、Reverb、Convolution、Gate、Transient Shaper、Compressor、Limiter、Vocoder、Envelope Transfer、Spectral Morph |

LayerはGeneratorの出力がMonoでもStereoでも同じChainを使い、出力Channel数に応じたStateをCompile時に確保します。VoiceとGlobalのDynamicsは左右のPeakをリンクして処理します。Chorus、Flanger、PhaserはGlobal Chainに1つのStateを持ち、Voiceごとには複製しません。

## FieldとRange

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
| Gate（Voice / Global） | `threshold_db`: -80〜0 dB、`hysteresis_db`: 0〜12 dB、`attack_ms`: 0.1〜100 ms、`hold_ms`: 0〜500 ms、`release_ms`: 5〜2000 ms、`range_db`: -96〜0 dB（0 dBではGate閉時もUnity） | `threshold_db`、`range_db` | `detector`、`hysteresis_db`、各Time Field。Globalの`external_audio`は入力整列後に検出 |
| Transient Shaper（Voice / Global） | `attack` / `sustain`: -1〜1、`mix`: 0〜1 | `attack`、`sustain`、`mix` | Fast / Slow EnvelopeのTime Constant |
| Compressor（Voice / Global） | `threshold_db`: -60〜0 dB、`ratio`: 1〜20、`attack_ms`: 0.1〜200、`release_ms`: 5〜2000、`knee_db`: 0〜24、`makeup_gain_db`: -12〜24 dB、`mix`: 0〜1 | `threshold_db`、`ratio`、`makeup_gain_db`、`mix` | `detector`、`attack_ms`、`release_ms`、`knee_db`。Globalの`external_audio`は入力整列後に検出 |
| Vocoder（Globalのみ） | `attack_ms`: 0.1〜100 ms、`release_ms`: 5〜1000 ms、`modulator_gain_db` / `output_gain_db`: -24〜24 dB、`mix`: 0〜1 | `modulator_gain_db`、`output_gain_db`、`mix` | 24帯域、0 frames |
| Envelope Transfer（Globalのみ） | `attack_ms`: 0.1〜200 ms、`release_ms`: 1〜2000 ms、`input_gain_db`: -24〜24 dB、`floor_db`: -96〜0 dB、`mix`: 0〜1 | `input_gain_db`、`floor_db`、`mix` | 0 frames |
| Spectral Morph（Globalのみ） | `morph`: 0〜1、`output_gain_db`: -24〜24 dB | `morph`、`output_gain_db` | FFT 1024 / Hop 256、1024 frames |
| Limiter | `ceiling_db`: -12〜0 dBFS、`release_ms`: 5〜1000、`input_gain_db`: -24〜24 dB | `ceiling_db`、`input_gain_db` | `release_ms` |

`Dynamic Parameter`列はModulation RouteやParameter Changeで動かせるField、`Static Field`列はCompile時に確定するFieldです。Filterの`cutoff_hz`だけ、既存のCanonical IDとして`cutoff`をCatalog IDに用います。Processorの種類・ID・配置・順序やStatic Fieldを変えた場合は、再Compileが必要です。

## 種類ごとの補足

**Formant** — Profileは`id`と周波数昇順の5 Bandを持ち、Band FieldはGeneratorのFormantと共通です（`frequency_hz`: 100〜12000 Hz、`bandwidth_hz`: 20〜5000 Hz、`gain_db`: -60〜12 dB）。

**Delay** — `time`は`{"value": ..., "unit": "seconds" | "beats"}`で、`beats`は1 beatを4分音符1つとして現在のProcess Tempoから秒へ変換します。`feedback_mode`は`stereo`または`ping_pong`。Tapは`{"time": <timeと同じ形式>, "gain_db": ...}`を最大8個まで持ち、Wet出力だけへ加算されます。

**Convolution** — `ir`はAsset参照（`{"path": ..., "sha256": ...}`）で、Mono / StereoのWAVを最大10秒・2個まで使えます。

**Gate / Compressor** — `detector`は`"self_signal"`（通過信号を検出）または`"external_audio"`（外部Sidechain。Global Chainのみ、`external_audio`の宣言が必要）です。

## 固定Latency

Frequency ShifterとConvolution、Spectral Morphは、それぞれ127 framesと256 frames、1024 framesの固定Latencyを持ちます。固定Latencyの合計は`inspect`の`reported_latency_frames`へ反映され、Render時の前置きLatencyとしてCLIが補正します。

FilterのCutoffが処理できる上限（20 kHzとSample Rateから決まる値の小さい方）を超える定義は、Warningを出して上限へ制限します。
