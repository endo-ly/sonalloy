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

## 音声Reviewのルール

- Metricsは手入力せず、`scripts/measure_wav.py`でWAVから生成します。
- 自動Testの期待値は`testdata/expected/sine_metrics.json`で管理します。
- Review Artifactには、音声、Metrics、Render条件、受入結果を保存します。
- 自動TestではWAV Metadata、Finite性、再現性、Metricsを確認し、音質の最終判断は人間が行います。
