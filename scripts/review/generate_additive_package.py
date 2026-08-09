#!/usr/bin/env python3
"""Generate the deterministic Additive Generator sound review package."""

from __future__ import annotations

import copy
import json
import math
from pathlib import Path
import struct

from common import (
    BASE_BLOCK_SIZE,
    BLOCK_SIZES,
    ROOT,
    SAMPLE_RATE,
    measure_stereo,
    render_events,
    render_note,
    run_cli,
    sha256_file,
    write_definition,
    write_events,
    write_utf8,
)
from measure_wav import compare_wav, measure, read_float_wav

BLOCK_SIZE_MAX_DIFFERENCE = 1.0e-5
SINE_TABLE_LENGTH = 4096


def partial(
    identifier: str,
    ratio: float,
    amplitude_a: float,
    amplitude_b: float,
    phase: float = 0.0,
    envelope: dict[str, float] | None = None,
) -> dict[str, object]:
    value: dict[str, object] = {
        "id": identifier,
        "ratio": ratio,
        "amplitude_a": amplitude_a,
        "amplitude_b": amplitude_b,
        "phase": phase,
    }
    if envelope is not None:
        value["envelope"] = envelope
    return value


def layer(value: dict[str, object]) -> dict[str, object]:
    return value["layers"][0]


def additive(
    source: dict[str, object],
    partials: list[dict[str, object]],
    morph: float = 0.0,
    tilt: float = 0.0,
    inharmonicity: float = 0.0,
) -> dict[str, object]:
    value = copy.deepcopy(source)
    target_layer = layer(value)
    target_layer["gain_db"] = -6.0
    target_layer["pan"] = 0.0
    target_layer["envelope"] = {
        "attack_seconds": 0.0,
        "decay_seconds": 0.05,
        "sustain_level": 1.0,
        "release_seconds": 0.12,
    }
    target_layer["processors"] = []
    target_layer["generator"] = {
        "additive": {
            "phase_reset": True,
            "morph": morph,
            "spectrum_tilt_db_per_octave": tilt,
            "inharmonicity": inharmonicity,
            "partials": partials,
        }
    }
    value["voice_processors"] = []
    value["global_processors"] = []
    value["modulation"] = None
    return value


def render_event_file(
    definition: Path,
    event_file: Path,
    output: Path,
    duration_frames: int = 16_384,
) -> None:
    render_events(
        definition,
        event_file,
        output,
        BASE_BLOCK_SIZE,
        duration_frames=duration_frames,
        tail_seconds=0.0,
    )


def _float32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def sine_table_metrics() -> dict[str, float | int]:
    table = [
        _float32(math.sin(math.tau * index / SINE_TABLE_LENGTH))
        for index in range(SINE_TABLE_LENGTH + 1)
    ]
    maximum_error = 0.0
    sample_count = SINE_TABLE_LENGTH * 16
    for index in range(sample_count + 1):
        phase = _float32(index / sample_count)
        position = _float32(phase * SINE_TABLE_LENGTH)
        table_index = min(int(position), SINE_TABLE_LENGTH - 1)
        fraction = _float32(position - table_index)
        interpolated = _float32(
            table[table_index]
            + _float32((table[table_index + 1] - table[table_index]) * fraction)
        )
        maximum_error = max(
            maximum_error,
            abs(interpolated - _float32(math.sin(math.tau * phase))),
        )
    return {
        "length": SINE_TABLE_LENGTH,
        "guard_samples": 1,
        "sample_count": sample_count + 1,
        "max_absolute_error": maximum_error,
    }


def high_frequency_energy(path: Path, cutoff_ratio: float = 0.2) -> dict[str, float | int]:
    sample_rate, channels, samples = read_float_wav(path)
    frames = len(samples) // channels
    fft_size = min(4096, frames)
    if fft_size < 4:
        return {
            "sample_rate": sample_rate,
            "cutoff_ratio": cutoff_ratio,
            "cutoff_hz": sample_rate * cutoff_ratio,
            "fft_size": fft_size,
            "high_frequency_energy_ratio": 0.0,
        }
    left = samples[0::channels]
    start = min(frames - fft_size, int(sample_rate * 0.2))
    window = [
        left[start + index]
        * (0.5 - 0.5 * math.cos(2.0 * math.pi * index / (fft_size - 1)))
        for index in range(fft_size)
    ]
    total_energy = 0.0
    high_energy = 0.0
    bin_width = sample_rate / fft_size
    for bin_index in range(1, fft_size // 2 + 1):
        angle_step = 2.0 * math.pi * bin_index / fft_size
        real = sum(sample * math.cos(angle_step * index) for index, sample in enumerate(window))
        imaginary = sum(
            -sample * math.sin(angle_step * index)
            for index, sample in enumerate(window)
        )
        energy = real * real + imaginary * imaginary
        total_energy += energy
        if bin_index * bin_width >= sample_rate * cutoff_ratio:
            high_energy += energy
    return {
        "sample_rate": sample_rate,
        "cutoff_ratio": cutoff_ratio,
        "cutoff_hz": sample_rate * cutoff_ratio,
        "fft_size": fft_size,
        "high_frequency_energy_ratio": high_energy / total_energy if total_energy else 0.0,
    }


def main() -> None:
    source_path = ROOT / "examples" / "instruments" / "additive-generator-reference.json"
    source = json.loads(source_path.read_text(encoding="utf-8"))
    review_root = ROOT / "review-output" / "additive-generator"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    technical_dir = review_root / "audio" / "technical"
    for directory in (definition_dir, event_dir, technical_dir):
        directory.mkdir(parents=True, exist_ok=True)

    harmonic_partials = [
        partial("fundamental", 1.0, 1.0, 0.7),
        partial("second", 2.0, 0.45, 0.8),
        partial("third", 3.0, 0.3, 0.65),
        partial("fifth", 5.0, 0.22, 0.45),
        partial("seventh", 7.0, 0.12, 0.3),
        partial("ninth", 9.0, 0.08, 0.22),
        partial("twelfth", 12.0, 0.04, 0.16),
    ]
    definitions = {
        "fundamental": additive(
            source, [partial("fundamental", 1.0, 1.0, 1.0)]
        ),
        "harmonic-organ": additive(source, harmonic_partials, tilt=-3.0),
        "inharmonic-bell": additive(
            source,
            harmonic_partials
            + [partial("metal", 2.73, 0.15, 0.5, 0.25)],
            tilt=-6.0,
            inharmonicity=1.0,
        ),
        "spectrum-a": additive(source, harmonic_partials, tilt=-12.0),
        "spectrum-b": additive(
            source,
            [
                partial("fundamental", 1.0, 0.7, 0.7),
                partial("second", 2.0, 0.8, 0.8),
                partial("third", 3.0, 0.65, 0.65),
                partial("fifth", 5.0, 0.45, 0.45),
                partial("seventh", 7.0, 0.3, 0.3),
                partial("ninth", 9.0, 0.22, 0.22),
                partial("twelfth", 12.0, 0.16, 0.16),
            ],
            tilt=6.0,
        ),
        "morph-sweep": additive(source, harmonic_partials, tilt=-12.0),
        "tilt-sweep": additive(source, harmonic_partials, tilt=-24.0),
        "inharmonicity-sweep": additive(source, harmonic_partials),
        "partial-envelope-bell": additive(
            source,
            [
                partial("fundamental", 1.0, 0.9, 0.9),
                partial(
                    "transient",
                    3.0,
                    0.8,
                    0.8,
                    envelope={
                        "attack_seconds": 0.0,
                        "decay_seconds": 0.18,
                        "sustain_level": 0.0,
                        "release_seconds": 0.05,
                    },
                ),
                partial(
                    "metal",
                    5.37,
                    0.4,
                    0.4,
                    0.13,
                    {
                        "attack_seconds": 0.0,
                        "decay_seconds": 0.35,
                        "sustain_level": 0.08,
                        "release_seconds": 0.1,
                    },
                ),
            ],
            tilt=-3.0,
        ),
        "high-note-alias": additive(
            source,
            harmonic_partials
            + [partial("upper_16", 16.0, 0.2, 0.2), partial("upper_32", 32.0, 0.12, 0.12)],
            tilt=0.0,
        ),
        "additive-polyphony": additive(source, harmonic_partials, tilt=-3.0),
    }

    definition_paths: dict[str, Path] = {}
    for name, value in definitions.items():
        path = definition_dir / f"{name}.json"
        write_definition(path, value)
        definition_paths[name] = path
        run_cli(["instrument", "validate", str(path), "--json"])

    inspect = run_cli(
        ["instrument", "inspect", str(definition_paths["harmonic-organ"]), "--json"]
    )
    write_utf8(review_root / "inspect.json", inspect)

    event_values = {
        "morph-sweep": [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 1,
                "note": 60,
                "velocity": 112,
            },
            {
                "absolute_frame": 4_096,
                "type": "parameter_change",
                "parameter": "layer.body.generator.additive_morph",
                "normalized": 1.0,
            },
            {"absolute_frame": 12_000, "type": "note_off", "note_id": 1},
        ],
        "tilt-sweep": [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 2,
                "note": 60,
                "velocity": 112,
            },
            {
                "absolute_frame": 4_096,
                "type": "parameter_change",
                "parameter": "layer.body.generator.additive_spectrum_tilt",
                "normalized": 1.0,
            },
            {"absolute_frame": 12_000, "type": "note_off", "note_id": 2},
        ],
        "inharmonicity-sweep": [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 3,
                "note": 60,
                "velocity": 112,
            },
            {
                "absolute_frame": 4_096,
                "type": "parameter_change",
                "parameter": "layer.body.generator.additive_inharmonicity",
                "normalized": 1.0,
            },
            {"absolute_frame": 12_000, "type": "note_off", "note_id": 3},
        ],
    }
    event_paths: dict[str, Path] = {}
    for name, events in event_values.items():
        path = event_dir / f"{name}.json"
        write_events(path, events)
        event_paths[name] = path

    note_jobs = [
        ("01-additive-fundamental.wav", "fundamental", 60),
        ("02-harmonic-organ.wav", "harmonic-organ", 60),
        ("03-inharmonic-bell.wav", "inharmonic-bell", 60),
        ("04-spectrum-a.wav", "spectrum-a", 60),
        ("05-spectrum-b.wav", "spectrum-b", 60),
        ("09-partial-envelope-bell.wav", "partial-envelope-bell", 60),
        ("10-high-note-alias-check.wav", "high-note-alias", 108),
    ]
    generated_paths: list[Path] = []
    for audio_name, definition_name, note in note_jobs:
        path = technical_dir / audio_name
        render_note(
            definition_paths[definition_name],
            note,
            path,
            BASE_BLOCK_SIZE,
            gate_seconds=0.25,
            tail_seconds=0.1,
        )
        generated_paths.append(path)

    for audio_name, name in (
        ("06-spectrum-morph-sweep.wav", "morph-sweep"),
        ("07-spectrum-tilt-sweep.wav", "tilt-sweep"),
        ("08-inharmonicity-sweep.wav", "inharmonicity-sweep"),
    ):
        path = technical_dir / audio_name
        render_event_file(definition_paths[name], event_paths[name], path)
        generated_paths.append(path)

    polyphony_events = [
        {
            "absolute_frame": 0,
            "type": "note_on",
            "note_id": index + 1,
            "note": 48 + index,
            "velocity": 96 + (index % 24),
        }
        for index in range(16)
    ]
    polyphony_events.extend(
        {"absolute_frame": 12_000 + index * 64, "type": "note_off", "note_id": index + 1}
        for index in range(16)
    )
    polyphony_path = event_dir / "additive-polyphony.json"
    write_events(polyphony_path, polyphony_events)
    polyphony_audio = technical_dir / "11-additive-polyphony.wav"
    render_event_file(
        definition_paths["additive-polyphony"],
        polyphony_path,
        polyphony_audio,
        duration_frames=16_384,
    )
    generated_paths.append(polyphony_audio)

    regression_paths: dict[str, Path] = {}
    for block_size in BLOCK_SIZES:
        path = technical_dir / f"regression-block-{block_size}.wav"
        render_note(
            definition_paths["harmonic-organ"],
            60,
            path,
            block_size,
            gate_seconds=0.25,
            tail_seconds=0.1,
        )
        regression_paths[str(block_size)] = path
        generated_paths.append(path)

    sample_rate_paths: dict[str, Path] = {}
    for sample_rate in (44_100, SAMPLE_RATE, 96_000):
        path = technical_dir / f"sample-rate-{sample_rate}.wav"
        render_note(
            definition_paths["harmonic-organ"],
            60,
            path,
            BASE_BLOCK_SIZE,
            sample_rate,
            gate_seconds=0.25,
            tail_seconds=0.1,
        )
        sample_rate_paths[str(sample_rate)] = path
        generated_paths.append(path)

    fresh_a = technical_dir / "fresh-a.wav"
    fresh_b = technical_dir / "fresh-b.wav"
    render_note(definition_paths["harmonic-organ"], 60, fresh_a, BASE_BLOCK_SIZE, gate_seconds=0.25, tail_seconds=0.1)
    render_note(definition_paths["harmonic-organ"], 60, fresh_b, BASE_BLOCK_SIZE, gate_seconds=0.25, tail_seconds=0.1)
    generated_paths.extend((fresh_a, fresh_b))

    metrics: dict[str, object] = {
        "sample_rate": SAMPLE_RATE,
        "base_block_size": BASE_BLOCK_SIZE,
        "block_sizes": list(BLOCK_SIZES),
        "sine_table": sine_table_metrics(),
        "audio": {},
    }
    audio_metrics: dict[str, object] = {}
    for path in sorted(generated_paths):
        values = measure(
            path,
            list(BLOCK_SIZES),
            include_spectrum=path.name in {
                "01-additive-fundamental.wav",
                "02-harmonic-organ.wav",
                "03-inharmonic-bell.wav",
                "10-high-note-alias-check.wav",
            },
        )
        values.update(measure_stereo(path))
        audio_metrics[path.name] = values
    metrics["audio"] = audio_metrics

    invalid_audio = [
        name for name, values in audio_metrics.items() if not values["finite"]
    ]
    if invalid_audio:
        raise RuntimeError(f"additive audio checks failed: {invalid_audio}")
    if metrics["sine_table"]["max_absolute_error"] > 1.0e-5:
        raise RuntimeError(f"sine table error exceeded the contract: {metrics['sine_table']}")

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
        raise RuntimeError(f"additive block-size mismatch: {invalid_block_comparisons}")
    fresh_comparison = compare_wav(fresh_a, fresh_b)
    if (
        not fresh_comparison.get("compatible")
        or fresh_comparison.get("max_abs_difference", 1.0) != 0.0
    ):
        raise RuntimeError(f"additive fresh render is not reproducible: {fresh_comparison}")
    metrics["block_size_comparisons"] = block_comparisons
    metrics["parameter_comparisons"] = {
        "spectrum_a_to_b": compare_wav(
            technical_dir / "04-spectrum-a.wav", technical_dir / "05-spectrum-b.wav"
        ),
        "harmonic_to_inharmonic": compare_wav(
            technical_dir / "02-harmonic-organ.wav", technical_dir / "03-inharmonic-bell.wav"
        ),
    }
    metrics["high_frequency_energy"] = {
        "harmonic_organ": high_frequency_energy(technical_dir / "02-harmonic-organ.wav"),
        "high_note_alias_check": high_frequency_energy(
            technical_dir / "10-high-note-alias-check.wav"
        ),
    }
    metrics["sample_rate_metrics"] = {
        sample_rate: audio_metrics[path.name]
        for sample_rate, path in sample_rate_paths.items()
    }
    metrics["fresh_render_comparison"] = {
        **fresh_comparison,
        "first_sha256": sha256_file(fresh_a),
        "second_sha256": sha256_file(fresh_b),
    }
    write_utf8(review_root / "metrics.json", json.dumps(metrics, ensure_ascii=False, indent=2) + "\n")

    summary = """# Additive Generator Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV

## 入力

Definitionは`definitions/`、Eventは`events/`、WAVは`audio/technical/`へ保存しています。`inspect.json`にはPartial構造とParameter Descriptorを保存しています。

再生成：

```bash
python scripts/review/generate_additive_package.py
```

## 音声一覧

| WAV | 目的 |
|---|---|
| `01-additive-fundamental.wav` | Single Fundamental |
| `02-harmonic-organ.wav` | Harmonic Organ |
| `03-inharmonic-bell.wav` | Fractional Ratio and Inharmonicity |
| `04-spectrum-a.wav` / `05-spectrum-b.wav` | Spectrum A / B |
| `06-spectrum-morph-sweep.wav` | Spectrum Morph |
| `07-spectrum-tilt-sweep.wav` | Spectrum Tilt |
| `08-inharmonicity-sweep.wav` | Global Inharmonicity |
| `09-partial-envelope-bell.wav` | Partial Envelope |
| `10-high-note-alias-check.wav` | High-note Alias Fade |
| `11-additive-polyphony.wav` | 16-note Polyphony |

## 機械検査

`metrics.json`はSine TableのLength / Guard / Lookup最大絶対誤差、Finite性、Peak、RMS、DC、隣接Frame差分、単音Spectrum、Spectrum A / B差分、Inharmonicity差分、高周波Energy、Sample Rate別値、Block Size比較、Fresh Render再現性を記録します。WAVは正規化せず、Metricsと試聴で同じ生出力を使用します。

## 人間の確認

- Harmonic Organで基音と整数倍Partialが明確に聞こえ、BzzzやClickがない
- Inharmonic BellでInteger Harmonicとの差と金属的な質感が聞き取れる
- Spectrum Morphが連続し、中間値で音量が急落・急増しない
- Partial Envelope終了時に残りPartialのGainが段差変化しない
- High-note Aliasで高域Partialが主音として折り返さず、自然に薄くなる
- Polyphonyで音量、Pitch、Reset、Voice Stealingが安定している
"""
    write_utf8(review_root / "review-summary.md", summary)


if __name__ == "__main__":
    main()
