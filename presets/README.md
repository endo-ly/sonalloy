# Presets

`presets-defs.md`の代表60音源に対応する音源定義と試聴WAV。1音源 = 1 Directoryで、`definition.json`と試聴用WAVを置く。

| ファイル | 内容 |
|---|---|
| `definition.json` | 音源定義 |
| `note-<key>.wav` | カテゴリの代表単音（Velocity 100）。ベースは`note-c2.wav`（C2）、リードは`note-c4.wav`（C4）。Attack / Sustain / Releaseの素性を確認する |
| `phrase.wav` | Velocity差付きの試聴フレーズ。発音分離と音色の一貫性を確認する |

`assets/`はSample、Wavetableなど、定義から参照するWAV Assetの配置先である。

## Library metadata

各`definition.json`の`metadata`には、音源をLibraryで見つけて用途を判断し、短い演奏で試聴するための情報を設定する。一般のDefinitionではLibrary項目を省略できるが、Built-in Presetでは`category`、`tags`、`recommended_range`、`preview`をすべて設定する。

### Category

Categoryは1音源の主な役割を表す。分類を固定することで、検索結果のまとまりと表示順を保つ。

| Category | 意味 |
|---|---|
| `Bass` | 低域を支える音源 |
| `Lead` | 単音の旋律や前景を担う音源 |
| `Pad` | 長く持続する背景・空間音 |
| `Keys` | ピアノやオルガンのような鍵盤音 |
| `Poly` | コードや複数音を重ねて使う汎用音 |
| `Stab` | 短いコードを一打で鳴らす音源 |
| `Pluck` | 発音直後の輪郭と自然な減衰を使う音源 |
| `Mallet` | ベルやマレットのような有音程打撃音 |
| `Drums` | キック、スネア、ハイハットなどのドラム音 |
| `Percussion` | シェイカーや金属音などの打楽器 |
| `Sequence` | 保持中にリズムや音色が進む音源 |
| `FX` | 上昇、下降、衝撃、質感などの演出音 |

### Tag vocabulary

Tagは音色、音源方式、動き、役割を補足する。Preset間で同じ語を使うため、表記違いの同義語を増やさない。

| 分類 | 使用できるTag |
|---|---|
| Tone / texture | `Warm`, `Bright`, `Dark`, `Clean`, `Noisy`, `Metallic`, `Glassy`, `Soft`, `Aggressive`, `Punchy`, `Wide`, `Deep` |
| Synthesis / source | `Analog`, `Digital`, `FM`, `Wavetable`, `Wavefold`, `Additive`, `Formant`, `Granular`, `Physical`, `Spectral`, `Noise` |
| Behavior | `Mono`, `Polyphonic`, `Motion`, `Rhythmic`, `Sustained`, `Plucky`, `Percussive`, `Evolving`, `Gated`, `Random` |
| Character / role | `Sub`, `Acid`, `Reese`, `Supersaw`, `Drone`, `Chord`, `Bell`, `Kick`, `Snare`, `Clap`, `Hihat`, `Crash`, `Tom`, `Rim`, `Shaker`, `Riser`, `Impact`, `Sequence`, `Texture` |

各Presetには2〜5個のTagを設定する。Tagは音源の実装と説明に基づいて選び、名前だけから追加しない。

### Recommended Range

`recommended_range`はRuntimeが発音できる範囲ではなく、その音源を実用的に使いやすいMIDI Noteの範囲である。代表Noteを必ず含め、低端・中央・高端で有効な音声を確認して設定する。固定打撃音は代表Note付近へ絞り、音程に追従する音源は用途を保てる連続範囲を設定する。

### Preview

`preview`はBrowserからすぐ試聴するためのNote列である。共通値は120 BPM、480 ticks/beat、4/4とし、同じTickのNoteでChordを表現する。Previewの全NoteはRecommended Range内に置く。

| Category | 長さ | 構成 |
|---|---:|---|
| Bass | 1920 ticks | 低域の3音をtick 0 / 480 / 960へ配置 |
| Lead | 1920 ticks | 中域の4音をtick 0 / 360 / 720 / 1080へ配置 |
| Pad | 1920 ticks | 3〜4音のChordをtick 0から1440 ticks保持 |
| Keys | 1920 ticks | 1回のChordと2回の単音 |
| Poly | 1920 ticks | 4音Chordを2回 |
| Stab | 1920 ticks | 短い3〜4音Chordを2回 |
| Pluck | 1920 ticks | 短音4つを順番に配置 |
| Mallet | 1920 ticks | 中短音4つを順番に配置 |
| Drums | 1920 ticks | 代表Noteを4回、Velocity 90 / 110 / 100 / 120 |
| Percussion | 1920 ticks | 代表Noteを8分刻みで6回 |
| Sequence | 3840 ticks | 代表Noteをtick 0から3360 ticks保持 |
| FX | Presetごと | 音源の役割に合わせた長さとNote列 |

55〜60の個別ルールは次のとおり。55はNote 48を4.5秒、56はNote 60を3.5秒、57はNote 36を3.2秒、58はNote 45を3.2秒保持する。59はNote 60を短く3回、60はNote 60を6秒保持する。

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
| パッド | C4（MIDI Note 60） | 16.0 / 7.0 | `pad-audition-pattern.json` |
| キー／コード（25〜29） | C4（MIDI Note 60） | 5.0 / 3.0 | `keys-audition-pattern.json` |
| コードスタブ（30〜31） | C4（MIDI Note 60） | 1.5 / 3.0 | `chord-stab-pattern.json` |
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

17〜24のパッドは、長音の中での倍音変化とコードをつないだときの余韻を試聴する。`pad-audition-pattern.json`は14秒と13秒の4音コードを1秒重ねて演奏し、後半にMod Wheelを操作する。`phrase.wav`は`render pattern`に`--tail 7`を指定して再生成する。Velocityで音量、Pitch Bendで±2半音、Mod Wheelで明るさを調整できる。

25〜29の`keys-audition-pattern.json`は、同音の弱・中・強、音域をまたぐ短い旋律、4音と6音のコードを演奏する。後半にはMod Wheel、Pitch Bend、Sustain Pedalを含む。30〜31の`chord-stab-pattern.json`は短いコードを反復し、最後の長押しで減衰を確認する。どちらも`phrase.wav`は`render pattern`に`--tail 3`を指定して再生成する。

25〜31はVelocityで音量、Pitch Bendで±2半音を調整できる。Mod Wheelは25のChorusの深さ、26の回転感の速さと深さ、27〜31の明るさを変える。25のエレクトリックピアノと31のハウス・コードスタブは長押しでも自然に減衰し、26〜29は保持中も持続する。30のシンセブラスは強い立ち上がりから控えめな持続へ移る。

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

50の素材WAVは`python3 presets/assets/generate-wave-seq-steps.py`で再生成できる。8フレームの単周期Wavetable素材を生成し、音源定義のAsset SHA-256も更新する。
