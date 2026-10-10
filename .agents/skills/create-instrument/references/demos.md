# Demo（複数音源のオフライン確認）

Demoは、複数のInstrumentへそれぞれのAudition Patternを送り、同じ時間軸で音を確認するためのCLI定義ファイルです。Partの出力を別PartのExternal Audio入力へ渡す接続も指定できます。1つのDemoから、ステレオのMix、PartごとのStem、Type 1 MIDI、必要に応じてMaster済みWAVやMP3を生成できます。

楽曲制作のProjectはHost / DAWが管理します。DemoのRoutingはPart出力から別PartのExternal Audio入力への接続に限ります。TrackやClipの配置、Arrangement、Recording、Automation、任意Bus、Send / Return、本格的なMixer、複数InstrumentのRealtime HostingはDemoの役割に含めません。

## 定義ファイル

Demoの`schema_version`は`1`です。定義にない項目は受け付けません。`instrument`と`pattern`の相対パスはDemo JSONが置かれたディレクトリを基準に解決し、絶対パスも指定できます。

```json
{
  "schema_version": 1,
  "name": "Mercury Satellite",
  "parts": [
    {
      "id": "mercury-bloom",
      "instrument": "instruments/mercury-bloom.json",
      "pattern": "patterns/mercury-bloom.json",
      "gain_db": 0.0,
      "midi_channel": 1
    },
    {
      "id": "rubber-orbit",
      "instrument": "instruments/rubber-orbit.json",
      "pattern": "patterns/rubber-orbit.json",
      "gain_db": -1.4
    }
  ],
  "mix": {
    "fade_out_seconds": 2.7,
    "master": {
      "integrated_lufs": -16.0,
      "true_peak_db": -1.0,
      "loudness_range_lu": 11.0
    }
  }
}
```

| 項目 | 内容 | 初期値 |
|---|---|---|
| `schema_version` | Schemaのバージョン。現在は`1`のみ | — |
| `name` | Demoの表示名 | `null` |
| `parts` | 1件以上のPart | — |
| `mix` | FadeとMasterの設定 | 空の設定 |

### Part

| 項目 | 内容 | 初期値 |
|---|---|---|
| `id` | Partの識別子。Stemのファイル名とMIDI Track Nameに使う | — |
| `instrument` | Instrument Definitionへのパス | — |
| `pattern` | このPartへ送るAudition Patternへのパス | — |
| `gain_db` | Mixへ加える前の固定Gain（dB） | `0.0` |
| `midi_channel` | MIDI出力で使うチャンネル（1〜16） | 定義順に自動割り当て |
| `audio_input` | InstrumentのExternal Audio入力へ接続するSource Part | なし |

`id`にはASCII英数字、`.`、`_`、`-`だけを使い、先頭を英数字にします。長さは1〜64文字です。ASCIIの大文字小文字を区別しない重複を許さず、Windowsの予約デバイス名（`CON`、`PRN`、`AUX`、`NUL`、`COM1`〜`COM9`、`LPT1`〜`LPT9`）も指定できません。予約名は拡張子の前の名前として判定します。

`gain_db`には有限値を指定します。値は各Partへ一度だけ適用される固定Gainで、PatternのEventや時間変化は持ちません。Audioへ変換した結果を有限値として扱えない値は検証エラーになります。

`audio_input`は`{"part":"kick"}`の形で別のPart IDを1つ参照します。参照は大文字小文字も含めて完全一致し、自身への参照、存在しないID、循環する接続は検証エラーです。External Audioを必要とするInstrumentには接続が必須で、入力を使わないInstrumentには指定できません。

接続へ渡す音はSource InstrumentのRender、Latency補正、Render Tail、Source Part Gainまでを適用したStereo音声です。Global FadeとMasterは含みません。同じSourceを複数Partが使う場合も、各接続で同じSource音声を共有します。

### MIDI Channel

`midi_channel`を指定する場合は1〜16の範囲にし、明示したChannelをPart間で重複させません。省略したPartには、Demoの定義順に未使用Channelの小さい番号から割り当てます。

16個のチャンネルを使い切った場合、残りのPartも音声のRenderには参加できます。チャンネルが割り当てられないPartを含むDemoは`demo export-midi`で`MIDI_ERROR`になります。

### Mix

| 項目 | 内容 | 初期値 |
|---|---|---|
| `fade_out_seconds` | 最終Mixの末尾へ適用するLinear Fade Out | `0.0` |
| `master` | FFmpegによるMaster設定 | `null` |

`fade_out_seconds`には有限値かつ0以上を指定します。最終Mixの長さも超えないことはRender時に検証し、その長さには`render demo --tail`で追加する余韻を含みます。Demo JSONだけを読む`demo validate`と`demo inspect`では、Mix長との比較は行いません。`master`を指定する場合の範囲は次のとおりです。

| 項目 | 範囲 |
|---|---:|
| `integrated_lufs` | `-70.0 ..= -5.0` |
| `true_peak_db` | `-9.0 ..= 0.0` |
| `loudness_range_lu` | `1.0 ..= 50.0` |

## 時間軸と検証

各PartのPatternはTick 0から始まります。Partの開始TickやClip Offsetはなく、途中から鳴らす場合はPatternの先頭に無音を置きます。

Demoの共通時間軸は、`length_ticks`が最も大きいPatternを基準にします。すべてのPatternで`ticks_per_beat`を一致させ、短いPatternについては自身の`length_ticks`未満にあるTempo変更と拍子変更だけを、基準Patternの同じTick位置と一致させます。基準Patternの後半にだけ存在する変更は、短いPatternへ要求しません。

Demo全体の`length_ticks`はPatternの最大値です。`musical_duration_seconds`とMIDIのConductor Trackは、基準PatternのTempoと拍子から計算します。`render demo --tail`で追加する余韻は、音楽的な長さに含めません。

検証では、各Partについて次の処理を行います。

1. Instrument JSONを読み込み、既存のInstrument Compileを実行する
2. Pattern JSONを読み込み、Pattern自身を検証する
3. Patternを対象InstrumentのParameter CatalogへCompileする
4. 全Partの共通時間軸を検証する
5. External Audio接続の参照、Instrumentの入力要件、循環の有無を検証する

External AudioのSourceはConsumerより先にRenderされます。Render順が変わっても、Stem、Mixへの加算、Report、MIDI TrackにはDemo定義順を使います。参照先の診断には`parts[i].instrument`、`parts[i].pattern`、`parts[i].audio_input`または`parts[i].audio_input.part`の位置を付けます。参照パスを解決できない場合は、解決後のパスとI/O Errorを診断の`detail`へ含めます。

```text
Pattern単体:   events[4].velocity
DemoのPattern: parts[2].pattern.events[4].velocity

Instrument単体: layers[0].generator.foo
Demoの音源:     parts[2].instrument.layers[0].generator.foo
```

## 出力

### MIDI

`demo export-midi`は、基準Patternの`ticks_per_beat`を使うStandard MIDI File Type 1を生成します。Conductor TrackにはDemoのName（指定時）、基準PatternのTempoと拍子、Demo全体の終端を入れます。Part TrackにはPart ID、解決済みChannel、Note、Sustain、Pitch Bend、Mod Wheel、Aftertouchを入れます。

すべてのTrackのEnd Of TrackはDemoの`length_ticks`に揃えます。Parameter Change、同じ音程のNote Overlap、MIDI Channel不足は`MIDI_ERROR`になります。既存の出力ファイルは上書きします。MIDI Marker、Section、Program Change、SysExは出力しません。

### AudioとStem

`render demo`は各Partを既存の`render pattern`と同じRender経路で処理します。External Audio接続がある場合はSourceから先にRenderし、接続のないPartは定義順に処理します。固定Latencyは既存Renderと同じ方法で補正し、Patternの終端後には`--tail`で指定した余韻を加えます。

`--stems-dir`を指定すると、`<part.id>.wav`としてPartごとのStemを保存します。Stemは、Stereo Render、Latency補正、Render Tail追加までを済ませた音です。Part Gain、Global Fade、MasterはStemへ適用しません。

MixはStereoの`f32`です。短いPartは無音で長いPartに揃え、各Partの固定Gainを適用してDemo定義順に加算したあと、Mixの末尾へFadeを適用します。自動Normalize、Clamp、Limiterは行いません。WAVはStereo、指定Sample Rate、32-bit float形式です。`--analyze`を指定すると、Master前のMixを`mix_analysis`で、完成WAVを`output_analysis`で解析します。

### MasterとMP3

`mix.master`または`--mp3-output`を指定した場合だけFFmpegを使います。Masterは`loudnorm`の2-pass処理後に完成WAVを再測定し、True PeakがTargetを超えていればその超過量だけGainを下げて再確認します。超過が残れば処理は失敗します。Reportの`master`にはTarget、Mix入力、完成WAV出力、OutputとTargetの偏差、`normalization_type`、True Peak補正量を記録します。True Peakは保証しますが、Integrated LUFSとLRAはTargetとの差をReportし、失敗条件にはしません。

MP3はMaster済みWAVがあればそこから、なければ通常のMixから生成し、`libmp3lame`、`256k`固定です。生成後のMP3を再測定し、Integrated LUFS、True Peak、LRAを`mp3_measurement`へ記録します。MP3の値はCodec変換後の測定であり、Master済みWAVのTrue Peak保証には含まれません。

## Bundle Format v1

`demo pack`は、Demoと参照先を1つのディレクトリへまとめます。制作ディレクトリを移動・削除したあとも、Bundle内の`demo.json`を既存の`demo validate`と`render demo`へ直接渡せます。編集可能な楽曲データはDemo、Pattern、Instrument Definitionと音源アセットにあり、同梱WAVは参照音声として利用します。

```bash
sonalloy demo pack demo.json --output song-bundle --with-render
sonalloy demo validate song-bundle/demo.json
sonalloy render demo song-bundle/demo.json --output replay.wav
```

Bundle内の配置はPart IDで決まります。同じInstrumentやPatternを複数Partが使う場合も、各Partへ個別に配置します。

| 相対パス | 内容 |
|---|---|
| `bundle.json` | Version、配置、完全性を表すManifest |
| `demo.json` | 既存Demo Schema v1の楽曲定義 |
| `patterns/<part-id>.json` | 既存Pattern Schema v1の演奏データ |
| `instruments/<part-id>/definition.json` | Instrument Definition |
| `instruments/<part-id>/assets/<sha256>.<拡張子>` | Instrumentの参照アセット |
| `render/mix.wav` | `--with-render`指定時の完成Mix |
| `render/stems/<part-id>.wav` | `--with-render`指定時の全Part Stem |

Part順序、ID、Gain、MIDI Channel指定とその省略、External Audio接続、Fade、Master設定は保持します。Patternの時間軸と全イベントは元の定義順で保存します。Instrumentが参照する全アセットをコピーし、参照をDefinitionからの相対パスへ変更して実ファイルのSHA-256を設定します。同じInstrument内の同一内容はHashで1ファイルへ集約し、元の拡張子を小文字で保持します。入力のSymbolic Linkは実ファイルとしてコピーします。

Manifestのトップレベルは次の5項目に固定します。`format_version`はDemo・Pattern・InstrumentのSchema Versionから独立しています。

| Field | 値・内容 |
|---|---|
| `format_version` | 整数`1` |
| `demo` | `"demo.json"` |
| `render_settings` | `sample_rate`（正の整数、Hz）、`block_size`（正の整数、Frames）、`tail_seconds`（有限数かつ0以上、秒） |
| `render` | WAV同梱時は`mix: "render/mix.wav"`と、Part IDをキー、`render/stems/<part-id>.wav`を値とする`stems`オブジェクト。同梱しない場合は`null` |
| `files` | `path`と`sha256`を持つオブジェクトの配列。`bundle.json`自身を除く全ファイルをちょうど1回ずつ、パス文字列の昇順で記録 |

`files[].sha256`はファイル内容のSHA-256を小文字64桁の16進数で表します。ManifestのパスはBundleルートから、Demoの参照はDemo JSONから、アセットの参照はInstrument Definitionから解決します。すべて`/`区切りの相対パスで、絶対パス、`..`、空要素、`.`、バックスラッシュ、Bundle外への参照を許しません。ファイル配置は大文字小文字を区別しない環境でも衝突せず、Bundle内にSymbolic Linkを含めません。

Render SettingsはWAVを同梱しない場合も再現条件として記録します。`--with-render`指定時はBundle内Demoから、上記のAudio・Stem・Masterと同じ処理でMixと全Stemを生成します。出力先は新規ディレクトリに限ります。親ディレクトリの一時領域で配置・再検証・必要なRender・Manifest照合を完了してから出力を確定し、失敗時は一時領域を削除します。

## 操作の流れ

```bash
sonalloy demo validate demo.json
sonalloy demo inspect demo.json --json
sonalloy render demo demo.json --output out/demo.wav --stems-dir out/stems --analyze --json
sonalloy demo export-midi demo.json --output out/demo.mid
```

Patternには曲固有のNote、Chord、Rhythm、展開を記述します。Demoはそれらを同じ時間軸へまとめ、オフラインの確認結果を出力します。各コマンドのOptionとReport項目は[CLIリファレンス](cli.md)を参照してください。
