# Demo仕様

Demoは、複数のInstrumentと、それぞれへ送るAudition Patternを同じ時間軸で確認するためのCLI用JSONです。各Partを既存のOffline Render経路で生成し、必要に応じてStem、Multi-track MIDI、固定Gainを適用したStereo WAV、MP3を出力します。

Demoは楽曲制作のProjectではありません。Track / Clipの配置、Arrangement、Recording、Automation、Routing、本格的なMixer、Realtime複数Instrument HostはHost / DAWが管理します。

## Definitionの構造

Demo DefinitionのSchema Versionは`1`です。未知のFieldは受け付けません。InstrumentとPatternのPathはDemo JSONのあるDirectoryを基準に解決されます。絶対Pathも指定できます。

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

| Field | 内容 | Default |
|---|---|---|
| `schema_version` | Demo Schema Version。現在は`1`のみ | — |
| `name` | Demoの表示名 | `null` |
| `parts` | 1件以上のPart | — |
| `mix` | FadeとOptional Masterの設定 | 空のMix |

### Part

| Field | 内容 | Default |
|---|---|---|
| `id` | Demo内でASCIIの大文字小文字を区別せず一意なPart名。StemのFile名とMIDI Track Nameにも使う | — |
| `instrument` | Instrument DefinitionへのPath | — |
| `pattern` | 1つのInstrumentへ送るAudition PatternへのPath | — |
| `gain_db` | Partへ適用する固定Gain（dB） | `0.0` |
| `midi_channel` | MIDI Export時のChannel。1〜16 | 定義順に自動割り当て |

`id`は空にできません。Stem File名として安全に扱えるよう、ASCIIの英数字、`.`、`_`、`-`だけを使い、先頭は英数字、長さは1〜64文字にします。大文字小文字を区別しない重複を許さず、Windowsの予約デバイス名（`CON`、`PRN`、`AUX`、`NUL`、`COM1`〜`COM9`、`LPT1`〜`LPT9`）も指定できません。

`gain_db`は有限値を指定します。Mix時には`10 ^ (gain_db / 20)`へ変換して各Partへ一度だけ適用します。GainはPatternのEventや時間変化を持たない固定値です。変換結果が有限値にならない値はValidation Errorです。

### MIDI Channel

`midi_channel`を指定する場合は1〜16の範囲にし、明示ChannelをPart間で重複させません。省略したPartには、Demo Definitionの順番で未使用Channelの小さい番号から割り当てます。

```text
Part A: 2
Part B: omitted
Part C: 5
Part D: omitted

A = 2, B = 1, C = 5, D = 3
```

16個のChannelを使い切った後のPartはChannelなしで読み込めます。Audio Renderは続行できますが、`demo export-midi`は`MIDI_ERROR`で失敗します。

### Mix

| Field | 内容 | Default |
|---|---|---|
| `fade_out_seconds` | 最終Mix末尾へ適用するLinear Fade Out | `0.0` |
| `master` | FFmpeg `loudnorm`によるOptional Master設定 | `null` |

Masterの値は次の範囲で指定します。

| Field | 範囲 |
|---|---:|
| `integrated_lufs` | `-70.0 ..= -5.0` |
| `true_peak_db` | `-9.0 ..= 0.0` |
| `loudness_range_lu` | `1.0 ..= 50.0` |

## 時間軸とValidation

各PartのPatternはTick 0から始まります。Demo内にPartの開始TickやClip Offsetはありません。曲の途中から鳴らす場合は、Patternの先頭へ無音の区間を記述します。

最長PatternをDemoの共通時間軸の基準にします。短いPatternでは、自身の`length_ticks`未満にある変更だけを基準Patternの同じTick位置と一致させます。最長Patternの後半だけにある変更は、短いPatternへ要求しません。

共通時間軸には次の情報を使います。

```text
ticks_per_beat
tempo_changes
time_signature_changes
```

Demoの`length_ticks`は全Patternの最大値です。`musical_duration_seconds`とMIDIのConductor Trackは最長PatternのTempo / Time Signatureを使います。Render Tailは音楽的な長さに含みません。

各Partは次の順に検証されます。

1. Instrument JSONを読み込み、既存のInstrument Compileを実行する
2. Pattern JSONを読み込み、Pattern自身をValidationする
3. Patternを対象InstrumentのParameter CatalogへCompileする
4. 他Partとの共通時間軸を検証する

ValidationはPartごとに可能な範囲まで進み、最初のErrorだけで処理を終了しません。参照先のDiagnosticにはPart位置を付けます。

DemoのOffline Renderは外部Audio入力を受けないため、外部Audioを必要とするInstrumentはValidation Errorになります。

```text
Pattern単体: events[4].velocity
Demo:        parts[2].pattern.events[4].velocity

Instrument単体: layers[0].generator.foo
Demo:           parts[2].instrument.layers[0].generator.foo
```

参照Fileを開けない場合のPathは`parts[i].instrument`または`parts[i].pattern`です。実際に解決したPathとI/O ErrorはDiagnosticの`detail`に含まれます。

## MIDI Export

`demo export-midi`は共通の`ticks_per_beat`を持つStandard MIDI File Type 1を生成します。

| Track | 内容 |
|---|---|
| Conductor Track | DemoのName（指定時）、最長PatternのTempo / Time Signature、End Of Track |
| Part Track | Part ID、Note / Sustain / Pitch Bend / Mod Wheel / Aftertouch、End Of Track |

各Part Trackは解決済みChannelを使い、全TrackのEnd Of TrackをDemoの`length_ticks`へ揃えます。PatternのParameter Changeと同音程のNote Overlapは、Standard MIDIで意味を保持できないため既存のPattern Export規則どおり`MIDI_ERROR`になります。

MIDI Marker、Section、Program Change、SysExはDemo Exportへ含めません。

## Offline RenderとStem

`render demo`は各Partを順番に既存の`render pattern`と同じCore経路で処理します。各PartにはPatternの終端後、`--tail`で指定した秒数を追加します。固定Latencyを持つInstrumentは既存Renderと同じ方法で補正されます。

`--stems-dir`を指定すると、Directoryを作成して`<part.id>.wav`を書き出します。Stemは次の状態です。

```text
InstrumentのStereo出力
→ Latency補正
→ Render Tail追加後
→ Demo Gain適用前
→ Global Fade適用前
→ Master処理前
```

MixはStereoの`f32`です。短いPartの終端は無音として扱い、長いPartに合わせてMixを拡張します。PartのGainを適用して加算したあと、最終Mixの末尾へ指定したFadeを適用します。Mixには自動Normalize、Clamp、Limiterを加えません。

出力WAVは既存Renderと同じStereo、指定Sample Rate、32-bit float形式です。`--analyze`を指定すると、Master前でFade後の最終Mixを既存Audio Analysisへ渡します。

## MasterとMP3

`mix.master`を指定した場合、または`--mp3-output`を指定した場合だけ、CLIはPATH上の`ffmpeg`を実行します。Masterでは`loudnorm`を2-passで使い、実測値と実際の`normalization_type`をRender Reportへ記録します。FFmpegの条件によってDynamic Normalizationへ切り替わる場合も、その結果を受け入れます。

Master済みWAVはWAV形式、Stereo、指定Sample Rate、`pcm_f32le`で出力します。MP3はMasterがあればMaster済みWAVから、なければ通常のMix WAVから生成し、MP3形式、Codecは`libmp3lame`、Bitrateは`256k`に固定します。

FFmpegが必要な状態で見つからない場合は、次のDiagnosticで失敗します。

```text
exit code: 4
code: RENDER_ERROR
message: FFmpeg is required for Demo mastering or MP3 output
detail: install ffmpeg and make it available on PATH
```

Masterの一時WAVは安全な一時Fileとして扱い、処理の成否にかかわらず残しません。Demoの設定はDemo JSON、実行結果は`--json` Reportが正本です。

## Workflow

```bash
sonalloy demo validate demo.json
sonalloy demo inspect demo.json --json
sonalloy render demo demo.json --output out/demo.wav --stems-dir out/stems --analyze --json
sonalloy demo export-midi demo.json --output out/demo.mid
```

Patternの作成と曲固有のNote、Chord、Rhythm、展開は各Patternで行います。Demoはそれらを同じ時間軸へまとめ、Offlineの確認結果を出力するために使います。
