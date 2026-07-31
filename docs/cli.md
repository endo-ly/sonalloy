# CLI

Binary名は`sonalloy`です。CLIはCoreのRendererを呼び出し、生成されたAudioをWAVへ保存します。

## `dev render-sine`

```bash
sonalloy dev render-sine \
  --frequency 440 \
  --duration 1.0 \
  --sample-rate 48000 \
  --block-size 257 \
  --output out/sine.wav
```

| Option | 必須 | Default | 内容 |
|---|---:|---:|---|
| `--frequency <Hz>` | No | `440` | Sine周波数。有限かつ0以上 |
| `--duration <seconds>` | Yes | — | Main Render時間。有限かつ0以上 |
| `--sample-rate <Hz>` | No | `48000` | 正の整数。WAV HeaderとDSPへ同じ値を渡す |
| `--block-size <frames>` | No | `257` | Process最大Block Size |
| `--tail <seconds>` | No | `0` | Main Render後の追加Frame |
| `--output <path>` | Yes | — | Stereo WAV出力先 |
| `--json` | No | Off | ResultまたはDiagnosticをJSONで出力 |

出力は32-bit float、2 Channel、指定Sample RateのWAVです。親Directoryは事前に作成してください。

## Exit Code

| Code | 意味 |
|---:|---|
| `0` | 成功 |
| `1` | Definition / Compile Error（予約） |
| `2` | CLI入力またはRender Request Error |
| `3` | Core Process / Render Error |
| `4` | WAV出力 Error |

入力不正はJSON時に次の形で返ります。

```json
{
  "status": "error",
  "exit_code": 2,
  "diagnostics": [
    {
      "code": "VALUE_OUT_OF_RANGE",
      "severity": "error",
      "path": null,
      "message": "block size must be greater than zero",
      "detail": null
    }
  ]
}
```

## 責務境界

CLIはclapのArgument型、Terminal表示、Path、WAV Writer、Exit Codeを所有します。CoreへCLI型やhound型を渡しません。Native DSPを直接呼ばず、必ずCore Rendererを経由します。
