# 実行時の動作

## 本書の範囲

本書ではSonalloyの**実行時の動作**を説明します。1つのNoteが鳴って消えるまでの仕組み、1つのBlockの処理手順、Sampleの再生、準備とリセット、エラー時の扱いです。

| 本書で扱わない内容 | 参照先 |
|---|---|
| 部品の構造と所有関係 | `docs/architecture.md` |
| CLIの使い方・Option・Exit Code | `docs/cli.md` |
| Instrument Definition（JSON）の形式と制約 | `docs/instrument-definition.md` |

## 全体像

音源（Instrument Runtime）は、Polyphony数分のVoiceを固定で持っています。`process`を呼ばれるたびに、1つのBlock（最大Block SizeまでのSample列）を出力します。

1回の`process`は次の順で進みます。

```mermaid
flowchart LR
    A[出力をゼロで埋める] --> B[Eventの位置で区切る]
    B --> C[区間ごとにVoiceを鳴らす]
    C --> D[Eventを適用]
    D --> E{次のEventはあるか}
    E -- あり --> C
    E -- なし --> F[Blockの最後まで鳴らす]
```

- 出力はStereoで、左右のチャンネルを分けた`f32`バッファに書き込みます
- Eventは「いつ・どのNoteを押す/離す」の情報だけで、音の計算は毎Sample行われます
- Note OnからNote Offまでの間、そのNoteは1つのVoiceに割り当てられます

## Blockの処理

`process`に渡すものは、処理するFrame数、Blockの先頭位置（`absolute_frame`）、Event列、出力バッファです。EventはBlock内のSample位置（`sample_offset`）を持ち、その位置で正確に適用されます。

- 出力は先にゼロで埋めてから書き込みます。前のBlockの残りは混ざりません
- 区間ごとのRenderとEventの適用を繰り返し、最後にBlockの末尾まで鳴らします
- Frame数が0のBlockは、何もせず成功として扱います

**Eventの規則**

- Eventは`sample_offset`の昇順に並べます
- 同じ位置では、対応するNote OffをNote Onより先に置きます
- 位置が前後している、または同じNoteの順序が不正な場合はエラーになります

## Noteの一生

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Active: Note On
    Active --> Releasing: Note Off
    Releasing --> Idle: Release完了
    Active --> StealFading: 別のNoteに奪われる
    StealFading --> Active: Fade完了で新しいNoteを開始
    StealFading --> Idle: 新しいNoteがキャンセルされた
```

**Note On**

1. どのLayerもTrigger条件（Key / Velocity）に合わないNoteは無視します
2. Voiceを1つ選びます。Idle → 最も音量の小さいReleasing → 最古のActive の順です
3. 選んだVoiceがIdleなら即座にNoteを開始します。空きがない場合は、5msのFadeで古い音を消してから新しいNoteを開始します（Voice Stealing）

**Note Off**

- Note IDでVoiceを探し、今のADSRの値からReleaseを始めます
- Voice Stealingの待機中だったNoteは、ここでキャンセルできます

**Voice Stealing**

- 古い音は5msで音量をゼロへFadeします。Fade中にNote Offが来たら、待機中の新しいNoteをキャンセルします
- Fadeが終わると待機していたNoteを開始し、すべてのLayerが終わったらIdleへ戻ります

## Voiceの中身

Voiceは複数のLayerと左右独立のFilterを持ちます。Layerは「Generator + ADSR + Gain + Pan」のセットで、Trigger条件に合ったLayerだけが鳴ります。

```mermaid
flowchart TD
    G[Generator] --> E[ADSR]
    E --> A[Gain]
    A --> P[Pan]
    P --> M[Layerの出力をすべて合成]
    M --> F[Voice Filter 左右独立]
    F --> O[Stereo出力]
```

- **ADSR**：Note OnでAttackから始まり、Decayを経てSustainで待ちます。Note Offで現在の値からReleaseへ進みます。長さ0の区間は飛ばします
- **Gain**：5msで目標へ滑らかに変化させ、Note開始時のクリックを防ぎます。目標値はLayerのGainにVelocity応答を掛けた値です
- **Pan**：Constant-powerで左右へ振り分けます
- **Filter**：Cutoffは10msで滑らかに変化し、Velocity応答で開閉します。`voice_filter`が無ければかけません

**Generatorの種類**

- **Oscillator**：Note番号とTuningから周波数を決め、Sine / Sawを生成します。`phase_reset`が有効ならNoteごとに位相を先頭へ戻します
- **Sample**：後述のSample再生を使います。Compileで無効になったLayerは鳴りません

## Sampleの再生

SampleはCompile時に読み込み済みで、全Voiceで共有します。Voiceごとに再生位置（Cursor）だけを持ちます。

- Note OnでCursorを先頭へ戻し、再生速度は`2^((note - root) / 12) × Tuning`です
- Cursorは再生速度で進み、4点Cubic補間で読み出します
- one_shotでは末尾の5msをゼロへFadeし、音が急に切れないようにします
- Note OffではCursorを止めず、ADSRのReleaseだけが進みます。Sampleが終わるとそのLayerの音は終わります

## 準備とリセット

- **Prepare**：Polyphony数分のVoiceを作り、Scratch BufferとNative Handleを確保します。Sample RateがCompile時と一致しない場合は失敗します。Block Sizeの変更だけは許されます
- **Reset**：全Voice、Oscillatorの位相、SampleのCursor、ADSR、Filter、Scratch、絶対位置を最初の状態へ戻します。Reset後は同じ入力に対して同じ出力になります
- Prepareに失敗した場合は、それまでの状態を破棄して利用できない状態にします

## Sine Runtime（開発用）

- `dev render-sine`で使う単音のRuntimeです。Voiceの仕組みはなく、Event列を受け取るとエラーになります
- Prepareで周波数を検証し（Nyquist以下）、Native Oscillatorで生成した同じ信号を左右へコピーします

## 約束事

**Process中にしてはいけないこと**

- JSONの解析、ファイルの読み書き、素材の読み込み・Sample Rate変換・Hash計算
- メモリの新規確保
- 通信、同期型のログ出力、ブロックする待ち合わせ

**エラー時の扱い**

- 不正な入力や位置のずれ（Context不一致）はエラーにし、そのBlockの出力は無音にします
- Native側の失敗は`ProcessError::DspFailure`へ変換します
- エラーとExit Codeの対応は`docs/cli.md`を参照してください

## 書き出し（Offline Render）

ファイルへの書き出しは、CoreのRendererが同じProcessをBlock単位で繰り返します。

```mermaid
flowchart LR
    A[長さをFrame数へ変換] --> B[Prepare]
    B --> C[BlockごとにProcess]
    C --> D[RenderedAudio]
```

- 長さは「秒 × Sample Rate」を最も近い整数に丸め、TailのFrame数を足します
- 最後のBlockは残りのFrame数だけを処理するので、余分なSampleはできません
- Coreは`RenderedAudio`（左右のSample列）を返し、WAVへの変換はCLI側の仕事です
