# Physical / Modal Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Backend：DaisySP V1.0.0 (`a0494a3adb67f549e18dfd71a35fa656f65b38b6`)

## 生成物

`definitions/`にTechnical Definitionと3つのMusical Definition、`validation/`に全DefinitionのCLI Validate JSON、`inspect/`に3つのMusical DefinitionのInspect JSON、`audio/technical/`と`audio/musical/`に同じ生出力を保存しています。`metrics.json`はCLIの`--analyze --json`を基礎にFinite性、Level、DC、Continuity、Spectrum、Block Size、Sample Rate、Fresh Runtime、SHA-256、Performanceを記録し、Stiffness 0のPhysical Stringは時間領域の自己相関でPitch Errorを20 cents以内へ検証します。FFTのNearest Bin値は分解能の参考値として併記します。

再生成：

```bash
python3 review/generate/generate_physical_modal_package.py
```

## Musical Definition

| Definition | 目的 | WAV |
|---|---|---|
| `physical_pluck` | StringのPitch、Natural Decay、Brightness、既存Processorとの組み合わせ | `audio/musical/physical_pluck.wav` |
| `modal_mallet` | Wood / Bar方向のAttack、Mode Density、Body Decay | `audio/musical/modal_mallet.wav` |
| `imaginary_metal_body` | Physical String + Modal + Processorによる架空の金属Body | `audio/musical/imaginary_metal_body.wav` |

Parameter Changeを含むHybridの出力は`audio/musical/imaginary_metal_body-parameter-sweep.wav`です。

## Technical Definition

String：Impulse、Noise BurstのSoft / Bright、Short / Long Decay、Loop Brightness、Low / High Stiffnessを含みます。Modal：4 / 8 / 12 / 16 / 20 / 24 Mode、Harmonic / Stretched Structure、Dark / Bright、Short / Long Decay、Impulse / Noise Burstを含みます。

## 人間の試聴欄

- [ ] StringのPitchがNote間で安定し、Stiffness 0でHarmonic寄りに聞こえる
- [ ] StringのShort / Long Decayが単なる音量差ではなくTailの長さとして聞こえる
- [ ] StringのLoop Brightnessで高域Lossが変化する
- [ ] StringのStiffnessを上げると高次成分が硬く、Metallic方向へ変化する
- [ ] Modalの4 / 12 / 24 Modeで共鳴密度の差が聞こえる
- [ ] ModalのStructureでMode配置のCharacterが変わる
- [ ] ModalのBrightnessで高次Modeの存在感が変わる
- [ ] ModalのDecayで共鳴Tailの長さが変わる
- [ ] `physical_pluck`が撥弦系の実用的な基準音色として成立する
- [ ] `modal_mallet`が木質・棒状のBody方向として成立する
- [ ] `imaginary_metal_body`が既存Processorと混ぜても破綻しない架空音色として成立する
- [ ] Block SizeやSample Rateを変えてClick・Timing差・大きな音色破綻がない

### 人間の回答

- 判定：
- 修正指示：
- 確認者：
- 確認日：
