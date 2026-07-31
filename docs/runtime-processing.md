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

Sine RuntimeはEventを適用しません。Event列が渡された場合は無音化して`EventsUnsupported`を返します。Event型はNote IDとSample Offsetを持つため、後続Runtimeが同じContractへ接続できます。

## Sine Runtime

Prepareで次を行います。

1. `ProcessSpec`を検証する。
2. Native OscillatorをSample RateとSineへ設定する。
3. OscillatorをResetする。
4. 最大Block SizeのScratch Bufferを確保する。
5. Absolute Frameを0へ戻す。

Processでは、Native OscillatorでScratch BufferへBlock生成し、同じ信号をLeft / Rightへコピーします。Scratch BufferはPrepare時に確保済みで、Process中の容量拡張はありません。

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
- 有効なBufferでProcessのContextまたはEventが不正な場合、対象範囲を無音にする。
- Native DSP ErrorはCoreの`ProcessError::DspFailure`へ変換する。
- CLIはProcess ErrorをExit Code 3、Input ErrorをExit Code 2で返す。
- WAV書き込みの失敗はExit Code 4で返し、成功結果を表示しない。

Process中にJSON解析、File I/O、Decode、Resample、Hash計算、Network、同期I/O Logging、Blocking Mutexを行いません。
