# Testing and Sound Review

## テスト配置

- Moduleの内部契約は、実装Moduleと同じ`src/`内のUnit Testで検証します。
- CrateのPublic APIを利用する経路は、対象Crateの`tests/`にIntegration Testを置いて検証します。
- Workspace直下にはTestを置きません。
- 複数のTestで共有する期待値は、対象Crateに依存しない`testdata/expected/`へ置きます。

## テスト記述ルール

- 1つのTestでは、1つの振る舞いとその結果を検証します。
- Test名は、入力条件と期待する結果が分かる名前にします。
- 実装の内部構造ではなく、公開された結果、Error、状態遷移を検証します。
- 正常系、境界値、異常系を分け、入力と期待値をTest内で明示します。
- 時刻、乱数、外部Service、実行環境の音声Deviceには依存させません。
- 共有FixtureやBuilderは、Testの意図を隠さない範囲で使用します。
- Native境界の故障経路はTest用故障注入で検証し、通常のBuildへTest用経路を含めません。

## Native境界の検証ルール

- C++からRustへ例外を越境させず、Result Codeへ変換します。
- Native Error時は出力Bufferを無音化し、Rust側でError、Buffer長、所有権、Destroy、Resetを検証します。
- Process領域の前後にGuardを置くTestでは、領域外が変更されていないことを確認します。
- Native境界を含むTestは、Linux CIでAddressSanitizer、UndefinedBehaviorSanitizer、Leak Detectionの対象にします。

## 自動検証

- DefinitionのSchema Version、未知Field、必須Field、Range、Layer数、ID重複、NaN/Infinityを検証します。
- CompilerのdB→Linear、cent→Ratio、ADSR Frame変換、Filter Cutoff ClampとWarningを検証します。
- ADSRの0秒Segment、Attack中Note Off、Release、Voice Allocation、Note ID、Steal Fadeを検証します。
- EventのSample Offset、同一Offset順序、Block Size 64/257/1024のTiming一致を検証します。
- Sine/Saw、Stereo Mix、Pan、Velocity、Filter、Reset再現性、Finite性、Peak/RMS/DCを検証します。
- CLIの`instrument init/validate/inspect`、`render note`、`render midi`、Invalid Definition、MIDI Tempo、WAV出力を検証します。

## 音声Reviewのルール

- Metricsは手入力せず、`scripts/review/measure_wav.py`でWAVから生成します。
- 自動Testの期待値は`testdata/expected/sine_metrics.json`で管理します。
- Review Artifactには、音声、Metrics、Render条件、受入結果を保存します。
- 自動TestではWAV Metadata、Finite性、再現性、Metricsを確認し、音質の最終判断は人間が行います。

## Review Package

試聴資料は`review-output/basic-poly-synth/`へ保存します。

```text
review-output/basic-poly-synth/
├─ audio/
│  ├─ 01-sine-reference.wav
│  ├─ 02-saw-registers.wav
│  ├─ 02-saw-registers-filter-closed.wav
│  ├─ 03-attack-release.wav
│  ├─ 03-attack-release-slow-attack.wav
│  ├─ 04-repeated-notes.wav
│  ├─ 05-polyphony-and-stealing.wav
│  ├─ 06-filter-and-velocity.wav
│  └─ 07-musical-phrase.wav
├─ definitions/
├─ midi/
├─ metrics.json
└─ review-summary.md
```

すべてのWAVは48 kHz、Block Size 257を基準に生成し、`scripts/review/measure_wav.py`でMetricsを生成します。Metrics合格は音質合格を意味しません。人間はSaw高音域、Note境界、Attack/Release、同音連打、Voice Stealing、Filter/Velocity、楽曲での実用性を確認します。

MetricsにはFinite性、Peak / RMS / DC、推定基本周波数、隣接Frame差分、大きな不連続候補数と候補Frame位置を含めます。Saw比較には参考用の簡易Spectrumピークも保存します。さらに、基準WAVをBlock Size 64 / 257 / 1024で実際に再Renderし、`block_size_comparisons`へSample差分を記録します。これらは不具合の切り分け用であり、Alias感や音の魅力の合否判定には使いません。

Review資料の刺激条件を変えずに再生成する場合は、次を実行します。

```bash
python scripts/review/generate_basic_poly_synth_package.py
```

SawのFilter開閉は`02-saw-registers.wav`と`02-saw-registers-filter-closed.wav`、Attack/Releaseは`03-attack-release.wav`と`03-attack-release-slow-attack.wav`を同じヘッドホン/音量で比較します。音色の最終的な魅力は自動判定せず、人間が試聴します。

## Metallic Hybrid Review

試聴資料は`review-output/metallic-hybrid/`へ保存します。Sampleの原音、Decode・Resample後のRoot Note、Pitch Range、Oscillator-only、Sample-only、Hybrid Mix、Velocity Response、Musical Phrase、Missing Asset Fallbackを同じ条件で提示します。

```text
review-output/metallic-hybrid/
├─ audio/
│  ├─ 01-sample-source.wav
│  ├─ 02-sample-decoded-root.wav
│  ├─ 03-sample-pitch-range.wav
│  ├─ 04-oscillator-only.wav
│  ├─ 05-sample-only.wav
│  ├─ 06-hybrid-mix.wav
│  ├─ 07-velocity-response.wav
│  ├─ 08-musical-phrase.wav
│  └─ 09-missing-asset-fallback.wav
├─ definitions/
├─ midi/
├─ assets/
├─ metrics.json
└─ review-summary.md
```

同じ資料を再生成するCommandは次です。

```bash
python scripts/review/generate_metallic_hybrid_package.py
```

基準条件は48 kHz、Block Size 257、Stereo、32-bit float WAVです。`metrics.json`では全SampleのFinite性、Peak、RMS、DC、推定基本周波数、隣接Frame差分、終端候補、Block Size 64 / 257 / 1024の再現性を確認します。Metrics合格はHybridの音質合格を意味しません。

| 音源 | 人間が確認する意図 |
|---|---|
| `01-sample-source.wav` | Engine処理前の原音を基準にする |
| `02-sample-decoded-root.wav` | Decode、Hash検証、44.1 kHzから48 kHzへの準備、Root Note再生による変化を確認する |
| `03-sample-pitch-range.wav` | C3 / C4 / C5で同じSampleの音程展開と終端を確認する |
| `04-oscillator-only.wav` | Bodyの音程、Envelope、余韻を単独で確認する |
| `05-sample-only.wav` | AttackのTransient役割と持続Bodyを含まないことを確認する |
| `06-hybrid-mix.wav` | Sample AttackとOscillator Bodyが一つのInstrumentに聞こえるか確認する |
| `07-velocity-response.wav` | VelocityによるGainとFilterの変化が自然か確認する |
| `08-musical-phrase.wav` | 実際のPhraseで明瞭さ、バランス、演奏感を確認する |
| `09-missing-asset-fallback.wav` | Sample欠落時にもOscillator Bodyが正常に残ることを確認する |

人間は、原音との差、Pitch品質、Sample終端のClick、Attackの初速、Bodyの芯と余韻、SoloとMixの一体感、Velocityの自然さ、Phraseでの実用性、Hybridとして使いたいかを`review-summary.md`へ記録します。音量差を比較するときは同じ再生環境を使用し、Sample-only音源はLayer GainとOne-shot後の無音を含むことを確認します。

Review専用Scriptの役割とコミット対象の考え方は`scripts/review/README.md`に記録しています。
