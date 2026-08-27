# Extended Processing Review

Schema 4のProcessor Chainと、固定Latency・Tempo Sync・Asset準備を確認するReview Packageです。Definitionは`definitions/`、演奏Eventは`events/`、Musical Time Patternは`patterns/`、決定的なIRは`assets/`に置きます。

## 生成

```bash
python3 review/extended-processing/scripts/generate_package.py
```

IRだけを再生成する場合は次を実行します。

```bash
python3 review/extended-processing/scripts/generate_ir.py
```

生成Scriptは製品CLIで全DefinitionをValidate / Inspectし、48,000 Hz・Block Size 257・4秒のbounded tailを含むWAVとAnalysis JSONを作成します。Full HybridはParameter Change EventとTraceを生成し、Frequency Shift Bellは`-420 → 0 → +420 Hz`のイベントで符号付き移動を確認します。44,100 / 48,000 / 96,000 Hz、Block Size 32 / 64 / 128 / 257での比較は、同じDefinitionを使って再生成できます。

## Fixture

| Fixture | 主な確認対象 |
|---|---|
| `ladder-acid-bass` | Ladder Filter、Drive、MacroからのCutoff modulation |
| `formant-filter-sweep` | Formant Processor、Profile interpolation、Vowel position |
| `frequency-shift-bell` | Frequency Shifter、negative / positive shift、127-frame latency |
| `convolution-body` | Mono IR、FFT partition、256-frame latency、tail |
| `gate-dynamics` | Stereo-linked Gate、hysteresis、hold、release |
| `transient-drum` | Attack / Sustain shaping |
| `tempo-ping-pong-delay` | Beats、Process Tempo、Ping-Pong feedback |
| `multi-tap-delay` | Feed-forward taps、gain normalization |
| `full-extended-processing-hybrid` | Layer / Voice / Globalの同時使用、Latency、Trace、Reset |

`SUMMARY.md`は自動検証の範囲と試聴確認の記録です。数値が有限でも音質の合否を代替しないため、試聴結果は実際の再生後に記録します。
