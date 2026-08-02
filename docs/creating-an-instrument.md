# 音源（Instrument）の作り方

このガイドは、Sonalloyで自分の音源を作ってWAVを出すまでの道筋を説明します。ひな形の生成から始め、パラメータの意味を理解しながら、試聴して、必要なら自作WAVをSampleとして組み込むところまで進みます。

> **本書の範囲**：音源作成の操作手順（人間向けガイド）です。仕様の詳細は本書に書かず、各仕様文書へ委ねます。
>
> | 本書に書かないこと | 参照先 |
> |---|---|
> | DefinitionのJSON仕様・制約・Range | `docs/instrument-definition.md` |
> | 実行時の挙動（Voice・ADSR・Sample再生） | `docs/runtime-processing.md` |
> | CLIの全Option・Exit Codeの定義 | `docs/cli.md` |
> | Agentが音源作成を実行する手順 | `.agents/skills/create-instrument/` |

## 全体の流れ

```text
ひな形の生成 → 音色の編集 → 検証 → （Sample追加） → 試聴 → 仕上げ
   Step 1       Step 2     Step 3       Step 4       Step 5   Step 6
```

| Step | 内容 | 使うコマンド |
|---|---|---|
| 1 | ひな形の生成（新規の場合のみ） | `instrument init` |
| 2 | 音色の編集（Layer、ADSR、Filterなど） | エディタでJSONを編集 |
| 3 | 検証 | `instrument validate` / `instrument inspect` |
| 4 | 自作WAVをSampleとして組み込み | SHA-256計算 → JSON編集 |
| 5 | 試聴 | `render note` / `render midi` |
| 6 | 仕上げ（名前・説明・関連docsへの反映） | — |

## Step 1. ひな形を生成する

次のコマンドで、Saw Oscillatorの最小Definitionが生成されます。

```bash
sonalloy instrument init my-instrument.json
```

生成されるJSONは、Polyphony 16、ADSR `0.005 / 0.18 / 0.65 / 0.3`、Gain `-14 dB`、Filter `12000 Hz / 0.12`のBasic Poly Synth型です。これを土台に音色を編集していきます。既存音源（`examples/instruments/`）のコピーから始めても構いません。

## Step 2. 音色を編集する

### Layerとは何か

音源は1つ以上の**Layer**で構成されます。Layerは「1つの音の発生源」です。

```text
Note On
  │
  ▼
Layer 1（Oscillator）→ ADSR → Layer Gain → Pan ─┐
                                                  ├→ Voice Filter → ステレオ出力
Layer 2（Sample）    → ADSR → Layer Gain → Pan ─┘
```

- Layerごとに**独立したADSR**と**Gain / Pan / Tuning**を持ちます。
- Layer同士は**同じVoice内でMix**されるため、Sampleのアタック＋Oscillatorの余韻のように、別々の音が一つの音色として聞こえます。
- 発音条件は`trigger`で制御します（`key_min / key_max`で鍵盤の範囲、`velocity_min / velocity_max`で打鍵の強さの範囲）。

### ADSRで音の輪郭を作る

ADSRは音の音量変化（エンベロープ）を形作る4つの区間です。

```text
Level
  ▲
  │        ┌──── sustain ────┐
  │       ╱                 ╲
  │      ╱                  ╲
  │     ╱                   ╲
  │    ╱                    ╲
  └───┴──────────────────────┴───────▶ Time
    attack  decay        release
```

| パラメータ | 役割 | 目安 |
|---|---|---|
| `attack_seconds` | 押してから最大音量まで達する時間 | 0（瞬発）〜数秒（うねり） |
| `decay_seconds` | 最大音量からSustainレベルへ下がる時間 | 0.05〜0.3が一般的 |
| `sustain_level` | 押している間の音量（0〜1） | 0だと短い音、1だと伸びる音 |
| `release_seconds` | 離してから消えるまでの時間 | 0だとバツンと切れる |

### そのほかのパラメータ

| パラメータ | 意味 | 注意 |
|---|---|---|
| `gain_db` | Layerの音量（-60〜12 dB） | Sampleを複数重ねる場合は重なり分を下げる |
| `pan` | 左右位置（-1 = 左、0 = 中央、1 = 右） | Constant-powerで自然に定位する |
| `tuning_cents` | 半音の100分の1単位の音程調整 | -1200〜1200 |
| `voice_filter` | 全Layer Mix後のLow-pass Filter | `cutoff_hz`（20〜20000）と`resonance`（0〜1） |
| `modulation` | Velocity、LFO、Envelope、RandomなどのSourceをTargetへ接続 | `routes`でLayer Gain、Pan、Tuning、Filterへ反映 |

打鍵の強さや発音中の変化を設定する場合は、`modulation.sources`へSourceを定義し、`modulation.routes`でTargetへ接続します。VelocityとKey Trackingは組み込みSourceなので、Source定義なしで参照できます。詳細なID、Range、Curveは[`docs/instrument-definition.md`](instrument-definition.md)を参照してください。

```json
"modulation": {
  "routes": [
    { "source": "velocity", "target": "layer.main.gain_db", "amount": 12.0, "curve": "linear" },
    { "source": "lfo", "target": "voice_filter.cutoff_hz", "amount": 1200.0, "curve": "linear" }
  ],
  "sources": [
    { "id": "lfo", "kind": "lfo", "waveform": "sine", "rate_hz": 0.5, "phase": 0.0 }
  ]
}
```

## Step 3. 検証する

編集したら、必ず検証します。

```bash
sonalloy instrument validate my-instrument.json
sonalloy instrument inspect my-instrument.json
```

- `validate`はJSON Parse、Validation、Compileまで実行し、問題がなければ`valid`と表示されます。
- `inspect`は実行値を人間が読める形で表示します（`--json`で機械可読）。Gain・Pan・Tuning・Envelopeに加え、Parameter、Source、Routeが意図どおりにCompileされたかをここで確認します。
- Errorには`layers[0].envelope.attack_seconds`のようなField Pathが付くので、そのまま該当箇所を修正できます。

発音中のParameter変更を確認する場合は、Event Sequence JSONを用意して次のようにRenderします。

```bash
sonalloy render events my-instrument.json events.json --duration-frames 96000 --output out/events.wav
```

Event SequenceではNote Eventと同じ絶対Frame位置にParameter Change、Pitch Bend、Mod Wheel、Aftertouchを記述できます。`render midi`ではMIDI Pitch Bend、CC1、Channel Aftertouchも同じRuntime Eventへ変換されます。

## Step 4. 自作WAVをSampleとして使う

録音や生成したWAVを、Sample Layerの音源として組み込めます。

**1. WAVを準備する**

PCM 16/24 bitまたはFloat 32のWAVです。Mono推奨ですが、StereoでもCompile時に自動でMonoへ平均Downmixされます。`testdata/assets/`へ置くのが慣例です。

**2. SHA-256を計算する**

```powershell
# Windows
Get-FileHash -Algorithm SHA256 testdata\assets\my-sample.wav
```

```bash
# Linux
sha256sum testdata/assets/my-sample.wav
```

**3. Layerの`generator`へ`sample`を記述する**

```json
{
  "id": "attack",
  "enabled": true,
  "trigger": {
    "key_min": 0, "key_max": 127,
    "velocity_min": 1, "velocity_max": 127
  },
  "gain_db": -18.0,
  "pan": 0.0,
  "envelope": {
    "attack_seconds": 0.0,
    "decay_seconds": 0.08,
    "sustain_level": 0.0,
    "release_seconds": 0.1
  },
  "generator": {
    "sample": {
      "asset": { "path": "../../testdata/assets/my-sample.wav", "sha256": "<計算した値>" },
      "root_note": 60,
      "playback_mode": "one_shot",
      "interpolation": "cubic"
    }
  }
}
```

| パラメータ | 意味 |
|---|---|
| `asset.path` | DefinitionのあるDirectoryを基準にした相対Path（または絶対Path） |
| `asset.sha256` | 起動時の検証用ハッシュ。省略するとWarningが出ます |
| `root_note` | このSampleが基準の音程（MIDI Note番号。60 = C4） |
| `playback_mode` | `one_shot`（最後まで1回だけ再生） |
| `interpolation` | `cubic`（4点補間） |

SampleのPath違いやハッシュ不一致の場合は**そのSample Layerだけが無効化され**、ほかのLayerでRenderは継続します。SHA-256を省略した場合はWarningだけが付きます。

## Step 5. 音を出す

**単音の確認**（音色の素性を確かめます）：

```bash
sonalloy render note my-instrument.json \
  --note 60 --velocity 100 --gate 0.5 --tail 0.5 \
  --sample-rate 48000 --block-size 257 --output out/note.wav
```

**フレーズの確認**（演奏感を確かめます）：

```bash
sonalloy render midi my-instrument.json \
  testdata/midi/basic-poly-synth-phrase.mid \
  --sample-rate 48000 --block-size 257 --tail 1.0 \
  --output out/phrase.wav
```

| Option | 意味 | 既定値 |
|---|---|---|
| `--note` | MIDI Note番号 | `60` |
| `--velocity` | 打鍵の強さ | `100` |
| `--gate` | Note OnからNote Offまでの時間（秒） | `0.5` |
| `--tail` | 最後のNote Off後の追加時間（秒） | note: `0.5` / midi: `1.0` |
| `--sample-rate` | Sample Rate（Hz） | `48000` |
| `--block-size` | Process最大Block Size（Frame） | `257` |
| `--output` | Stereo WAV出力先（必須） | — |

出力は**32-bit float・2 Channel**のStereo WAVです。親Directoryは事前に作成してください。既存のMIDIがなければ、`scripts/review/generate_midi_fixtures.py`で固定のテスト用MIDIを生成できます。

## Step 6. 仕上げる

- `metadata.name`と`metadata.description`を実際の音色に合わせます。
- 音色の設計意図（どのパラメータをどう調整したか）はDefinition内には書かず、必要な場合は`review-output/`のReview Packageへ記録します。
- 音源作成の一連の流れをAgentに実行させる場合は、`.agents/skills/create-instrument/`の手順が利用できます。

## 困ったときは

| Exit Code | 意味 | 対処 |
|---:|---|---|
| `0` | 成功 | — |
| `1` | Definition / Compile Error | `--json`を付けて再実行し、ErrorのField Pathを修正する |
| `2` | CLI入力またはRender Request Error | Option値（Sample Rate、Block Size、Tailなど）を確認する |
| `3` | Core Process / Render Error | `--json`の`DSP_ERROR`を確認する |
| `4` | WAV出力 Error | 出力先Directoryの存在と書き込み権限を確認する |

- **Warningが出た**：意図しないLayerの無効化（Sample欠落など）ではないかを`instrument inspect`で確認します。
- **音が鳴らない**：`enabled: true`、`trigger`の範囲に発音するNote / Velocityが含まれているかを確認します。
- **Sampleが無視された**：Asset PathとSHA-256の一致、WAV形式（PCM 16/24、Float 32）を確認します。

## 関連文書

| 文書 | 内容 |
|---|---|
| `docs/instrument-definition.md` | DefinitionのJSON仕様（全Fieldの単位・Range） |
| `docs/runtime-processing.md` | 実行時の挙動（Voice、ADSR、Sample再生） |
| `docs/cli.md` | CLIの全Command・Option・Exit Code |
| `docs/architecture.md` | システムの静的構造 |
| `.agents/skills/create-instrument/` | Agent向けの実行手順 |
