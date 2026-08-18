# Third-party Notices

Sonalloyは次の外部ソフトウェアを直接利用します。各Licenseの原文は依存SourceまたはCargo Registryに含まれるLicense Fileを参照してください。

## Native

### DaisySP V1.0.0

- Project: Electrosmith DaisySP
- Fixed Commit: `a0494a3adb67f549e18dfd71a35fa656f65b38b6`
- Repository: <https://github.com/electro-smith/DaisySP>
- License: MIT License
- Usage: `Source/Synthesis/oscillator.cpp`によるBasic Oscillator、`Source/Synthesis/variableshapeosc.cpp`によるHard Sync Oscillator、`Source/Filters/svf.cpp`によるFilter Processor、`Source/PhysicalModeling/resonator.cpp`によるModal Generator

SonalloyではDaisySPのSourceを変更せず、Sonalloy固有のOpaque HandleとResult CodeをWrapper側へ実装しています。

### Signalsmith Stretch 1.3.2

- Project: Signalsmith Stretch
- Fixed Commit: `57b93f4e9206a089a45387eaa39bdc9f310d3308`
- Repository: <https://github.com/Signalsmith-Audio/signalsmith-stretch>
- License: MIT License
- Usage: SampleのPitch分離、Fixed Stretch、Tempo Sync

### Signalsmith Linear 0.3.1

- Project: Signalsmith Linear
- Fixed Commit: `5668673560146a9cfe38c25315071e3fd68c8317`
- Repository: <https://github.com/Signalsmith-Audio/linear>
- License: MIT License
- Usage: Signalsmith Stretchが使用するSTFT / FFT基盤

## Rust direct dependencies

| Crate | Version | 用途 | License |
|---|---|---|---|
| `cmake` | 0.1.58 | Native CMake Build | MIT OR Apache-2.0 |
| `thiserror` | 2.0.19 | 構造化Error | MIT OR Apache-2.0 |
| `serde` | 1.0.229 | Diagnostic Serialize | MIT OR Apache-2.0 |
| `serde_json` | 1.0.151 | CLI JSON Output | MIT OR Apache-2.0 |
| `clap` | 4.6.4 | CLI Argument Parse | MIT OR Apache-2.0 |
| `hound` | 3.5.1 | WAV Encode | Apache-2.0 |
| `midly` | 0.5.3 | Standard MIDI File and live MIDI Message Decode | MIT |
| `sha2` | 0.11.0 | Sample Asset SHA-256検証 | MIT OR Apache-2.0 |
| `rubato` | 4.0.0 | Sample Rate変換 | MIT OR Apache-2.0 |
| `symphonia` | 0.6.0 | WAV Asset Probe / Decode | MPL-2.0 |
| `cpal` | 0.18.1 | Realtime Audio Device I/O | Apache-2.0 |
| `midir` | 0.11.0 | Realtime MIDI Input | MIT |
| `crossbeam-queue` | 0.3.13 | Fixed-capacity Realtime Event Queue | MIT OR Apache-2.0 |

## Test-only direct dependencies

| Crate | Version | 用途 | License |
|---|---|---|---|
| `approx` | 0.5.1 | Float近似比較 | MIT OR Apache-2.0 |
| `assert_cmd` | 2.2.2 | CLI結合Test | MIT OR Apache-2.0 |
| `predicates` | 3.1.4 | CLI出力確認 | MIT OR Apache-2.0 |
| `tempfile` | 3.27.0 | 一時Directory | MIT OR Apache-2.0 |

`Cargo.lock`で使用Versionを固定しています。間接依存もそれぞれのLicense条件に従います。
