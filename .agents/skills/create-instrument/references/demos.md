# Demo（複数音源のオフライン確認）

Demoは、複数のInstrumentへそれぞれのAudition Patternを送り、同じ時間軸で音を確認するためのCLI定義ファイルです。1つのDemoから、ステレオのMix、PartごとのStem、Type 1 MIDI、必要に応じてMaster済みWAVやMP3を生成できます。

楽曲制作のProjectはHost / DAWが管理します。TrackやClipの配置、Arrangement、Recording、Automation、Routing、本格的なMixer、複数InstrumentのRealtime HostingはDemoの役割に含めません。

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

`id`にはASCII英数字、`.`、`_`、`-`だけを使い、先頭を英数字にします。長さは1〜64文字です。ASCIIの大文字小文字を区別しない重複を許さず、Windowsの予約デバイス名（`CON`、`PRN`、`AUX`、`NUL`、`COM1`〜`COM9`、`LPT1`〜`LPT9`）も指定できません。予約名は拡張子の前の名前として判定します。

`gain_db`には有限値を指定します。値は各Partへ一度だけ適用される固定Gainで、PatternのEventや時間変化は持ちません。Audioへ変換した結果を有限値として扱えない値は検証エラーになります。

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

外部Audio入力を必要とするInstrumentは、DemoのオフラインRenderへ入力を渡せないため検証エラーになります。検証は処理できるPartまで進み、参照先の診断には`parts[i].instrument`または`parts[i].pattern`の位置を付けます。参照パスを解決できない場合は、解決後のパスとI/O Errorを診断の`detail`へ含めます。

```text
Pattern単体:   events[4].velocity
DemoのPattern: parts[2].pattern.events[4].velocity

Instrument単体: layers[0].generator.foo
Demoの音源:     parts[2].instrument.layers[0].generator.foo
```

## 出力

### MIDI

`demo export-midi`は、基準Patternの`ticks_per_beat`を使うStandard MIDI File Type 1を生成します。Conductor TrackにはDemoのName（指定時）、基準PatternのTempoと拍子、Demo全体の終端を入れます。Part TrackにはPart ID、解決済みChannel、Note、Sustain、Pitch Bend、Mod Wheel、Aftertouchを入れます。

すべてのTrackのEnd Of TrackはDemoの`length_ticks`に揃えます。Parameter Change、同じ音程のNote Overlap、MIDI Channel不足は`MIDI_ERROR`になります。MIDI Marker、Section、Program Change、SysExは出力しません。

### AudioとStem

`render demo`は各Partを順番に、既存の`render pattern`と同じRender経路で処理します。固定Latencyは既存Renderと同じ方法で補正し、Patternの終端後には`--tail`で指定した余韻を加えます。

`--stems-dir`を指定すると、`<part.id>.wav`としてPartごとのStemを保存します。Stemは、Stereo Render、Latency補正、Render Tail追加までを済ませた音です。Part Gain、Global Fade、MasterはStemへ適用しません。

MixはStereoの`f32`です。短いPartは無音で長いPartに揃え、各Partの固定Gainを適用して加算したあと、Mixの末尾へFadeを適用します。自動Normalize、Clamp、Limiterは行いません。WAVはStereo、指定Sample Rate、32-bit float形式です。`--analyze`を指定した場合は、Fade後かつMaster前のMixをAudio Analysisへ渡します。

### MasterとMP3

`mix.master`または`--mp3-output`を指定した場合だけFFmpegを使います。Masterは`loudnorm`の2-pass処理を使い、実測値と`normalization_type`をReportへ記録します。Master済みWAVもステレオ、指定Sample Rate、32-bit floatのWAV形式で出力します。MP3はMaster済みWAVがあればそこから、なければ通常のMixから生成し、`libmp3lame`、`256k`固定です。

## 操作の流れ

```bash
sonalloy demo validate demo.json
sonalloy demo inspect demo.json --json
sonalloy render demo demo.json --output out/demo.wav --stems-dir out/stems --analyze --json
sonalloy demo export-midi demo.json --output out/demo.mid
```

Patternには曲固有のNote、Chord、Rhythm、展開を記述します。Demoはそれらを同じ時間軸へまとめ、オフラインの確認結果を出力します。各コマンドのOptionとReport項目は[CLIリファレンス](cli.md)を参照してください。
