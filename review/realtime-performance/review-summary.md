# Realtime Performance Review

## 対象

WindowsとLinuxのRelease Buildで、既存Referenceを`sonalloy play`へ渡し、Audio出力、MIDI入力、Latency、Buffer、長時間安定性を確認する。Offline RenderのWAV品質は既存Packageで確認し、ここではRealtime経路だけを扱う。

## 記録

`metrics.json`へPlatformごとのDevice条件、Sample Rate、要求 / 実Callback Frame、Engine Latency、演奏時間、Xrun、Queue Overflow、Fatal状態を記録する。入力応答と音質の人間の判断はこの文書へ追記する。

| Platform | Device / Buffer | 入力応答 | 長時間安定性 | 判定 |
|---|---|---|---|---|
| Windows | 未測定 | 未測定 | 未測定 | 未測定 |
| Linux | 未測定 | 未測定 | 未測定 | 未測定 |

## 確認範囲

Note On / Note Off、Velocity 0、同音重複、Chord、Pitch Bend、Mod Wheel、Channel Aftertouch、Sustain Down / Up、Voice Stealing、Global Tail、重いGenerator、256 / 128 Frame、10分以上の連続演奏を確認する。

この実行環境では物理Audio / MIDI Deviceを利用できないため、Device依存の結果は推測せず未測定としている。Device非依存のAdapter、Core、Offline経路の自動検証結果は通常のCI / Test記録で確認する。
