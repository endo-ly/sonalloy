# Processor Expansion Review

## Automated checks

- Filter 4 Mode、EQ、Resonator、Bitcrusher、Chorus、Flanger、Phaser、Compressor、Limiter、Full Chain 3のDefinitionを生成した。
- 生成したDefinitionをValidateし、Compile後のInspect JSONを保存した。
- すべての技術WAVについてFinite性、Peak、RMS、DC、Stereo情報を測定した。
- 44.1 / 48 / 96 kHz、Block Size 32 / 64 / 257 / 1024、Fresh RuntimeとReset後の出力を比較した。
- Release BuildのRender時間とRealtime比を`metrics.json`へ記録した。

## Human listening record

次の確認を同じ再生環境・音量で行い、結果を追記する。Metrics合格だけでは音質合格としない。

| 対象 | 確認内容 | 結果 |
|---|---|---|
| `filter_*.wav` | Low / High / Band / Notchの差、Resonanceの破綻 | 未確認 |
| `eq_*.wav` | Boost / Cutが帯域変化として自然に聞こえるか | 未確認 |
| `resonator_*.wav` | 220 / 440 HzのPitch、Decay、Dampingの使いやすさ | 未確認 |
| `bitcrusher_*.wav` | Bit DepthとSample-rate ReductionのDigital Texture | 未確認 |
| `chorus_*.wav` / `flanger_*.wav` | Stereo幅、揺れ、Sweep、Feedbackの濁り | 未確認 |
| `phaser_*.wav` | Sweepの滑らかさ、4 / 8段の差、Jet感 | 未確認 |
| `compressor_*.wav` / `limiter.wav` | Punch、Pumping、Peak抑制、歪み | 未確認 |
| `full_chain_digital_pad.wav` | Padの広がり、EQ、Chorus、Reverb、Compressorの一体感 | 未確認 |
| `full_chain_metallic_pluck.wav` | Operator + SampleのAttack、Resonator、Phaser、Delay、Limiter | 未確認 |
| `full_chain_lofi_texture.wav` | Granular + Noise、Bitcrusher、Flanger、Reverbの実用性 | 未確認 |
| `full_chain_digital_block_*.wav` | Block Size変更によるClickや時間軸の差 | 未確認 |
| `full_chain_digital_sample_rate_*.wav` | Sample Rate変更による音色と安定性 | 未確認 |

人間の試聴後、各行の結果と必要な修正内容を具体的に記録する。
