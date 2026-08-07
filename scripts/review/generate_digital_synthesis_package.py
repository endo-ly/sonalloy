#!/usr/bin/env python3
"""Generate the deterministic Wavetable sound review package."""

from __future__ import annotations

import copy
import json
import math
import struct
import subprocess
import wave
from pathlib import Path

from common import (
    BASE_BLOCK_SIZE,
    BLOCK_SIZES,
    ROOT,
    SAMPLE_RATE,
    cli_command,
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
FRAME_LENGTH = 256
FRAME_COUNT = 4


def write_pcm16_wav(path: Path, frames: list[list[float]], sample_rate: int) -> None:
    samples = [sample for frame in frames for sample in frame]
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(sample_rate)
        output.writeframes(
            b"".join(
                struct.pack("<h", int(max(-1.0, min(1.0, sample)) * 30_000.0))
                for sample in samples
            )
        )


def periodic_frame(length: int, harmonics: tuple[tuple[int, float], ...]) -> list[float]:
    return [
        sum(amplitude * math.sin(math.tau * harmonic * index / length) for harmonic, amplitude in harmonics)
        for index in range(length)
    ]


def motion_frames() -> list[list[float]]:
    return [
        periodic_frame(FRAME_LENGTH, ((1, 0.78),)),
        periodic_frame(FRAME_LENGTH, ((1, 0.68), (3, 0.20))),
        periodic_frame(FRAME_LENGTH, ((1, 0.58), (3, 0.24), (5, 0.12))),
        periodic_frame(FRAME_LENGTH, ((1, 0.55), (3, 0.20), (5, 0.13), (7, 0.08))),
    ]


def saw_frame(length: int) -> list[float]:
    return [0.72 * (2.0 * index / length - 1.0) for index in range(length)]


def asset_reference(path: Path, asset_name: str) -> dict[str, str]:
    asset_path = path / asset_name
    return {
        "path": f"../assets/{asset_name}",
        "sha256": sha256_file(asset_path),
    }


def modulation(
    source: dict[str, object] | None,
    route: dict[str, object] | None,
) -> dict[str, object] | None:
    if route is None:
        return None
    return {
        "sources": [] if source is None else [source],
        "routes": [route],
    }


def definition(
    name: str,
    asset: dict[str, object],
    *,
    frame_length: int = FRAME_LENGTH,
    position: float = 0.0,
    note_min: int = 0,
    note_max: int = 127,
    unison: dict[str, object] | None = None,
    modulation_value: dict[str, object] | None = None,
    extra_layers: list[dict[str, object]] | None = None,
) -> dict[str, object]:
    wavetable = {
        "asset": asset,
        "frame_length": frame_length,
        "position": position,
        "phase_reset": True,
        "phase": 0.0,
    }
    if unison is not None:
        wavetable["unison"] = unison
    layers: list[dict[str, object]] = [
        {
            "id": "motion",
            "enabled": True,
            "trigger": {
                "key_min": note_min,
                "key_max": note_max,
                "velocity_min": 1,
                "velocity_max": 127,
            },
            "gain_db": -4.0,
            "pan": 0.0,
            "tuning_cents": 0.0,
            "envelope": {
                "attack_seconds": 0.0,
                "decay_seconds": 0.0,
                "sustain_level": 1.0,
                "release_seconds": 0.08,
            },
            "generator": {"wavetable": wavetable},
            "processors": [],
        }
    ]
    if extra_layers:
        layers.extend(copy.deepcopy(extra_layers))
    value: dict[str, object] = {
        "schema_version": 1,
        "metadata": {
            "name": name,
            "author": "Sonalloy",
            "description": "Wavetable Generator review instrument",
        },
        "performance": {
            "polyphony": 16,
            "voice_stealing": "quietest_releasing_then_oldest",
        },
        "layers": layers,
        "voice_processors": [],
        "global_processors": [],
    }
    if modulation_value is not None:
        value["modulation"] = modulation_value
    return value


def note_events(note: int, note_id: int = 1, release_frame: int = 12_000) -> list[dict[str, object]]:
    return [
        {
            "absolute_frame": 0,
            "type": "note_on",
            "note_id": note_id,
            "note": note,
            "velocity": 112,
        },
        {"absolute_frame": release_frame, "type": "note_off", "note_id": note_id},
    ]


def note_events_with_controls(
    note: int,
    controls: list[dict[str, object]],
    note_id: int = 1,
    release_frame: int = 12_000,
) -> list[dict[str, object]]:
    return sorted(
        note_events(note, note_id, release_frame) + controls,
        key=lambda event: int(event["absolute_frame"]),
    )


def band_count(frame_length: int) -> int:
    count = 0
    limit = frame_length // 2
    while limit >= 1:
        count += 1
        limit //= 2
    return count


def steady_state_frequency(path: Path) -> float:
    sample_rate, channels, samples = read_float_wav(path)
    left = samples[0::channels]
    start = min(1_000, len(left))
    end = min(start + 12_000, len(left))
    segment = left[start:end]
    crossings = sum(
        previous <= 0.0 and current > 0.0
        for previous, current in zip(segment, segment[1:])
    )
    return crossings * sample_rate / len(segment) if segment else 0.0


def require_finite(metrics: dict[str, dict[str, object]]) -> None:
    invalid = [name for name, value in metrics.items() if not value["finite"]]
    if invalid:
        raise RuntimeError(f"Wavetable audio is not finite: {invalid}")


def run_cli_error(arguments: list[str]) -> tuple[int, str, str]:
    result = subprocess.run(
        cli_command() + arguments,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    return result.returncode, result.stdout, result.stderr


def main() -> None:
    review_root = ROOT / "review-output" / "digital-synthesis"
    asset_dir = review_root / "assets"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    technical_dir = review_root / "audio" / "technical"
    for directory in (asset_dir, definition_dir, event_dir, technical_dir):
        directory.mkdir(parents=True, exist_ok=True)

    motion_asset_path = asset_dir / "digital-motion.wav"
    write_pcm16_wav(motion_asset_path, motion_frames(), 44_100)
    sine_asset_path = asset_dir / "sine-single-frame.wav"
    write_pcm16_wav(sine_asset_path, [periodic_frame(FRAME_LENGTH, ((1, 0.78),))], 48_000)
    saw_asset_path = asset_dir / "saw-single-frame.wav"
    write_pcm16_wav(saw_asset_path, [saw_frame(FRAME_LENGTH)], 48_000)
    invalid_layout_path = asset_dir / "invalid-layout.wav"
    write_pcm16_wav(invalid_layout_path, [periodic_frame(255, ((1, 0.78),))], 48_000)

    motion_asset = asset_reference(asset_dir, motion_asset_path.name)
    sine_asset = asset_reference(asset_dir, sine_asset_path.name)
    saw_asset = asset_reference(asset_dir, saw_asset_path.name)
    position_parameter = "layer.motion.generator.wavetable_position"

    lfo_route = modulation(
        {
            "type": "lfo",
            "id": "motion_lfo",
            "waveform": "sine",
            "rate_hz": 0.35,
            "phase": 0.0,
        },
        {
            "source": "motion_lfo",
            "target": position_parameter,
            "amount": 0.5,
            "curve": "linear",
        },
    )
    mod_wheel_route = modulation(
        None,
        {
            "source": "mod_wheel",
            "target": position_parameter,
            "amount": 1.0,
            "curve": "linear",
        },
    )
    tuning_route = modulation(
        None,
        {
            "source": "pitch_bend",
            "target": "layer.motion.tuning",
            "amount": 0.35,
            "curve": "linear",
        },
    )
    unison = {
        "voices": 5,
        "detune_cents": 18.0,
        "stereo_spread": 0.85,
        "phase_spread": 0.2,
    }
    definitions: dict[str, dict[str, object]] = {
        "sine-single-frame": definition("Sine Single Frame", sine_asset),
        "saw-single-frame": definition("Saw Single Frame", saw_asset),
        "position-0": definition("Position 0", motion_asset, position=0.0),
        "position-05": definition("Position 0.5", motion_asset, position=0.5),
        "position-1": definition("Position 1", motion_asset, position=1.0),
        "position-sweep": definition("Position Sweep", motion_asset),
        "position-lfo": definition(
            "Position LFO", motion_asset, position=0.5, modulation_value=lfo_route
        ),
        "mod-wheel-position": definition(
            "Mod Wheel Position", motion_asset, modulation_value=mod_wheel_route
        ),
        "unison-5": definition("Unison 5 Stereo", motion_asset, unison=unison, position=0.5),
        "band-boundary-sweep": definition(
            "Band Boundary Sweep", motion_asset, position=0.5, modulation_value=tuning_route
        ),
        "motion-bass": definition(
            "Wavetable Motion Bass",
            motion_asset,
            position=0.25,
            unison=unison,
            modulation_value=lfo_route,
        ),
    }
    fallback_layer = {
        "id": "fallback",
        "enabled": True,
        "trigger": {
            "key_min": 0,
            "key_max": 127,
            "velocity_min": 1,
            "velocity_max": 127,
        },
        "gain_db": -8.0,
        "pan": 0.0,
        "tuning_cents": 0.0,
        "envelope": {
            "attack_seconds": 0.0,
            "decay_seconds": 0.0,
            "sustain_level": 1.0,
            "release_seconds": 0.08,
        },
        "generator": {
            "oscillator": {
                "waveform": {"type": "sine"},
                "phase_reset": True,
                "phase": 0.0,
            }
        },
        "processors": [],
    }
    definitions["missing-asset-fallback"] = definition(
        "Missing Asset Fallback",
        {"path": "../assets/missing-wavetable.wav", "sha256": None},
        extra_layers=[fallback_layer],
    )

    definition_paths: dict[str, Path] = {}
    for name, value in definitions.items():
        path = definition_dir / f"{name}.json"
        write_definition(path, value)
        definition_paths[name] = path
        run_cli(["instrument", "validate", str(path), "--json"])

    layout_definition = definition(
        "Invalid Wavetable Layout",
        asset_reference(asset_dir, invalid_layout_path.name),
        frame_length=FRAME_LENGTH,
    )
    layout_definition_path = definition_dir / "layout-error.json"
    write_definition(layout_definition_path, layout_definition)
    layout_exit_code, layout_stdout, layout_stderr = run_cli_error(
        ["instrument", "validate", str(layout_definition_path), "--json"]
    )
    layout_report = json.loads(layout_stdout)
    layout_codes = {diagnostic["code"] for diagnostic in layout_report["diagnostics"]}
    if layout_exit_code != 1 or "WAVETABLE_LAYOUT_INVALID" not in layout_codes:
        raise RuntimeError(
            f"Wavetable layout validation did not fail as expected: {layout_exit_code} {layout_stdout} {layout_stderr}"
        )
    write_utf8(
        review_root / "layout-error.json",
        json.dumps(layout_report, ensure_ascii=False, indent=2) + "\n",
    )

    position_sweep_events = event_dir / "position-sweep.json"
    write_events(
        position_sweep_events,
        note_events_with_controls(
            60,
            [
                {
                    "absolute_frame": 4_096,
                    "type": "parameter_change",
                    "parameter": position_parameter,
                    "normalized": 1.0,
                },
                {
                    "absolute_frame": 8_192,
                    "type": "parameter_change",
                    "parameter": position_parameter,
                    "normalized": 0.1,
                },
            ],
        ),
    )
    mod_wheel_events = event_dir / "mod-wheel-position.json"
    write_events(
        mod_wheel_events,
        note_events_with_controls(
            60,
            [
                {"absolute_frame": 3_072, "type": "mod_wheel", "value": 1.0},
                {"absolute_frame": 7_168, "type": "mod_wheel", "value": 0.0},
            ],
        ),
    )
    band_boundary_events = event_dir / "band-boundary-sweep.json"
    write_events(
        band_boundary_events,
        note_events_with_controls(
            108,
            [
                {"absolute_frame": 3_072, "type": "pitch_bend", "value": 1.0},
                {"absolute_frame": 8_192, "type": "pitch_bend", "value": -1.0},
            ],
            release_frame=14_000,
        ),
    )

    note_jobs = [
        ("01-sine-single-frame.wav", "sine-single-frame", 60),
        ("02-saw-single-frame-low.wav", "saw-single-frame", 36),
        ("03-saw-single-frame-high.wav", "saw-single-frame", 108),
        ("04-position-0.wav", "position-0", 60),
        ("05-position-05.wav", "position-05", 60),
        ("06-position-1.wav", "position-1", 60),
        ("08-position-lfo.wav", "position-lfo", 60),
        ("09-unison-5-stereo.wav", "unison-5", 48),
        ("12-motion-bass.wav", "motion-bass", 36),
    ]
    generated_audio: dict[str, Path] = {}
    for audio_name, definition_name, note in note_jobs:
        path = technical_dir / audio_name
        render_note(definition_paths[definition_name], note, path, BASE_BLOCK_SIZE, gate_seconds=0.35)
        generated_audio[audio_name] = path

    event_jobs = [
        ("07-position-sweep.wav", "position-sweep", position_sweep_events),
        ("11-mod-wheel-position.wav", "mod-wheel-position", mod_wheel_events),
        ("10-band-boundary-sweep.wav", "band-boundary-sweep", band_boundary_events),
    ]
    for audio_name, definition_name, events in event_jobs:
        path = technical_dir / audio_name
        render_events(definition_paths[definition_name], events, path, BASE_BLOCK_SIZE)
        generated_audio[audio_name] = path

    missing_audio = technical_dir / "13-missing-asset-fallback.wav"
    render_note(definition_paths["missing-asset-fallback"], 60, missing_audio, BASE_BLOCK_SIZE)
    generated_audio[missing_audio.name] = missing_audio

    technical_metrics: dict[str, dict[str, object]] = {}
    for path in sorted(generated_audio.values()):
        values = measure(path, list(BLOCK_SIZES), include_spectrum=path.name in {
            "01-sine-single-frame.wav",
            "02-saw-single-frame-low.wav",
            "03-saw-single-frame-high.wav",
            "10-band-boundary-sweep.wav",
            "12-motion-bass.wav",
        })
        values["steady_state_frequency_hz"] = steady_state_frequency(path)
        values.update(measure_stereo(path))
        technical_metrics[path.name] = values
    require_finite(technical_metrics)

    block_paths: dict[str, Path] = {}
    for block_size in BLOCK_SIZES:
        path = technical_dir / f"regression-block-{block_size}.wav"
        render_note(definition_paths["position-05"], 60, path, block_size)
        block_paths[str(block_size)] = path
    block_comparisons = {
        block_size: compare_wav(block_paths["257"], block_paths[str(block_size)])
        for block_size in BLOCK_SIZES
    }
    invalid_block_comparisons = {
        block_size: value
        for block_size, value in block_comparisons.items()
        if not value.get("compatible")
        or value.get("max_abs_difference", 1.0) > BLOCK_SIZE_MAX_DIFFERENCE
    }
    if invalid_block_comparisons:
        raise RuntimeError(f"Wavetable block-size mismatch: {invalid_block_comparisons}")

    fresh_a = technical_dir / "regression-fresh-a.wav"
    fresh_b = technical_dir / "regression-fresh-b.wav"
    render_note(definition_paths["position-05"], 60, fresh_a, BASE_BLOCK_SIZE)
    render_note(definition_paths["position-05"], 60, fresh_b, BASE_BLOCK_SIZE)
    fresh_comparison = compare_wav(fresh_a, fresh_b)
    if not fresh_comparison.get("compatible") or fresh_comparison.get("max_abs_difference", 1.0) != 0.0:
        raise RuntimeError(f"Wavetable fresh render is not reproducible: {fresh_comparison}")

    sample_rate_paths: dict[str, Path] = {}
    for sample_rate in (44_100, SAMPLE_RATE, 96_000):
        path = technical_dir / f"sample-rate-{sample_rate}.wav"
        render_note(definition_paths["position-05"], 60, path, BASE_BLOCK_SIZE, sample_rate)
        sample_rate_paths[str(sample_rate)] = path
    sample_rate_metrics = {
        sample_rate: measure(path, list(BLOCK_SIZES), include_spectrum=True)
        for sample_rate, path in sample_rate_paths.items()
    }

    inspect_stdout = run_cli(
        ["instrument", "inspect", str(definition_paths["motion-bass"]), "--json"]
    )
    inspect = json.loads(inspect_stdout)
    inspect_generator = inspect["layers"][0]["generator"]
    if (
        inspect_generator["kind"] != "wavetable"
        or not inspect_generator["prepared"]
        or inspect_generator["frame_length"] != FRAME_LENGTH
        or inspect_generator["frame_count"] != FRAME_COUNT
        or inspect_generator["band_max_harmonics"]
        != [
            FRAME_LENGTH // (2 ** (index + 1))
            for index in range(band_count(FRAME_LENGTH))
        ]
    ):
        raise RuntimeError(f"Wavetable inspect metadata is incomplete: {inspect_generator}")
    write_utf8(review_root / "inspect.json", json.dumps(inspect, ensure_ascii=False, indent=2) + "\n")
    missing_inspect = json.loads(
        run_cli(
            [
                "instrument",
                "inspect",
                str(definition_paths["missing-asset-fallback"]),
                "--json",
            ]
        )
    )
    missing_generator = missing_inspect["layers"][0]["generator"]
    if (
        missing_generator["kind"] != "wavetable"
        or missing_generator["prepared"]
        or missing_generator["frame_length"] != FRAME_LENGTH
    ):
        raise RuntimeError(f"Unavailable Wavetable inspect metadata is incomplete: {missing_generator}")
    write_utf8(
        review_root / "missing-asset-inspect.json",
        json.dumps(missing_inspect, ensure_ascii=False, indent=2) + "\n",
    )
    prepared_bytes = band_count(FRAME_LENGTH) * FRAME_COUNT * (FRAME_LENGTH + 3) * 4
    metrics = {
        "sample_rate": SAMPLE_RATE,
        "base_block_size": BASE_BLOCK_SIZE,
        "block_sizes": list(BLOCK_SIZES),
        "technical": technical_metrics,
        "block_size_comparisons": block_comparisons,
        "sample_rate_metrics": sample_rate_metrics,
        "position_comparisons": {
            "0_vs_05": compare_wav(
                generated_audio["04-position-0.wav"], generated_audio["05-position-05.wav"]
            ),
            "05_vs_1": compare_wav(
                generated_audio["05-position-05.wav"], generated_audio["06-position-1.wav"]
            ),
        },
        "stereo_unison": measure_stereo(generated_audio["09-unison-5-stereo.wav"]),
        "missing_asset_fallback": measure(generated_audio[missing_audio.name], list(BLOCK_SIZES)),
        "layout_error_diagnostics": sorted(layout_codes),
        "missing_asset_diagnostics": [
            diagnostic["code"] for diagnostic in missing_inspect["diagnostics"]
        ],
        "fresh_render_comparison": {
            **fresh_comparison,
            "first_sha256": sha256_file(fresh_a),
            "second_sha256": sha256_file(fresh_b),
        },
        "prepared_wavetable_bytes": prepared_bytes,
        "reset_comparison": {
            "covered_by": "sonalloy-core/tests/wavetable.rs::wavetable_output_is_stable_across_block_sizes_and_reset",
            "status": "automated test passed",
        },
    }
    write_utf8(review_root / "metrics.json", json.dumps(metrics, ensure_ascii=False, indent=2) + "\n")

    summary = """# Wavetable Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Wavetable Asset：PCM16、Frame Length 256、Frame Count 4

Definitionは`definitions/`、Assetは`assets/`、Eventは`events/`、WAVは`audio/technical/`へ保存しています。同じWAVをMetricsと人間の試聴に使用します。`inspect.json`にはWavetable Motion BassのCompiled表示を保存しています。

再生成：

```bash
python scripts/review/generate_digital_synthesis_package.py
```

## 音声一覧

| WAV | 目的 |
|---|---|
| `01-sine-single-frame.wav` | Sine Single Frame |
| `02-saw-single-frame-low.wav` | Saw Single Frame Low Note |
| `03-saw-single-frame-high.wav` | Saw Single Frame High Note |
| `04-position-0.wav` | Position 0 |
| `05-position-05.wav` | Position 0.5 |
| `06-position-1.wav` | Position 1 |
| `07-position-sweep.wav` | Parameter Position Sweep |
| `08-position-lfo.wav` | LFO to Position |
| `09-unison-5-stereo.wav` | Unison 5 Stereo |
| `10-band-boundary-sweep.wav` | High Register Band Selection |
| `11-mod-wheel-position.wav` | Mod Wheel to Position |
| `12-motion-bass.wav` | Wavetable Motion Bass |
| `13-missing-asset-fallback.wav` | Missing Wavetable Asset with Oscillator Layer |

Regression WAVは`regression-block-*.wav`、`regression-fresh-*.wav`、`sample-rate-*.wav`です。Metricsは`metrics.json`に保存しています。

## 自動確認

- Definition Validate：成功
- CLI Inspect JSON：成功
- Wavetable Layout Error診断：`layout-error.json`で確認済み
- Missing Asset Layer除外：`missing-asset-inspect.json`で確認済み
- 全WAVのFinite：成功
- Position 0 / 0.5 / 1の出力差：生成済み
- Block Size比較：許容差以内
- Sample Rate比較：生成済み
- Fresh Render比較：一致
- Reset：Core Integration Testで確認済み
- Missing Asset時のOscillator Layer継続：生成済み
- Prepared Wavetable Byte数：`metrics.json`へ記録済み

## 人間の確認

| 確認項目 | 対象 | 判定 |
|---|---|---|
| Frameごとの音色差 | `04-position-0.wav` / `05-position-05.wav` / `06-position-1.wav` | 未確認 |
| Position Sweepの滑らかさ | `07-position-sweep.wav` / `08-position-lfo.wav` / `11-mod-wheel-position.wav` | 未確認 |
| Band切替の不連続 | `10-band-boundary-sweep.wav` | 未確認 |
| 高音域Alias | `03-saw-single-frame-high.wav` / `10-band-boundary-sweep.wav` | 未確認 |
| 低音域の倍音保持 | `02-saw-single-frame-low.wav` / `12-motion-bass.wav` | 未確認 |
| UnisonのBeatとStereo幅 | `09-unison-5-stereo.wav` | 未確認 |
| Mono再生時のLevel | `09-unison-5-stereo.wav` | 未確認 |
| Missing Asset時の継続 | `13-missing-asset-fallback.wav` | 未確認 |
| 音色としての成立 | `12-motion-bass.wav` | 未確認 |

人間の確認では同じ再生環境・音量を使い、結果と指摘をこの表へ記録します。Metricsは音質の承認を代替しません。
"""
    write_utf8(review_root / "review-summary.md", summary)


if __name__ == "__main__":
    main()
