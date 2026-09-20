# プリセット整理ルール

`presets/` は、音源定義とプレビュー素材をカテゴリ単位で管理する。カテゴリ、ID、ディレクトリ、メタデータの形式を統一する。

音源ごとの説明は [presets-defs.md](presets-defs.md) に記載する。このREADMEには、プリセットの配置と記述ルールだけを記載する。

## カテゴリ

カテゴリは音源の主な役割と鳴り方で決める。

| コード | `category` | 内容 |
|---|---|---|
| `BASS` | `Bass` | ベースラインや最低域を担当する音 |
| `LEAD` | `Lead` | 主旋律やソロなど前景を担当する音 |
| `PAD` | `Pad` | 長く持続し、背景や和音を形成する音。ドローンを含む |
| `KEYS` | `Keys` | 鍵盤的に演奏するポリ音源、コード音、スタブ |
| `PLUCK` | `Pluck` | 発音後に自然減衰する短い有音程音 |
| `MALLET` | `Mallet` | ベル、マリンバなど打撃由来の有音程音 |
| `DRUM` | `Drum` | ドラム、パーカッションなどリズムを構成する打撃音 |
| `SEQ` | `Sequence` | プリセット自身の反復や時間変化が主要な音 |
| `FX` | `FX` | ライザー、インパクト、グリッチ、テクスチャなどの演出音 |

## IDとディレクトリ

IDは `<CATEGORY>-<NNN>` 形式で、連番はカテゴリごとに管理する。

```text
BASS-001
KEYS-008
FX-003
```

新しいプリセットは対象カテゴリの末尾に追加し、既存IDは実装変更後も保持する。

ディレクトリ名は次の形式にする。

```text
presets/<CATEGORY>/<NNN-kebab-case-name>/
```

例:

```text
presets/BASS/001-clean-sub-bass/
presets/KEYS/008-chrome-stab/
```

`CATEGORY` はカテゴリコード、`NNN` はカテゴリ内の3桁連番、末尾は音源名を英小文字のkebab-caseで表す。

## ファイル構成

```text
presets/
├─ BASS/
│  └─ 001-clean-sub-bass/
│     ├─ definition.json
│     ├─ note-*.wav
│     ├─ phrase.wav
│     └─ pattern.json       # 音源固有のパターンがある場合
├─ LEAD/
├─ PAD/
├─ KEYS/
├─ PLUCK/
├─ MALLET/
├─ DRUM/
├─ SEQ/
├─ FX/
├─ common-patterns/         # 複数音源で共有するパターン
├─ assets/                  # 定義から参照する外部素材
├─ presets-defs.md          # 音源カタログ
└─ README.md
```

各プリセットディレクトリには `definition.json` を1つ置く。`note-*.wav` と `phrase.wav` は定義から生成するプレビュー音源として同じディレクトリに置く。

音源固有のパターンは `pattern.json`、共有パターンは `common-patterns/<descriptive-name>-pattern.json` に置く。

外部WAV素材は `assets/` にまとめ、カテゴリ配下の定義から `../../assets/<file>` で参照する。

## メタデータ

`definition.json` の `metadata` に、次の項目を記載する。

| 項目 | 役割 |
|---|---|
| `name` | 音源を識別する名称 |
| `category` | 音源の主な役割。カテゴリ表の表示名と一致させる |
| `description` | 音そのものを説明する文章 |
| `tags` | 音の特徴を検索するための語彙 |
| `recommended_range` | 推奨演奏音域 |

### `name`

- 英語で2〜4語にする。
- 音色や方式の特徴と、音の種類を組み合わせる。
- `FM`、`Wavetable`、`Hard Sync`、`808`、`Reese` など、音色名として定着した語は使用できる。
- 楽曲ジャンル、利用シーン、アーティスト名、地域名は含めない。

### `description`

- 原則3文、120〜220文字程度にする。情報量が少ない単純な音は短くしてよい。
- 次の順番で記述する。
  1. **音源の正体と音色の核**。Description単独でも何の音源か分かるように、Nameに含まれる重要な音型・方式・楽器種を自然に含め、そのうえで帯域、倍音、質感を説明する
  2. 発音から減衰・持続までの時間変化、広がり、余韻
  3. 短音、長音、連打、和音、Velocityなどでの振る舞い
- 実装値の羅列ではなく、聴感または演奏で確認できる性質を記述する。
- FM、Granular、Formantなど、音色の理解に必要な方式は自然に言及してよい。
- 楽曲ジャンル、アーティスト、地域、流行、特定の利用シーンで用途を限定しない。

音源ごとの実際の説明は [presets-defs.md](presets-defs.md) に記載する。

### `tags`

3〜5個を目安に、次の語彙から選ぶ。4分類すべてから選ぶ必要はなく、音を特徴付けるものだけを付ける。

#### 音色・周波数

| Tag | 定義 |
|---|---|
| `Bright` | 高域や高次倍音が明確 |
| `Dark` | 高域が抑えられ、低中域中心 |
| `Warm` | 丸く穏やかな倍音構成 |
| `Mellow` | 刺激が少なく柔らかい音色 |
| `Clean` | 歪みやノイズが少なく純度が高い |
| `Metallic` | 非整数倍音などによる金属的な響き |
| `Glassy` | 透明で硬質な高域を持つ |
| `Noisy` | ノイズ成分が主要な要素 |
| `Hollow` | 中域が抜けたような空洞感を持つ |
| `Airy` | 軽く開いた高域や空気感を持つ |
| `Sub` | 最低域と基音が音色の中心 |
| `Resonant` | 明確な共鳴ピークを持つ |

#### 質感・音像

| Tag | 定義 |
|---|---|
| `Smooth` | 倍音や時間変化が滑らか |
| `Rough` | 粗い倍音やざらつきを持つ |
| `Dense` | 倍音や声部の重なりが多く密度が高い |
| `Thin` | 倍音量や音像が軽く細い |
| `Punchy` | 発音直後の押し出しが強い |
| `Sharp` | 輪郭や高域の立ち上がりが鋭い |
| `Soft` | アタックや輪郭が柔らかい |
| `Wide` | 左右方向へ大きく広がる |
| `Narrow` | ステレオ幅が小さい |
| `Centered` | 中央に明確な音像の芯を持つ |
| `Detuned` | 微細な音程差による揺れや厚みが明確 |
| `Diffuse` | 音像が広く分散し、輪郭が拡散している |

#### 時間変化

| Tag | 定義 |
|---|---|
| `Short` | 全体が短時間で収束する |
| `Sustained` | 保持中に安定して鳴り続ける |
| `Plucky` | 明確なアタックから自然に減衰する有音程音 |
| `Percussive` | 打撃的なアタックが主要な特徴 |
| `Swelling` | ゆっくり立ち上がりながら存在感が増す |
| `Evolving` | 保持中に音色が継続的に変化する |
| `Rhythmic` | 音源内部に周期的またはステップ的な反復がある |
| `Stable` | 保持中の音色変化が少ない |
| `Long-Tail` | 発音終了後も長い余韻が続く |

#### 生成方式・音響処理

| Tag | 定義 |
|---|---|
| `Analog-Style` | アナログシンセに由来する音色設計 |
| `FM` | FMによる倍音変化が主要な特徴 |
| `PM` | Phase Modulationが主要な特徴 |
| `AM` | Amplitude Modulationが主要な特徴 |
| `Ring-Mod` | Ring Modulationが主要な特徴 |
| `Wavetable` | Wavetableの波形・走査が主要な特徴 |
| `Additive` | Partialの加算構成が主要な特徴 |
| `Granular` | Grainによる粒状構造が主要な特徴 |
| `Formant` | 母音・声道的な共鳴が主要な特徴 |
| `Wavefold` | Wavefoldによる折り返し倍音が主要な特徴 |
| `Hard-Sync` | Hard Sync特有の倍音構造が主要な特徴 |
| `Phase-Distortion` | Phase Distortionによる倍音変化が主要な特徴 |
| `Physical` | 物理モデルによる発音が主要な特徴 |
| `Modal` | 共鳴Modeの集合による響きが主要な特徴 |
| `Sample-Based` | Sample再生が音色の主体 |
| `Wave-Sequence` | 複数の音色・Sampleの時間的な切り替えが主体 |
| `Spectral` | スペクトル処理・再構成が主要な特徴 |

生成方式タグは、definition内部で方式を使っているだけでは付けない。音色の特徴として明確に現れる場合に使う。

新しいタグを追加する場合は、既存タグと意味が異なり、音を聞いて判断でき、複数のプリセットで再利用でき、検索条件として意味を持つことを確認する。一つのプリセットだけを説明する固有語は、`name` または `description` で表現する。

楽曲ジャンル、感情・世界観、利用シーン、アーティスト、地域を表すタグは追加しない。

## 追加・変更手順

1. カテゴリ末尾のIDでディレクトリを作り、`definition.json` を配置する。
2. プレビューWAVと、必要なパターンを同じ構成に揃える。
3. `presets-defs.md` のカタログを更新する。
4. パスを参照するスクリプト、テスト、Pattern、デモを確認する。

## 検証

```text
sonalloy instrument validate presets/<CATEGORY>/<NNN-name>/definition.json
sonalloy instrument inspect presets/<CATEGORY>/<NNN-name>/definition.json --json
sonalloy pattern validate presets/<CATEGORY>/<NNN-name>/pattern.json
sonalloy pattern validate presets/common-patterns/<name>-pattern.json
```

音源固有の `pattern.json` と共有パターンを、それぞれの配置に応じて検証する。共有パターンを変更した場合は、参照する音源とデモも確認する。
