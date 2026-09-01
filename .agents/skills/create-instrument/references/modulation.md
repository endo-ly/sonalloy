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

- `depth.value`はSigned値、`depth.unit`はTargetのModulation Unitです（Linear TargetはNative Unit、Log2 TargetはOctaves）。TargetごとのUnitは`instrument inspect --json`のParameter一覧で確認できます
- `curve`は`linear`または`smooth_step`です
- `curved_source × depth.value`をNative Domainへ加算し、Log2 TargetはOctave Domainで加算して`base × 2^sum`へ変換します
- RouteはDefinition順に加算し、最後にTarget範囲へClampします
- Parameter IDの解決とRouteの計算はコンパイル前に完了するため、音声処理中に文字列IDやJSONを扱いません

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
