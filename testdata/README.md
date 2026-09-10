# testdata

TestやCIが参照する固定入力（Fixture）の正本を置く。プロダクトコードからは参照されない。

## 構成

| Directory | 内容 |
|---|---|
| `instruments/` | 音源Definition。Rust Test（`crates/*/tests/`、`src/`内Unit Test）が読み込む |
| `assets/` | 音声Asset（WAV）。生成方法とSHA-256は`assets/README.md`へ記録する |
| `midi/` | TestやCIが参照するMIDI入力 |
| `events/` | Event Sequence入力（JSON） |
| `expected/` | 複数Testで共有する期待値 |
| `generate/` | Fixtureを決定的に再生成するScript。一覧は`generate/README.md` |

## 運用ルール

- **ここに入れてよいもの**: Testコード（`#[cfg(test)]`、`tests/`）が実際に読み込むFixtureと、その生成Scriptだけ。参照される見込みのないものは置かない
- **CI専用の入力もここで管理する**: 外部AudioのCIスモークテストで使うDefinition、Event、Assetも該当するディレクトリへ置く
- **生成できるAssetはScriptを`generate/`へ置く**: 手書きせず決定的に生成し、固定Seed・固定成分から作る。実行のたびに同じ結果にならなければならない
- **手書きFixtureを変更するとき**: 読み込むTestの期待値に影響する場合があるため、`cargo test --workspace`を通してからCommitする
