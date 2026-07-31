# Sonalloy Architecture

## 責務境界

依存方向は一方向です。

```text
sonalloy-cli
    ↓
sonalloy-core
    ↓
sonalloy-dsp-sys
    ↓
Internal C ABI
    ↓
DaisySP
```

`sonalloy-core`はCLI、clap、hound、C++ Header、Audio Device APIを知りません。CLIはCoreのRendererを呼び出し、Coreが返したPlanar AudioをWAVへ変換します。

## Crate

### `sonalloy-core`

Coreが所有するProcess ContractとRuntimeを提供します。

- `process`: `ProcessSpec`、`ProcessContext`、`ProcessBlock`、共通Lifecycle
- `runtime`: DaisySP Sineを使うRuntime
- `render`: Frame単位のOffline Render Loop
- `diagnostics`: Frontend非依存のCode、Severity、Message

CoreのAudio Pathは、Prepare時に確保したScratch BufferとNative Handleだけを使用します。

### `sonalloy-dsp-sys`

Native ABIの宣言と、Raw Pointerを隠蔽するSafe Rust Wrapperを所有します。`DspOscillator`がOpaque HandleをDrop時に破棄し、CoreへResult CodeやNative Layoutを漏らしません。

Build Scriptは`native/daisysp-wrapper`をCMakeでBuildし、Static LibraryをRustへLinkします。DaisySPのVersionは次の固定Commitです。

```text
DaisySP V1.0.0
a0494a3adb67f549e18dfd71a35fa656f65b38b6
```

Native WrapperはDaisySPの`oscillator.cpp`だけをTargetへ追加します。DaisySPのClass名やEnumはWrapperの実装内部に留まり、DefinitionやCore Public APIには露出しません。

### `sonalloy-cli`

CLI固有の責務を所有します。

- clapによる引数解釈
- 秒からFrameへの変換
- Core Rendererの呼び出し
- houndによるStereo WAV出力
- Text / JSON Diagnostics
- Process終了Code

CLIはDaisySP FFIを直接呼びません。

## Native境界

C ABIは外部製品向けのPublic ABIではなく、`sonalloy-dsp-sys`からNative Wrapperを呼ぶための内部境界です。

```c
typedef struct sonalloy_dsp_oscillator sonalloy_dsp_oscillator;

sonalloy_dsp_oscillator* sonalloy_dsp_oscillator_create(void);
int32_t sonalloy_dsp_oscillator_prepare(...);
int32_t sonalloy_dsp_oscillator_reset(...);
int32_t sonalloy_dsp_oscillator_process(...);
void sonalloy_dsp_oscillator_destroy(...);
```

Native関数はNull Handle、引数、Buffer、例外を検査し、整数Result Codeへ変換します。Process CallはCaller所有のBufferへ書き込み、Native側で継続的なHeap Allocationを行いません。

## Lifecycle

```text
Prepare → Process（繰り返し） → Reset
```

PrepareでSample Rate、最大Block Size、Stereo出力、Scratch Buffer、Oscillatorを確定します。ProcessはBlockのFrame数だけを扱い、ResetはOscillator Phase、Scratch、Absolute Frameを初期化します。同じ入力をReset後に再度与えると同じ出力になります。
