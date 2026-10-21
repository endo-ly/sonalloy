# Definition仕様（音源定義の構造と制約）

音源定義（JSONファイル）のトップレベル構造、Performance、Layer、Macro / Vector、External Audio、コンパイル時の変換をまとめます。

## 全体構造

音源定義は、次のトップレベルFieldを持ちます。

| Field | 内容 |
|---|---|
| `schema_version` | スキーマ版。現在は`6`。それ以外はUnsupportedとして拒否 |
| `external_audio` | 外部Audio入力のChannel構成（省略可）。使用する場合はMonoまたはStereoを指定 |
| `metadata` | `name`、`author`、`description`、`category`、`tags`、`recommended_range` |
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
  "schema_version": 6,
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

## Metadata

`metadata`は音源の名前と説明に加えて、Libraryで検索・分類・試聴するための情報を持ちます。`category`、`tags`、`recommended_range`は一般のDefinitionでは省略できます。Built-in Presetでは3項目をすべて設定します。

| Field | 内容と制約 |
|---|---|
| `name` | 音源名。空文字は指定できません |
| `author` | 作者名（省略可） |
| `description` | 音源の説明（省略可） |
| `category` | LibraryのPrimary Category。指定時は前後に空白を持たない1〜64文字で、制御文字を含めません |
| `tags` | 検索用Tag。0〜12件。各Tagは前後に空白を持たない1〜32文字で、制御文字を含めず、ASCIIの大文字・小文字を区別した重複を持ちません |
| `recommended_range` | 実用的な音域。`min_midi` / `max_midi`を0〜127で指定し、最小値を最大値以下にします |

短いブラウザ試聴の演奏は、プリセットと同じディレクトリの `preview.score.yaml` に記述します。音源メタデータは分類と音域を持ち、音符や演奏時間は[Score YAML](score_spec.md)から取得します。CLIはメタデータを使ってBuilt-in Presetを一覧・検索します。

## Performance

`performance`はTagged Objectです。`mode`を省略したり、別ModeのFieldを混ぜたりできません。

### Polyphonic

同時に保持するVoice数を`polyphony`（1〜64）で指定し、上限到達時は`voice_stealing`で既存Voiceを選びます。

```json
"performance": {
  "mode": "polyphonic",
  "polyphony": 16,
  "voice_stealing": "quietest_releasing_then_oldest",
  "choke_groups": [{ "keys": [42, 44, 46] }]
}
```

`choke_groups`は省略可能で、互いに音を止め合うKeyの集合を並べます。Groupに属するKeyのNote Onは、同じGroupのKeyで鳴っているNoteを5msでフェードして止めます。同じKeyの再発音も前の音を止めます。オープンハイハットをクローズドで止めるような、同じ楽器の奏法どうしの関係に使います。各Groupは1個以上のKey（0〜127）を持ち、1つのKeyは1つのGroupにだけ属せます。

### Monophonic

常に1 Voiceを使い、Held NoteはLast-note priorityで切り替えます。`legato: true`では接続したNote OnでEnvelopeとGeneratorを再Triggerせず、`portamento`があれば音程だけを指定秒数で滑らかに移動します。`legato: false`ではNote Onごとに再Triggerします。

```json
"performance": {
  "mode": "monophonic",
  "legato": true,
  "portamento": { "time_seconds": 0.08 }
}
```

`portamento.time_seconds`は0より大きく10秒以下です。Monophonicでは`polyphony`、`voice_stealing`、`choke_groups`を指定しません。

## Layer

Layerは「Generator + Layer Processor + ADSR + Gain + Pan」のセットで、Trigger条件に合ったLayerだけが鳴ります。`layers`は書かれた順に同じVoiceへMixし、`enabled: false`のLayerはコンパイル対象外です。

| Field | Range | 内容 |
|---|---|---|
| `id` | — | Layer識別子。一意 |
| `enabled` | Boolean | 発音の有無 |
| `trigger` | 下記Trigger表 | 発音条件 |
| `choke_group` | 非空文字列（省略可） | 同じGroupの古いLayerを、新しい発音時に短く消音する。`note_on` Trigger専用 |
| `gain_db` | -60〜12 dB | Layer音量 |
| `pan` | -1〜1 | 定位。定電力で配置する |
| `tuning_cents` | 有限値 | 音程（Cent。100 = 半音）。有限で正のPitch比率を計算できる値 |
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

### Velocityの作用

Note OnのVelocityは、Layerの発音条件、Generator固有の制御、Modulation Sourceの3通りで利用できます。

1. `trigger.velocity_min`〜`velocity_max`はLayerの発音範囲です。
2. GeneratorごとのVelocity制御は各仕様に従います。Sample Generatorは各ZoneのVelocity範囲で再生Zoneを選びます。
3. Modulation Sourceの`velocity`は、Note Onの値を`0..1`へ正規化したVoice単位の値です。音量を調整する場合はGain ParameterへRouteします。RouteのID、Unit、計算規則は[Modulation仕様](modulation.md)に従います。全体構造の例では、`layer.body.gain`へのRouteがVelocityに応じて最大8 dBを加算します。

Layerの`gain_db`はLayerの基準音量です。Layer構造の例は「全体構造」の`layers[0]`を参照してください。

Open HatとClosed Hatのような排他発音では、別々のNote範囲を持つLayerに同じ`choke_group: "hats"`を指定します。実際に発音するLayerだけがChokeを起こし、同Groupの古いLayerはADSRのRelease設定によらず5msで停止します。Voice内の他GroupのLayerとGlobal Processorの残響は継続します。`performance.choke_groups`も宣言した場合は、Keyに応じたNote全体の停止も適用されます。

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

外部Audioは、音源定義の`external_audio`で入力Channel数を固定し、外部Audio Consumerへ共有する入力Busです。Mono入力は左右へ同じ値を渡し、Stereo入力は左右を独立して扱います。入力Busを使う定義は、Compile時と実行時に同じChannel数を要求します。

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

外部Audio Busは、Instrument単位のEnvelope FollowerまたはGlobal Processorから利用できます。Gate / Compressorの外部DetectorとCross Synthesis ProcessorはGlobal Chainに限ります。Processorの共通設定は[Processor仕様](processors.md)、Envelope FollowerのSourceは[Modulation仕様](modulation.md)を参照してください。

| Consumer | 入力の用途 |
|---|---|
| Envelope Follower | 左右をリンクした入力振幅をModulation Sourceとして使う |
| External Sidechain | 入力をGate / Compressorの検出信号として使う |
| Vocoder | 左右別の帯域EnvelopeをCarrierへ適用する |
| Envelope Transfer | 左右をリンクした入力振幅で信号のGainを制御する |
| Spectral Morph | 左右別のSpectrumをCarrierのMagnitude Morphに使う |

- 入力を要求するConsumerがある場合は`external_audio`が必要です。Consumerを持たない入力Busは無効です
- VocoderとSpectral Morphはそれぞれ1つまでです。Cross Synthesis ProcessorはVoice / Layer Chainへ置けません

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

Sample、Wavetable、Spectral、Granular、Wave SequenceとConvolutionは、Definitionから参照したWAVをCompile時に読み込みます。Assetとして使うWAVはPCM 16-bit / 24-bitまたはFloat 32-bitで、MonoまたはStereoにします。相対PathはDefinitionファイルのDirectoryを基準とし、絶対Pathも使えます。`sha256`は任意で、指定した場合は元ファイルのSHA-256を照合します。同じAssetを複数のZoneやGeneratorが参照する場合、準備済みの音声データを共有します。

Windowsでは、読み込み可能なAssetを絶対Pathへ解決してからHash照合とDecodeを行います。長いPath、UNC Path、拡張形式のPathも同じAsset読み込み経路で扱い、InspectではDefinitionに記載したPathを表示します。

有効なLayerまたはProcessorが参照するAssetの欠落・Hash不一致・Decodeや事前解析の失敗はCompile Errorです。`enabled: false`のLayerはAsset準備と発音の対象外です。Sample RateとChannelの変換はGeneratorごとに異なります。

| Asset Consumer | Sample Rate | Channel |
|---|---|---|
| Sample / Granular / Wave Sequence | 処理Sample RateへResampleする | Mono / Stereoを保つ。GranularはMonoでもStereo出力 |
| Wavetable | Compile時にResampleしない | MonoへDownmixする |
| Spectral | 処理Sample Rateへ変換してSTFT解析し、Rate依存のFrameを作る | A/B AssetのChannel数を一致させる |
| Convolution Processor | 処理Sample Rateへ準備する | Mono / Stereo IRを使う |

**ErrorとWarning**

- Errorが1つでもあれば、コンパイル結果を返しません
- Warningだけなら、Warning付きのコンパイル結果を返して処理を続けます
- Parameter ID、Source ID、Source設定、Route Target、Depth Unit / 範囲のErrorはコンパイル前にまとめて返します
