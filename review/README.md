# review

音声Reviewの成果物と、それを同じ条件で再生成するための道具を置く。Testとは独立で、人間が試聴して音を判断するための領域。手順と判定基準は[docs/testing-and-sound-review.md](../docs/testing-and-sound-review.md)が正本。

## 構成

| Directory | 内容 |
|---|---|
| `generate/` | Review Package生成Script。一覧は`generate/README.md` |
| `generate/fixtures/` | Package生成だけが使う音源Definition。`testdata/instruments/`とは役割が違う |
| `<package>/` | 生成済みReview Package（WAV、Definition、Metrics、`review-summary.md`） |

## 運用ルール

- **Packageは必ずScriptから生成する**: 手作業でWAVやMetricsを書き換えない。生成Scriptの実行結果とRepositoryの内容が一致している状態を保つ
- **入力の正本はtestdata**: MIDI・音声Assetは[`testdata/`](../testdata/)の正本からPackageへコピーする。Package内の入力は、そのPackageの条件を固定するためのスナップショット
- **生成し直したら試聴する**: Metricsは機械的確認で、音質の合格は人間の試聴で判断する。結果はPackageの`review-summary.md`へ記録する
- **成果物はGit管理する**: 同じ入力から同じ音が出ていることを履歴で追跡できるようにするため

新しい合成方式を追加したときだけPackageを増やす。既存Packageの観点が変わらない範囲の修正では、Packageを増やさず生成Scriptを更新する。
