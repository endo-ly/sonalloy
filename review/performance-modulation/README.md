# Performance and Modulation Review

Performance Mode、演奏中の音高遷移、Voice Source、Macro、Vector、Transport Phase、Musical Timeを同じCore Runtimeで確認するReview Packageです。

## 再生成

```bash
python3 review/generate/generate_performance_modulation_package.py
```

基準Renderは48,000 Hz、Stereo、32-bit float WAV、Block Size 257 frames、Tail 0.5秒です。Block Size 32 / 64 / 128 / 257 framesを同じ入力でRenderし、WAV差分を`metrics.json`へ記録します。各Referenceは44,100 / 48,000 / 96,000 HzでもRenderし、時間長、Finite性、非無音、Peakを記録します。

## 構成

| Directory / File | 内容 |
|---|---|
| `definitions/` | Mono Portamento、MSEG、Step、Random、Macro、Vector、Transport PhaseのInstrument Definition |
| `events/` | Offline Event列。MonoのSustain、MSEGのLoop途中Note Offを含む |
| `patterns/` | Tempo / Meter変更とMacro / Vector Axis変更を含むPattern |
| `validation/` | InstrumentとPatternの製品CLI Validate結果 |
| `inspect/` | InstrumentとPatternの製品CLI Inspect結果 |
| `trace/` | 製品CLIのTrace結果 |
| `audio/` | 試聴用WAV |
| `analysis/` | 製品CLIのAudio Analysis結果 |
| `metrics.json` | Sample Rate、Finite性、レベル、連続性、Block Size差分 |
| `review-summary.md` | 自動検証と人間による試聴結果 |

`analysis/`と`trace/`は製品CLIのJSON出力を保存したものです。Review用Scriptはそれらを再計算せず、入力の再現とWAV間の比較だけを行います。
