param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("Debug", "Release")]
    [string]$Configuration
)

$ErrorActionPreference = "Stop"

$isDebug = $Configuration -eq "Debug"
$targetDirectory = Join-Path "target" $Configuration.ToLowerInvariant()
$runtimeOption = if ($isDebug) { "debug" } else { "release" }
$runtimeFlag = if ($isDebug) { "/MDd" } else { "/MD" }
$iteratorDebugLevel = if ($isDebug) { "2" } else { "0" }
$headerObject = Join-Path $targetDirectory "sonalloy-capi-header.obj"
$consumerObject = Join-Path $targetDirectory "sonalloy-capi-consumer.obj"
$consumerExecutable = Join-Path $targetDirectory "sonalloy-capi-consumer.exe"
$staticLibrary = Join-Path $targetDirectory "sonalloy_capi.lib"

$previousRuntimeOption = [Environment]::GetEnvironmentVariable(
    "SONALLOY_DSP_MSVC_RUNTIME", "Process")
$env:SONALLOY_DSP_MSVC_RUNTIME = $runtimeOption

try {
    if ($isDebug) {
        & cargo rustc `
            --package sonalloy-capi `
            --lib `
            --crate-type staticlib
    } else {
        & cargo rustc `
            --package sonalloy-capi `
            --lib `
            --release `
            --crate-type staticlib
    }
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    & cl /nologo /TC /c `
        /I crates\sonalloy-capi\include `
        crates\sonalloy-capi\tests\header.c `
        /Fo:$headerObject
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    & cl /nologo /EHsc /TP $runtimeFlag "/D_ITERATOR_DEBUG_LEVEL=$iteratorDebugLevel" `
        /I crates\sonalloy-capi\include `
        /c crates\sonalloy-capi\tests\msvc_cpp_consumer.cpp `
        /Fo:$consumerObject
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    & link /nologo `
        "/OUT:$consumerExecutable" `
        $consumerObject `
        $staticLibrary `
        bcrypt.lib advapi32.lib ntdll.lib userenv.lib ws2_32.lib
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    & $consumerExecutable
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
finally {
    if ($null -eq $previousRuntimeOption) {
        Remove-Item Env:SONALLOY_DSP_MSVC_RUNTIME -ErrorAction SilentlyContinue
    } else {
        $env:SONALLOY_DSP_MSVC_RUNTIME = $previousRuntimeOption
    }
}
