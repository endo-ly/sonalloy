# 公開C API

`sonalloy-capi`は、C / C++ ApplicationからSonalloyのCompile、Parameter Catalog、Runtime Lifecycle、Planar Audio Processを利用するための境界です。内部のRust型、CPAL、Midir、JUCE、Plugin SDKはヘッダへ現れません。

## ビルドとリンク

WorkspaceからLibraryをビルドします。

```bash
cargo build -p sonalloy-capi --release
```

公開ヘッダは[`crates/sonalloy-capi/include/sonalloy.h`](../crates/sonalloy-capi/include/sonalloy.h)です。生成されるLibraryは環境に応じて`libsonalloy_capi.a`、`libsonalloy_capi.so`、または`libsonalloy_capi.dylib`になります。ApplicationはヘッダをIncludeし、LibraryとSonalloyが使用するNative依存Libraryをリンクします。

WindowsのRust dev / test buildはRelease CRTを使用します。C++ Applicationからstatic libraryへリンクする場合は、Cargoのcrate typeをstatic libraryだけに指定してビルドします。Debug CRTを使用する場合は、Native wrapperもDebug CRTでビルドするため、次のように設定してください。

```powershell
$env:SONALLOY_DSP_MSVC_RUNTIME="debug"
cargo rustc -p sonalloy-capi --lib --crate-type staticlib
```

Release CRTの場合は次のように`release`を指定します。

```powershell
$env:SONALLOY_DSP_MSVC_RUNTIME="release"
cargo rustc -p sonalloy-capi --lib --release --crate-type staticlib
```

ABI Versionは`sonalloy_c_api_version()`で確認できます。HeaderとLibraryが同じVersionを返すことを、Applicationの起動時に確認してください。

## 文字列とHandle

文字列はNUL終端を必要としないUTF-8のPointerとLengthで渡します。`length`が0の場合は`data`をNULLにできます。`length`が0より大きいときのNULL Pointer、またはUTF-8でない入力は`SONALLOY_INVALID_ARGUMENT`です。

Compileで得られるHandleはApplicationが所有します。Handleの破棄は対応するDestroy関数で行い、NULLを渡したDestroyは何もしません。

```text
CompiledInstrument ──→ Runtime ──→ PreparedUpdate
       │                   │
       └── Diagnostics      └── Reclaimable
```

Parameter DescriptorとDiagnostic Viewの文字列は、それぞれ元のCompiled InstrumentまたはDiagnostics Handleが生きている間だけ有効です。Viewの内容を保持する場合はApplication側でコピーします。

## Compile

`sonalloy_compile_json`へDefinition JSON、Assetを解決するBase Directory、ProcessSpecを渡します。CompileとAssetの準備はControl Threadで行います。成功するとCompiled Handleが返り、Warningを含むDiagnostics Handleも返ります。Errorがある場合はCompiled HandleがNULLになり、Runtimeへ変更は伝わりません。

```c
SonalloyCompiledInstrument* compiled = NULL;
SonalloyDiagnostics* diagnostics = NULL;
SonalloyProcessSpec spec = { 48000.0, 256, 0, 2 };
SonalloyStringView json = { definition_bytes, definition_length };
SonalloyStringView base = { asset_directory, asset_directory_length };

SonalloyResult result = sonalloy_compile_json(
    json, base, spec, &compiled, &diagnostics);
if (result != SONALLOY_OK) {
    /* diagnostics_count / diagnostics_get で表示する */
}
```

Diagnosticsの`path`、`message`、`detail`はViewとして取得します。表示が終わったらDiagnostics Handleを破棄します。Parameter Count、Revision、Reported Latency、Required Input ChannelsはCompiled Handleから取得できます。

## Parameter Catalog

Catalogの順序はCompile時に固定されます。Parameter IDをControl側でDense Handleへ解決し、Parameter Eventには同じCatalogのRevisionを付けます。Native Unitの値をEventへ変換するときは、DescriptorのNormalize / Denormalize関数を利用してください。

Catalog Revisionが異なるParameter Eventは、Runtimeが安全に無視します。現在のRevisionで存在しないHandleはProcess Errorになります。

## Runtime Lifecycle

Runtimeは次の順序で使用します。

```text
runtime_create → runtime_prepare → runtime_activate
                                      ↓
                         runtime_process / runtime_publish / runtime_reset
                                      ↓
                              runtime_deactivate
```

`runtime_prepare`はVoice、Processor、Scratch、外部Audio Stateを確保します。`runtime_activate`は準備済みRuntimeをAudio Streamへ接続する状態へ移します。`runtime_deactivate`はResourceを保持したままProcessを停止します。Audio ProcessはActive状態だけで実行できます。

Process ContextにはAbsolute Frame、Tempo、Beat / Bar Position、Time Signature、Transport Stateを渡します。Audio BufferはPlanar形式で、出力は2 Channelです。EventはSample Offsetの昇順で渡し、Parameter ChangeのValueは`0..=1`の正規化値です。Process中の最大Event数は1024です。1回のProcessで渡すFrame数はPrepare時の`max_block_size`以下でなければなりません。

入力Channel同士は同じMemoryを参照できます。出力Channel同士の範囲は重複できず、入力Channelと出力Channelの範囲も重複できません。違反したBuffer配置は`SONALLOY_INVALID_ARGUMENT`になります。

```c
SonalloyRuntime* runtime = NULL;
sonalloy_runtime_create(compiled, &runtime);
sonalloy_runtime_prepare(runtime, spec);
sonalloy_runtime_activate(runtime);

float left[256] = { 0 };
float right[256] = { 0 };
float* output[] = { left, right };
SonalloyProcessContext context = {
    0, 120.0, 0.0, 0.0, 4, 4, SONALLOY_TRANSPORT_PLAYING
};
sonalloy_runtime_process(
    runtime, &context, NULL, 0, NULL, 0, output, 2, 256);
```

`reset`はVoice、Held Note、Sustain、Parameter Base、Global Processor、Absolute Frameを初期状態へ戻します。Fatal Process ErrorはRuntimeをFaultedへ移すため、再利用にはPrepareが必要です。最後のProcess Errorは固定長の`SonalloyRuntimeErrorInfo`で参照できます。

## Runtime Update

新しいDefinitionはControl ThreadでCompileし、候補Runtimeが使用するProcessSpecを使って`sonalloy_update_prepare`します。Live Publishでは現在のRuntimeと同じProcessSpecを渡し、External Input Channel数を変更する場合は変更後のProcessSpecを渡します。Prepared UpdateをAudio Threadへ所有権移動し、Process Blockの開始前に`sonalloy_runtime_publish`を呼びます。

Publish後の新しいNoteは新Generationで始まり、発音中のNoteは旧Generationで継続します。Note Off、Sustain、Pitch Bend、Mod Wheel、AftertouchはLive Generationへ伝わり、Parameter ChangeはActive Generationだけが受け取ります。

Publishの失敗時はUpdateを消費せず、同じHandleを後で再試行できます。ProcessSpec、Reported Latency、External Input Channel数が変わるUpdateは`SONALLOY_UPDATE_INCOMPATIBLE`となるため、Streamを停止して再Prepare / Activateします。Global Processorの切替中は`SONALLOY_TRANSITION_BUSY`です。

```c
SonalloyPreparedUpdate* update = NULL;
/* compiled_b は新しい Definition をCompileして得たHandle */
sonalloy_update_prepare(compiled_b, spec, &update);

SonalloyPublishOutcome outcome;
SonalloyResult result = sonalloy_runtime_publish(runtime, update, &outcome);
if (result == SONALLOY_OK) {
    sonalloy_update_destroy(update);
} else {
    /* Updateは保持されているので、後で同じHandleを再試行できる */
}
```

## Reclaim

旧Generationと旧Global ProcessorはAudio Callbackで破棄せず、`sonalloy_runtime_take_reclaimable`で取得します。Handleの記憶領域はRuntimeが事前確保した固定Poolにあり、Runtimeの生存中は移動しません。取得したHandleはResourceの所有権を持つものとしてControl Threadへ渡し、`sonalloy_reclaimable_destroy`で破棄します。Audio Threadは空いたPool slotだけを再利用し、Resourceの書き込みと破棄はAtomicな所有権移動で同期されます。Runtimeを破棄する前に、取得済みのReclaimable Handleを全て破棄してください。Runtimeは取得済みHandleが残っている間も生存させます。

## Thread Ownership

同じRuntime Handleを複数Threadから同時に呼び出しません。Compile、Diagnostics取得、Catalog取得、Update Prepare、Compiled / Update / Reclaimable DestroyはControl Threadで行います。Runtime Publish、Process、Reset、Reclaimable取得はAudio Threadで行えます。Runtime HandleはActivate後、Audio Threadが排他的に所有します。

内部でMutexを使ったConcurrent Callの同期は行いません。Audio CallbackではCompile、JSON Parse、File I/O、Asset Decode、Processor構築、Heap Expansion、Blocking処理を行わないでください。

## Error Handling

すべての公開操作はResult Codeを返し、Version / Metadata Queryは固定の数値を返します。Fallibleな公開操作ではRust Panicを`SONALLOY_INTERNAL_PANIC`へ変換します。外部Pointer、Length、Enum、Channel数、Event種類はABI境界で検証されます。Error発生後のRuntime状態は`sonalloy_runtime_state`で確認できます。

対応済みCapabilityはRealtime Runtime Update、External Audio Input、Transport Context、Parameter Catalog Revisionです。Note Expression、State Serialization、Neural Backendは未対応として報告されます。
