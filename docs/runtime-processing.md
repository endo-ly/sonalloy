# Runtime Processing

## Process Contract

CoreはChannel分離のPlanar `f32` Bufferを使用します。出力はStereo固定です。

```rust
pub struct ProcessSpec {
    pub sample_rate: f64,
    pub max_block_size: usize,
    pub output_channels: usize,
}

pub struct ProcessContext {
    pub absolute_frame: u64,
    pub tempo_bpm: f64,
}

pub struct ProcessBlock<'a> {
    pub frames: usize,
    pub context: ProcessContext,
    pub events: &'a [ProcessEvent],
    pub output: &'a mut [&'a mut [f32]],
}
```

Prepare時の制約は次のとおりです。

- Sample Rateは有限かつ正の値
- 最大Block Sizeは1以上
- Output Channel数は2
- 一回のProcessのFrame数は最大Block Size以下
- 全Output SliceはFrame数以上
- EventのSample Offsetは0以上、Block Frames未満

Process開始時に対象範囲をZero Clearし、Runtimeは対象範囲の全Sampleを書き込みます。0 Frameは安全なNo-opです。

## FrameとContext

`absolute_frame`はRender開始からの絶対位置です。Runtime内部のFrame位置とContextが一致しない場合は処理を失敗させ、対象Bufferを無音にします。これにより、Block分割や呼び出し側の位置ずれを黙って受け入れません。

Sine RuntimeはEventを適用しません。Event列が渡された場合は無音化して`EventsUnsupported`を返します。Event型はNote IDとSample Offsetを持ち、同一Offsetではmatching Note OffをNote Onより先に適用します。非昇順Eventや同一Noteの順序違反はProcess Errorです。

## CompileとRuntime

```text
Instrument Definition
  → Parse / Validate
    → Compile（dB、cent、ADSR、Filter上限を変換）
      → InstrumentRuntime::prepare
        → 固定Voice Pool / Scratch / Native Handle
```

`CompiledInstrument`はRuntime状態を持たない不変値です。Compile時にSample AssetのSHA-256検証、WAV Decode、Mono Downmix、必要なResampleを行い、Decode済みBufferとCompile時のProcess Sample RateをCompiled Instrumentへ保持します。`InstrumentRuntime`はPrepare時にDefinitionのPolyphony分のVoiceを作り、Voiceごとに複数Layer、LayerごとのOscillatorまたはSample Runtime、左右独立のFilterを所有します。Runtime PrepareはCompile時と異なるSample Rateを明示的に拒否し、Block Sizeだけの変更は許可します。Process中にJSON Parse、File I/O、Decode、Resample、Hash計算、Voice Pool拡張を行いません。

## VoiceとADSR

VoiceのStateは`Idle → Active → Releasing → Idle`です。Note OffはNote IDで対象Voiceを特定し、現在のEnvelope値からReleaseを開始します。ADSRはAttack、Decay、Sustain、ReleaseをSample単位で更新し、0秒Segmentは次のStateへ直ちに進みます。Voice StealingはIdle、最も小さい`estimated_level`のReleasing、最古のActiveの順に候補を選び、5 msのSteal Fade後にPending Noteを開始します。

## Sample Accurate Segment Render

```text
Block Start
  → Event OffsetまでRender
  → OffsetのEventを適用
  → 次のEvent OffsetまでRender
  → Block EndまでRender
```

各SegmentではLayerごとのOscillatorまたはSampleのMono信号へADSRとLayer Gainを乗算し、Constant-power PanでStereo化します。複数Layerを同じVoice内でMixした後にVoice FilterのLeft/Rightを適用し、Instrument Outputへ加算します。Gainは5 ms、Voice開始時のFilter Cutoffは10 msの固定Smootherを使用します。CutoffのRampはSample単位の値をNative側の1回のBlock処理へ渡し、Host Block Sizeに依存しないままRustとNativeの境界越えをBlock単位に抑えます。

## 信号経路

```text
MIDI Note
  → Voice Allocation
      → Layer Trigger
        ├─ MIDI Note × Tuning Ratio → DaisySP Sine / Saw
        └─ Root Note × Tuning Ratio → Sample Cursor / Cubic Interpolation
              → ADSR
                → Layer Gain × Velocity Gain
                  → Constant-power Pan
                    → Layer Mix
                      → Voice Low-pass Filter
                        → Stereo Output
```

## Sine Runtime

Prepareで次を行います。

1. `ProcessSpec`を検証する。
2. Native OscillatorをSample RateとSineへ設定する。
3. OscillatorをResetする。
4. 最大Block SizeのScratch Bufferを確保する。
5. Absolute Frameを0へ戻す。

Processでは、Native OscillatorでScratch BufferへBlock生成し、同じ信号をLeft / Rightへコピーします。Scratch BufferはPrepare時に確保済みで、Process中の容量拡張はありません。

## Sample Runtime

PrepareでCompile済みのMono Sampleを各VoiceのSample Runtimeへ共有し、VoiceごとにCursorだけを所有します。Note OnでCursorを0へ戻し、MIDI NoteとSample Root Noteの差を半音単位の`2^((note - root) / 12)`へ変換してLayer Tuning Ratioを乗算します。CursorはSample Rateへ応じた再生速度で進み、4点Cubic Interpolationで読み出します。

Sampleの`one_shot`は末尾でGeneratorを完了させます。終端の最後5 ms（短いSourceでは全再生区間以内）は出力上の残りFrame数を基準に0へFadeし、Sourceの最後の値が0でない場合も不連続を作りません。Note OffではCursorを停止せず、LayerのADSRだけをReleaseへ遷移させます。Sample AssetがCompileできなかったLayerはDisabledとして扱い、ほかの有効Layerの処理を継続します。

## Offline Render Loop

RendererはCore内で次の順序を繰り返します。

```text
Request検証
  → Runtime Prepare
    → 可変長Blockを切り出す
      → ProcessContextを付与
        → Process
          → RenderedAudioへ格納
```

Durationは秒をSample Rateへ乗算し、最近傍へ丸めてFrameへ変換します。TailはDurationへ加算されます。最終Blockは残りFrameだけを渡すため、余分なSampleは生成しません。

Core RendererはFile PathやWAV Writerを知りません。`RenderedAudio`を返し、ファイル形式の変換はCLIが担当します。

## Error時の規則

- Prepare失敗時はRuntimeを利用可能状態にしない。
- AssetのMissing、Hash Mismatch、Decode、Resample失敗はCompile Warningとして保持し、同じInstrumentのほかの有効LayerをRenderできる。
- 有効なBufferでProcessのContextまたはEventが不正な場合、対象範囲を無音にする。
- Native DSP ErrorはCoreの`ProcessError::DspFailure`へ変換する。
- CLIはProcess ErrorをExit Code 3、Input ErrorをExit Code 2で返す。
- WAV書き込みの失敗はExit Code 4で返し、成功結果を表示しない。

Process中にJSON解析、File I/O、Decode、Resample、Hash計算、Network、同期I/O Logging、Blocking Mutexを行いません。
