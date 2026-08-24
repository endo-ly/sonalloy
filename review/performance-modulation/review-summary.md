# Performance and Modulation Sound Review

## Render条件

- Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- Output：Stereo、32-bit float WAV
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 128 / 257 frames
- Tail：0.5秒
- Analysis / Trace：製品CLIの`--analyze --json` / `--trace`出力

## 自動検証

| 確認項目 | 結果 |
|---|---|
| Instrument Definition Validate | 6件すべてok |
| Pattern Validate | Tempo Step Bass、Vector Hybridの2件すべてok |
| Instrument / Pattern Inspect | すべてok |
| 全WAVのFinite性 | 7 Reference × 3 Sample Rateすべてtrue |
| 非無音・Full Scale内 | 7 Reference × 3 Sample Rateすべてok |
| Stereo出力 | 7 Referenceすべて2 Channel |
| Block Size差分 | `metrics.json`へ実測値を保存 |

Block Size差分の最大値は次のとおりです。値は基準Block Size 257 framesとの差分で、Renderの決定性を確認するための測定値です。

| Reference | 最大絶対差分 |
|---|---:|
| Mono Portamento Lead | 0.00208536 |
| MSEG Motion Pad | 0.0000000894 |
| Random Comparison | 0.000000715 |
| Macro Hybrid | 0.000063546 |
| Vector Hybrid | 0.000392884 |
| Tempo Step Bass | 0.0000000745 |
| Vector Hybrid Pattern | 0.0000394583 |

Step / Sample & Holdの離散境界はBlock Sizeに依存しません。Tempo Step BassはStepに加えてBeat Phase / Bar PhaseをFilterへRouteし、Tempo / Meter変更を含む条件で比較しています。Portamentoと連続的な音高・Source変化を含む音色では、既存のBlock内Span補間による差分を記録しています。Sample Rate比較では各Referenceの時間長、Finite性、非無音、Full Scale内を確認しています。

## 音声一覧

| WAV | 確認内容 |
|---|---|
| `audio/mono-portamento-lead.wav` | Last-note、Legato、Portamento、Pitch Bend、Sustain |
| `audio/mseg-motion-pad.wav` | MSEGのLoop、Loop途中のNote Off、Release Segment、Filter変化 |
| `audio/tempo-step-bass.wav` | Per-beat Step、Beat Phase、Bar Phase、Tempo変更、Meter変更 |
| `audio/random-comparison.wav` | Sample & HoldとSmooth Randomの決定性・連続性 |
| `audio/macro-hybrid.wav` | MacroによるFilter、Layer Gain、Reverb Mixの同時変化 |
| `audio/vector-hybrid.wav` | 4-Way Vector、Axis変化、4 LayerのConstant-power Weight |
| `audio/vector-hybrid-pattern.wav` | Pattern経由のMacro / Vector Axis変更、Tempo / Meter変更 |
| Pattern `vector-hybrid.json` | Pattern上のMacro Parameter Change、Vector Axis Parameter Change、Tempo / Meter変更 |

## 人間の試聴

| Reference | 試聴 | 判定 |
|---|---|---|
| Mono Portamento Lead | 未実施 | 未測定 |
| MSEG Motion Pad | 未実施 | 未測定 |
| Tempo Step Bass | 未実施 | 未測定 |
| Random Comparison | 未実施 | 未測定 |
| Macro Hybrid | 未実施 | 未測定 |
| Vector Hybrid | 未実施 | 未測定 |
| Realtime Mono Play | Audio / MIDI Device未接続 | 未測定 |

この環境では物理Audio / MIDI Deviceを利用できないため、Click、音色の自然さ、演奏応答、Realtime安定性の承認結果は記録していません。
