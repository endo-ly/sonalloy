#!/usr/bin/env python3
"""Generate the deterministic Basic Generator sound review package."""

from __future__ import annotations

import copy
import hashlib
import json
import math
import subprocess
from pathlib import Path

from measure_wav import compare_wav, measure, read_float_wav

ROOT = Path(__file__).resolve().parents[2]
SAMPLE_RATE = 48_000
BASE_BLOCK_SIZE = 257
BLOCK_SIZES = (32, 64, 257, 1024)
BLOCK_SIZE_MAX_DIFFERENCE = 1.0e-5
EVENT_DURATION_FRAMES = 16_384


def cli_command() -> list[str]:
    candidates = (
        ROOT / "target" / "debug" / "sonalloy.exe",
        ROOT / "target" / "debug" / "sonalloy",
        ROOT / "target" / "release" / "sonalloy.exe",
        ROOT / "target" / "release" / "sonalloy",
    )
    for candidate in candidates:
        if candidate.exists():
            return [str(candidate)]
    return ["cargo", "run", "--quiet", "-p", "sonalloy-cli", "--"]


def run_cli(arguments: list[str]) -> str:
    result = subprocess.run(
        cli_command() + arguments,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if result.returncode != 0:
        details = "\n".join(
            part for part in (result.stdout, result.stderr) if part
        ).strip()
        raise RuntimeError(f"CLI failed with exit code {result.returncode}: {details}")
    return result.stdout


def write_utf8(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content.encode("utf-8"))


def write_definition(path: Path, value: dict[str, object]) -> None:
    write_utf8(path, json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def write_events(path: Path, events: list[dict[str, object]]) -> None:
    write_utf8(path, json.dumps({"events": events}, ensure_ascii=False, indent=2) + "\n")


def layer(value: dict[str, object], layer_id: str) -> dict[str, object]:
    for candidate in value["layers"]:
        if candidate["id"] == layer_id:
            return candidate
    raise KeyError(layer_id)


def without_modulation(value: dict[str, object]) -> dict[str, object]:
    result = copy.deepcopy(value)
    result.pop("modulation", None)
    return result


def set_pulse_width(value: dict[str, object], pulse_width: float) -> None:
    generator = layer(value, "pulse")["generator"]["oscillator"]
    generator["waveform"]["pulse_width"] = pulse_width


def set_oscillator_waveform(
    value: dict[str, object], layer_id: str, waveform: str
) -> None:
    generator = layer(value, layer_id)["generator"]["oscillator"]
    generator["waveform"] = {"type": waveform}


def set_noise_correlation(value: dict[str, object], correlation: float) -> None:
    layer(value, "pink")["generator"]["noise"]["stereo_correlation"] = correlation


def render_note(
    definition: Path,
    note: int,
    output: Path,
    block_size: int,
    sample_rate: int = SAMPLE_RATE,
) -> None:
    run_cli(
        [
            "render",
            "note",
            str(definition),
            "--note",
            str(note),
            "--velocity",
            "112",
            "--gate",
            "0.15",
            "--tail",
            "0.1",
            "--sample-rate",
            str(sample_rate),
            "--block-size",
            str(block_size),
            "--output",
            str(output),
            "--json",
        ]
    )


def render_events(definition: Path, events: Path, output: Path, block_size: int) -> None:
    run_cli(
        [
            "render",
            "events",
            str(definition),
            str(events),
            "--duration-frames",
            str(EVENT_DURATION_FRAMES),
            "--sample-rate",
            str(SAMPLE_RATE),
            "--block-size",
            str(block_size),
            "--tail",
            "0",
            "--output",
            str(output),
            "--json",
        ]
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def measure_stereo(path: Path) -> dict[str, object]:
    sample_rate, channels, samples = read_float_wav(path)
    if channels != 2:
        raise ValueError(f"expected stereo WAV: {path}")
    left = samples[0::2]
    right = samples[1::2]
    left_mean = sum(left) / len(left) if left else 0.0
    right_mean = sum(right) / len(right) if right else 0.0
    covariance = sum(
        (left_sample - left_mean) * (right_sample - right_mean)
        for left_sample, right_sample in zip(left, right)
    )
    left_variance = sum((sample - left_mean) ** 2 for sample in left)
    right_variance = sum((sample - right_mean) ** 2 for sample in right)
    denominator = math.sqrt(left_variance * right_variance)
    correlation = covariance / denominator if denominator > 0.0 else 1.0
    difference_rms = math.sqrt(
        sum((left_sample - right_sample) ** 2 for left_sample, right_sample in zip(left, right))
        / len(left)
    ) if left else 0.0
    return {
        "sample_rate": sample_rate,
        "stereo_rms_difference": difference_rms,
        "stereo_correlation": correlation,
    }


def main() -> None:
    source_path = ROOT / "examples" / "instruments" / "basic-generators-reference.json"
    source = json.loads(source_path.read_text(encoding="utf-8"))
    review_root = ROOT / "review-output" / "basic-generators"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    technical_dir = review_root / "audio" / "technical"
    for directory in (definition_dir, event_dir, technical_dir):
        directory.mkdir(parents=True, exist_ok=True)

    definitions: dict[str, dict[str, object]] = {}
    static = without_modulation(source)
    definitions["basic-generators-reference"] = source

    sine = copy.deepcopy(static)
    set_oscillator_waveform(sine, "square", "sine")
    definitions["sine-reference"] = sine

    saw = copy.deepcopy(static)
    set_oscillator_waveform(saw, "square", "saw")
    definitions["saw-reference"] = saw

    definitions["square"] = static
    definitions["triangle"] = static
    definitions["pulse-width-025"] = static

    high_register_square = copy.deepcopy(static)
    high_register_trigger = layer(high_register_square, "square")["trigger"]
    high_register_trigger["key_min"] = 108
    high_register_trigger["key_max"] = 108
    definitions["square-high-register"] = high_register_square

    pulse_wide = copy.deepcopy(static)
    set_pulse_width(pulse_wide, 0.75)
    definitions["pulse-width-075"] = pulse_wide

    definitions["white-noise"] = static
    definitions["pink-noise"] = static
    definitions["brown-noise"] = static

    pink_correlation_one = copy.deepcopy(static)
    set_noise_correlation(pink_correlation_one, 1.0)
    definitions["pink-correlation-1"] = pink_correlation_one

    pink_correlation_zero = copy.deepcopy(static)
    set_noise_correlation(pink_correlation_zero, 0.0)
    definitions["pink-correlation-0"] = pink_correlation_zero

    correlation_ramp = copy.deepcopy(static)
    set_noise_correlation(correlation_ramp, 1.0)
    definitions["noise-correlation-ramp"] = correlation_ramp

    definition_paths: dict[str, Path] = {}
    for name, value in definitions.items():
        path = definition_dir / f"{name}.json"
        write_definition(path, value)
        definition_paths[name] = path
        run_cli(["instrument", "validate", str(path), "--json"])

    inspect_json = run_cli(
        ["instrument", "inspect", str(definition_paths["basic-generators-reference"]), "--json"]
    )
    write_utf8(review_root / "inspect.json", inspect_json)

    pwm_events = event_dir / "pwm-lfo.json"
    write_events(
        pwm_events,
        [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 1,
                "note": 55,
                "velocity": 112,
            },
            {
                "absolute_frame": 4_096,
                "type": "parameter_change",
                "parameter": "layer.pulse.generator.pulse_width",
                "normalized": 0.7777778,
            },
            {"absolute_frame": 12_000, "type": "note_off", "note_id": 1},
        ],
    )
    correlation_events = event_dir / "noise-correlation-ramp.json"
    write_events(
        correlation_events,
        [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 2,
                "note": 64,
                "velocity": 112,
            },
            {
                "absolute_frame": 4_096,
                "type": "parameter_change",
                "parameter": "layer.pink.generator.noise_correlation",
                "normalized": 0.0,
            },
            {"absolute_frame": 12_000, "type": "note_off", "note_id": 2},
        ],
    )

    note_jobs = [
        ("01-sine-reference.wav", "sine-reference", 48),
        ("02-saw-reference.wav", "saw-reference", 48),
        ("03-square.wav", "square", 48),
        ("04-triangle.wav", "triangle", 52),
        ("05-pulse-width-025.wav", "pulse-width-025", 55),
        ("06-pulse-width-075.wav", "pulse-width-075", 55),
        ("07-white-noise.wav", "white-noise", 60),
        ("08-pink-noise.wav", "pink-noise", 64),
        ("09-brown-noise.wav", "brown-noise", 67),
        ("10-pink-correlation-1.wav", "pink-correlation-1", 64),
        ("11-pink-correlation-0.wav", "pink-correlation-0", 64),
        ("14-high-register-square.wav", "square-high-register", 108),
    ]
    for audio_name, definition_name, note in note_jobs:
        render_note(
            definition_paths[definition_name],
            note,
            technical_dir / audio_name,
            BASE_BLOCK_SIZE,
        )
    render_events(
        definition_paths["basic-generators-reference"],
        pwm_events,
        technical_dir / "12-pwm-lfo.wav",
        BASE_BLOCK_SIZE,
    )
    render_events(
        definition_paths["noise-correlation-ramp"],
        correlation_events,
        technical_dir / "13-noise-correlation-ramp.wav",
        BASE_BLOCK_SIZE,
    )

    regression_definition = definition_paths["pink-correlation-1"]
    regression_paths: dict[str, Path] = {}
    for block_size in BLOCK_SIZES:
        path = technical_dir / f"regression-block-{block_size}.wav"
        render_note(regression_definition, 64, path, block_size)
        regression_paths[str(block_size)] = path
    reset_a = technical_dir / "regression-reset-a.wav"
    reset_b = technical_dir / "regression-reset-b.wav"
    render_note(regression_definition, 64, reset_a, BASE_BLOCK_SIZE)
    render_note(regression_definition, 64, reset_b, BASE_BLOCK_SIZE)

    sample_rate_paths: dict[str, Path] = {}
    for sample_rate in (44_100, SAMPLE_RATE, 96_000):
        path = technical_dir / f"sample-rate-{sample_rate}.wav"
        render_note(regression_definition, 64, path, BASE_BLOCK_SIZE, sample_rate)
        sample_rate_paths[str(sample_rate)] = path

    technical_metrics: dict[str, dict[str, object]] = {}
    for path in sorted(technical_dir.glob("*.wav")):
        values = measure(
            path,
            list(BLOCK_SIZES),
            include_spectrum=path.name in {
                "01-sine-reference.wav",
                "02-saw-reference.wav",
                "03-square.wav",
                "04-triangle.wav",
                "05-pulse-width-025.wav",
                "06-pulse-width-075.wav",
                "14-high-register-square.wav",
            },
        )
        values.update(measure_stereo(path))
        technical_metrics[path.name] = values
    invalid_audio = [
        name for name, values in technical_metrics.items() if not values["finite"]
    ]
    if invalid_audio:
        raise RuntimeError(f"basic generator audio checks failed: {invalid_audio}")
    block_comparisons = {
        block_size: compare_wav(regression_paths["257"], regression_paths[str(block_size)])
        for block_size in BLOCK_SIZES
    }
    invalid_block_comparisons = {
        block_size: comparison
        for block_size, comparison in block_comparisons.items()
        if not comparison.get("compatible")
        or comparison.get("max_abs_difference", 1.0) > BLOCK_SIZE_MAX_DIFFERENCE
    }
    if invalid_block_comparisons:
        raise RuntimeError(f"basic generator block-size mismatch: {invalid_block_comparisons}")
    reset_comparison = compare_wav(reset_a, reset_b)
    if (
        not reset_comparison.get("compatible")
        or reset_comparison.get("max_abs_difference", 1.0) != 0.0
    ):
        raise RuntimeError(f"basic generator reset is not reproducible: {reset_comparison}")
    metrics = {
        "sample_rate": SAMPLE_RATE,
        "base_block_size": BASE_BLOCK_SIZE,
        "block_sizes": list(BLOCK_SIZES),
        "technical": technical_metrics,
        "block_size_comparisons": block_comparisons,
        "sample_rate_metrics": {
            sample_rate: technical_metrics[path.name]
            for sample_rate, path in sample_rate_paths.items()
        },
        "reset_comparison": {
            **reset_comparison,
            "reference_sha256": sha256_file(reset_a),
            "repeat_sha256": sha256_file(reset_b),
        },
    }
    write_utf8(review_root / "metrics.json", json.dumps(metrics, ensure_ascii=False, indent=2) + "\n")

    summary = """# Basic Generator Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Backend：DaisySP V1.0.0 (`a0494a3adb67f549e18dfd71a35fa656f65b38b6`)

## 入力

Definitionは`definitions/`、Eventは`events/`、WAVは`audio/technical/`へ保存しています。同じWAVをMetricsと人間の試聴に使用します。`inspect.json`にはBasic GeneratorのCompiled表示を保存しています。

再生成：

```bash
python scripts/review/generate_basic_generators_package.py
```

## 音声一覧

| WAV | 目的 |
|---|---|
| `01-sine-reference.wav` | Existing Sine Baseline |
| `02-saw-reference.wav` | Existing Saw Baseline |
| `03-square.wav` | Band-limited Square |
| `04-triangle.wav` | Band-limited Triangle |
| `05-pulse-width-025.wav` | Pulse Width 0.25 |
| `06-pulse-width-075.wav` | Pulse Width 0.75 |
| `07-white-noise.wav` | White Noise |
| `08-pink-noise.wav` | Pink Noise |
| `09-brown-noise.wav` | Brown Noise |
| `10-pink-correlation-1.wav` | Correlation 1 |
| `11-pink-correlation-0.wav` | Correlation 0 |
| `12-pwm-lfo.wav` | Existing LFOによるPulse Width Modulation |
| `13-noise-correlation-ramp.wav` | Noise Correlation Parameter Change |
| `14-high-register-square.wav` | High-register aliasing |

## 機械検査

`metrics.json`は全WAVのFinite性、Peak、RMS、DC、隣接Frame差分、固定長Spectrum、左右差、Stereo Correlation、Sample Rate別値、Block Size比較、Reset比較を記録します。WAVは正規化せず、Metricsと試聴で同じ生出力を使用します。聴感比較時の音量は再生側で調整してください。

## 人間の確認欄

- [ ] Square / Triangle / Pulseの音色差が明確である
- [ ] 高音域で耳障りなAliasが強すぎない
- [ ] Pulse Width 0.25 / 0.75の差が明確である
- [ ] PWMにClickやBlock境界の不連続がない
- [ ] White / Pink / Brownの差が明確である
- [ ] Brownが低域へ過度に偏らず、DC感が強すぎない
- [ ] Pinkに不自然な周期性がない
- [ ] Correlation 0 / 1でStereo幅の差が明確である
- [ ] Reset後にNoiseの冒頭が不自然に変化しない

### 人間の回答

- 判定：
- 修正指示：
- 確認者：
- 確認日：
"""
    write_utf8(review_root / "review-summary.md", summary)


if __name__ == "__main__":
    main()
