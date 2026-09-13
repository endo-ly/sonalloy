# Presets

`presets-defs.md`の代表60音源に対応する音源定義と試聴WAV。1音源 = 1 Directoryで、`definition.json`と試聴用WAVを置く。

| ファイル | 内容 |
|---|---|
| `definition.json` | 音源定義 |
| `note-<key>.wav` | カテゴリの代表単音（Velocity 100）。ベースは`note-c2.wav`（C2）、リードは`note-c4.wav`（C4）。Attack / Sustain / Releaseの素性を確認する |
| `phrase.wav` | Velocity差付きの試聴フレーズ。発音分離と音色の一貫性を確認する |

`assets/`はSample、Wavetableなど、定義から参照するWAV Assetの配置先である。

## 定義の検証

```bash
sonalloy instrument validate <preset>/definition.json
sonalloy instrument inspect <preset>/definition.json --json
```

## 試聴WAVの再生成

代表音のMIDI Noteと演奏データ、Gate / Tailは、音源のカテゴリごとに使い分ける。

| カテゴリ | 代表音 | Gate / Tail | 演奏データ |
|---|---|---|---|
| ベース | C2（MIDI Note 36） | 0.5 / 0.5 | `bass-audition-pattern.json` |
| リード | C4（MIDI Note 60） | 0.5 / 0.5 | `lead-audition-pattern.json`（11は`supersaw-chords-pattern.json`） |
| パッド | C4（MIDI Note 60） | 2.0 / 2.0 | `padline-events.json` |
| キー／コード | C4（MIDI Note 60） | 1.5 / 1.5 | `padline-events.json` |
| プラック／マレット | C4（MIDI Note 60） | 1.5 / 1.5 | `pluck-bell-mallet-pattern.json` |
| ベル | C4（MIDI Note 60） | 4.0 / 4.0 | `pluck-bell-mallet-pattern.json` |
| ドラム／パーカッション | プリセットごとの代表音（下表） | 1.5 / 1.0（クラッシュは3.5 / 2.5） | `drums-<種別>-pattern.json`（下表） |
| シーケンス／リズム音 | C4（MIDI Note 60） | 8.0 / 0.8 | `seq-hold-pattern.json` |
| 演出音／テクスチャ | プリセットごとの代表音（下表） | 下表 | `seq-hold-pattern.json` / `fx-<種別>-pattern.json`（下表） |

```bash
sonalloy render note <preset>/definition.json \
  --note <MIDI Note番号> --velocity 100 --gate <Gate> --tail <Tail> \
  --sample-rate 48000 --block-size 257 \
  --output <preset>/note-<key>.wav

sonalloy render events <preset>/definition.json <Event列> \
  --duration-frames 115200 --tail 0.6 \
  --sample-rate 48000 --block-size 257 \
  --output <preset>/phrase.wav
```

ベースとリードの`phrase.wav`は、120 BPMのPatternで音域、Velocity差、短い発音、重なる音、長音を確認する。後半にはMod WheelとPitch Bendの操作を含む。11のスーパーソウは`supersaw-chords-pattern.json`を使い、単音と4音のコードを確認する。いずれも`render pattern`に`--tail 0.6`を指定して再生成する。

```bash
sonalloy render pattern <preset>/definition.json presets/<Pattern名>.json \
  --sample-rate 48000 --block-size 257 --tail 0.6 \
  --output <preset>/phrase.wav
```

1〜16はPitch Bendで±2半音、Mod Wheelで音色を調整できる。5のアシッドベースと15のポルタメントリードは、音を重ねると音程が滑らかにつながる。

`padline-events.json`は120 BPMの4/4を基準にしたEvent列で、絶対Frame位置はSample Rate 48000 Hzを前提とする。パッドとキー／コードの`phrase.wav`は、`render events`に`--duration-frames 230400 --tail 2.5`を指定して再生成する。

プラック／ベル／マレットの`phrase.wav`は、120 BPMの共通Patternで音色と減衰を比較する。同音の弱・中・強、8分音符のアルペジオ、4音の和音、長く保持する単音の順に演奏する。プラック／マレットは`--tail 2`、ベルは`--tail 4`で再生成する。

```bash
sonalloy render pattern <preset>/definition.json presets/pluck-bell-mallet-pattern.json \
  --sample-rate 48000 --block-size 257 --tail <余韻の秒数> \
  --output <preset>/phrase.wav
```

32〜37はすべて自然に減衰する音源で、Velocityが音量と明るさを変える。Pitch Bendは±2半音、Mod Wheelは明るさの調整に使える。外部WAV Assetは不要。

## ドラム／パーカッション（38〜49）

打撃音は鍵盤を保持しても自然に減衰し、短く離すとReleaseの長さに応じて余韻が収まる。Velocityが音量や明るさを変える。胴鳴りや共鳴体を持つキック・スネア・タム・リム・メタリック・パーカッションは鍵盤の音程に追従するため、曲のキーへ合わせて鳴らせる。試聴WAVはGeneral MIDIのドラムマップに準じた代表音で生成する。Pitch Bendは有音程のLayerを±2半音、Mod Wheelは明るさや余韻の量を調整する。外部WAV Assetは不要。

| プリセット | 代表音 | 演奏データ |
|---|---|---|
| 38 エレクトロニック・キック | C2（36） | `drums-kick-pattern.json` |
| 39 ディープ・サブキック | C2（36） | `drums-kick-pattern.json` |
| 40 エレクトロニック・スネア | D2（38） | `drums-snare-pattern.json` |
| 41 ノイズ・スネア | D2（38） | `drums-snare-pattern.json` |
| 42 ハンドクラップ | D#2（39） | `drums-clap-pattern.json` |
| 43 クローズド・ハイハット | F#2（42） | `drums-hihat-pattern.json` |
| 44 オープン・ハイハット | A#2（46） | `drums-open-hihat-pattern.json` |
| 45 エレクトロニック・クラッシュ | C#3（49） | `drums-crash-pattern.json` |
| 46 エレクトロニック・タム | A2（45） | `drums-tom-pattern.json` |
| 47 リム／クリック | C#2（37） | `drums-rim-pattern.json` |
| 48 シェイカー | A#4（70） | `drums-shaker-pattern.json` |
| 49 メタリック・パーカッション | C5（72） | `drums-metallic-pattern.json` |

`phrase.wav`の再生成はクラッシュのみ`--tail 4.5`、他は`--tail 0.8`で行う。

## シーケンス／リズム音・演出音（50〜60）

50〜54と60は、鍵盤を押して保持している間に内部シーケンスやモーションが進む音源で、ゲートを長く取るほど展開が分かる。55〜59は場面転換向けの演出音で、テンポ同期のスイープ（55は8拍で上昇して0.3拍で収束、56は6拍で下降）と時間固定の変化（58は2.6秒、59は約0.25秒のバースト）、残響つきの一発音（57）がある。50と60は`assets/`配下のWAV Assetを参照し、SHA-256は定義に記録済みで外部準備は不要。ステップやMSEGの拍基準の Source はNote Onを基準に進むため、コードは同じタイミングで押さえると揃う。

| プリセット | 代表音 | Gate / Tail | 演奏データ | phraseのTail |
|---|---|---|---|---|
| 50 リズミック・ウェーブシーケンス | C4（60） | 8.0 / 0.8 | `seq-hold-pattern.json` | 0.6 |
| 51 パルス・ゲートシーケンス | C4（60） | 8.0 / 0.8 | `seq-hold-pattern.json` | 0.6 |
| 52 モーション・ステップシーケンス | C4（60） | 8.0 / 0.8 | `seq-hold-pattern.json` | 0.6 |
| 53 パーカッシブ・シーケンス | C4（60） | 8.0 / 0.8 | `seq-hold-pattern.json` | 0.6 |
| 54 ランダム・ステップシーケンス | C4（60） | 8.0 / 0.8 | `seq-hold-pattern.json` | 0.6 |
| 55 ノイズライザー | C3（48） | 4.5 / 0.5 | `fx-riser-pattern.json` | 1.0 |
| 56 ダウンリフター | C4（60） | 3.5 / 1.0 | `fx-downlifter-pattern.json` | 1.2 |
| 57 シネマティック・インパクト | C2（36） | 3.2 / 3.0 | `fx-impact-pattern.json` | 4.0 |
| 58 サブドロップ | A2（45） | 3.2 / 1.2 | `fx-sub-drop-pattern.json` | 1.2 |
| 59 グリッチ・バースト | C4（60） | 0.8 / 0.5 | `fx-hit-pattern.json` | 0.6 |
| 60 スペクトラル・フリーズテクスチャ | C4（60） | 6.0 / 3.0 | `seq-hold-pattern.json` | 3.0 |

Velocityは音量、Mod Wheelは明るさやフリーズの深さ、Pitch Bendは音程の調整に使える。

55〜58の試奏は同じ代表音を弱・中・強で鳴らし、スイープと余韻が収まる間隔を取る。59は短いバーストを反復し、60は長音と和音で変化を確認する。

50の素材WAVは`python3 presets/assets/generate-wave-seq-steps.py`で再生成できる。8種類のC4の断片を生成し、音源定義の参照範囲とSHA-256も更新する。
