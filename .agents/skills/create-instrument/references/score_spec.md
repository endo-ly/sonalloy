# Sonalloy Score YAML — 正式言語仕様（`score_spec.md`）

楽曲・短いプリセットプレビュー・詳細試奏・共通試奏に共通する、1ファイルの音楽記述言語。**このファイルが文法・データ契約・意味論の唯一の正本。** `demo.json` / `pattern.json` / 楽曲Bundle v1は楽曲の入力形式として廃止する。

仕様対象は12平均律の楽譜と、その演奏への一意な展開。音源そのものの `definition.json` は独立した正本であり、本仕様の変更対象外。

## 目次

1. [目的と基本原則](#1-目的と基本原則)
2. [ファイル構造](#2-ファイル構造)
3. [音高とスケール](#3-音高とスケール)
4. [時間とパターン](#4-時間とパターン)
5. [コードと伴奏](#5-コードと伴奏)
6. [素材の参照・反復・変奏](#6-素材の参照反復変奏)
7. [セクションと演奏イベント](#7-セクションと演奏イベント)
8. [音源参照・演奏データ境界](#8-音源参照演奏データ境界)
9. [詳細時間・端数小節・PPQ](#9-詳細時間端数小節ppq)
10. [演奏イベント・マクロ・優先規則](#10-演奏イベントマクロ優先規則)
11. [プリセット試聴・プレビュー・CLI境界](#11-プリセット試聴プレビューcli境界)
12. [解析・診断・安全性](#12-解析診断安全性)
13. [正規形・決定性・受入基準](#13-正規形決定性受入基準)
14. [追加のフィールド契約・文法一覧](#14-追加のフィールド契約文法一覧)

## 1. 目的と基本原則

1ファイルで**曲の構成、転調、コード進行、メロディ、各楽器の役割、演奏イベント**を俯瞰・編集するためのYAML形式。主な編集者は人間とAIエージェント。Sonalloyのデモ楽曲制作とプリセット試奏における**唯一の楽譜正本**として扱い、Scoreをメモリ内の演奏計画へ直接展開してSonalloyが演奏・書き出しを行う。

- **音符とコードのルートは相対度数で統一する。** 度数の `1` は現在のセクションの主音。同主長調の度数を固定の物差しとし、`b` / `#` で半音単位の変更を明示する。
- **スケールは作曲上の文脈であり、音符を自動変換しない。** AエオリアンとAドリアンでも `3` は常にAからの長3度。短3度を使うなら `b3` と書く。
- **楽曲の全体像を先に置く。** `meta` → `form` → `sections` → 音楽素材 → `instruments` の順に記述する。曲の並び・小節数・主音・スケールは `form` だけが所有する。
- **短い記法の守備範囲を限定する。** 音符とリズムは拍のグリッド表現を使い、特別な長さ・位置・奏法・演出は構造化されたフィールドに記述する。実行される自然言語指示は認めない。
- **意味を推測して実行しない。** 未定義の語句、欠落した参照、矛盾した編集は検証エラーにする。`notes`、`transitions[].description`、`role`、`mark`は説明・表示用で、演奏に作用しない。
- **最初は12平均律専用。** このファイル形式の度数とコード構成音は12平均律で定義する。微分音の作曲言語は別途検討し、ここへ仮の互換ルールを追加しない。

## 2. ファイル構造

トップレベルは次の項目を使用する。`melodies`や`drums`などの辞書は名前付きの再利用可能素材を定義する。キー名が識別子となり、出力される音は`sections`の参照で決まる。

| フィールド | 内容 | 必須 |
|---|---|---|
| `meta` | 曲名、テンポ、拍子、調律、音域の基準、音価とマスター設定 | 必須 |
| `form` | **再生順**と各セクションの長さ・主音・スケール・拍子・テンポ | 必須 |
| `time_changes` | 曲の途中のテンポ変更（曲頭からの有理数四分音符位置） | 任意 |
| `sections` | コード、演奏する楽器・素材、セクションごとのイベント | 必須 |
| `melodies` | 度数による旋律フレーズと変奏 | 任意 |
| `scale_definitions` | 組込み以外のスケール名と度数集合を定義。音高の解釈には使わない | 任意 |
| `drums` | 楽器別の打撃パターンと小節条件 | 任意 |
| `bass` | コード連動のベースフレーズ | 任意 |
| `comp` | コード伴奏の発音リズム | 任意 |
| `arps` / `counters` | アルペジオ・応答フレーズの生成規則 | 任意 |
| `macros` | 再利用可能な、構造化された演出処理 | 任意 |
| `instruments` | 識別子、プリセット参照、音量、音域 | 必須 |
| `transitions` / `notes` | 転調意図・制作ノート。再生に影響しない | 任意 |

`meta` は以下の定義を使う。

```yaml
meta:
  title: DOPAMINE OVERLOAD
  bpm: 168
  meter: 4/4
  tuning: 12edo
  reference_octave: 4
  gate: 0.92
  master:
    target_lufs: -12
    max_true_peak_db: -1
    target_lra_lu: 7
```

`bpm` は四分音符毎分、`reference_octave` は音符 `1` が属するオクターブ（C4＝MIDIノート60）、`gate` は生成音符の基本音価へ掛ける倍率（0より大きく1以下）。`master` のLUFS・True Peak・LRAは出力の目標／上限であり、音源の物理的実現性はレンダラーが検証する。`target_lra_lu`は測定値との比較目標であり、強制的に達成するマスタリング処理は規定しない。測定結果・差分・計測できない理由をレポートする。目標との差が0.5 LUを超える場合、または計測できない場合はwarningを返す。

`form` の各項目は `{section, bars, tonic, scale}` を持ち、必要なら`bpm`、`meter`、`last_bar_quarters`を追加できる。テンポと拍子は曲頭で`meta`の値を採用し、`form`で値を変更した場合は後続セクションにも引き継ぐ。`section` は `sections` のキー、`bars` は正の整数で、最後の端数小節も1小節として数える。`tonic` は `C` / `F#` / `Bb` などの音名、`scale` は`ionian` / `dorian` / `phrygian` / `lydian` / `mixolydian` / `aeolian` / `locrian` などの既知の音階名とする。`harmonic_minor`、`melodic_minor`、`major_pentatonic`、`minor_pentatonic`、`whole_tone`も組込み名として扱う。組込み以外はトップレベルの`scale_definitions`に、名前をキー、度数トークンの配列を値として登録する。未登録名はエラーとし、黙って長音階にはしない。**`scale` はすでに書いた音符の音程を変えない。**

組込みスケールの構成音（音高決定には使わず、生成・分析の文脈に使用）：

| スケール | 度数 |
|---|---|
| `ionian` | `1 2 3 4 5 6 7` |
| `dorian` | `1 2 b3 4 5 6 b7` |
| `phrygian` | `1 b2 b3 4 5 b6 b7` |
| `lydian` | `1 2 3 #4 5 6 7` |
| `mixolydian` | `1 2 3 4 5 6 b7` |
| `aeolian` | `1 2 b3 4 5 b6 b7` |
| `locrian` | `1 b2 b3 4 b5 b6 b7` |
| `harmonic_minor` | `1 2 b3 4 5 b6 7` |
| `melodic_minor` | `1 2 b3 4 5 6 7`（ジャズで用いる上行形） |
| `major_pentatonic` | `1 2 3 5 6` |
| `minor_pentatonic` | `1 b3 4 5 b7` |
| `whole_tone` | `1 2 3 #4 #5 b7` |

複数回出る同名の `section` は同じ演奏定義を再利用する。各出現で異なる転調が可能だが、各出現の`bars`と、そのセクションが参照する素材長が一致するかを検証する。現行の曲は、各セクションが1回ずつ出現する。

## 3. 音高とスケール

### 3.1 度数の意味

`1`は`tonic`、`2`は長2度、`3`は長3度、`4`は完全4度、`5`は完全5度、`6`は長6度、`7`は長7度。12平均律では、主音からの半音数は順に **0、2、4、5、7、9、11**。

| 記法 | 主音Cでの実音 | 定義 |
|---|---|---|
| `1` | C | 基準オクターブの主音 |
| `b2` | Db | 短2度 |
| `#1` | C# | 増1度 |
| `b3` | Eb | 短3度 |
| `#4` | F# | 増4度 |
| `b6` | Ab | 短6度 |
| `b7` | Bb | 短7度 |
| `1'` | C（1オクターブ上） | `reference_octave` + 1 |
| `7,` | B（1オクターブ下） | `reference_octave` − 1 |

音符トークンの文法：`[b|#|bb|##]? [1-7] [ '... | ,... ]?`（空白なし）。`'`は1個につき1オクターブ上、`,`は1個につき1オクターブ下。混用は無効。`bb` / `##` はそれぞれ2半音変更。音符・コードで同じ半音計算を使う。`#1` と `b2` は12平均律では同じ高さだが、解析結果には元の音程綴りを保持する。出力MIDIは音高だけを保持する。

`tonic: A, scale: aeolian`でもA＝`1`、C＝`b3`、F＝`b6`、G＝`b7`。AドリアンならF#＝`6`。**実音を決めるのは`tonic`と音符トークン**であり、`scale`はAIの作曲判断や表示・分析の参考とする。スケール外音を禁止しない。

### 3.2 音域

`reference_octave: 4` の場合、セクション`tonic: Eb`では `1`＝Eb4（MIDI63）、`7,`＝D4（MIDI62）、`1'`＝Eb5（MIDI75）。`tonic: A`なら `1`＝A4（MIDI69）。楽器ごとの低音域などは、音程表記を変える代わりに`transpose`（半音単位）または音源別の演奏音域で制御する。

`voicing_floor_midi` は和音の配置に使う**技術的な下限値**であり、メロディの音符記法ではない。コード構成音は度数から決め、その音域配置だけをMIDIノート番号で指定する。

## 4. 時間とパターン

### 4.1 共通時間軸

演奏イベントの時間は**有理数の四分音符**で計算し、出力段階でTickへ変換する。標準的な4/4では1小節16ステップ、1ステップ＝16分音符。`bar`と`step`は**1始まり・セクション内相対**とする。`{bar: last}`は末尾の小節、`step: 13`は4拍目の先頭。

拍子`n/d`では、1小節は `n × 4/d` 個の四分音符に相当する。パターンの`|`は、**分母の音符1個分の記譜区間**を分ける。通常のグリッドはその区間の長さに応じた16分音符の数を使う（4/4なら4マス×4区間、6/8なら2マス×6区間）。この区切りは記譜上のもので、強拍を自動決定しない。連符の分数長は丸めず保持する。

`form`の開始位置を順に足すことで曲全体の小節位置と経過時間が確定する。曲中のテンポ・拍子変更は`form`のセクション開始時点、または途中のテンポだけを扱う`time_changes`へ記述し、音符の四分音符基準の位置は維持する。現行の曲は全区間168 BPM・4/4。

### 4.2 メロディ文字列

```yaml
melodies:
  hook:
    - "3 - 2 3 | - - 5 - | 4 - - 2 | 2 - . ."
```

- 度数：その位置で新しい音を発音。同じ度数が隣り合っても発音し直す。
- `-`：直前の音を継続する（小節先頭では前小節から継続していてもよい）。曲の開始時など継続対象がないときはエラー。
- `.`：休符。継続中の音を終了する。
- `|`：拍・区間の区切り。小節ごとの文字列は拍数・区間数を満たす。
- `[1 2 b3]`：1つの拍・区間を3等分した連符。各要素は同じ長さを持つ。括弧内は空白区切りで、連続文字列のドラム表記とは区別する。

音符の基本長は、次の新しい発音・休符または文字列の終了まで。長さが確定した後、`gate`を適用する。独立した同時発音や精密な音価は`placements`の構造化イベントを使用する。

### 4.3 ドラム文字列

```yaml
drums:
  four:
    parts:
      kick: "X... | X... | X... | X..."
      hat:  "XxXx | XxXx | XxXx | XxXx"
```

ドラム1マスは`.`（休み）、`o`（弱）、`x`（通常）、`X`（アクセント）。基本Velocityは`o=64`、`x=96`、`X=120`、発音長は1グリッドマス。`[oooxxx]`なら1区間を6等分し、各文字で同じ長さの音を発音する。音の種類は`parts`のキーが参照する音源で指定する。ドラム以外のメロディ・ベース・コード伴奏のVelocityとは独立する。

## 5. コードと伴奏

### 5.1 コード記法

1つのコードは `<root>:<quality>[/<bass>]` の形を取る。`root` と `bass` は第3章と同じ相対度数で、コードの構成音は`root`からの相対半音数で計算する。`bass`を省略した場合はルート音。`NC`はコードなしであり、コード参照演奏を発音させない。

| 種類 | 構成音（ルートからの半音数） |
|---|---|
| `maj` | 0, 4, 7 |
| `m` | 0, 3, 7 |
| `dim` | 0, 3, 6 |
| `aug` | 0, 4, 8 |
| `maj7` | 0, 4, 7, 11 |
| `m7` | 0, 3, 7, 10 |
| `7` | 0, 4, 7, 10 |
| `m7b5` | 0, 3, 6, 10 |
| `dim7` | 0, 3, 6, 9 |
| `sus4` | 0, 5, 7 |
| `7sus4` | 0, 5, 7, 10 |

例：`tonic: Eb`で `5:7/7` はB♭7/D、`tonic: A`で `1:m` はAm、`tonic: C`で `#1:dim7` はC#dim7。`/`の後も**コードルートからの音程ではなくセクション主音からの度数**。コードのルートと構成音がスケール外でも許容する。

1小節のコードが1個なら全小節、2個なら前後半の同じ長さで割り当てる。より細かい和声変化はセクション内`chord_changes: [{at: ..., chord: ...}]`を用いる（時刻の競合はエラー）。元の曲は1小節につき1～2コードのみ。

### 5.2 ベース参照

`bass.<name>.parts`の `R` は**その発音位置で有効なコードのベース音**（分数コードならスラッシュ以降）を指す。`R5`はその音の完全5度上、`N`は次のコードイベントのベース音。`R'`などの `'` / `,` はその音をオクターブ移動する。`choices.T`のような名前付きトークンは宣言済みのものだけ許容し、小節の奇数・偶数で置換してから解析する。奇数・偶数はセクション内1始まり。

この曲の基準ベース音域は **E1～Eb2（MIDI28～39）** とする。各コードのベース音をその範囲の同名音へ配置してから `R5` / `N` とオクターブ変化を適用する。`NC`上で `R` を参照した場合は検証エラー。`N`が次セクションを参照する場合は`form`上の次のコードを使用する。次のコードが存在しない場合もエラー。

### 5.3 伴奏・アルペジオ・対旋律

- `comp` の `x` は、現在のコードをその位置で発音。`-`で保持、`.`で休止。`hold`に限り、1小節中にコードが替わった地点で前の和音を終了し、新しい和音を発音し直す。
- `instruments.*.voicing_floor_midi`は、クローズボイシングをその値以上に配置する下限。転回形のベース指定は和音の最低音へ反映する。**ボイシングの決定規則**：`voicing_floor_midi`以上で最も低い指定ベース音を第1声部とする。残りのコード構成音は、第1声部より上の最も近い位置から昇順に配置する。同じ構成音の重複は除く。ベースがコード構成音でない場合は、ベースとは別に構成音全てを追加する。これを標準クローズボイシングとする。
- `arps.<name>.order`の `0 1 2 3` は、上記ボイシングの構成音を下から並べた音列のインデックス。コード音が4個未満の場合は構成音を上オクターブへ反復して必要数を作る。`retrigger_each_step`の省略時または`true`では1マスずつ再発音し、`false`では隣接する同音の音価を結合してからGateを適用する。
- `counters.<name>.when: lead_silent_full_beat` は、その拍の**全期間**にリード音が鳴っていない場合だけ、`cell` の連続音をその1拍に等分配置する。リードの休みはNote Off・Gate処理後に判定する。

## 6. 素材の参照・反復・変奏

定義の再利用には、規則が異なる構文を増やさず、次の3種類を使用する。

| 形式 | 用途 | 解決方法 |
|---|---|---|
| `from: <name>` | 辞書内の定義を継承 | 継承先のトップレベルフィールドを単位として上書き。省略フィールドは継承。循環参照は禁止 |
| `replace: {5: "..."}` | `melodies`の指定小節だけ変更 | 展開済みメロディの5小節目をその文字列へ置換。小節番号は1始まり |
| `{use: verse, bars: [1, 2], shift_steps: -2}` | メロディの区間を抜き出し時間移動 | 元の1～2小節を2小節分の領域として挿入し、音符イベントだけ指定数ステップ平行移動 |

`sections`の`from`は`chords`、`lead`、`comp`、`events`などの**フィールドごと**に上書きする。`events`配列は追記ではなく置換。`drums`の`from`は`parts`を楽器ごとに統合し、`overrides`を継承後に追記する。`melodies`の`from`は小節列を継承し、`replace`が指定小節を置換する。継承後の参照は全て検証する。

`melodies`のリストに文字列と`use`を混在できる。`use`の負方向シフトで配置領域の先頭より前に出るイベントは、セクション境界を越える意図が明示された場合だけ許す。出力曲の時刻0より前には配置できない。文字列は1小節、`use`は指定された小節数だけ挿入領域を消費する。時間移動したイベントがその領域の外に出る場合は、参照元／先の長さと配置可能範囲を検証する。`lead.slice_bars: [5,8]` はメロディの5～8小節だけを選び、セクションの1小節目へ配置する。

`drums.overrides` は`when: {odd: true}`、`when: {every: 4}`、`when: {bar: 5}`、`when: {bar: last}`を使う。条件に一致した小節の該当楽器のパターンを完全に置換する。別楽器の変更は共存できるが、**同じ小節・同じ楽器への複数の上書きが同時成立した場合はエラー**。

## 7. セクションと演奏イベント

### 7.1 セクションの基本フィールド

| フィールド | 意味 |
|---|---|
| `chords` | 小節数と同じ長さのコード列 |
| `lead` | `{instrument, melody, slice_bars?, layers?}`。基本メロディをその楽器で演奏 |
| `drums` | `drums`内のパターン名 |
| `bass` | `{pattern, add_parts?}`。`add_parts`は新しい楽器のベースパターンを追加 |
| `bass_overrides` | 指定小節のベース楽器別パターンの上書き |
| `comp` | `{楽器名: compパターン名}` |
| `pad` | `{instrument, pattern}`。`hold`はコードを持続し、コード変更時に再発音 |
| `arp` / `counter` | `{instrument, pattern}`。それぞれの名前付き規則を参照 |
| `answers` | `{instrument, melody, at}`の配列。単発の応答フレーズ配置 |
| `placements` | `{instrument, at, ...}`の配列。特定の音符・コード・切り出しフレーズを指定 |
| `events` | 構造化された演出・制御・マクロの配列 |
| `velocity` | メロディ・コード・ベース等の既定MIDI Velocity（1～127） |

`lead.layers` は同じメロディを別楽器で重ねる配列。`transpose`は半音、`gain_db`はその演奏の追加ゲイン。`lead`から派生した音符の音高を計算してから移調する。`bass.add_parts`の文字列は通常の`bass`文字列と同じ文法を使う。

### 7.2 placements

各`placement`は `instrument` と `at: {bar, step}`、および次のいずれか1つの演奏対象を持つ。

- `melody`：既存の素材名。`slice_steps: {start, count}`で開始位置と切り出すステップ数、`take_notes`で先頭からの発音数を指定できる。切り出した音符イベントの時刻は選択範囲の先頭を0として`at`に配置する。
- `degree`：指定した度数の1音。`duration_steps`、`velocity`を付与できる。
- `chord`：指定した度数コードの和音。必要なら`voicing_floor_midi`と`add_root_degree`を指定する。
- `bass_note`：`R` / `R5` / `N`など、ベース参照による1音。

追加の`transpose`（半音）、`gain_db`（dB）、`mark`（タイムライン表示用の識別子）は許可する。`duration_steps`は正の有理数として解析する。YAMLの数値`1.5`は3/2として正確に扱う。イベントがセクション境界を越える場合は、明示された長さと出力先の許容範囲で検証する。

### 7.3 events と macros

| `kind` / `macro` | 意味 |
|---|---|
| `hit` / `hits` | `instrument`または`instruments`を指定位置で1回鳴らす |
| `silence` | 指定期間において、`except_instruments`以外の音を発音させず、継続中の音も期間開始時に終了させる |
| `part_window` | 対象楽器のパターンを指定小節範囲だけ有効化する |
| `step_roll` | 指定小節の`from_step`～`through_step`へ16分刻みの打撃を置き、Velocityを線形補間する |
| `drum_roll` | `bars`小節ぶんのロール。前半の発音密度・最後の小節の密度・停止位置・強弱をすべて数値で与える |
| `automation` | `instrument` / `parameter` / `from_value` / `to_value` / `duration_steps`で時間変化を宣言する。対応可能なパラメータは出力先で検証する |
| `macro: name` | `macros.name`を展開する。呼び出し元の`bars`等の明示値で同名フィールドを置換する |

`hit`は音源に1回のNote Onを送るイベントとして扱う。既定ノートはMIDI60、Velocityはセクションの`velocity`（省略時100）、ノート長は1ステップ。実際にドラム・FXをどの音程で駆動するかは音源の演奏契約に依存するため、実装時にプリセットとの対応を検証する。

この曲のマクロの意味：

- `drop`：セクション1小節目・ステップ1にcrash / impact / subdrop。
- `gasp`：最終小節ステップ13～16でchop以外を停止。
- `hey`：最終小節ステップ13にchopで度数5の1ステップの音（Velocity124）。
- `roll`：指定された末尾N小節のうち、最後以外は各拍2打。最終小節は最初の3拍を各拍4打とし、4拍目は演奏しない（4/4の既定Macro）。Velocityを85から120まで発音順に線形上昇。riserはロール開始時に1回トリガーする。`bars: all`はセクションの全小節。

同じ楽器・同じ開始時刻の打撃が通常のドラムパターンとロールで重なるときは、ロールで通常の打撃を置換し、2重発音しない。`silence`は最後に適用し、対象期間へ重なる音をクリップする。マクロと既存イベントで矛盾するパラメータ指定がある場合は検証エラーにする。


## 8. 音源参照・演奏データ境界

### 8.1 正本と内部実行モデル

```text
score.yaml / preview.score.yaml / showcase.score.yaml
    ↓ 1種類の厳密なScoreパーサー
ResolvedScore（セクションの出現、調性、和声、素材、音源参照）
    ↓ 音楽的時刻・音高・イベントの決定的な展開
PerformancePlan（TimeMap、楽器別Lane、Event、Audio Routing、Mix）
    ├─ Sonalloy音声Core → WAV / Stems / Master / MP3
    ├─ 既存のRealtime Event Feed → Audition
    ├─ MIDI書き出し / MIDIからScore生成
    └─ CLI Inspect / Preview情報生成

Instrument definition.json + 音源Assets → 音色Compile / 音声Core
```

`PerformancePlan`等は実装のためのメモリ内型である。実際のファイルとして`demo.json`や`patterns/*.json`を出力・再入力する段階を設けない。Scoreを編集元、WAV・MIDI・生成Manifestを成果物とする。音源定義JSONと低レベル診断用Event JSONは別責務であり、今回のScoreに吸収しない。

### 8.2 楽器の定義

`instruments.<id>`は、`preset`、`definition`、`external: true`のうち**ちょうど1種類**を指定する。同時指定・省略をエラーとする。

```yaml
instruments:
  kick: {preset: DRUM/001-hard-punch-kick, gain_db: -2, role: drums}
  custom: {definition: instruments/custom/definition.json, gain_db: -3, role: lead}
  preview: {external: true, gain_db: 0, role: audition}
```

- `preset`：`<preset_root>/<CATEGORY>/<NNN-kebab-name>/definition.json`へ厳密に解決。`preset_root`はCLIの`--preset-root <PATH>`を優先し、未指定なら環境変数`SONALLOY_PRESET_ROOT`、さらに未指定ならScoreファイルの親ディレクトリを上へ辿って最初に見つかる`presets/`を使う。いずれでも見つからなければ、`--preset-root`の指定を要求するエラー。存在しないID、プリセットルート外参照は禁止。プリセットの識別子は実際のディレクトリ名であり、音源を勝手に置換しない。
- `definition`：Score所在ディレクトリを基点に参照。解決先はInstrument Definition JSON。`definition.json`の`schema_version`等は既存Coreに従う。
- `external: true`：試奏用Scoreなどの未束縛楽器。CLIの`--bind <instrument_id>=<definition_path>`を**必須**とし、未束縛での実行をエラーにする。固定`preset`・`definition`を持つ楽器への束縛を拒否する。
- `gain_db`：楽器全体の固定ゲイン（省略時0、有限数）。`role`は表示・分類だけに利用。
- `voicing_floor_midi`：和音の最下声配置下限（0～127）。`trigger_note_midi`：Drum/FXなど、音程を指定しない打撃・単発音を鳴らすMIDIノート（0～127）。打撃・単発音を生成する場合は必須であり、音源ごとに明示する。`bass_floor_midi`/`bass_ceiling_midi`はベース参照音の標準配置範囲（省略時28～39）。
- `audio_input: <id>`：ほかの楽器のステレオ出力を音声入力として受ける。入力能力のある音源だけ許可。対象不存在、自己参照、循環、外部入力のチャンネル数不一致はエラー。音源から入力へ渡る音声にはその楽器のPart Gainを適用し、グローバルフェード・マスターを含めない。
- 技術的な`*_midi`は演奏エンジンのMIDIノート配置用である。**メロディ・コードの音高を書くための第2記法とはしない。**

### 8.3 演奏Laneと追加ゲイン

同じ楽器の複数フレーズは原則1つのイベント列に統合する。`lead.layers[].gain_db`や`placements[].gain_db`など、特定演奏へ追加ゲインが指定された場合、その発音を同じ音源を参照する別の内部Laneへ割り当てる。実効Gain＝楽器`gain_db`＋追加`gain_db`。独立Laneは音源のVoice/FX状態を共有しないので、通常Laneと同音源が同一に継続すると仮定しない。Laneは `(instrument_id, effective_gain_db, source_order)` に基づいて決定的に生成し、Stemは**元の楽器ID単位**で音量補正後Laneを合算する。

同音源への`audio_input`の接続先は元の楽器IDとし、必要なLaneを合算後に渡す。Lane間のコントローラーを共有すべき演奏で追加Gainを使う場合は、検証で状態分離の影響を可視化する。**ゲイン差を消して単一Laneへ黙って畳み込まない。**

### 8.4 出力・マスター

`meta.master.target_lufs`は現行マスターのIntegrated LUFS目標、`max_true_peak_db`はTrue Peak上限、`target_lra_lu`は測定比較目標とする。最初の2項目は現在のMaster実行時の失敗条件を維持し、LRAは計測値・差・未計測理由を出力する。`meta.fade_out_seconds`を楽曲全体のフェードアウト（省略時0）に使用する。

`sample_rate`、`block_size`、`tail_seconds`、WAV/MP3/Stem/Premasterの出力先は**コマンド実行条件**であり、楽譜には持たせない。`--json`レポートにセクション・音符数・使用楽器・音声接続・正規化された演奏時間を返す。

## 9. 詳細時間・端数小節・PPQ

### 9.1 時間単位と範囲

内部時刻は**曲頭を0とする有理数の四分音符**で管理する。1小節の長さは`meter = n/d`なら`n * 4/d`四分音符。`form`中の`bpm`/`meter`は該当セクションの開始点から後続セクションへ継承する。曲中のテンポ変更は`time_changes`を用い、拍子変更は`form`境界に記述する。

```yaml
time_changes:
  - {at_quarters: "13/2", bpm: 140}
```

`at_quarters`は曲頭からの四分音符数（非負）。同時刻の重複・`form`側の同時刻テンポ指定との衝突はエラー。曲終端を超える位置もエラー。厳密な`at_quarters`を整数Tickへ変換するための解像度は下記に従う。

**位置**：`at: {bar: 1, step: 1}`は、セクション内・1始まりであり、`step`は16分音符単位。4/4は1～16、6/8は1～12。`at: {bar: last, step: 1}`も許す。グリッドに乗らないイベントは`at: {bar: 1, offset_quarters: "1/3"}`を使う。`offset_quarters`はその小節頭からの0始まりの量。`step`と`offset_quarters`の併用は禁止。

**長さ**：`duration_steps`は16分音符数であり、正の整数・有限10進数・分数文字列を受理する。`duration_quarters`は四分音符数で、同じく正の有理数。両方の指定は禁止。パターンの`gate`が掛かるのは自動計算音価のみ。`placements`や明示ノートの`duration_*`にはGateを再適用しない。

**比率の解析**：入力の整数・有限小数・`p/q`を正確に整数分数へ展開し、約分して保持する。早期に`f64`へ丸めない。分母0、負の禁止範囲、溢れをエラーにする。

### 9.2 部分小節

`form[]`は`last_bar_quarters: "p/q"`を許す。`bars`は**最後の短い小節を含む正の整数**。最後の小節長を`L`、通常小節長を`B=n*4/d`とすると、`0 < L < B`を要求し、セクション長は`(bars-1)*B + L`。最終小節の発音位置は`[0,L)`であり、明示ノートの終端は曲終端以下とする。Sustain解除などの瞬時制御は末尾小節の`offset_quarters: L`でセクション終端にも配置できる。短い小節の素材は**実際の有効長**を消費する。最後の部分を暗黙にトリミングしたり、無音を自動で埋めたりしない。

```yaml
form:
  - {section: fx_preview, bars: 8, last_bar_quarters: "2", tonic: C, scale: ionian}
```

これは4/4の通常7小節＋最後2四分音符＝7.5小節。480 PPQなら`length_ticks=14400`。`presets/common-scores/fx-downlifter.score.yaml`と`fx-sub-drop.score.yaml`はこの長さを使う。拍子変更が曲中小節の途中にあるMIDIも、旧拍子側を部分小節として終えてから新拍子のセクションを開始する。

### 9.3 記法の単位と連符

4/4は1つの`|`区間が四分音符で4スロット、6/8は1つの`|`区間が八分音符で2スロット。1/4より短い分母単位の拍子でも、16分以上の長さを持つ区間には正確なスロット数を割り当てる。分母が32以上で16分スロットを持たない区間では文字列グリッドを使わず、`placements`/構造化イベントで書く。拍子の分母として1,2,4,8,16,32,64,128を受理する。`|`は区間の境界であり、**各区間を内部的に16分スロットへ分割する**。

`[1 2 b3]`および`[oxX]`は、1区間全体を3等分する連符とする。括弧単位は単独で区間を構成し、その同一区間に通常の4スロット等を並置しない。連符内の休符・Tieも同じ内部文法に従う。4/4の1小節ではパイプで4区間、6/8では6区間を必須とする。短い最終小節は最後の有効区間に合わせて短縮したパターンを要求する。16分境界以外の最終切断では構造化配置で厳密に指定する。

### 9.4 PPQと秒への変換

展開された全イベント・音価・テンポ/拍子変更位置の有理数分母について最小公倍数を取り、**480の整数倍で最小のPPQ**を選ぶ（結果は1～32767）。そのPPQで全時刻と全長が整数Tickにできることを確認する。表現不能なら丸めずにエラー。Sample Rateに依存する秒・Frame変換は既存`musical_time.rs`の音楽時間処理に接続する。Tick→Frameは変化区間を通算してから最終フレームへ丸め、区間ごとに累積丸めをしない。

`sonalloy score import-midi`は元SMFのPPQを正確な音楽時刻へ変換して保持し、書き出しで厳密に整数Tickを得る。MIDIが大きすぎるPPQや表現不能な分数を含む場合は診断し、音程/Timingを暗黙にクオンタイズしない。

## 10. 演奏イベント・マクロ・優先規則

### 10.1 イベント形式

セクションの`events`は構造化されたマップのリスト。`macro: <name>`はトップレベル`macros`の同名イベント定義を参照する。`kind`と`macro`の同時指定は禁止。全イベントは、対象音源を含めてセクション内座標を解決する。

| `kind` | 必須フィールド（共通`at`は省略） | 意味 |
|---|---|---|
| `hit` | `instrument` | 指定Trigger Noteを1回発音。長さは省略時1ステップ。 |
| `hits` | `instruments` | 指定音源群への`hit`と同値。順序はリスト順。 |
| `degree` | `instrument`, `degree` | その度数の1音。マクロ本体にも指定可。 |
| `silence` | `duration_steps`または`duration_quarters`; `except_instruments`は任意 | 指定範囲の対象Noteを抑制・切断。明示の例外は抑制しない。 |
| `part_window` | `instrument`, `bars: [start,end]` | 対象楽器の演奏をその小節範囲だけ有効にする。 |
| `step_roll` | `instrument`, `bar`, `from_step`, `through_step`, `velocity_from`, `velocity_to` | 両端を含む16分連続打撃を線形Velocityで生成。 |
| `drum_roll` | `instrument`, `bars`, `early_hits_per_beat`, `final_hits_per_beat`, `last_bar_end_beat`, `velocity_from`, `velocity_to`, `riser?` | 対象末尾小節で密度が変わるロール。 |
| `sustain_pedal` | `instrument`, `down` | MIDI CC64相当。 |
| `pitch_bend` | `instrument`, `value` | −1～1の既存値。 |
| `mod_wheel` | `instrument`, `value` | 0～1。 |
| `aftertouch` | `instrument`, `value` | 0～1。 |
| `parameter_change` | `instrument`, `parameter`, `native_value` | 音源ParameterのNative値に設定。 |
| `automation` | `instrument`, `parameter`, `from_value`, `to_value`, `duration_steps`または`duration_quarters` | 既存Parameter Rampと同じ正規化空間の線形補間。 |

- `silence`は指定した小節区間のNote Onを抑制し、境界にまたがるNoteの音価を切断する。演奏制御とParameter Rampには作用しない。Release音・Reverb・External Audioの残響まで完全ミュートする意味ではない。
- `part_window`は対象InstrumentのそのSection内で始まるNote Onと点の演奏制御を半開区間`[start,end)`に限定する。前Sectionで始まり当該Sectionへ持続するNoteは、後続SectionのWindowでは変更しない。対象Section内でWindowに含まれるNoteの音価が終端を越える場合は境界で切断する。点の演奏制御は区間内のイベントだけを残す。区間開始では以前の状態をニュートラル値またはParameterのDefinition既定値へ戻し、区間終端でも同じ既定状態へ戻して後続Sectionへ状態を漏らさない。窓をまたぐParameter Rampは許可区間との交差部分へ切り詰め、両端値を元の線形Rampから補間する。窓が空の場合はSection中に制御を残さない。同じInstrument・Sectionへ複数の窓を指定した場合は許可区間の共通部分を使う。
- 同一楽器・同時刻の`drum_roll`/`step_roll`と通常ドラム打撃が衝突すればロールが勝つ。**ロール同士**、同一Parameterへの異なる値・重複矛盾、複数の同一楽器Overrideが重なればエラー。`silence`はNoteだけを抑制・切断し、`part_window`は対象InstrumentのNoteと演奏制御を制限する。
- Note、Sustain、Bend、Wheel、Aftertouch、ParameterなどのEventで同時刻が起きても、ソース定義順を保持し、Note Off→Control→Note Onの基本優先順位を適用する。旧Pattern RuntimeのNote ID/レイテンシ/Loop状態を維持する。
- `kind: drum_roll`で`bars: all`はセクションの全小節、正整数なら末尾N小節。4/4既定マクロ`roll`は最後以外を各拍2打、最後の小節の3拍目までを各拍4打、4拍目は無音。発音順Velocityを指定両端間で線形補間し、整数値への丸め方は`round half away from zero`とする。末尾の`riser`がある場合はロール開始時に1回打撃する。
- `macro: name`に付いた上書きフィールドはマクロ本体の同名フィールドを置換する。存在しないフィールドの追加は事前にイベント型検証。マクロ循環禁止。自然言語文字列から実行内容を推測しない。
- `drums`、`hit`/`hits`、`step_roll`、`drum_roll`、およびそれらを含むMacroは、対象Instrumentの`trigger_note_midi`を必須とし、省略やMIDI 0～127外の値をエラーにする。未指定値からノートを推測しない。`recommended_range`は音域の目安であり、Trigger Noteの許容範囲ではない。実際の演奏に関係する音源Range/Triggerを検証する。`note`が存在しない外部音声処理Partも必要な場合は許す（ダミーNoteを生成しない）。

### 10.2 継承・再利用の合成順

1. 名前付き素材の`from`を再帰解決（サイクル禁止）。
2. `melodies.replace`を小節単位で適用し、`use`・`slice_bars`・`shift_steps`を解決。
3. `sections.from`を**フィールド単位**で上書き。`events`配列は全体置換、`comp`等マップも基本全体置換。`drums.from`は`parts`を音源IDごとに合成、`overrides`リストを追記。
4. `form`出現に応じて調・時間・コードを解決し、グリッドから音符を生成。
5. 初期メロディ・伴奏・ベースを展開し、**暗黙の音価にだけ一度Gateを適用**してNote On/Offを確定する。
6. `drums.overrides`、`bass_overrides`、`answers`、`placements`、`macros`、`events`を展開。`counters.when: lead_silent_full_beat`は**Gate適用後のリードの発音区間**で判定する。
7. 同時刻打撃、状態付き演奏操作、優先順位・衝突を確定し、最後に`silence`/`part_window`を反映する。移調・Gain、音源適合、曲終端、PPQを検証して内部Eventを生成する。

`melodies.answers.gap`のような`.`区切り名を、再利用素材の名前空間参照として扱う。辞書は葉に文字列小節リストまたは`from`/`replace`を持つ継承定義を置き、それ以外は名前空間辞書として再帰する。音名・コード以外の「意味の似た別名」を自動導入しない。

### 10.3 音源能力と演奏制御

- MIDI Noteは0～127、Velocityは1～127。`recommended_range`から外れた発音はwarning。実際のGeneratorのKey/Velocity Triggerと照合して**発音可能Layerがない場合**はerror。シンセによっては音程無関係のTrigger Noteを必要とする。
- `parameter_change`/`automation`のParameter IDはInstrumentのCatalogに存在し、Native値は範囲内の有限数でなければならない。Mod Wheelなどのレンダラー標準演奏操作と、音源固有Parameterの対象を混同しない。
- 同一LaneでのSustainやPitch Bendは時刻順に演奏される。追加GainによりLaneを分割するとController状態の作用域もLaneごとになるので、試奏差分の監査対象とする。
- `events`の有理数時刻は出力WAV/MIDI/Realtimesで共通の`PerformancePlan`を使う。レンダー専用に別解釈してはいけない。

## 11. プリセット試聴・プレビュー・CLI境界

### 11.1 同じScore文法を3用途に使用

| 用途 | 正本 | 基本の音源束縛 |
|---|---|---|
| デモ楽曲 | `score.yaml` | `preset:`/`definition:`で確定 |
| プリセット短いプレビュー | `presets/<CAT>/<NNN-name>/preview.score.yaml` | 基本的に自分の`preset:` |
| プリセット詳細試奏 | `presets/<CAT>/<NNN-name>/showcase.score.yaml` | 基本的に自分の`preset:` |
| 複数音源で共用する試奏 | `presets/common-scores/*.score.yaml` | `external: true`をCLIで束縛 |

短いプレビューと詳細試奏が異なる音楽なら、異なるScoreでよい。演奏データはScoreに記録し、音源の名前、タグ、音域、音色は`definition.json`に定義する。

ブラウザ向けプレビューは1楽器・1テンポ・1拍子のNote列とし、30〜300 BPM、1〜32音、音楽時間10秒以内に収める。

内蔵プリセットの`manifest.json`を作る`stage-builtin-bundle.mjs`は**配布用データ生成**であって、廃止する楽曲Bundle v1とは別物。生成Manifestの`preview`はScoreを単一のRustコンパイラで展開したNote/Tempo/Lengthから得る。NodeスクリプトへScore DSLを再実装しない。

### 11.2 公開CLIと内部仕様の境界

コマンドのOptionとReportは[CLIリファレンス](cli.md)を参照する。Scoreに関わる公開入口は`score init/validate/inspect/import-midi/export-midi`、`render score`、`audition score`。移行済み楽譜・試奏を同じ解析器へ通す。旧`demo`/`pattern`の入力CLIと楽曲Bundle v1を削除。`score pack`は**導入しない**。

`score import-midi`でMIDI半音クラスを度数へ書き直す際の固定綴り：

| 半音class（主音=0） | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 度数 | 1 | #1 | 2 | b3 | 3 | 4 | #4 | 5 | b6 | 6 | b7 | 7 |

MIDIは元の調性・異名同音を必ずしも保持しないので、入力時の主音は明示`--tonic`またはCとし、`scale`を表示用の`ionian`に設定する。MIDI Noteの絶対音高は`reference_octave`と度数+オクターブで一致させる。受理できないCC、同音Note重複、16チャネル制限等については、既存Sonalloyの入出力契約に従い、表現不能な情報を黙って欠落させない。

## 12. 解析・診断・安全性

1. **構文**：YAML 1.2相当、重複キー・未知フィールド・カスタムTag・YAMLエイリアス・マージキー禁止。非文字列Keyや曖昧な暗黙キャストを禁止。分数は`"p/q"`で記述、`#`を含む値は引用文字列にする。
2. **参照**：`form.section`、Instrument ID、素材、`from`、`macro`、`definition`、`preset`、`audio_input`の存在と循環を検証。名前付き辞書の参照を勝手に作らない。
3. **音楽**：度数とコード品質、各小節のセル長、楽器/音域、和声タイムライン、`NC`中のR/N参照、Overrideの重複、端数小節終端、区間外Note/Control、PPQ上限を検証。
4. **状態**：Automation/Parameterの未登録ID・非有限値・範囲外、同時刻競合、Pitch Bend/Wheel/Aftertouch範囲を検証。
5. **音声経路**：External Audioの必要/不要・チャネル数・循環、出力Stem名の大文字小文字衝突、Windows予約名、危険なPath Traversal、意図しない上書きを拒否。
6. **診断**：診断に`path`（Score YAML上の階層）、`section`、`bar`/`step`、入力行/列、`actual`、`expected`、詳細メッセージを保持。`--json`にも含む。`warning`と`error`を分ける。Error発生時は意図した楽曲の代わりに別の音を生成しない。
7. **決定性**：YAMLのマップ列挙順と参照の解決順を定義し、重複音・Velocity順序・安定IDが毎回同じであることを保証する。ランダムに音を選ぶ機能は、明示Seedを持つ仕様が定まるまで採用しない。

## 13. 正規形・決定性・受入基準

### 13.1 内部イベントと音高

演奏の正規形は、全ての音楽的時間を`RationalQuarter`、音程を「元の度数綴り＋12平均律MIDI音高」、Velocityを1～127の整数、演奏操作を既存のCore Event enumへマップできる型として保持する。`ResolvedScore`は`form`を順に積算した**1つの共通Tempo/Meter/Sectionマップ**を持つ。旧Demoの「最長Patternからテンポと曲終端を推定」という方式は廃止する。

### 13.2 実行保証

- 同じScoreと音源群、CLI条件、Sample Rateなら、同じ音楽Event/Stem/レンダリング条件へ到達する。
- 出力時にScore正本以外の楽曲JSONを保持・再読込しない。
- 異なるジャンル・拍子・転調・五音音階を、同じ記法と内部イベントモデルで扱う。
- 旧Patternに含まれるPitch Bend、Mod Wheel、Sustain、Parameter Change等を表現・実行できる。
- MIDIとWAVは、同じ解決済み演奏の出力形式である。MIDIで表現できない演奏操作は具体的な診断を返す。
- 演奏音高を暗黙に変更する`scale`指定や、人間の文章を実行するコードは存在しない。

### 13.3 参照演奏

本仕様の具体的な統合受入対象は、すでに試作された「DOPAMINE OVERLOAD」のScore。**14セクション・70小節・42楽器**の構造を落とさず、全セクションと発音イベントが展開されること。別途、既存51個のPattern（個別31/共通20）と109のPreset Previewを新Scoreへ移す。受入用の完全Scoreは実装時に`testdata/scores/dopamine_overload.score.yaml`へ配置する。

## 14. 追加のフィールド契約・文法一覧

| 場所 | 許可された主要キー | 解釈 |
|---|---|---|
| ルート | `meta,form,time_changes,sections,melodies,scale_definitions,drums,bass,comp,arps,counters,macros,instruments,transitions,notes` | 全て定義済み、追加は禁止 |
| `meta` | `title,bpm,meter,tuning,reference_octave,gate,master,fade_out_seconds` | `tuning: 12edo`だけ受理 |
| `meta.master` | `target_lufs,max_true_peak_db,target_lra_lu` | Master/計測の設定 |
| `form[]` | `section,bars,tonic,scale,bpm,meter,last_bar_quarters` | 曲中の出現と時間正本 |
| `time_changes[]` | `at_quarters,bpm` | 曲頭からのテンポ変更 |
| `sections.<name>` | `from,chords,chord_changes,lead,drums,bass,bass_overrides,comp,pad,arp,counter,answers,placements,events,velocity` | 1セクションの演奏宣言 |
| `melodies` | 小節文字列のリスト、`{from,replace}`、名前空間辞書 | 相対度数と再利用 |
| `drums.<name>` | `from,parts,overrides` | 1小節Grooveをセクションで反復 |
| `drums.overrides[]` | `when,parts` | 小節条件を満たしたパートへ置換 |
| `bass.<name>` | `parts,choices` | 現在・次コード参照 |
| `bass.choices.<token>` | `odd,even` | セクション内奇数・偶数 |
| `arps.<name>` | `order,voicing_floor_midi,retrigger_each_step` | 和音の声部を音域配置してArp |
| `counters.<name>` | `when,cell` | 無音拍に置く対旋律 |
| `instruments.<id>` | `preset / definition / external`, `gain_db,role,voicing_floor_midi,trigger_note_midi,bass_floor_midi,bass_ceiling_midi,audio_input` | 音源と演奏音域 |
| `placements[]` | `instrument,at,melody/degree/chord/bass_note,velocity,duration_steps,duration_quarters,transpose,gain_db,mark,slice_steps,take_notes,voicing_floor_midi,add_root_degree` | 明示的発音 |
| `events[] / macros.<name>` | `kind/macro,at,instrument,instruments,duration_*,parameter,native_value,from_value,to_value,down,value,degree,except_instruments,bars,bar,from_step,through_step,velocity_from,velocity_to,early_hits_per_beat,final_hits_per_beat,last_bar_end_beat,riser` | 演奏・制御・演出 |
| `transitions[]` | `from,to,method,description` | 解説専用 |
| `notes` | UTF-8文字列 | 制作ノート、演奏に影響しない |

**注意：上表はフィールドの所在一覧であり、組合せと排他関係は各章で定義する。** 未定義の連想的エイリアス（`key`、`mode`、`chord_type`等）や、自然言語の演奏指示は受理しない。既存Scoreで使う辞書参照の数値キー、`1'`や`#4`は必要に応じて引用してYAMLの型・コメント解釈を回避する。
