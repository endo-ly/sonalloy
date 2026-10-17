# Sonalloy Score YAML 言語仕様

楽曲・短いプリセットプレビュー・詳細試奏・共通試奏に共通する、1ファイルの音楽記述言語。**このファイルが文法・データ契約・意味論の唯一の正本である。**

仕様対象は12平均律の楽譜と、その演奏への一意な展開。音源そのものの `definition.json` は別の正本であり、本仕様の対象外。

## 目次

1. [目的と基本原則](#1-目的と基本原則)
2. [ファイル構造](#2-ファイル構造)
3. [音高とスケール](#3-音高とスケール)
4. [時間とパターン](#4-時間とパターン)
5. [コードと伴奏](#5-コードと伴奏)
6. [素材の参照・反復・変奏](#6-素材の参照反復変奏)
7. [セクションの構成と配置](#7-セクションの構成と配置)
8. [音源参照・演奏データ境界](#8-音源参照演奏データ境界)
9. [詳細時間・端数小節](#9-詳細時間端数小節)
10. [演奏イベント・マクロ・優先規則](#10-演奏イベントマクロ優先規則)
11. [プリセット試聴・プレビュー・CLI境界](#11-プリセット試聴プレビューcli境界)
12. [解析・診断](#12-解析診断)
13. [フィールド契約・文法一覧](#13-フィールド契約文法一覧)

## 1. 目的と基本原則

1ファイルで**曲の構成、転調、コード進行、メロディ、各楽器の役割、演奏イベント**を俯瞰・編集するためのYAML形式。主な編集者は人間とAIエージェント。Sonalloyのデモ楽曲制作とプリセット試奏における**唯一の楽譜正本**として扱い、Scoreをメモリ内の演奏計画へ直接展開してSonalloyが演奏・書き出しを行う。

- **音符とコードのルートは相対度数で統一する。** 度数の `1` は現在のセクションの主音。同主長調の度数を固定の物差しとし、`b` / `#` で半音単位の変更を明示する。
- **スケールは作曲上の文脈であり、音符を自動変換しない。** AエオリアンとAドリアンでも `3` は常にAからの長3度。短3度を使うなら `b3` と書く。
- **楽曲の全体像を先に置く。** `meta` → `form` → `sections` → 音楽素材 → `instruments` の順に記述する。曲の並び・小節数・主音・スケールは `form` だけが所有する。
- **短い記法の守備範囲を限定する。** 音符とリズムは拍のグリッド表現を使い、特別な長さ・位置・奏法・演出は構造化されたフィールドに記述する。
- **意味を推測して実行しない。** 未定義の語句、欠落した参照、矛盾した編集、自然言語による演奏指示は検証エラーにする。`notes`、`transitions[].description`、`role`、`mark`は説明・表示用で、演奏に作用しない。
- **12平均律専用。** 度数とコード構成音は12平均律の半音で定義する。

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
  title: Example
  bpm: 120
  meter: 4/4
  tuning: 12edo
  reference_octave: 4
  gate: 0.92
  master:
    target_lufs: -12
    max_true_peak_db: -1
    target_lra_lu: 7
```

`bpm` は四分音符毎分、`reference_octave` は音符 `1` が属するオクターブ（C4＝MIDIノート60）、`gate` は生成音符の基本音価へ掛ける倍率（0より大きく1以下）。`master` の`target_lufs`と`max_true_peak_db`はレンダラーの達成条件であり、外れた出力はエラーになる。`target_lra_lu`は測定値との比較目標で、目標に到達させるマスタリング処理は行わない。測定結果・差分・計測できない理由をレポートし、差が0.5 LUを超えるか計測できない場合はwarningを返す。

`form` の各項目は `{section, bars, tonic}` を必須とし、必要なら`scale`、`bpm`、`meter`、`last_bar_quarters`、`swing`を追加できる。テンポと拍子は曲頭で`meta`の値を採用し、`form`で値を変更した場合は後続セクションにも引き継ぐ。`section` は `sections` のキー、`bars` は正の整数で、最後の端数小節も1小節として数える。`tonic` は `C` / `F#` / `Bb` などの音名、`scale` は`ionian` / `dorian` / `phrygian` / `lydian` / `mixolydian` / `aeolian` / `locrian` などの既知の音階名とする。`harmonic_minor`、`melodic_minor`、`major_pentatonic`、`minor_pentatonic`、`whole_tone`も組込み名として扱う。組込み以外はトップレベルの`scale_definitions`に、名前をキー、度数トークンの配列を値として登録する。未登録名はエラーとする。`scale`は任意であり、省略した場合はスケール未指定として扱い、音符・コード・演奏イベントは変化しない。

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

複数回出る同名の `section` は同じ演奏定義を再利用する。各出現で異なる転調が可能だが、各出現の`bars`と、そのセクションが参照する素材長が一致するかを検証する。

次は第3章以降の記法を組み合わせた完成例である。`main`は`intro`の和声とパッドを`from`で継承してリード・ドラム・ベースを加え、2回再生される。`last`は`main`を継承し、終止のコードとフックの最終小節だけを置き換える。Scoreはこの例と本仕様だけで記述でき、ほかのScoreファイルを参照する必要はない。

```yaml
meta:
  title: Example
  bpm: 174
  meter: 4/4
  tuning: 12edo
  reference_octave: 4
  swing: {subdivision: 16, ratio: "27/50"}
  master: {target_lufs: -12, max_true_peak_db: -1}
form:
  - {section: intro, bars: 4, tonic: F, scale: aeolian}
  - {section: main, bars: 4, tonic: F, scale: aeolian}
  - {section: main, bars: 4, tonic: F, scale: aeolian}
  - {section: last, bars: 4, tonic: F, scale: aeolian}
sections:
  intro:
    chords: ["1:m9", "b6:maj7", "4:m7", "5:7sus4 5:7(b9)"]
    pad: {instrument: pad, pattern: hold}
    drums: hats
    velocity: 84
    placements:
      - {instrument: riser, degree: "1,", at: {bar: 3, step: 1}, duration_quarters: "8", velocity: 100}
    events:
      - {kind: drum_roll, instrument: snare, bars: 1, early_hits_per_beat: 2, final_hits_per_beat: 4, last_bar_end_beat: 4, velocity_from: 40, velocity_to: 118}
  main:
    from: intro
    lead: {instrument: lead, melody: hook}
    drums: break
    bass: {pattern: roll}
    velocity: 100
    placements: []
    events:
      - {kind: hits, instruments: [crash, kick], at: {bar: 1, step: 1}, velocity: 120}
  last:
    from: main
    chords: ["1:m9", "b6:maj7", "4:m7", "1:m9"]
    lead: {instrument: lead, melody: hook_end}
melodies:
  hook:
    - "5 . b3' . | 1' - . 5 | b7 . 1'~ 2' | b3' - . ."
    - "b6 . 1' . | [b3' 4' b3'] | 1' - b6 - | 5 - . ."
    - ". . 4 b6 | 1' . . 4' | b3' - 1' . | b6 - 4 ."
    - "5 . 7 . | 2' . 4'@116 - | - - b3' . | 2' - . ."
  hook_end:
    from: hook
    replace: {4: "1' - - - | - - . . | . . . . | . . . ."}
drums:
  hats:
    parts:
      hat: "..x. | ..x. | ..x. | ..xo"
  break:
    from: hats
    parts:
      kick: "X... | .... | ..X. | ...."
      snare: ".... | X..o | .... | X..."
    overrides:
      - {when: {bar: last}, parts: {snare: ".... | X..o | ..o. | XoXx"}}
bass:
  roll:
    relative_to: chord_bass
    parts:
      bass: "1@116 - . 1 | . . 1' . | . 1 - b7, | 1 . T ."
    choices:
      T: {odd: "b3", even: "5,"}
instruments:
  kick: {preset: DRUM/001-hard-punch-kick, gain_db: -6, trigger_note_midi: 38}
  snare: {preset: DRUM/003-tone-snare, gain_db: -3, trigger_note_midi: 38}
  hat: {preset: DRUM/019-needle-closed-hat, gain_db: -6, pan: 0.2, trigger_note_midi: 42}
  crash: {preset: DRUM/008-wide-wash-crash, gain_db: -10, trigger_note_midi: 49}
  riser: {preset: FX/001-noise-riser, gain_db: -12}
  bass: {preset: BASS/007-reese-bass, gain_db: -4}
  pad: {preset: PAD/001-warm-analog-pad, gain_db: -10, voicing_floor_midi: 53}
  lead: {preset: PLUCK/002-fm-pluck, gain_db: -4, gate: 0.6}
```

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

`tonic: A, scale: aeolian`でもA＝`1`、C＝`b3`、F＝`b6`、G＝`b7`。AドリアンならF#＝`6`。**実音を決めるのは`tonic`と音符トークン**であり、`scale`は作曲判断や表示・分析の参考とする。スケールに含まれない度数もそのまま発音する。

### 3.2 音域

`reference_octave: 4` の場合、セクション`tonic: Eb`では `1`＝Eb4（MIDI63）、`7,`＝D4（MIDI62）、`1'`＝Eb5（MIDI75）。`tonic: A`なら `1`＝A4（MIDI69）。楽器ごとの低音域などは、音程表記を変える代わりに`transpose`（半音単位）または音源別の演奏音域で制御する。

`voicing_floor_midi` は和音の配置に使う**技術的な下限値**であり、メロディの音符記法ではない。コード構成音は度数から決め、その音域配置だけをMIDIノート番号で指定する。

## 4. 時間とパターン

### 4.1 共通時間軸

演奏イベントの時間は**有理数の四分音符**で計算し、出力段階でTickへ変換する。標準的な4/4では1小節16ステップ、1ステップ＝16分音符。`bar`と`step`は**1始まり・セクション内相対**とする。`{bar: last}`は末尾の小節、`step: 13`は4拍目の先頭。

拍子`n/d`では、1小節は `n × 4/d` 個の四分音符に相当する。パターンの`|`は、**分母の音符1個分の記譜区間**を分ける。通常のグリッドはその区間の長さに応じた16分音符の数を使う（4/4なら4マス×4区間、6/8なら2マス×6区間）。この区切りは記譜上のもので、強拍を意味しない。連符の分数長は丸めず保持する。

`form`の開始位置を順に足すことで曲全体の小節位置と経過時間が確定する。曲中のテンポ・拍子変更は`form`のセクション開始時点、または途中のテンポだけを扱う`time_changes`へ記述し、音符の四分音符基準の位置は維持する。

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

メロディ・ベース・対旋律の度数には、`3@110`のように整数1～127のVelocityを付けられる。省略時はセクションの`velocity`（既定100）を使う。臨時記号とオクターブも組み合わせられ、`b3'@90`のように書く。

音符の基本長は、後続の`-`を含めた長さで決める。Gateは楽器の`gate`、`meta.gate`、既定値1の順に選び、基本音価に一度だけ掛ける。値は0より大きく1以下とする。ドラム・コード伴奏・アルペジオにも同じ優先順位を使う。

音符末尾の`~`は次音へのレガート接続で、`1~ 3@110~ 5 -`のように使う。接続元はGateによる短縮を受けず、次音の開始まで保持して短く重なる。重なりは接続先の発音中に収める。休符を越える接続と接続先のない`~`はエラー。単音レガート・ポルタメントの音高遷移は音源定義の演奏設定に従い、ポリフォニック音源では音符の重なりとして演奏する。独立した同時発音や精密な音価は`placements`の構造化イベントを使用する。

### 4.3 ドラム文字列

```yaml
drums:
  four:
    parts:
      kick: "X... | X... | X... | X..."
      hat:  "XxXx | XxXx | XxXx | XxXx"
```

ドラム1マスは`.`（休み）、`o`（弱）、`x`（通常）、`X`（アクセント）。基本Velocityは`o=64`、`x=96`、`X=120`、発音長は1グリッドマス。`[oooxxx]`なら1区間を6等分し、各文字で同じ長さの音を発音する。音の種類は`parts`のキーが参照する音源で指定する。ドラム以外のメロディ・ベース・コード伴奏のVelocityとは独立する。

### 4.4 スウィング

`meta.swing: {subdivision: 8, ratio: "2/3"}`で8分スウィング、`subdivision: 16`で16分スウィングを指定する。`ratio`は0より大きく1より小さい有理数で、`1/2`は均等、`2/3`は2対1。`form[].swing`は、そのセクションの出現だけで全体設定を上書きする。

メロディ、ドラム、ベース、コード伴奏、アルペジオ、対旋律のグリッド位置と基本音価へ適用する。小節境界を固定し、小節末の不完全な組は均等なまま扱う。連符は組の開始・終了を変換して内部を等分する。変換後に継続・Gate・レガートを解決する。`placements`と`events`の明示位置・音価は保持する。

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

例：`tonic: Eb`で `5:7/7` はB♭7/D、`tonic: A`で `1:m` はAm、`tonic: C`で `#1:dim7` はC#dim7。`/`の後も**コードルートからの音程ではなくセクション主音からの度数**。コードのルートと構成音がスケール外でも許容する。

1小節のコードが1個なら全小節、2個なら前後半の同じ長さで割り当てる。より細かい和声変化はセクション内`chord_changes: [{at: ..., chord: ...}]`を用いる（時刻の競合はエラー）。

### 5.2 ベースの度数と基準音

ベースもメロディと同じ度数・臨時記号・オクターブ・Velocity・レガート記法を使う。

```yaml
bass:
  groove:
    relative_to: chord_bass
    parts:
      bass: "1 - 5@110 . | b3 - . . | 1 - - - | 5 - . ."
```

`relative_to`の省略時または`tonic`ではセクション主音、`chord_bass`では発音位置のコードベース音を基準にする。分数コードはスラッシュ指定の音を使う。コード参照を`NC`上で使うとエラーになる。

`c1`、`c3`、`c5`、`c7`、`c9`のように`c`を付けたTokenは、発音位置のChord構成音を指定するBass専用記法です。`c3`はMajor / Minorなど現在のChordが持つ実際の3度へ解決し、`relative_to`の基準音ではなくChord Rootを基準に配置します。`c1`～`c13`と臨時記号、オクターブ記号を使えます。指定した構成音がない、同じ度数が曖昧、またはChordが`NC`の場合はErrorです。Velocity（`@1`～`@127`）とLegato（`~`）は通常のBass Tokenと共通です。

基準音を`bass_floor_midi`～`bass_ceiling_midi`（既定28～39）に配置してから、度数とオクターブ移動を加える。度数適用後の音高はその音域へ折り返さず、最終MIDIノート0～127を検証する。

`choices.T`のような名前付きトークンは、小節の奇数・偶数に従って置換してから音符を解析する。奇数・偶数はセクション内1始まり。`bass_overrides`と`add_parts`も同じ基準音・音符記法を使う。

### 5.3 伴奏・アルペジオ・対旋律

`voicing: ["1", "3", "7", "9'"]`で演奏する構成音とオクターブを選べる。`3`はコード品質に含まれる3度を指し、短三和音では短3度になる。同じ度数の変化音が複数ある場合は`b9`や`#9`で特定する。含まれない音や曖昧な指定はエラーとする。

指定先は`instruments.<id>.voicing`、`sections.<id>.comp.<instrument>`の`{pattern, voicing}`形式、`pad`、`arp`、`placements[]`。使用箇所、楽器既定、自動配置の順に優先する。`comp`の値はパターン名の文字列でも指定できる。

明示配置では、下限以上の指定ベース音からコードルートの配置基準を決め、各構成音の複合音程とオクターブ移動を適用して音高順に並べる。配列から外した構成音は演奏せず、分数コードのベースは最低音として保持する。オクターブ移動後の構成音が指定ベースより低くなる場合はエラーとし、音高を自動補正しない。未指定時は以下の自動配置を使う。

- `comp` の `x` は、現在のコードをその位置で発音。`-`で保持、`.`で休止。組込みパターン`hold`は小節頭のコードを1小節持続し、1小節中にコードが替わった地点で前の和音を終了して新しい和音を発音し直す。`hold`は`comp`での定義・上書きができない予約名である。
- `instruments.*.voicing_floor_midi`は、自動ボイシングのベース配置下限。下限以上で最も低い指定ベース音を最低音とし、1～7度の構成音はその直上の最も近い位置に配置する。8度以上の複合音程は、ベース以上の最も低いコードルートを基準として、ルートからの半音数を保持する。例えば下限48の`1:9`は`48,52,55,58,62`となる。同じ音高の重複を除き、昇順に整列する。ベースが構成音でない場合は、ベースと構成音全てを含める。
- `arps.<name>.order`の各セルは、上記ボイシングの構成音を下から並べた音列のインデックス（非負整数）か、`.`（休符）、`-`（直前の音の継続）である。`-`は直前に発音した同じ音高の音価を延長し、継続中にコードが変わっても発音済みの音高は変わらない。小節をまたぐ継続も許可し、先行する発音がない`-`はエラー。コード音が4個未満の場合は構成音を上オクターブへ反復して必要数を作る。`retrigger_each_step`の省略時または`true`では数値セルごとに再発音し（`-`は常に音価を延長）、`false`では隣接する同音の数値セルを結合してからGateを適用する。`.`はどちらの場合も休符として発音を区切る。Gateは音価が確定した後に一度だけ掛かる。
- `counters.<name>.when: lead_silent_full_beat` は、その拍の**全期間**にリード音が鳴っていない場合だけ、`cell`の各セルをその1拍に等分配置する。`.`は休符、`-`は直前の音の継続で、先行音のない継続はエラー。リードの休みは、重ねたリードも含めてNote Off・Gate処理後に判定する。

## 6. 素材の参照・反復・変奏

定義の再利用には次の3形式を使用する。

| 形式 | 用途 | 解決方法 |
|---|---|---|
| `from: <name>` | 辞書内の定義を継承 | 継承先のトップレベルフィールドを単位として上書き。省略フィールドは継承。循環参照は禁止 |
| `replace: {5: "..."}` | `melodies`の指定小節だけ変更 | 展開済みメロディの5小節目をその文字列へ置換。小節番号は1始まり |
| `{use: verse, bars: [1, 2], shift_steps: -2}` | メロディの区間を抜き出し時間移動 | 元の1～2小節を2小節分の領域として挿入し、音符イベントだけ指定数ステップ平行移動 |

`sections`の`from`は`chords`、`lead`、`comp`、`events`などの**フィールドごと**に上書きする。`placements`は置換、`add_placements`は継承・置換後の配置リストの末尾へ順番どおり追記する。`add_placements`だけを持つ独立セクションでは、空の配置リストへ追加する。複数段継承では各段階で`add_placements`を実際の`placements`へ統合するため、`A → B → C`の追加結果は`A → B → C`の順番で保持され、解決済みのセクションに`add_placements`は残らない。統合後の配置も定義元は追跡され、その配置に関する診断の`path`は継承先のセクションではなく**配置を定義した元のYAML位置**（`sections.<元のセクション>.placements[i]`または`<そのセクション>.add_placements[i]`）を指す。`events`配列と`comp`などのマップは追記ではなく置換。`drums`の`from`は`parts`を楽器ごとに統合し、`overrides`を継承後に追記する。`melodies`の`from`は小節列を継承し、`replace`が指定小節を置換する。継承後の参照は全て検証する。

`melodies.answers.gap`のように`.`で区切った名前は名前空間付きの素材参照となる。辞書の葉には小節文字列のリストまたは`from`/`replace`を持つ継承定義を置き、それ以外の辞書は名前空間として扱う。

`melodies`のリストに文字列と`use`を混在できる。`use`の負方向シフトで配置領域の先頭より前に出るイベントは、セクション境界を越える意図が明示された場合だけ許す。出力曲の時刻0より前には配置できない。文字列は1小節、`use`は指定された小節数だけ挿入領域を消費する。時間移動したイベントがその領域の外に出る場合は、参照元／先の長さと配置可能範囲を検証する。`lead.slice_bars: [5,8]` はメロディの5～8小節だけを選び、セクションの1小節目へ配置する。

`drums.overrides` と `sections.bass_overrides` は、共通の小節条件`when`を使う。指定できるのは`when: {odd: true}`、`when: {even: true}`、`when: {every: 4}`、`when: {bar: 5}`、`when: {bar: last}`のいずれか1つ。小節番号はセクション内1始まりで、`every: n`はnの倍数小節（n, 2n, ...）に一致する。条件に一致した小節の該当楽器のパターンを完全に置換する。別楽器の変更は共存できるが、**同じ小節・同じ楽器への複数の上書きが同時成立した場合はエラー**。

## 7. セクションの構成と配置

### 7.1 セクションの基本フィールド

| フィールド | 意味 |
|---|---|
| `chords` | 小節数と同じ長さのコード列 |
| `lead` | `{instrument, melody, slice_bars?, layers?}`。基本メロディをその楽器で演奏 |
| `drums` | `drums`内のパターン名 |
| `bass` | `{pattern, add_parts?}`。`add_parts`は新しい楽器のベースパターンを追加 |
| `bass_overrides` | `when`条件に一致した小節のベース楽器別パターンの上書き。条件は`drums.overrides`と共通（第6章） |
| `comp` | `{楽器名: パターン名}`または`{楽器名: {pattern, voicing}}` |
| `pad` | `{instrument, pattern, voicing?}`。組込みパターン`hold`はコードを1小節持続し、コード変更時に再発音 |
| `arp` / `counter` | `{instrument, pattern}`。`arp`は`voicing`も指定できる。それぞれの名前付き規則を参照 |
| `placements` | `{instrument, at, ...}`の配列。特定の音符・コード・切り出しフレーズを指定 |
| `add_placements` | `placements[]`と同じ要素の配列。継承時に既存の配置リストの末尾へ追記 |
| `events` | 構造化された演出・制御・マクロの配列（詳細は第10章） |
| `velocity` | メロディ・コード・ベース等の既定MIDI Velocity（1～127） |

`lead.layers` は同じメロディを別楽器で重ねる配列。`transpose`は半音、`gain_db`はその演奏の追加ゲイン。`lead`から派生した音符の音高を計算してから移調する。`bass.add_parts`の文字列は通常の`bass`文字列と同じ文法を使う。

### 7.2 placements

各`placement`は `instrument` と `at: {bar, step}`、および次のいずれか1つの演奏対象を持つ。

- `melody`：既存の素材名。`slice_steps: {start, count}`で開始位置と切り出すステップ数、`take_notes`で先頭からの発音数を指定できる。切り出した音符イベントの時刻は選択範囲の先頭を0として`at`に配置する。
- `degree`：指定した度数の1音。`duration_steps`、`velocity`を付与できる。
- `chord`：指定した度数コードの和音。必要なら`voicing`、`voicing_floor_midi`、`add_root_degree`を指定する。
- `bass_note`：主音を楽器のベース音域へ配置してから度数を加えた1音。

追加の`transpose`（半音）、`gain_db`（dB）、`mark`（タイムライン表示用の識別子）は許可する。`duration_steps`は正の有理数として解析する。YAMLの数値`1.5`は3/2として正確に扱う。イベントがセクション境界を越える場合は、明示された長さと出力先の許容範囲で検証する。

## 8. 音源参照・演奏データ境界

### 8.1 楽器の定義

`instruments.<id>`は、`preset`、`definition`、`external: true`のうち**ちょうど1種類**を指定する。同時指定・省略をエラーとする。

```yaml
instruments:
  kick: {preset: DRUM/001-hard-punch-kick, gain_db: -2, role: drums}
  custom: {definition: instruments/custom/definition.json, gain_db: -3, role: lead}
  preview: {external: true, gain_db: 0, role: audition}
```

- `preset`：`<preset_root>/<CATEGORY>/<NNN-kebab-name>/definition.json`へ厳密に解決。`preset_root`はCLIの`--preset-root <PATH>`を優先し、未指定なら環境変数`SONALLOY_PRESET_ROOT`、さらに未指定ならScoreファイルの親ディレクトリを上へ辿って最初に見つかる`presets/`を使う。いずれでも見つからなければ、`--preset-root`の指定を要求するエラー。存在しないID、プリセットルート外の参照はエラーとする。プリセットの識別子は実際のディレクトリ名と対応し、別の音源へ置き換えて解決しない。
- `definition`：Score所在ディレクトリを基点に参照。解決先はInstrument Definition JSON。`definition.json`の`schema_version`等は既存Coreに従う。
- `external: true`：試奏用Scoreなどの未束縛楽器。CLIの`--bind <instrument_id>=<definition_path>`を**必須**とし、未束縛での実行をエラーにする。固定`preset`・`definition`を持つ楽器への束縛を拒否する。
- `gain_db`：楽器全体の固定ゲイン（省略時0、有限数）。`role`は表示・分類だけに利用。
- `pan`：楽器単位の左右定位。`-1.0`が左、`0.0`が中央、`1.0`が右で、許容範囲は`-1.0..=1.0`の有限数、省略時は`0.0`。同じ楽器の複数ゲインレーンには同一のPanが適用される。中央と省略時は無加工であり、振った側と反対のチャンネルを段階的に減衰させるステレオ・バランス方式（`pan <= 0`は左`1`／右`1 + pan`、`pan > 0`は左`1 - pan`／右`1`）。Panはゲインレーン合算の直後に一度だけ適用され、最終ミックス・Stem・`audio_input`へ渡されるパート音声は同じ定位を持つ。
- `voicing_floor_midi`：和音の最下声配置下限（0～127）。`trigger_note_midi`：Drum/FXなど、音程を指定しない打撃・単発音を鳴らすMIDIノート（0～127）。打撃・単発音を生成する場合は必須であり、音源ごとに明示する。`bass_floor_midi`/`bass_ceiling_midi`はベース参照音の標準配置範囲（省略時28～39）。
- `audio_input: <id>`：ほかの楽器のステレオ出力を音声入力として受ける。入力能力のある音源だけ許可。対象不存在、自己参照、循環、外部入力のチャンネル数不一致はエラー。音源から入力へ渡る音声にはその楽器のPart Gainを適用し、グローバルフェード・マスターを含めない。
- 技術的な`*_midi`は演奏エンジンのMIDIノート配置用である。**メロディ・コードの音高を書くための第2記法とはしない。**

### 8.2 演奏Laneと追加ゲイン

同じ楽器の複数フレーズは1つのイベント列に統合する。`lead.layers[].gain_db`や`placements[].gain_db`など、特定演奏へ追加ゲインを指定した発音は、同じ音源を参照する別のLaneで鳴る。実効Gain＝楽器`gain_db`＋追加`gain_db`。別Laneは音源のVoice・FX状態を共有しないため、モノフォニックのレガートやディレイの残響は通常Laneとつながらない。Sustain・Pitch Bend・Mod Wheelなどの制御イベントは基本のLaneだけに作用する。Stemと`audio_input`は元の楽器ID単位でLaneを合算する。

### 8.3 出力・マスター

`meta.fade_out_seconds`を楽曲全体のフェードアウト（省略時0）に使用する。MP3出力時、`meta.master.max_true_peak_db`が設定されており、MP3エンコード後の測定`true_peak_db`がその上限を超える場合は、測定値・上限・超過量を含むwarningを返す（対象は`meta.master.max_true_peak_db`。上限以内ならwarningは出ない）。

`sample_rate`、`block_size`、`tail_seconds`、WAV/MP3/Stem/Premasterの出力先は**コマンド実行条件**であり、楽譜には持たせない。

## 9. 詳細時間・端数小節

### 9.1 時間単位と範囲

イベントの位置と長さの詳細規則を定める。曲中のテンポ変更は`time_changes`へ記述する。

```yaml
time_changes:
  - {at_quarters: "13/2", bpm: 140}
```

`at_quarters`は曲頭からの四分音符数（非負）。同時刻の重複・`form`側の同時刻テンポ指定との衝突はエラー。曲終端を超える位置もエラー。

**位置**：`at: {bar: 1, step: 1}`は、セクション内・1始まりであり、`step`は16分音符単位。4/4は1～16、6/8は1～12。`at: {bar: last, step: 1}`も許す。グリッドに乗らないイベントは`at: {bar: 1, offset_quarters: "1/3"}`を使う。`offset_quarters`はその小節頭からの0始まりの量。`step`と`offset_quarters`の併用は禁止。

**長さ**：`duration_steps`は16分音符数であり、正の整数・有限10進数・分数文字列を受理する。`duration_quarters`は四分音符数で、同じく正の有理数。両方の指定は禁止。パターンの`gate`が掛かるのは自動計算音価のみ。`placements`や明示ノートの`duration_*`にはGateを再適用しない。

**比率の解析**：整数・有限小数・`p/q`は丸めずに分数として扱う。分母0と負の値はエラー。全イベントを整数Tickで表すPPQは自動で決まり、上限32767を超える細かい分数の組合せはエラーになる。

### 9.2 部分小節

`form[]`は`last_bar_quarters: "p/q"`を許す。`bars`は**最後の短い小節を含む正の整数**。最後の小節長を`L`、通常小節長を`B=n*4/d`とすると、`0 < L < B`を要求し、セクション長は`(bars-1)*B + L`。最終小節の発音位置は`[0,L)`であり、明示ノートの終端は曲終端以下とする。Sustain解除などの瞬時制御は末尾小節の`offset_quarters: L`でセクション終端にも配置できる。短い小節の素材は**実際の有効長**を消費し、端数分のトリミングや無音の補填は行わない。

```yaml
form:
  - {section: fx_preview, bars: 8, last_bar_quarters: "2", tonic: C, scale: ionian}
```

これは4/4の通常7小節＋最後2四分音符＝7.5小節。

### 9.3 記法の単位と連符

4/4は1つの`|`区間が四分音符で4スロット、6/8は1つの`|`区間が八分音符で2スロット。1/4より短い分母単位の拍子でも、16分以上の長さを持つ区間には正確なスロット数を割り当てる。分母が32以上で16分スロットを持たない区間では文字列グリッドを使わず、`placements`/構造化イベントで書く。拍子の分母として1,2,4,8,16,32,64,128を受理する。`|`は区間の境界であり、**各区間を内部的に16分スロットへ分割する**。

`[1 2 b3]`および`[oxX]`は、1区間全体を3等分する連符とする。括弧単位は単独で区間を構成し、その同一区間に通常の4スロット等を並置しない。連符内の休符・Tieも同じ内部文法に従う。4/4の1小節ではパイプで4区間、6/8では6区間を必須とする。短い最終小節は最後の有効区間に合わせて短縮したパターンを要求する。16分境界以外の最終切断では構造化配置で厳密に指定する。

## 10. 演奏イベント・マクロ・優先規則

### 10.1 イベント形式

セクションの`events`は構造化された演奏イベントのマップのリスト。`kind`は言語に組み込まれたイベント種別であり、利用者は種別を追加できない。`macro: <name>`は、そのScore YAMLのトップレベル`macros`で明示的に宣言された名前を参照し、`kind`と`macro`の同時指定はエラーとする。マクロ名やメロディ素材名は言語が内蔵する予約語ではなく、利用者が定義する識別子である。全イベントは、対象音源を含めてセクション内座標を解決する。

| `kind` | 必須フィールド（共通`at`は省略） | 意味 |
|---|---|---|
| `hit` | `instrument` | 指定Trigger Noteを1回発音。Velocityの既定値はセクションの`velocity`（省略時100）、長さの既定値は1ステップ。 |
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
- `part_window`の`bars: [start, end]`は**1始まりの小節番号**であり、`start`と`end`の両小節を含む。`bars: [2, 3]`は2小節目と3小節目の両方を対象とし、`last`は最後の小節、端数小節（`last_bar_quarters`で短くなった小節）も通常の1小節として数える。内部では、開始小節の先頭から終了小節の末尾までを`[開始時刻, 終了時刻)`の半開区間として扱う。
- `part_window`は対象楽器の、そのセクション内で始まるNote Onと点の演奏制御を半開区間`[start,end)`に限定する。前のセクションで始まり当該セクションへ持続するNoteは、このセクションの窓では変更しない。窓に含まれるNoteの音価が終端を越える場合は境界で切断し、点の演奏制御は区間内のイベントだけを残す。区間開始では以前の状態をニュートラル値またはParameter定義の既定値へ戻し、区間終端でも同じ既定状態へ戻して状態を後続セクションへ漏らさない。窓をまたぐParameter Rampは許可区間との交差部分へ切り詰め、両端値を元の線形Rampから補間する。窓が空の場合はセクション中に制御を残さない。同じ楽器・セクションへ複数の窓を指定した場合は許可区間の共通部分を使う。
- 同一楽器・同時刻の`drum_roll`/`step_roll`と通常ドラム打撃が衝突すればロールが勝つ。**ロール同士**、同一Parameterへの異なる値・重複矛盾、複数の同一楽器Overrideが重なればエラー。`silence`はNoteだけを抑制・切断し、`part_window`は対象楽器のNoteと演奏制御を制限する。
- Note、Sustain、Bend、Wheel、Aftertouch、ParameterなどのEventが同時刻に起きる場合は、ソース定義順を保持し、Note Off→Control→Note Onの基本優先順位を適用する。
- `kind: drum_roll`で`bars: all`はセクションの全小節、正整数なら末尾N小節。発音順Velocityを指定両端間で線形補間し、整数値への丸め方は`round half away from zero`とする。末尾の`riser`がある場合はロール開始時に1回打撃する。
- `macro: name`に付いた上書きフィールドはマクロ本体の同名フィールドを置換し、展開結果はイベント型として検証する。マクロの循環参照はエラーとする。
- `drums`、`hit`/`hits`、`step_roll`、`drum_roll`、およびそれらを含むMacroは、対象楽器の`trigger_note_midi`を必須とし、省略やMIDI 0～127外の値はエラーになる。`recommended_range`は音域の目安であり、Trigger Noteの許容範囲ではない。音符を発音しない音声処理専用の楽器（外部音声の受のみ）も許可し、ダミーNoteは生成しない。

### 10.2 音源能力と演奏制御

- MIDI Noteは0～127、Velocityは1～127。`recommended_range`から外れた発音はwarning。実際のGeneratorのKey/Velocity Triggerと照合して**発音可能Layerがない場合**はerror。シンセによっては音程無関係のTrigger Noteを必要とする。
- `parameter_change`/`automation`のParameter IDは楽器のCatalogに存在し、Native値は範囲内の有限数とする。Mod Wheelなどのレンダラー標準演奏操作と、音源固有Parameterの対象は区別する。

## 11. プリセット試聴・プレビュー・CLI境界

### 11.1 同じScore文法を4用途に使用

| 用途 | 正本 | 基本の音源束縛 |
|---|---|---|
| デモ楽曲 | `score.yaml` | `preset:`/`definition:`で確定 |
| プリセット短いプレビュー | `presets/<CAT>/<NNN-name>/preview.score.yaml` | 基本的に自分の`preset:` |
| プリセット詳細試奏 | `presets/<CAT>/<NNN-name>/showcase.score.yaml` | 基本的に自分の`preset:` |
| 複数音源で共用する試奏 | `presets/common-scores/*.score.yaml` | `external: true`をCLIで束縛 |

短いプレビューと詳細試奏が異なる音楽なら、異なるScoreでよい。演奏データはScoreに記録し、音源の名前、タグ、音域、音色は`definition.json`に定義する。

ブラウザ向けプレビューは1楽器・1テンポ・1拍子のNote列とし、30〜300 BPM、1〜32音、音楽時間10秒以内に収める。

### 11.2 CLIとMIDI取り込み

コマンドのOptionとReportは[CLIリファレンス](cli.md)を参照する。Scoreに関わる公開入口は`score init/validate/inspect/import-midi/export-midi`、`render score`、`audition score`。

`score import-midi`は主音を`--tonic`（既定C）とし、主音からの半音を`1 #1 2 b3 3 4 #4 5 b6 6 b7 7`の固定綴りで度数へ書き直す。`scale`は付与しない。取り込めない情報は診断として報告し、音程とタイミングを暗黙にクオンタイズしない。

### 11.3 MIDI出力

`score export-midi`は、Conductorと楽器ごとの演奏トラックを持つType 1 MIDIを出力する。テンポ・拍子変更、全ゲインレーンの音符と元のVelocity、Sustain Pedal、Pitch Bend、Mod Wheel、Aftertouchを対象とする。音声GainをVelocityへ変換しない。

演奏トラックの0始まり番号を`i`として、ポート`i / 16`、チャンネル`i % 16`を割り当てる。ポート0は既定とし、追加ポートにはMIDI Port情報を記録する。最大128ポートを使用できる。

| 警告対象 | MIDIへの出力 |
|---|---|
| 発音レーンの非ゼロの実効Gain | 音声Gainを省略し、楽器IDとレーンごとのGain値を集約して診断。Velocityは保持 |
| ゲインレーン間で異なる制御列 | 制御の種類ごとに比較し、異なる種類だけ省略。同じ列は1列に統合 |
| Sonalloy独自Parameter | ParameterChange・ParameterRampを省略し、件数とParameter IDを診断 |
| 時間的に重複する同音 | Note On・Note Offの件数と時刻を保持して出力。Note IDを持たないMIDI 1.0では、終了対応が再生側に依存することを診断 |

警告は対象楽器を示し、通常表示とJSON成功Reportの`diagnostics`から取得できる。JSONは`status`と`output`も返す。変換に失敗した場合や出力先が存在する場合は、その出力先を変更しない。

## 12. 解析・診断

1. **構文**：YAML 1.2相当。重複キー・未知フィールド・カスタムTag・YAMLエイリアス・マージキー・非文字列Key・曖昧な暗黙キャストはエラーとする。分数は`"p/q"`で記述し、`1'`や`#4`のようにYAMLの型解釈やコメント記号と衝突する値は引用文字列にする。
2. **参照**：`form.section`、楽器ID、素材、`from`、`macro`、`definition`、`preset`、`audio_input`の存在と循環を検証する。`key`、`mode`、`chord_type`のような別名や自然言語の演奏指示は受理しない。楽器IDはStemのファイル名になるため、1〜64文字のASCII英数字・`.`・`_`・`-`で構成して英数字で始め、大文字小文字だけが異なるIDやWindowsの予約名（`con`など）はエラーとする。
3. **診断**：診断は`path`（Score YAML上の階層）、`section`、`bar`/`step`、入力行/列、`actual`、`expected`を持ち、`warning`と`error`を区別する。Errorがあれば演奏を生成しない。

## 13. フィールド契約・文法一覧

| 場所 | 許可された主要キー | 解釈 |
|---|---|---|
| ルート | `meta,form,time_changes,sections,melodies,scale_definitions,drums,bass,comp,arps,counters,macros,instruments,transitions,notes` | 全て定義済み、追加は禁止 |
| `meta` | `title,bpm,meter,tuning,reference_octave,gate,swing,master,fade_out_seconds` | `tuning: 12edo`だけ受理 |
| `meta.swing` / `form[].swing` | `subdivision,ratio` | グリッドのスウィング |
| `meta.master` | `target_lufs,max_true_peak_db,target_lra_lu` | Master/計測の設定 |
| `form[]` | `section,bars,tonic,scale,bpm,meter,last_bar_quarters,swing` | 曲中の出現と時間正本。`section`,`bars`,`tonic`は必須で、`scale`は任意（省略時はスケール未指定） |
| `time_changes[]` | `at_quarters,bpm` | 曲頭からのテンポ変更 |
| `sections.<name>` | `from,chords,chord_changes,lead,drums,bass,bass_overrides,comp,pad,arp,counter,placements,add_placements,events,velocity,part_gains` | 1セクションの演奏宣言 |
| `melodies` | 小節文字列のリスト、`{from,replace}`、名前空間辞書 | 相対度数と再利用 |
| `drums.<name>` | `from,parts,overrides` | 1小節Grooveをセクションで反復 |
| `drums.overrides[]` | `when,parts` | 小節条件を満たしたパートへ置換 |
| `bass_overrides[]` | `when,parts` | 小節条件を満たしたベースパートへ置換。条件は`drums.overrides[]`と共通 |
| `bass.<name>` | `parts,choices,relative_to` | 主音またはコードベース基準の度数。`c1`などは構成音参照 |
| `bass.choices.<token>` | `odd,even` | セクション内奇数・偶数 |
| `arps.<name>` | `order,voicing_floor_midi,retrigger_each_step` | 和音の声部を音域配置してArp。`order`のセルは非負整数、`.`（休符）、`-`（継続） |
| `counters.<name>` | `when,cell` | 無音拍に置く対旋律 |
| `sections.<name>.part_gains` | `instrument_id: number` | Section中だけ加算するPart Gain（dB） |
| `instruments.<id>` | `preset / definition / external`, `gain_db,pan,role,gate,voicing,voicing_floor_midi,trigger_note_midi,bass_floor_midi,bass_ceiling_midi,audio_input` | 音源と演奏音域・定位 |
| `placements[]` | `instrument,at,melody/degree/chord/bass_note,velocity,duration_steps,duration_quarters,transpose,gain_db,mark,slice_steps,take_notes,voicing,voicing_floor_midi,add_root_degree` | 明示的発音 |
| `add_placements[]` | `placements[]`と同じ | 継承時に配置リストの末尾へ追記 |
| `events[] / macros.<name>` | `kind/macro,at,instrument,instruments,duration_*,parameter,native_value,from_value,to_value,down,value,degree,except_instruments,bars,bar,from_step,through_step,velocity_from,velocity_to,early_hits_per_beat,final_hits_per_beat,last_bar_end_beat,riser` | 演奏・制御・演出 |
| `transitions[]` | `from,to,method,description` | 解説専用 |
| `notes` | UTF-8文字列 | 制作ノート、演奏に影響しない |

上表はフィールドの所在一覧であり、組合せと排他関係は各章の定義に従う。
