param(
    [string]$Sonalloy = "$PSScriptRoot/../../../../target/debug/sonalloy.exe",
    [string]$OutputDirectory = "$PSScriptRoot/../../out/audio/chrome-afterglow"
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$stems = Join-Path $OutputDirectory 'stems'
New-Item -ItemType Directory -Force $stems | Out-Null
$raw = Join-Path $OutputDirectory 'chrome-afterglow-mix.wav'
$wav = Join-Path $OutputDirectory 'chrome-afterglow-balanced.wav'
$mp3 = Join-Path $OutputDirectory 'chrome-afterglow-balanced.mp3'

& $Sonalloy render demo "$PSScriptRoot/config/demo.json" --tail 2 --stems-dir $stems --analyze --json --output $raw
if ($LASTEXITCODE -ne 0) { throw 'demo render failed' }

& ffmpeg -hide_banner -loglevel error -y -i $raw -af 'aresample=192000,volume=4dB,alimiter=limit=0.841395:attack=5:release=70:level=false:latency=true,aresample=48000' -c:a pcm_f32le $wav
if ($LASTEXITCODE -ne 0) { throw 'audio mastering failed' }

& ffmpeg -hide_banner -loglevel error -y -i $wav -c:a libmp3lame -b:a 256k $mp3
if ($LASTEXITCODE -ne 0) { throw 'mp3 export failed' }
