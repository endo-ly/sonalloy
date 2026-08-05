# Essential Synthesis and Sampling Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample AssetのSource Sample Rate：44,100 Hz
- 比較Sample Rate：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Asset：Review Scriptが生成したMono PCM16 Synthetic WAV

`audio/technical/`の生出力をMetricsと人間の試聴で共用します。試聴専用の正規化コピーは保存していません。

## 自動検査

- 全WAVがFinite：pass
- Float WAV範囲内：pass
- Block Size再現：pass
- 同一入力の再Render再現：pass
- Key Zone切替：pass
- Velocity Layer差：pass
- Round Robin順序：pass
- Round Robin音源差：pass
- Forward Loop継続：pass
- Explicit Slice範囲：pass
- Asset Cache共有：pass
- Voice Stealing Pending：pass
- Essential Hybrid：pass

`metrics.json`にはFinite性、Peak、RMS、DC、推定周波数、隣接Frame差分、Sample Rate別値、Block Size比較、再RenderSHA、Round Robin選択順、Loop周期、Slice Region長、Asset Cacheの共有数を保存しています。

## 音声一覧

| WAV | 確認内容 |
|---|---|
| `23-key-zone-scale.wav` | Low / Mid / High Key Zoneの境界とPitch Mapping |
| `24-velocity-layer-soft-hard.wav` | Soft / Hard Velocity Layerの差 |
| `25-round-robin-repeated-hit.wav` | `hit_a → hit_b → hit_a → hit_b`の決定的選択 |
| `26-forward-loop-hold.wav` | Note保持中のForward Loop周期と境界 |
| `27-forward-loop-release.wav` | Note Off後のLoop継続とRelease |
| `28-explicit-slice-sequence.wav` | 同一Assetの3つのOne-shot Region |
| `29-multi-sample-melody.wav` | 複数Key ZoneによるMelody |
| `30-full-mapped-sample-instrument.wav` | Key / Velocity / Round Robinを組み合わせたReference |
| `31-essential-hybrid-instrument.wav` | Sample、Oscillator、Processor ChainのHybrid |
| `32-regression-block-*.wav` | Block Size比較 |
| `33-sample-rate-*.wav` | Sample Rate比較 |
| `34-reset-*.wav` | 同一入力の再現性 |
| `35-voice-stealing-pending-zone.wav` | Pending NoteのZone選択保持 |

## 人間の確認欄

| 確認項目 | 判定 | コメント |
|---|---|---|
| Key境界で意図したZoneへ切り替わる |  |  |
| Velocity Layerの音量・音色差が明確 |  |  |
| Round Robin順が聞き取れ、順番が崩れない |  |  |
| Pitch Mappingが自然 |  |  |
| Forward LoopにClickがなく周期が安定 |  |  |
| Release中のLoopが自然 |  |  |
| Sliceが指定Region外を再生しない |  |  |
| Missing Asset時も別Zone・別Layerが継続する |  |  |
| Voice Stealing後の音源が破綻しない |  |  |
| Essential Hybridが音色として成立する |  |  |

## 再生成

```bash
python scripts/review/generate_essential_synthesis_sampling_package.py
```

同じDefinition、Event、Asset、Render条件からPackageを再生成できます。
