# Third-party Notices

Sonalloyは次の外部ソフトウェアを直接利用します。各Licenseの原文は依存SourceまたはCargo Registryに含まれるLicense Fileを参照してください。

## Native

### DaisySP V1.0.0

- Project: Electrosmith DaisySP
- Fixed Commit: `a0494a3adb67f549e18dfd71a35fa656f65b38b6`
- Repository: <https://github.com/electro-smith/DaisySP>
- License: MIT License
- Usage: `Source/Synthesis/oscillator.cpp`によるSine / PolyBLEP Saw生成

SonalloyではDaisySPのSourceを変更せず、Sonalloy固有のOpaque HandleとResult CodeをWrapper側へ実装しています。

## Rust direct dependencies

| Crate | 用途 | License |
|---|---|---|
| `cmake` | Native CMake Build | MIT OR Apache-2.0 |
| `thiserror` | 構造化Error | MIT OR Apache-2.0 |
| `serde` | Diagnostic Serialize | MIT OR Apache-2.0 |
| `serde_json` | CLI JSON Output | MIT OR Apache-2.0 |
| `clap` | CLI Argument Parse | MIT OR Apache-2.0 |
| `hound` | WAV Encode | Apache-2.0 |

## Test-only direct dependencies

| Crate | 用途 | License |
|---|---|---|
| `approx` | Float近似比較 | MIT OR Apache-2.0 |
| `assert_cmd` | CLI結合Test | MIT OR Apache-2.0 |
| `predicates` | CLI出力確認 | MIT OR Apache-2.0 |
| `tempfile` | 一時Directory | MIT OR Apache-2.0 |

`Cargo.lock`で使用Versionを固定しています。間接依存もそれぞれのLicense条件に従います。
