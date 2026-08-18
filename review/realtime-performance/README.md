# Realtime Performance Review

Realtime AdapterのDevice依存Reviewを記録するPackageです。音源Definition、MIDI、WAVは複製せず、既存のReferenceと`sonalloy device list` / `sonalloy play`を使用します。

## 手順

1. Windows / LinuxのRelease Buildで`sonalloy device list`を実行し、Audio OutputとMIDI Inputを確認する
2. `sonalloy device list --json`でOpaque ID、Default Config、Sample Format、Buffer範囲のReportを確認する
3. 256 FrameでNote、同音連打、Chord、Pitch Bend、Mod Wheel、Channel Aftertouch、Sustainを演奏する
4. 128 Frameで同じ入力を追加確認する
5. 各OSで10分以上連続演奏し、Xrun、Fatal Fault、Stuck Note、Queue Overflow、Memory増加を記録する
6. `metrics.json`へ機械的な測定値を、`review-summary.md`へ入力応答と音質の判断を記録する

Opaque IDはMachine-specificな値のため、公開Review Artifactへ残さない。Device Name、Backend、Sample Rate、Buffer、Latency、Xrun、Fault状態だけを記録する。
