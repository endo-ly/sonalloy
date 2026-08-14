# Processor Chain Review

## Inputs

- ProcessorなしのBaseline
- Layer Filter / Drive
- Voice Filter / Drive
- Global Filter / Drive
- Global Delay / Reverb
- Processed Hybrid（Sample Attack、Saw Body、Layer / Voice / Global Processor）
- Processor Parameter Change、Global Mod Wheel、Voice Stealing、Reset、Block Size、Sample Rate

## Human listening items

- `02-layer-filter.wav`: Attack LayerだけへのCutoff / Resonanceの作用
- `03-layer-drive.wav`: Body LayerだけへのAmount / Mix、低Amountの自然さ、高AmountのAliasing
- `04-voice-filter.wav` / `05-voice-drive.wav`: Layer Mix全体への作用とParameter ChangeのClick
- `06-global-filter.wav` / `07-global-drive.wav`: Voice Sum後の一回だけの処理とLevel Balance
- `08-delay-impulse.wav`: Echo間隔、Feedback減衰、左右独立、Dry / Wet、Tail
- `09-reverb-impulse.wav`: 初期反射、金属的なRing、Tail、Damping、Width、Mix
- `10-processed-hybrid.wav`: AttackとBodyの一体感、Global Effectの量、楽曲での実用性
- `13-voice-stealing.wav`: Steal Fade、Tail、Note間のState分離
- `processed-hybrid-block-*.wav`: Block SizeによるClickや時間軸の差
- `processed-hybrid-sample-rate-*.wav`: Sample Rateごとの音色と安定性

## Automated checks

- すべてのWAVがStereoでFiniteである
- Block Size 32 / 64 / 257 / 1024の出力差が閾値以内である
- Sample Rate 44.1 / 48 / 96 kHzの出力がFiniteである
- 同じ入力を二度Renderした出力が一致する
- `metrics.json`へ測定値を保存する

音質の判定はMetricsだけでは完了しない。人間の試聴結果を`review-summary.md`へ追記する。
