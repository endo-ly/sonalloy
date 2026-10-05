# リリース

## 概要

`main` で `CI` が成功すると、GitHub Actions の Release ワークフローが `Cargo.toml` の workspace version と最新 Release のタグを比較します。バージョンが変わっていれば `vX.Y.Z` タグを作成し、4 プラットフォームのバイナリをビルドして GitHub Release（アセット付き）を公開します。利用者は `scripts/install.sh` を実行するだけで最新版をインストールできます。

## バージョン規則

- セマンティックバージョニング（`vX.Y.Z`）に従う
- `Cargo.toml` の workspace `version` は `MAJOR.MINOR.PATCH` 形式で記述する
- Release ワークフローが `v<version>` タグを作成し、タグとバージョンを一致させる
- リリースノートはタグ間の Conventional Commits から自動生成される

## リリース手順

1. `Cargo.toml` の workspace `version` を更新し、`main` へ push する
2. CI 成功後に Release ワークフローがタグ作成とビルドを行う
3. Actions の Release ワークフロー完了後、GitHub Releases にアセットが公開される
4. インストールスクリプトで動作確認する

   ```bash
   curl -fsSL https://raw.githubusercontent.com/endo-ly/sonalloy/main/scripts/install.sh | bash
   sonalloy --version
   ```

## 成果物

| ファイル | 内容 |
|---|---|
| `sonalloy-{version}-{triple}.tar.gz` | バイナリ単体のアーカイブ。Native ライブラリはすべて静的リンク済み |
| `SHA256SUMS.txt` | 各アーカイブの SHA-256 チェックサム |

対応プラットフォームとアーカイブの `{triple}`:

| プラットフォーム | triple |
|---|---|
| Linux x86_64 | `x86_64-unknown-linux-gnu` |
| Linux arm64 | `aarch64-unknown-linux-gnu` |
| macOS arm64 | `aarch64-apple-darwin` |
| Windows x86_64 | `x86_64-pc-windows-msvc` |
