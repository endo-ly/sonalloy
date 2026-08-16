# Processor Expansion Review

Processor ExpansionのDefinition、Inspect、技術WAV、Metricsを同じ条件で再生成するPackageです。

## 生成

```bash
python3 review/generate/generate_processor_expansion.py
```

`definitions/`にはProcessorごとの比較用Definition、`events/`にはParameter Changeを含むEvent Sequence、`audio/technical/`には未正規化の技術確認用WAV、`inspect/`にはCompile後のJSONを保存します。

`metrics.json`はFinite性、Peak / RMS / DC、Stereo情報、Filter Mode差、Block Size、Sample Rate、Reset再現性、Release RenderのRealtime比を記録します。音質の判定は`review-summary.md`へ人間が記入します。
