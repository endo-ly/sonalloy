# Sonalloy Score YAML 言語仕様

楽曲・短いプリセットプレビュー・詳細試奏・共通試奏に共通する、1ファイルの音楽記述言語。**このファイルが文法・データ契約・意味論の唯一の正本である。**

仕様対象は12平均律の楽譜と、その演奏への一意な展開。音源そのものの `definition.json` は別の正本であり、本仕様の対象外。

## 1. 全体構造と基本設定

1つのYAMLファイルで**曲の構成、転調、コード進行、旋律、各楽器の役割、演奏の表情**を記述する。

- **「何を鳴らすか」と「どう鳴らすか」を分けて書く。** 音高・リズムはトークン列に、強弱の流れ・奏法の効き方・ノリ・声部配置は構造化されたフィールドとイベントに記述する。
- **自動で決まる部分は決定的にし、明示した指定が常に勝つ。**
- **意味を推測して実行しない。** 未定義の語句、欠落した参照、矛盾した編集、自然言語の演奏指示は検証エラーとする。`notes`、`role`、`mark`、`sound`などの表示用項目は演奏に作用しない。

トップレベルでは`meta`、`form`、`sections`、`instruments`を必須とし、任意で`time_changes`、`melodies`、`scale_definitions`、`drums`、`comp`、`arps`、`counters`、`macros`、`transitions`、`notes`を記述できる（`kits`、`lyrics`、`mix`は10.2の無視対象）。全フィールドの契約は10章を参照。

```yaml
meta:
  title: Example
  bpm: 120
  meter: 4/4
  tuning: 12edo
  reference_octave: 4
  gate: 0.92
  fade_out_seconds: 0
  master: {target_lufs: -12, max_true_peak_db: -1, target_lra_lu: 7}
```

- `bpm`: 四分音符毎分
- `tuning`: `12edo`のみ受理
- `reference_octave`: 音符 `1` が属するオクターブ（C4＝MIDIノート60）
- `gate`: 生成音符の基本音価へ掛ける倍率（0より大きく1以下）
- `fade_out_seconds`: 楽曲全体のフェードアウト秒数（0以上、省略時0）
- `master`: 記述する場合は`target_lufs`（`-70.0..=-5.0`）と`max_true_peak_db`（`-9.0..=0.0`）が必須で、レンダラーの達成条件（外れた出力はエラー）。`target_lra_lu`（`>= 0.0`）は任意の測定比較目標で、差が0.5 LUを超えるか計測できない場合はwarningを返す
- MP3出力時、`meta.master.max_true_peak_db`が設定されておりMP3エンコード後の測定`true_peak_db`が上限を超える場合はwarningを返す
- `sample_rate`、`block_size`、`tail_seconds`、WAV/MP3/Stem/Premasterの出力先は**コマンド実行条件**であり、楽譜には持たせない

`form` の各項目は `{section, bars, tonic}` を必須とし、`scale`、`bpm`、`meter`、`last_bar_quarters`、`swing`を追加できる。テンポと拍子は曲頭で`meta`の値を採用し、`form`で値を変更した場合は後続セクションにも引き継ぐ。

- `section`: `sections` のキー。同名`section`が複数回出現する場合は同じ演奏定義を再利用し、各出現で異なる転調が可能だが、各出現の`bars`と参照素材長の一致を検証する
- `bars`: 正の整数（最後の端数小節も1小節として数える）
- `tonic`: `C` / `F#` / `Bb` などの音名
- `scale`: 組込みスケール（下表）またはトップレベルの`scale_definitions`（名前→度数トークン配列）に登録した名前を指定する。未登録名はエラー。省略時はスケール未指定として扱い、音符・コード・演奏イベントは変化しない

組込みスケール（音高決定には使わず、生成・分析の文脈に使用）：

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

ScoreはほかのScoreファイルを参照できず、1ファイルと本仕様だけで記述する。

## 2. 音高とスケール

### 2.1 度数の意味

`1`〜`7`は長音階の各度数を表し、12平均律では主音からの半音数は順に **0、2、4、5、7、9、11**。

| 記法 | 主音Cでの実音 | 定義 |
|---|---|---|
| `1` | C | 基準オクターブの主音 |
| `b2` | Db | 短2度 |
| `#1` | C# | 増1度 |
| `1'` | C（1オクターブ上） | `reference_octave` + 1 |
| `7,` | B（1オクターブ下） | `reference_octave` − 1 |

度数トークンの文法は `[b|#|bb|##]? [1-7] [ '... | ,... ]?`（空白なし）。

- `'`は1個につき1オクターブ上、`,`は1個につき1オクターブ下。混用は無効
- `bb` / `##` はそれぞれ2半音変更。音符・コードで同じ半音計算を使う
- `#1` と `b2` は12平均律では同じ高さだが、解析結果には元の音程綴りを保持する。出力MIDIは音高だけを保持する

**実音を決めるのは基準音と音符トークン**であり、`scale`に関わらずスケールに含まれない度数もそのまま発音する。

### 2.2 音域

ベースなどの低音域は、オクターブ記号を重ねる代わりに`register: bass`（2.3）で配置し、それ以外の音域調整は`transpose`（半音単位）で行う。`voicing_floor_midi` / `voicing_ceiling_midi` は和音配置用の技術的範囲（MIDIノート番号）であり、旋律の音符記法ではない。

### 2.3 基準音・音域の選択・主音の上書き

旋律素材（第5章）と`placements`の度数は、**基準音**からの度数として音高になる。

| 決める項目 | 指定箇所 | 値 | 意味 |
|---|---|---|---|
| 基準 | 旋律素材の`relative_to` | `tonic`（既定） | 主音を基準にする |
| | | `chord_bass` | 発音位置のコードのベース音を基準にする。分数コードはスラッシュ指定の音。`NC`上ではエラー |
| 音域 | `lines.<名前>.register` / `placements[].register` | `melody`（既定） | 基準音の音高クラスを`reference_octave`のC〜Bに置く |
| | | `bass` | 基準音の音高クラスを、楽器の`bass_floor_midi`～`bass_ceiling_midi`（既定28～39）で最も低い位置に置く。範囲内に該当音がなければエラー |
| 主音 | `lines.<名前>.tonic` / `placements[].tonic` | 音名 | その演奏に限り、セクションの`tonic`の代わりにこの主音で度数を解釈する |

音高は「配置した基準音 ＋ 度数（臨時記号・オクターブ記号を含む）＋ `transpose`」で決まる。度数適用後の音高は音域へ折り返さず、最終MIDIノート0～127を検証する。

`c`を付けたトークン（`c1`〜`c13`）は、その位置のコード構成音を指定する。`relative_to`ではなく**コードルート**を基準にする点が度数トークンと異なる。

- `c3`はコードの長短に応じた3度を指す。コードルートの音高クラスを`register`の規則で配置し、その構成音の音程（複合音程を含む）とオクターブ記号を加える
- 臨時記号は同じ度数の変化音を特定する（`cb9` / `c#9`）
- 指定した度数がコードにない場合、複数の構成音に当てはまる場合、`NC`上で使った場合は検証エラー
- どの旋律素材でも使え、同じ形をコードに合わせて移すシーケンスを書ける

主音の上書きは、調と関係なく鳴る物、転調をまたぐペダル音、別の調で書いた素材の引用に使う。`relative_to: chord_bass`の素材に`tonic`を指定するとエラーとする。`c`トークンとコードは、上書きした主音ではなくセクションの和声に従う。

## 3. 時間とパターン

### 3.1 共通時間軸

演奏イベントの時間は**有理数の四分音符**で計算し、出力段階でTickへ変換する。標準的な4/4では1小節16ステップ、1ステップ＝16分音符。`bar`と`step`は**1始まり・セクション内相対**とする。`{bar: last}`は末尾の小節、`step: 13`は4拍目の先頭。

拍子`n/d`では、1小節は `n × 4/d` 個の四分音符に相当する。分母の音符1個分を**区間**と呼ぶ（4/4なら四分音符、6/8なら八分音符）。区間は記譜上の単位で、強拍を意味しない。連符の分数長は丸めず保持する。

`form`の開始位置を順に足すことで曲全体の小節位置と経過時間が確定する。曲中のテンポ・拍子変更は`form`のセクション開始時点、または途中のテンポだけを扱う`time_changes`へ記述し、音符の四分音符基準の位置は維持する。

### 3.2 パターン文字列の共通文法

旋律素材の要素、ドラム、コード伴奏（`comp`）、アルペジオの`order`は、すべて同じ時間文法で書く。

- **トークン**：旋律・`comp`・`order`は空白区切り、ドラムは1文字が1トークン（空白は無視）。
- **長さ**：空白区切りのトークンは末尾に`:<長さ>`を付けられる。長さはステップ（16分音符）単位の正の整数または`"p/q"`形式の分数で、省略時は1。`5:6`と`5 - - - - -`は同じ演奏になる。ドラムのトークンは常に1ステップ。
- **連符**：`[1 2 b3]`は角括弧内の要素で1区間を等分する。`[1 2 b3]:4`のように括弧の後へ長さを付けると、その長さを等分する。長さを省略した連符は区間の境界から始まらなければならず、小節の残りが1区間より短い場合は残りの長さを等分する。括弧内のトークンには長さを付けられない。ドラムの連符（`[oooxxx]`）は長さを省略した形だけを使う。
- **区間の表明`|`**：その位置が区間の境界であることを表明する。
- **小節線の表明`||`**：その位置が小節の境界であることを表明する。

表明は任意であり、書いた位置が境界でなければエラーとする。トークンの間にだけ置け、文字列の先頭・末尾や連続した表明は書けない。境界は小節の先頭から数え、端数小節の終端も境界である。音がまたがる境界には表明を書かない。

ドラム・`comp`・`order`の1文字列は1小節を表し、その長さは小節長と一致しなければならない。旋律素材の要素は任意の長さを持てる（第5章）。

### 3.3 旋律文字列

```yaml
melodies:
  hook:
    - "3:2 2 3 | -:2 5:2 | 4:3 2 | 2:2 .:2"
```

トークンは次の順に並べる（空白なし）：

```text
<本体> [<奏法記号>...] [@<velocity>] [:<長さ>] [~]
```

- **本体**：度数（2.1）、`c`トークン（2.3）、`-`、`.`。
  - 度数・`c`トークン：その位置で新しい音を発音する。同じ度数が隣り合っても発音し直す。
  - `-`：直前の音を継続する。要素の先頭では前の要素から継続していてもよい。継続対象がないときはエラー。
  - `.`：休符。継続中の音を終了する。
  - `-`と`.`は長さだけを持てる。
- **奏法記号**：`^`（アクセント）、`!`（スタッカート）、`_`（テヌート）。1つの音に各記号を1回まで付けられる。`!`と`_`は同時に使えない（3.4）。
- **Velocity**：`@`の後に整数1～127。省略時はセクションの`velocity`（既定100）。
- **レガート**：末尾の`~`は次音へのレガート接続。

音符の基本長は、トークンの長さと後続の`-`の合計で決める。Gateは基本音価に一度だけ掛け、値は0より大きく1以下とし、次順で選ぶ。

1. スタッカート・テヌートの記号があればその奏法のGate（3.4）
2. 楽器の`gate`
3. `meta.gate`
4. 既定値1

ドラム・コード伴奏・アルペジオも2〜4の順を使う。

`~`の接続元はGateによる短縮を受けず、次音の開始まで保持して短く重なる。重なりは接続先の発音中に収める。休符を越える接続、接続先のない`~`、`!`・`_`との併用はエラー。

音高遷移の扱いは音源の演奏モードに従う。単音レガート・ポルタメントは音源定義の演奏設定に従い、ポリフォニック音源では音符の重なりとして演奏する。独立した同時発音や精密な音価は`placements`の構造化イベントを使う。

### 3.4 奏法記号

| 記号 | 奏法 | Gate | Velocityへの加算 |
|---|---|---|---|
| `^` | `accent` | 変更しない | 既定 +16 |
| `!` | `staccato` | 既定 `1/2` | 既定 0 |
| `_` | `tenuto` | 既定 `1` | 既定 0 |

楽器ごとに`instruments.<id>.articulations`で上書きできる。`accent`は`velocity`だけ、`staccato`と`tenuto`は`gate`と`velocity`を持てる。`velocity`は-126～126の整数、`gate`は0より大きく1以下。

```yaml
instruments:
  pizz: {preset: BASS/004-short-pluck-bass, articulations: {staccato: {gate: "1/4", velocity: -8}, accent: {velocity: 24}}}
```

音符のVelocityは「`@`またはセクションの`velocity`」に奏法の加算と`dynamics`（第8章）の加算を足した値である。結果が1～127の外になる場合はエラーとし、範囲内へ丸めない。

### 3.5 ドラム文字列

```yaml
drums:
  four:
    parts: {kick: "X... | X... | X... | X...", hat: "XxXx | XxXx | XxXx | XxXx"}
  phrase:
    parts:
      kick: ["X... | ..X. | X... | ....", "X... | ..X. | X.X. | ..X."]
```

ドラム1マスは`.`（休み）、`o`（弱）、`x`（通常）、`X`（アクセント）で、基本Velocityは`o=64`、`x=96`、`X=120`、発音長は1グリッドマス。`[oooxxx]`なら1区間を6等分し、各文字で同じ長さの音を発音する。音の種類は`parts`のキーが参照する音源で指定し、ドラム以外のメロディ・コード伴奏のVelocityとは独立する。

`parts.<楽器>`の取り方と繰り返しは次のとおり。

- 1小節の文字列: セクションの全小節で繰り返す
- 小節の文字列のリスト: N個ならN小節のフレーズとして繰り返す。セクションの小節数がNの倍数でない場合はエラー
- 楽器ごとにNは異なってよい

### 3.6 スウィング

`meta.swing: {subdivision: 8, ratio: "2/3"}`で8分スウィング、`subdivision: 16`で16分スウィングを指定する。`ratio`は0より大きく1より小さい有理数で、`1/2`は均等、`2/3`は2対1。設定箇所は次の順で上書きされる。

| 指定箇所 | 適用範囲 |
|---|---|
| `meta.swing` | 曲全体 |
| `form[].swing` | そのセクションの出現だけ |
| `instruments.<id>.swing` | その楽器について、`meta`と`form`の全出現に優先 |

ほかの楽器が跳ねている中でまっすぐ弾く楽器には`ratio: "1/2"`を指定する。

旋律パート（`lines`）、ドラム、コード伴奏、アルペジオ、対旋律の格子位置と基本音価へ適用する。変換規則は次のとおり。

- 変換は小節ごとに行い、小節境界を固定する。
- 小節をまたぐ音は開始と終了をそれぞれの小節で変換する。
- 小節末の不完全な組は均等なまま扱う。
- 連符は組の開始・終了を変換して内部を等分する。
- 変換後に継続・Gate・レガートを解決する。
- `placements`と`events`の明示位置・音価は保持する。

### 3.7 タイミングのずれ

`instruments.<id>.timing_offset_steps`は、その楽器の全イベントを指定ステップだけ前後にずらす（正で後ろ、負で前。符号付きの有理数）。

ずれはセクション・コード参照・リードの休み判定・`silence`・`part_window`・`dynamics`をすべて記譜上の時刻で解決した後に適用する。ずらした開始時刻が曲頭より前、または曲終端以降になる場合はエラーとし、音符の終端が曲終端を越える場合は曲終端で切る。

## 4. コードと伴奏

### 4.1 コード記法

1つのコードは `<root>:<quality>[/<bass>]` の形を取る。`root` と `bass` は第2章と同じ相対度数で、コードの構成音は`root`からの相対半音数で計算する。`bass`を省略した場合はルート音。`NC`はコードなしであり、コード参照演奏を発音させない。

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

基本構造に6度、7度、9度、11度、13度を組み合わせて指定できる。

| コード名 | 構成の規則 |
|---|---|
| `sus2` | 1, 2, 5 |
| `6` / `m6` | 長三和音・短三和音に6度を追加 |
| `mMaj7` | 短三和音に長7度を追加 |
| `9` / `11` / `13` | 長三和音に短7度と、9度・11度・13度を指定段階まで追加 |
| `maj9` / `maj11` / `maj13` | 長三和音に長7度と、指定段階までの拡張音を追加 |
| `m9` / `m11` / `m13` | 短三和音に短7度と、指定段階までの拡張音を追加 |
| `add9` / `madd9` | 長三和音・短三和音に9度を追加 |

9、11、13は複合音程として保持する。コード品質の後ろに、`1:7(b9,#11,omit5)`のように修飾子を付けられる。

| 修飾子 | 意味 |
|---|---|
| `add9`, `add11`, `add13` | 指定度数を追加 |
| `b5`, `#5`, `b9`, `#9`, `#11`, `b13` | 同度数の通常音を変化音に置換。通常音がなければ追加 |
| `omit3`, `omit5`, `omit7` | 指定度数の構成音を省略 |

追加、変化、省略の順で正規化し、同一構成音の重複は除く。`b9`と`#9`など異なる変化音は共存できる。同じ度数の変化と省略の併用はエラー。

`1:(1,b3,5,b7,#11)`は構成音の直接指定。括弧内はコードルート基準の1～13度と、`b`・`#`・二重臨時記号を使う。基本コード名と同じボイシング指定を利用できる。

`/`の後も**コードルートからの音程ではなくセクション主音からの度数**（例：`tonic: Eb`で`5:7/7`はB♭7/D）。コードのルートと構成音がスケール外でも許容する。

`chords`の各要素は1小節を表し、1小節のコードが1個なら全小節、2個なら前後半の同じ長さで割り当てる。より細かい和声変化はセクション内`chord_changes: [{at: ..., chord: ...}]`を用いる（時刻の競合はエラー）。

**食い**：`chords`のコードの先頭に`<`を付けると、そのコードは本来の位置より`<`の個数ぶんのステップだけ前から入る。`["4:maj9", "<3:m7"]`では2小節目の`3:m7`が16分1つ前から、`"<<3:m7"`なら8分1つ前から鳴る。

- 和声上はその小節に属したまま、切り替わる時刻だけを前に出す。前のセクションへ食い込んでもよい。
- 食い込んだ時刻が直前のコード変更以前になる場合、曲頭より前になる場合はエラーとする。
- コードを参照するすべての演奏（コード伴奏、アルペジオ、`chord_bass`基準の旋律、`c`トークン）は、切り替わった時刻からそのコードを使う。

### 4.2 ボイシング

和音を音高へ配置する方法は、**明示配置**と**自動配置**の2つである。

`voicing: ["1", "3", "7", "9'"]`で演奏する構成音とオクターブを選ぶと明示配置になる。

- 度数はそのコード品質に含まれる音を指す。`3`は短三和音なら短3度。同じ度数の変化音が複数ある場合は`b9`や`#9`で特定し、含まれない音や曖昧な指定はエラー
- 指定先は`instruments.<id>.voicing`、`sections.<id>.comp.<instrument>`の`{pattern, voicing}`形式、`pad`、`arp`、`placements[]`。使用箇所、楽器既定の順に優先し、どちらもなければ自動配置を使う

明示配置では、下限以上の指定ベース音からコードルートの配置基準を決め、各構成音の複合音程とオクターブ移動を適用して音高順に並べる。

- 配列から外した構成音は演奏せず、分数コードのベースは最低音として保持する。
- オクターブ移動後の構成音が指定ベースより低くなる場合はエラーとし、音高を自動補正しない。
- コードが変わっても同じ形のまま移動するため、平行移動させたい和音（スタブ、プレーニング）に使う。

自動配置は、コードの構成音を奏者のように**直前の和音から最小の動きでつなぐ**。構成音は音高クラスとして扱い、複合音程の高さを固定したい場合は明示配置を使う。

- ベース音は `voicing_floor_midi`（既定48）以上で最も低い位置、上声はベース音より上へ1音高クラスずつ `voicing_ceiling_midi`（既定は下限＋24）以下に置く。上声は最大6音高クラスで、超過や配置できない構成音は検証エラー
- 距離は「各候補音から直前の上声で最も近い音までの半音数の合計」と「直前の上声の各音から候補で最も近い音までの半音数の合計」の和で比較し、同点の場合は最高音が低い候補、さらに同点なら昇順に並べた音高列が辞書順で小さい候補を選ぶ
- 直前の和音は、同じ演奏（`comp.<楽器>`、`pad`、`arp`のそれぞれ）が同一セクション出現内で直前に自動配置した和音。最初の和音と`NC`直後の和音では、そのコードの**基準配置**を直前の上声とみなす（基準配置が範囲内に収まれば、最初の和音は基準配置そのものになる）。`placements[]`の`chord`も単発で前後を参照しない

基準配置は、ベース音の直上に1～7度を最も近い位置へ積み、8度以上をベース以上の最も低いコードルートから複合音程のまま積んだ音高順の配置とする（同じ音高は除く）。

配置結果は`score inspect`で確認できる。和音から生成した各音の`sources[].spelling`には、元のコード表記が入る。

### 4.3 伴奏・アルペジオ・対旋律

- **comp**: `x` は現在のコードをその位置で発音、`-`で保持、`.`で休止。組込みパターン`hold`は各小節頭でその時点のコードを発音して1小節持続し、1小節中にコードが替わった地点で前の和音を終了して新しい和音を発音し直す。食いで小節頭より前に入ったコードは、同じセクションの出現の中であれば、その小節頭で発音し直さずに保持を続ける。`hold`は`comp`での定義・上書きができない予約名である。
- **arps**: `arps.<name>.order`の各セルは、4.2のボイシングの構成音を下から並べた音列のインデックス（非負整数）か、`.`（休符）、`-`（直前の音の継続）である。
  - `-`は直前に発音した同じ音高の音価を延長し、継続中にコードが変わっても発音済みの音高は変わらない。小節をまたぐ継続も許可し、先行する発音がない`-`はエラーとする。
  - コード音が4個未満の場合は構成音を上オクターブへ反復して必要数を作る。
  - `retrigger_each_step`の省略時または`true`では数値セルごとに再発音する（`-`は常に音価を延長）。`false`では隣接する同音の数値セルを結合してからGateを適用する。`.`はどちらの場合も休符として発音を区切る。Gateは音価が確定した後に一度だけ掛かる。
- **counters**: `counters.<name>.when: silent_full_beat` は、セクションの`counter.follows`に挙げた旋律パートのどれもその拍の**全期間**に鳴っていない場合だけ、`cell`の各セルをその1拍に等分配置する。`cell`は空白区切りの度数・`.`（休符）・`-`（直前の音の継続）で、先行音のない継続はエラー。休みの判定はNote Off・Gate処理後の音価で行う。

## 5. 素材の参照・反復・変奏

`melodies`は旋律素材を定義する。素材は**要素**の並びで、各要素は任意の長さのパターン文字列（3.2・3.3）である。素材の長さは全要素の長さの合計で、使う側で検証する。

```yaml
melodies:
  hook:
    - "5 . b3' . | 1':2 . 5 | b7 . 1'~ 2' | b3':2 .:2"
  hook_end:
    from: hook
    replace: {4: "1':6 .:10"}
  walk:
    relative_to: chord_bass
    items: ["1:4 3:4 5:4 c7:4"]
  verse:
    pickup_steps: 3
    items: ["5 6 7 || 1':4 3:4 5:8"]
```

素材の葉は、要素のリスト、または`items`か`from`のどちらか一方を持つマッピングとする。どちらも持たないマッピングは名前空間として扱い、`melodies.answers.gap`のように`.`で区切った名前で参照する。マッピングの素材は次の属性を持てる。

| 属性 | 意味 |
|---|---|
| `items` | 要素のリスト |
| `from` | 継承元の素材。要素と属性を継承し、明示した属性で上書きする。循環参照は禁止 |
| `replace` | `from`と併用し、展開後の要素番号（1始まり）の要素を文字列で置換する。置換前と同じ長さでなければエラー |
| `relative_to` | `tonic`（既定）または`chord_bass`。度数の基準（2.3） |
| `pickup_steps` | 先頭から指定ステップを弱起として扱う。正の有理数で、素材の長さより短い |

要素のリストには文字列と`{use: verse, items: [1, 2], shift_steps: -2}`を混在できる。`use`は参照先の展開後の要素から指定範囲を抜き出してそのまま要素として挿入し、音符イベントだけ指定ステップ平行移動する。`use`の負方向シフトで挿入位置より前に出るイベントは、出力曲の時刻0より前に配置できない。時間移動したイベントが素材の外に出る場合は、配置可能範囲を検証する。

**弱起**：`pickup_steps`を持つ素材は、先頭の指定ステップを使用位置より前に鳴らす。旋律パートではセクションの開始より前、`placements`では`at`より前になり、前のセクションと重なってよいが、曲頭より前に出る場合はエラー。

弱起を除いた部分の先頭が素材の**記譜上の原点**であり、`|`・`||`の表明とスウィングの小節はここから数える。弱起の部分は、原点の直前にある同じ長さの小節として扱う。

`sections`の`from`は`chords`、`lines`、`comp`、`events`などの**フィールドごと**に上書きする。継承後の参照は全て検証する。

| 対象 | 統合規則 |
|---|---|
| `events`配列、`lines`・`comp`などのマップ | 追記ではなく置換 |
| `placements` | 置換。`add_placements`は継承・置換後の配置リストの末尾へ順番どおり追記する。`add_placements`だけを持つ独立セクションでは、空の配置リストへ追加する |
| `drums` | `parts`を楽器ごとに統合し、`overrides`を継承後に追記する |

複数段継承では各段階で`add_placements`を実際の`placements`へ統合するため、`A → B → C`の追加結果は`A → B → C`の順番で保持され、解決済みのセクションに`add_placements`は残らない。統合後の配置も定義元は追跡され、診断の`path`は継承先ではなく**配置を定義した元のYAML位置**（`sections.<元のセクション>.placements[i]`または`<そのセクション>.add_placements[i]`）を指す。

`drums.overrides`は条件`when`に一致した小節の該当楽器パターンを、その1小節の文字列で完全に置換する。`when`は次いずれか1つを指定する（小節番号はセクション内1始まり）。

| `when` | 一致する小節 |
|---|---|
| `{odd: true}` / `{even: true}` | 奇数小節 / 偶数小節 |
| `{every: n}` | nの倍数小節（n, 2n, ...） |
| `{bars: [3, 7, last]}` | 列挙した小節（`last`は末尾の小節） |

別楽器の変更は共存できるが、**同じ小節・同じ楽器への複数の上書きが同時成立した場合はエラー**とする。

## 6. セクション構成・時間変更・端数小節

### 6.1 旋律パート (`lines`)

```yaml
sections:
  dream:
    lines:
      flute: {instrument: flute, melody: dream_flute}
      double: {instrument: synth, melody: dream_flute, transpose: 12, gain_db: -6}
      bass: {instrument: bass, melody: dream_bass, register: bass, loop: true}
```

`lines`は名前付きの旋律パートのマップで、本数に制限はない。名前は`counter.follows`から参照する識別子であり、同じ楽器を複数のパートで使ってもよい。

| フィールド | 意味 | 必須 |
|---|---|---|
| `instrument` | 演奏する楽器 | 必須 |
| `melody` | 旋律素材名 | 必須 |
| `slice_items` | `[start, end]`。素材の展開後の要素から両端を含む範囲だけを使う | 任意 |
| `loop` | `true`なら素材を繰り返してセクションを満たす | 任意 |
| `register` | `melody`（既定）または`bass`（2.3） | 任意 |
| `tonic` | 主音の上書き（2.3） | 任意 |
| `transpose` | 半音単位の移調。音高を計算した後に加える | 任意 |
| `gain_db` | その演奏の追加ゲイン（7.2） | 任意 |

弱起を除いた素材の長さは、セクションの長さと一致しなければならない。`loop: true`ではセクションの長さが素材の長さの整数倍でなければならず、弱起は最初の1回だけ鳴らす。`slice_items`で最初の要素を含めない場合、弱起は使わない。

### 6.2 個別配置 (`placements`)

各`placement`は `instrument` と `at: {bar, step}`、および次のいずれか1つの演奏対象を持つ。

- `melody`：既存の素材名。素材の記譜上の原点を`at`に置く。`slice_steps: {start, count}`で弱起を含む素材の先頭から数えた開始ステップと切り出すステップ数、`take_notes`で先頭からの発音数を指定できる。切り出した場合は選択範囲の先頭を`at`に置く。
- `degree`：指定した度数の1音。`duration_steps`、`velocity`を付与できる。
- `chord`：指定した度数コードの和音。必要なら`voicing`、`voicing_floor_midi`、`add_root_degree`を指定する。

追加の`transpose`（半音）、`gain_db`（dB）、`tonic`（主音の上書き）、`mark`（タイムライン表示用の識別子）は許可する。`register`は`melody`と`degree`に指定でき、`degree: "1"`と`register: bass`で楽器のベース音域の主音になる。`duration_steps`は正の有理数として解析する（YAMLの数値`1.5`は3/2として扱う）。イベントがセクション境界を越える場合は、明示された長さと出力先の許容範囲で検証する。

### 6.3 テンポ変更 (`time_changes`)・位置と長さ

```yaml
time_changes:
  - {at_quarters: "13/2", bpm: 140}
  - {at_quarters: "120", bpm: 72, over_quarters: "8"}
```

`at_quarters`は曲頭からの四分音符数（非負）。同時刻の重複・`form`側の同時刻テンポ指定との衝突・曲終端を超える位置はエラー。

`over_quarters`を指定すると、`at_quarters`から`over_quarters`をかけて直前のテンポから`bpm`へ線形変化させる（リタルダンド・アッチェレランド）。`over_quarters`は1/4の正の倍数とし、終端は曲終端以下。変化は1ステップ（16分音符）ごとの段階的なテンポ変更として展開し（nステップ中k番目は「直前＋(`bpm`−直前)×(k＋1)/n」）、変化区間内の別テンポ変更はエラー。

- **位置**：`at: {bar: 1, step: 1}`はセクション内・1始まり、`step`は16分音符単位（4/4は1～16、6/8は1～12、`bar: last`可）。グリッド外は小節頭からの0始まり量`at: {bar: 1, offset_quarters: "1/3"}`を使う。`step`と`offset_quarters`の併用は禁止。
- **長さ**：`duration_steps`（16分音符数）と`duration_quarters`（四分音符数）は正の有理数を受理し、両方の指定は禁止。`gate`は自動計算音価のみに掛かり、`placements`や明示ノートの`duration_*`には再適用しない。
- **比率の解析**：整数・有限小数・`p/q`は丸めず分数として扱う。分母0と負の値はエラー。PPQ上限32767を超える細かい分数の組合せはエラー。

### 6.4 部分小節と拍子単位

`form[]`は`last_bar_quarters: "p/q"`を許し、末尾だけ短い端数小節を置ける。

- `bars`は**最後の短い小節を含む正の整数**。最終小節長`L`、通常小節長`B=n*4/d`として`0 < L < B`、セクション長は`(bars-1)*B + L`。
- 最終小節の発音位置は`[0,L)`、明示ノートの終端は曲終端以下。瞬時制御は末尾小節の`offset_quarters: L`でセクション終端にも配置できる。
- 短い小節の素材は**実際の有効長**を消費し、端数分のトリミングや無音の補填は行わない。

```yaml
form:
  - {section: fx_preview, bars: 8, last_bar_quarters: "2", tonic: C, scale: ionian} # 7.5小節
```

4/4は1区間が四分音符で4ステップ、6/8は1区間が八分音符で2ステップ。拍子分母は1,2,4,8,16,32,64,128を受理する。分母32以上で16分ステップを持たない区間では文字列グリッドを使わず、`placements`/構造化イベントで書く。ドラム・`comp`・`order`の1小節文字列は、端数小節では有効長に合わせて短縮したパターンを要求する。

## 7. 楽器・音源・演奏データ境界

### 7.1 楽器の定義 (`instruments.<id>`)

`instruments.<id>`は、`preset`、`definition`、`external: true`のうち**ちょうど1種類**を指定する。同時指定・省略をエラーとする。ただしSonalloyが解釈しない項目（10.2）だけを持つ楽器は省略として扱い、Sonalloyはその楽器を演奏しない。

```yaml
instruments:
  kick: {preset: DRUM/001-hard-punch-kick, gain_db: -2, role: drums}
  custom: {definition: instruments/custom/definition.json, gain_db: -3, role: lead}
  preview: {external: true, gain_db: 0, role: audition}
```

- `preset`: `<preset_root>/<CATEGORY>/<NNN-kebab-name>/definition.json`へ厳密に解決する。`preset_root`は次の順で決め、見つからなければ`--preset-root`を要求するエラーになる: 1. CLI `--preset-root <PATH>` / 2. 環境変数`SONALLOY_PRESET_ROOT` / 3. Score親ディレクトリを上へ辿って最初に見つかる`presets/`。
- `definition`：Score所在ディレクトリを基点に参照（Instrument Definition JSON）。
- `external: true`：未束縛楽器。CLIの`--bind <instrument_id>=<definition_path>`を**必須**とし、未束縛実行や固定音源への束縛をエラーにする。
- `gain_db`：楽器全体の固定ゲイン（省略時0、有限数）。`role`・`sound`は表示・分類用で演奏に作用しない。
- `pan`：`-1.0`（左）〜`1.0`（右）の有限数（省略時`0.0`）。ステレオ・バランス方式をゲインレーン合算後のパート音声へ一度適用する。
- `gate`・`articulations`・`swing`・`timing_offset_steps`：音価倍率（3.3）・奏法記号（3.4）・スウィング（3.6）・タイミングずれ（3.7）。
- `voicing`・`voicing_floor_midi`・`voicing_ceiling_midi`：和音配置と範囲（4.2、0～127、上限＞下限）。
- `trigger_note_midi`：打撃・単発音を鳴らすMIDIノート（0～127、打撃系使用時は必須）。
- `bass_floor_midi`/`bass_ceiling_midi`：`register: bass`の配置範囲（省略時28～39）。
- `audio_input: <id>`：入力能力のある音源のみ許可。対象不存在・自己参照・循環・チャンネル数不一致はエラー。Part Gainのみ適用後の音声を渡す。
- 技術的な`*_midi`は演奏エンジンのMIDIノート配置用であり、**メロディ・コードの音高を書くための第2記法とはしない。**

### 7.2 演奏Laneと追加ゲイン

同じ楽器の複数フレーズは1つのイベント列に統合する。`lines.<名前>.gain_db`や`placements[].gain_db`など、特定演奏へ追加ゲインを指定した発音は別のLaneで鳴る（実効Gain＝楽器`gain_db`＋追加`gain_db`）。

- 別LaneはVoice・FX状態を共有せず、Sustain・Pitch Bend・Mod Wheel等の制御は基本Laneのみに作用する。
- Stemと`audio_input`は元の楽器ID単位でLaneを合算する。

## 8. 演奏イベント・マクロ・優先規則

### 8.1 イベント形式 (`events`)

セクションの`events`は構造化された演奏イベントのマップのリスト。`kind`は組込み種別であり利用者は追加できない。`macro: <name>`はトップレベル`macros`で宣言された名前を参照し、`kind`と`macro`の同時指定はエラー。全イベントは対象音源を含めてセクション内座標を解決する。

| `kind` | 必須フィールド（共通`at`は省略） | 意味 |
|---|---|---|
| `hit` | `instrument` | 指定Trigger Noteを1回発音。Velocity既定はセクションの`velocity`（省略時100）、長さ既定は1ステップ。 |
| `hits` | `instruments` | 指定音源群への`hit`と同値。順序はリスト順。 |
| `degree` | `instrument`, `degree` | その度数の1音。マクロ本体にも指定可。 |
| `silence` | `duration_steps`または`duration_quarters`; `except_instruments`は任意 | 指定範囲の対象Noteを抑制・切断。明示の例外は抑制しない。 |
| `part_window` | `instrument`, `bars: [start,end]` | 対象楽器の演奏をその小節範囲だけ有効にする。 |
| `dynamics` | `instrument`, `from`, `to`, `duration_steps`または`duration_quarters` | 指定範囲で始まる音符のVelocityに、`from`から`to`へ線形に変わる値を加算する。 |
| `step_roll` | `instrument`, `bar`, `from_step`, `through_step`, `velocity_from`, `velocity_to` | 両端を含む16分連続打撃を線形Velocityで生成。 |
| `drum_roll` | `instrument`, `bars`, `early_hits_per_beat`, `final_hits_per_beat`, `last_bar_end_beat`, `velocity_from`, `velocity_to`, `riser?` | 対象末尾小節で密度が変わるロール。 |
| `sustain_pedal` | `instrument`, `down` | MIDI CC64相当。 |
| `pitch_bend` | `instrument`, `value` | −1～1の既存値。 |
| `mod_wheel` | `instrument`, `value` | 0～1。 |
| `aftertouch` | `instrument`, `value` | 0～1。 |
| `parameter_change` | `instrument`, `parameter`, `native_value` | 音源ParameterのNative値に設定。 |
| `automation` | `instrument`, `parameter`または`control`, `from_value`, `to_value`, `duration_steps`または`duration_quarters` | 音源Parameterまたは演奏コントロールの線形変化。 |

- `silence`は指定区間のNote Onを抑制し、境界にまたがるNoteの音価を切断する。演奏制御とParameter Rampには作用しない。
- `part_window`の`bars: [start, end]`は1始まりの両端を含む小節番号（`last`可、端数小節も1小節）で、半開区間`[start, end)`を表す。そのセクション内で始まるNote Onと点の演奏制御を`[start, end)`に限定し、終端を越えるNoteは境界で切断する。区間開始・終端では状態をニュートラル値または既定値へ戻し、窓をまたぐParameter Rampは許可区間へ切り詰めて補間する。複数指定時は共通部分を使う。
- `dynamics`は`[at, at+長さ)`で始まる対象楽器の全音符（全Lane、ロール含む）に`from`〜`to`（-126〜126の整数）を線形補間した値を加算する。加算後が1〜127外ならエラー。区間はセクション内に収め、同一楽器の重複はエラー。
- `automation`は`parameter`（正規化空間で線形補間）または`control`（`mod_wheel`・`aftertouch`: 0〜1、`pitch_bend`: −1〜1）を指定する。`at`から1/64四分音符ごとと終端に点を生成する。区間内に同一楽器・同一コントロールの別制御があればエラー。
- 同一楽器・同時刻のロールと通常ドラム打撃が衝突すればロールが勝つ。**ロール同士**、同一Parameterへの矛盾、複数の同一楽器Overrideの重複はエラー。
- 同時刻イベントはソース定義順を保持し、Note Off→Control→Note Onの優先順位を適用する。
- 展開順序：パターン・旋律・配置・イベント生成 → ロール優先 → `silence`と`part_window` → `dynamics` → `timing_offset_steps`。
- `drum_roll`の`bars: all`は全小節、正整数は末尾N小節。発音順Velocityを線形補間し、`riser`がある場合はロール開始時に1回打撃する。
- `macro: name`に付いた上書きフィールドはマクロ本体の同名フィールドを置換し、展開結果をイベント型として検証する。循環参照はエラー。
- `drums`、`hit`/`hits`、`step_roll`、`drum_roll`、およびそれらを含むMacroは対象楽器の`trigger_note_midi`（0～127）を必須とする。音声処理専用の楽器（外部音声の受のみ）はダミーNoteを生成しない。

### 8.2 音源能力と演奏制御

- MIDI Noteは0～127、Velocityは1～127。`recommended_range`から外れた発音はwarning。実際のGeneratorのKey/Velocity Triggerと照合して**発音可能Layerがない場合**はerror。
- `parameter_change`/`automation`のParameter IDは楽器のCatalogに存在し、Native値は範囲内の有限数とする。Mod Wheelなどのレンダラー標準演奏操作と、音源固有Parameterの対象は区別する。

## 9. プリセット試聴・プレビュー

### 9.1 Score文法の4用途

| 用途 | 正本 | 基本の音源束縛 |
|---|---|---|
| デモ楽曲 | `score.yaml` | `preset:`/`definition:`で確定 |
| プリセット短いプレビュー | `presets/<CAT>/<NNN-name>/preview.score.yaml` | 基本的に自分の`preset:` |
| プリセット詳細試奏 | `presets/<CAT>/<NNN-name>/showcase.score.yaml` | 基本的に自分の`preset:` |
| 複数音源で共用する試奏 | `presets/common-scores/*.score.yaml` | `external: true`をCLIで束縛 |

ブラウザ向けプレビューは1楽器・1テンポ・1拍子のNote列とし、30〜300 BPM、1〜32音、音楽時間10秒以内に収める。

### 9.2 MIDI取り込み・出力

- `score import-midi`: 主音を`--tonic`（既定C）とし、主音からの半音を`1 #1 2 b3 3 4 #4 5 b6 6 b7 7`の固定綴りで度数へ書き直す（`scale`は付与しない）。取り込めない情報は診断として報告し、音程とタイミングを暗黙にクオンタイズしない。
- `score export-midi`: Conductorと楽器ごとの演奏トラックを持つType 1 MIDIを出力する。テンポ・拍子変更、全ゲインレーンの音符と元のVelocity、Sustain Pedal、Pitch Bend、Mod Wheel、Aftertouchを対象とし、音声GainをVelocityへ変換しない。変換に失敗した場合や出力先が存在する場合は出力先を変更しない。

| 警告対象 | MIDIへの出力 |
|---|---|
| 発音レーンの非ゼロの実効Gain | 音声Gainを省略し、楽器IDとレーンごとのGain値を集約して診断。Velocityは保持 |
| ゲインレーン間で異なる制御列 | 制御の種類ごとに比較し、異なる種類だけ省略。同じ列は1列に統合 |
| Sonalloy独自Parameter | ParameterChange・ParameterRampを省略し、件数とParameter IDを診断 |
| 時間的に重複する同音 | Note On・Note Offの件数と時刻を保持して出力。終了対応が再生側に依存することを診断 |

## 10. 解析・診断・フィールド契約

### 10.1 構文・参照・診断

1. **構文**：YAML 1.2相当。重複キー・未知フィールド・カスタムTag・YAMLエイリアス・マージキー・非文字列Key・曖昧な暗黙キャストはエラー。分数は`"p/q"`で記述し、`1'`や`#4`のようにYAMLの型解釈やコメント記号と衝突する値は引用文字列にする。
2. **参照**：`form.section`、楽器ID、素材、`from`、`macro`、`definition`、`preset`、`audio_input`、`counter.follows`の存在と循環を検証する。`key`、`mode`、`chord_type`のような別名や自然言語の演奏指示は受理しない。楽器IDはStemのファイル名になるため、1〜64文字のASCII英数字・`.`・`_`・`-`で構成して英数字で始め、大文字小文字だけが異なるIDやWindowsの予約名（`con`など）はエラー。
3. **診断**：診断は`path`（Score YAML上の階層）、`section`、`bar`/`step`、入力行/列、`actual`、`expected`を持ち、`warning`と`error`を区別する。Errorがあれば演奏を生成しない。

### 10.2 Sonalloy が解釈しない項目

Scoreには、Sonalloy以外のツールが独自に使う項目が含まれることがある。Sonalloyは次のキーとイベント種別を演奏に使わず、読み飛ばしたうえで種類ごとに1回だけ`SCORE_EXTENSION_IGNORED`警告を返す。列挙にないキーとイベント種別は、入力ミスを黙って無視しないためエラーとする。読み飛ばした項目の内部は検証しない。

- キー: `kits`、`lyrics`、`mix`、`instruments.<id>.source`、`instruments.<id>.effects`、`instruments.<id>.kit`、`instruments.<id>.audio`、`sections.<name>.audio_clips`、`sections.<name>.lines.<line>.lyrics`
- イベント種別: `articulation`、`cc`、`breath`、`voice_dynamics`、`voice_style`
- `automation.control` の `cc:N`（Nは0～127の10進整数）。`mod_wheel`・`aftertouch`・`pitch_bend` は従来どおり演奏に使う

マクロ呼び出しでは、呼び出し側の上書きを適用した実効イベントで拡張かどうかを判定する。実効イベントが拡張になるときだけその呼び出しを読み飛ばし、上書きによって通常のイベントになる呼び出しは演奏する。呼び出されない拡張マクロ定義も、同じ種類の警告に`macros.<name>`として含める。

`source` だけ、または `audio: true` だけを持つ楽器は音楽構造だけを残して演奏対象から外し、その `audio_input` の参照元も外す。`instruments.<id>.sound` と `sections.<name>.notes` は読み飛ばしの対象ではなく、表示用の文字列として検証する。

### 10.3 フィールド契約・文法一覧

| 場所 | 許可された主要キー | 解釈 |
|---|---|---|
| ルート | `meta,form,time_changes,sections,melodies,scale_definitions,drums,comp,arps,counters,macros,instruments,transitions,notes` | 全て定義済み、追加は禁止。`kits,lyrics,mix` は解釈しない |
| `meta` | `title,bpm,meter,tuning,reference_octave,gate,swing,master,fade_out_seconds` | `tuning: 12edo`だけ受理 |
| `meta.swing` / `form[].swing` / `instruments.<id>.swing` | `subdivision,ratio` | グリッドのスウィング |
| `meta.master` | `target_lufs,max_true_peak_db,target_lra_lu` | Master/計測の設定（`target_lufs`, `max_true_peak_db`必須） |
| `form[]` | `section,bars,tonic,scale,bpm,meter,last_bar_quarters,swing` | 曲中の出現と時間正本（`section,bars,tonic`必須） |
| `time_changes[]` | `at_quarters,bpm,over_quarters` | 曲頭からのテンポ変更と漸次変化 |
| `sections.<name>` | `from,chords,chord_changes,lines,drums,comp,pad,arp,counter,placements,add_placements,events,velocity,part_gains,notes` | 1セクションの演奏宣言。`notes`は表示用の文字列、`audio_clips`は解釈しない |
| `sections.<name>.lines.<line>` | `instrument,melody,slice_items,loop,register,tonic,transpose,gain_db` | 旋律パート。`lyrics`は解釈しない |
| `sections.<name>.counter` | `instrument,pattern,follows` | 対旋律の演奏 |
| `sections.<name>.part_gains` | `instrument_id: number` | セクション内の楽器別Gain（dB） |
| `melodies` | 要素のリスト、`{items\|from, replace, relative_to, pickup_steps}`、名前空間辞書 | 相対度数と再利用 |
| `melodies`の要素 | 文字列、`{use,items,shift_steps}` | パターン文字列と参照挿入 |
| `drums.<name>` | `from,parts,overrides` | パートは1小節の文字列またはN小節のリスト |
| `drums.overrides[]` | `when,parts` | 小節条件を満たしたパートへ1小節を置換 |
| `when` | `odd,even,every,bars` のいずれか1つ | 小節条件 |
| `arps.<name>` | `order,voicing_floor_midi,retrigger_each_step` | 和音アルペジオ（`order`セルは非負整数、`.`、`-`） |
| `counters.<name>` | `when,cell` | 無音拍の対旋律（`when: silent_full_beat`） |
| `instruments.<id>` | `preset / definition / external`, `gain_db,pan,role,sound,gate,articulations,swing,timing_offset_steps,voicing,voicing_floor_midi,voicing_ceiling_midi,trigger_note_midi,bass_floor_midi,bass_ceiling_midi,audio_input` | 音源と演奏設定。`source,effects,kit,audio`は解釈しない |
| `instruments.<id>.articulations` | `accent{velocity}`,`staccato{gate,velocity}`,`tenuto{gate,velocity}` | 奏法記号の効き方 |
| `placements[]` | `instrument,at,melody/degree/chord,velocity,duration_steps,duration_quarters,transpose,gain_db,tonic,register,mark,slice_steps,take_notes,voicing,voicing_floor_midi,add_root_degree` | 明示的発音 |
| `add_placements[]` | `placements[]`と同じ | 継承時に配置リストの末尾へ追記 |
| `events[] / macros.<name>` | `kind/macro,at,instrument,instruments,duration_*,parameter,control,native_value,from_value,to_value,from,to,down,value,velocity,degree,except_instruments,bars,bar,from_step,through_step,velocity_from,velocity_to,early_hits_per_beat,final_hits_per_beat,last_bar_end_beat,riser` | 演奏・制御・演出 |
| `transitions[]` | `from,to,method,description` | 解説専用 |
| `notes` | UTF-8文字列 | 制作ノート、演奏に影響しない |

上表はフィールドの所在一覧であり、組合せと排他関係は各章の定義に従う。
