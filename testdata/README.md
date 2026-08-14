# testdata

Testが参照する固定入力（Fixture）の正本を置く。プロダクトコードからは参照されない。

## 構成

| Directory | 内容 |
|---|---|
| `instruments/` | 音源Definition。Rust Test（`crates/*/tests/`、`src/`内Unit Test）が読み込む |
| `assets/` | 音声Asset（WAV）。生成方法とSHA-256は`assets/README.md`へ記録する |
| `midi/` | TestとReviewで共有するMIDI入力 |
| `events/` | Event Sequence入力（JSON） |
| `expected/` | 複数Testで共有する期待値 |
| `generate/` | Fixtureを決定的に再生成するScript。一覧は`generate/README.md` |

## 運用ルール

- **ここに入れてよいもの**: Testコード（`#[cfg(test)]`、`tests/`）が実際に読み込むFixtureと、その生成Scriptだけ。参照される見込みのないものは置かない
- **Review専用の入力は置かない**: Review Package生成だけが使うDefinitionは[`review/generate/fixtures/`](../review/generate/)へ置く
- **生成できるAssetはScriptを`generate/`へ置く**: 手書きせず決定的に生成し、固定Seed・固定成分から作る。実行のたびに同じ結果にならなければならない
- **手書きFixtureを変更するとき**: 読み込むTestの期待値に影響する場合があるため、`cargo test --workspace`を通してからCommitする

依存の向きは一方向。`review/generate/`のPackage生成Scriptはtestdataを参照するが、その逆はない。
