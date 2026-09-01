# Definition仕様（音源定義の構造と制約）

音源定義（JSONファイル）のトップレベル構造、Performance、Layer、Macro / Vector、External Audio、コンパイル時の変換をまとめます。

## 全体構造

音源定義は、次のトップレベルFieldを持ちます。

| Field | 内容 |
|---|---|
| `schema_version` | スキーマ版。現在は`5`。それ以外はUnsupportedとして拒否 |
| `external_audio` | 外部Audio入力のChannel構成（省略可）。使用する場合はMonoまたはStereoを指定 |
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
  "schema_version": 5,
  "metadata": { "name": "Basic Poly Synth", "author": null, "description": "..." },
  "external_audio": null,
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

## Layer

Layerは「Generator + Layer Processor + ADSR + Gain + Pan」のセットで、Trigger条件に合ったLayerだけが鳴ります。`layers`は書かれた順に同じVoiceへMixし、`enabled: false`のLayerはコンパイル対象外です。

| Field | Range | 内容 |
|---|---|---|
| `id` | — | Layer識別子。一意 |
| `enabled` | Boolean | 発音の有無 |
| `trigger` | 下記Trigger表 | 発音条件 |
| `gain_db` | -60〜12 dB | Layer音量 |
| `pan` | -1〜1 | 定位。定電力で配置する |
| `tuning_cents` | -1200〜1200 | 音程（Cent。100 = 半音） |
| `envelope` | ADSR | 音量の輪郭。Attack / Decay / Sustain / Releaseの4区間 |
| `processors` | — | Generator後に直列適用するProcessor Chain。`processors.md`参照 |
| `generator` | — | 音源。`generators.md`参照 |

**Trigger**

| Field | Range | 内容 |
|---|---|---|
| `event` | `note_on` / `note_off` | `note_on`はNote Onで発音。`note_off`はNote Onで待機状態になり、対応するNote Offで発音する。Voice Stealingは演奏上のNote Offではないため、待機Layerを発音しない |
| `key_min` / `key_max` | 0〜127 | 発音するMIDI Note範囲 |
| `velocity_min` / `velocity_max` | 1〜127 | 発音するVelocity範囲 |

最小値は最大値以下にします。

Layerの全体例は「全体構造」の例にある`layers[0]`です。

## Macro

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

## Vector

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

| Type | Field | 内容 |
|---|---|---|
| `two_way` | `id` / `name` / `layer_a` / `layer_b` / `position` | `position` 0でLayer A、1でLayer B |
| `four_way` | `id` / `name` / `top_left` / `top_right` / `bottom_left` / `bottom_right` / `x` / `y` | X / Yの2軸で4 Layerを混ぜる |

2-WayのWeightは`A = cos(position × π/2)`、`B = sin(position × π/2)`です。4-WayはX / YそれぞれのSine / Cosineを組み合わせます。同じLayerを複数Vectorへ所属させることはできず、Vectorは最大8個です。

## External Audio

外部Audioは、音源定義の`external_audio`で入力Channel数を固定し、定義の外部Audio Consumerへ共有する入力Busです。Mono入力は左右へ同じ値を渡し、Stereo入力は左右を独立して扱います。入力Busを使う定義は、Compile時のProcess仕様にも同じChannel数を要求します。

```json
{
  "external_audio": { "channels": "stereo" },
  "global_processors": [
    {
      "type": "vocoder",
      "id": "voice",
      "attack_ms": 8.0,
      "release_ms": 80.0,
      "modulator_gain_db": 0.0,
      "output_gain_db": -3.0,
      "mix": 1.0
    }
  ]
}
```

外部AudioのConsumerはGlobal Processorへ置きます。Envelope FollowerはInstrument単位のModulation Sourceで、FilterなどのTargetへRouteできます。Gate / Compressorは`detector: "external_audio"`をGlobal Chainで指定すると外部Sidechainとして動作し、それ以外のDetectorは`"self_signal"`です。Vocoderは固定24帯域、Envelope Transferは外部振幅によるGain制御、Spectral Morphは外部SpectrumとのMagnitude Morphを行います。

| Consumer | 入力 | Dynamic Parameter | 固定Latency |
|---|---|---|---:|
| Envelope Follower | 外部Audioのリンクした振幅 | `attack_ms`、`release_ms`、`input_gain_db`は定義値 | 0 frames |
| External Sidechain | Global Gate / CompressorのDetector | `threshold_db`など既存Dynamics Parameter | 0 frames |
| Vocoder | 外部Audioの左右別24帯域Envelope | `modulator_gain_db`、`output_gain_db`、`mix` | 0 frames |
| Envelope Transfer | 外部Audioのリンクした振幅 | `input_gain_db`、`floor_db`、`mix` | 0 frames |
| Spectral Morph | 外部Audioの左右別Spectrum | `morph`、`output_gain_db` | 1024 frames |

- 入力を要求するConsumerがある場合は`external_audio`を省略できません
- 入力Busを宣言してもConsumerがない定義、複数のVocoderまたはSpectral Morph、Voice / Layerへ置いたCross Synthesis ProcessorはValidation Errorになります

## コンパイル時の変換

音声処理は音源定義を直接使わず、コンパイルして変換した値だけを使います。

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
