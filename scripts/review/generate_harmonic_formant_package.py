#!/usr/bin/env python3
"""Generate the harmonic and formant sound review package."""

from __future__ import annotations

import copy
import json
import math
from pathlib import Path
import shutil

from additive_review import (
    generate_additive_section,
    make_additive_definition,
    make_partial,
)
from common import (
    BASE_BLOCK_SIZE,
    BLOCK_SIZES,
    ROOT,
    SAMPLE_RATE,
    build_cli,
    measure_stereo,
    midi_note_frequency,
    render_events,
    render_midi,
    render_note,
    run_cli,
    sha256_file,
    timed_render,
    write_definition,
    write_events,
    write_utf8,
)
from measure_wav import compare_wav, measure, read_float_wav

BLOCK_SIZE_MAX_DIFFERENCE = 1.0e-5
MINIMUM_AUDIO_RMS = 1.0e-8
PERFORMANCE_DURATION_SECONDS = 2.0
PERFORMANCE_PARTIAL_COUNTS = (1, 16, 32, 64)
PERFORMANCE_VOICE_COUNTS = (1, 4, 8, 16)
PERFORMANCE_PROFILE_COUNTS = (1, 5, 8)


def layer(value: dict[str, object], index: int = 0) -> dict[str, object]:
    return value["layers"][index]


def formant(
    source: dict[str, object],
    vowel_position: float = 0.0,
    formant_shift_cents: float = 0.0,
    throat: float = 0.5,
    spectral_tilt_db_per_octave: float = -6.0,
) -> dict[str, object]:
    value = copy.deepcopy(source)
    target_layer = layer(value)
    target_layer["gain_db"] = -9.0
    target_layer["pan"] = 0.0
    target_layer["envelope"] = {
        "attack_seconds": 0.0,
        "decay_seconds": 0.05,
        "sustain_level": 1.0,
        "release_seconds": 0.12,
    }
    target_layer["processors"] = []
    target_layer["generator"] = {
        "formant": {
            "phase_reset": True,
            "partial_count": 48,
            "vowel_position": vowel_position,
            "formant_shift_cents": formant_shift_cents,
            "throat": throat,
            "spectral_tilt_db_per_octave": spectral_tilt_db_per_octave,
            "profiles": copy.deepcopy(
                source["layers"][0]["generator"]["formant"]["profiles"]
            ),
        }
    }
    value["voice_processors"] = []
    value["global_processors"] = []
    value["modulation"] = None
    return value


def performance_additive(
    source: dict[str, object], partial_count: int, voice_count: int
) -> dict[str, object]:
    partials = [
        make_partial(
            f"performance_{index}",
            float(index),
            0.42 / math.sqrt(index),
            0.42 / math.sqrt(index) * (0.9 + 0.05 * (index % 3)),
        )
        for index in range(1, partial_count + 1)
    ]
    value = make_additive_definition(source, partials, tilt=-3.0)
    layer(value)["gain_db"] = -24.0
    value["performance"]["polyphony"] = voice_count
    return value


def performance_profiles(
    source: dict[str, object], profile_count: int
) -> list[dict[str, object]]:
    source_profiles = source["layers"][0]["generator"]["formant"]["profiles"]
    profiles = copy.deepcopy(source_profiles[:profile_count])
    for index in range(len(profiles), profile_count):
        profile = copy.deepcopy(source_profiles[index % len(source_profiles)])
        profile["id"] = f"performance_{index + 1}"
        frequency_scale = 1.0 + 0.02 * (index - len(source_profiles) + 1)
        for band in profile["formants"]:
            band["frequency_hz"] *= frequency_scale
            band["bandwidth_hz"] *= frequency_scale
        profiles.append(profile)
    return profiles


def performance_formant(
    source: dict[str, object],
    partial_count: int,
    profile_count: int,
    voice_count: int,
) -> dict[str, object]:
    value = formant(source)
    generator = layer(value)["generator"]["formant"]
    generator["partial_count"] = partial_count
    generator["profiles"] = performance_profiles(source, profile_count)
    value["performance"]["polyphony"] = voice_count
    return value


def performance_events(
    voice_count: int, duration_frames: int
) -> list[dict[str, object]]:
    release_frame = duration_frames * 3 // 4
    events = [
        {
            "absolute_frame": 0,
            "type": "note_on",
            "note_id": index + 1,
            "note": 48 + index,
            "velocity": 112,
        }
        for index in range(voice_count)
    ]
    events.extend(
        {
            "absolute_frame": release_frame + index,
            "type": "note_off",
            "note_id": index + 1,
        }
        for index in range(voice_count)
    )
    return events


def performance_audio_metrics(path: Path) -> dict[str, object]:
    sample_rate, channels, samples = read_float_wav(path)
    finite = all(math.isfinite(sample) for sample in samples)
    peak = max((abs(sample) for sample in samples), default=0.0)
    rms = (
        math.sqrt(sum(sample * sample for sample in samples) / len(samples))
        if samples
        else 0.0
    )
    return {
        "sample_rate": sample_rate,
        "channels": channels,
        "frames": len(samples) // channels if channels else 0,
        "finite": finite,
        "peak": peak,
        "rms": rms,
    }


def relative_realtime_ratios(
    cases: dict[str, dict[str, object]], varying: str, fixed: str
) -> dict[str, dict[str, float]]:
    grouped: dict[str, dict[float, float]] = {}
    for case in cases.values():
        fixed_value = str(case[fixed])
        varying_value = float(case[varying])
        grouped.setdefault(fixed_value, {})[varying_value] = float(
            case["realtime_ratio"]
        )
    result: dict[str, dict[str, float]] = {}
    for fixed_value, values in grouped.items():
        baseline = values[min(values)]
        result[fixed_value] = {
            str(int(varying_value)): realtime_ratio / baseline
            for varying_value, realtime_ratio in sorted(values.items())
        }
    return result


def generate_performance_metrics(
    review_root: Path,
    additive_source: dict[str, object],
    formant_source: dict[str, object],
) -> dict[str, object]:
    duration_frames = int(PERFORMANCE_DURATION_SECONDS * SAMPLE_RATE)
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    performance_audio_dir = review_root / "audio" / "performance"
    performance_audio_dir.mkdir(parents=True, exist_ok=True)

    additive_cases: dict[str, dict[str, object]] = {}
    formant_cases: dict[str, dict[str, object]] = {}
    case_inputs: list[tuple[str, Path, Path, Path]] = []

    for partial_count in PERFORMANCE_PARTIAL_COUNTS:
        for voice_count in PERFORMANCE_VOICE_COUNTS:
            key = f"partials-{partial_count}-voices-{voice_count}"
            definition = performance_additive(
                additive_source, partial_count, voice_count
            )
            definition_path = definition_dir / f"performance-additive-{key}.json"
            event_path = event_dir / f"performance-additive-{key}.json"
            audio_path = performance_audio_dir / f"additive-{key}.wav"
            write_definition(definition_path, definition)
            write_events(event_path, performance_events(voice_count, duration_frames))
            run_cli(["instrument", "validate", str(definition_path), "--json"])
            case_inputs.append(("additive", definition_path, event_path, audio_path))
            additive_cases[key] = {
                "partial_count": partial_count,
                "voice_count": voice_count,
                "work_units": partial_count * voice_count,
            }

    for partial_count in (32, 64):
        for profile_count in PERFORMANCE_PROFILE_COUNTS:
            key = f"partials-{partial_count}-profiles-{profile_count}-voices-1"
            definition = performance_formant(
                formant_source, partial_count, profile_count, voice_count=1
            )
            definition_path = definition_dir / f"performance-formant-{key}.json"
            event_path = event_dir / f"performance-formant-{key}.json"
            audio_path = performance_audio_dir / f"formant-{key}.wav"
            write_definition(definition_path, definition)
            write_events(event_path, performance_events(1, duration_frames))
            run_cli(["instrument", "validate", str(definition_path), "--json"])
            case_inputs.append(("formant", definition_path, event_path, audio_path))
            formant_cases[key] = {
                "partial_count": partial_count,
                "profile_count": profile_count,
                "voice_count": 1,
                "work_units": partial_count * profile_count,
            }

    key = "partials-64-profiles-5-voices-16"
    definition = performance_formant(formant_source, 64, 5, voice_count=16)
    definition_path = definition_dir / f"performance-formant-{key}.json"
    event_path = event_dir / f"performance-formant-{key}.json"
    audio_path = performance_audio_dir / f"formant-{key}.wav"
    write_definition(definition_path, definition)
    write_events(event_path, performance_events(16, duration_frames))
    run_cli(["instrument", "validate", str(definition_path), "--json"])
    case_inputs.append(("formant", definition_path, event_path, audio_path))
    formant_cases[key] = {
        "partial_count": 64,
        "profile_count": 5,
        "voice_count": 16,
        "work_units": 64 * 5 * 16,
    }

    build_cli(release=True)
    for kind, definition_path, event_path, audio_path in case_inputs:
        result = timed_render(
            definition_path,
            event_path,
            audio_path,
            duration_frames,
            BASE_BLOCK_SIZE,
            SAMPLE_RATE,
            release=True,
        )
        audio = performance_audio_metrics(audio_path)
        if not audio["finite"] or audio["rms"] <= MINIMUM_AUDIO_RMS:
            raise RuntimeError(f"performance audio checks failed: {audio_path}")
        case_key = audio_path.stem.removeprefix(f"{kind}-")
        target_cases = additive_cases if kind == "additive" else formant_cases
        target_cases[case_key].update(
            {
                **result,
                "audio": audio,
                "output": str(audio_path.relative_to(review_root)),
            }
        )

    return {
        "build": "release",
        "sample_rate": SAMPLE_RATE,
        "block_size": BASE_BLOCK_SIZE,
        "duration_frames": duration_frames,
        "duration_seconds": PERFORMANCE_DURATION_SECONDS,
        "realtime_ratio": "elapsed_seconds / audio_duration_seconds",
        "additive": {
            "cases": additive_cases,
            "partial_scaling_by_voice": relative_realtime_ratios(
                additive_cases, "partial_count", "voice_count"
            ),
            "voice_scaling_by_partial": relative_realtime_ratios(
                additive_cases, "voice_count", "partial_count"
            ),
        },
        "formant": {
            "cases": formant_cases,
            "partial_scaling_by_profile": relative_realtime_ratios(
                {
                    key: case
                    for key, case in formant_cases.items()
                    if case["voice_count"] == 1
                },
                "partial_count",
                "profile_count",
            ),
            "profile_scaling_by_partial": relative_realtime_ratios(
                {
                    key: case
                    for key, case in formant_cases.items()
                    if case["voice_count"] == 1
                },
                "profile_count",
                "partial_count",
            ),
            "voice_scaling_at_64_partials_5_profiles": {
                "1": formant_cases["partials-64-profiles-5-voices-1"][
                    "realtime_ratio"
                ],
                "16": formant_cases["partials-64-profiles-5-voices-16"][
                    "realtime_ratio"
                ],
            },
        },
    }


def formant_with_lfo(source: dict[str, object]) -> dict[str, object]:
    value = formant(source)
    value["modulation"] = {
        "sources": [
            {
                "type": "lfo",
                "id": "vowel_position_lfo",
                "waveform": "sine",
                "rate_hz": 0.22,
                "phase": 0.0,
            }
        ],
        "routes": [
            {
                "source": "vowel_position_lfo",
                "target": "layer.voice.generator.formant_vowel_position",
                "amount": 0.5,
                "curve": "linear",
            }
        ],
    }
    return value


def formant_with_noise(source: dict[str, object]) -> dict[str, object]:
    value = formant(source, vowel_position=0.75)
    value["layers"].append(
        {
            "id": "air",
            "enabled": True,
            "trigger": {
                "event": "note_on",
                "key_min": 0,
                "key_max": 127,
                "velocity_min": 1,
                "velocity_max": 127,
            },
            "gain_db": -30.0,
            "pan": 0.0,
            "tuning_cents": 0.0,
            "envelope": {
                "attack_seconds": 0.0,
                "decay_seconds": 0.08,
                "sustain_level": 0.7,
                "release_seconds": 0.1,
            },
            "generator": {
                "noise": {
                    "color": "white",
                    "seed": 7123,
                    "stereo_correlation": 0.35,
                }
            },
            "processors": [],
        }
    )
    return value


def event_sequence(parameter: str, normalized: float, note_id: int) -> list[dict[str, object]]:
    return [
        {
            "absolute_frame": 0,
            "type": "note_on",
            "note_id": note_id,
            "note": 60,
            "velocity": 112,
        },
        {
            "absolute_frame": 4_096,
            "type": "parameter_change",
            "parameter": f"layer.voice.generator.{parameter}",
            "normalized": normalized,
        },
        {"absolute_frame": 12_000, "type": "note_off", "note_id": note_id},
    ]


def hybrid_event_sequence() -> list[dict[str, object]]:
    return [
        {
            "absolute_frame": 0,
            "type": "note_on",
            "note_id": 5,
            "note": 60,
            "velocity": 112,
        },
        {
            "absolute_frame": 4_096,
            "type": "parameter_change",
            "parameter": "layer.voice.generator.formant_shift",
            "normalized": 0.75,
        },
        {"absolute_frame": 8_192, "type": "mod_wheel", "value": 0.8},
        {"absolute_frame": 10_240, "type": "aftertouch", "value": 0.7},
        {"absolute_frame": 12_001, "type": "note_off", "note_id": 5},
    ]


def high_frequency_energy(path: Path, cutoff_ratio: float = 0.2) -> dict[str, float | int]:
    sample_rate, channels, samples = read_float_wav(path)
    frames = len(samples) // channels
    fft_size = min(2048, frames)
    if fft_size < 4:
        return {
            "sample_rate": sample_rate,
            "cutoff_ratio": cutoff_ratio,
            "cutoff_hz": sample_rate * cutoff_ratio,
            "fft_size": fft_size,
            "high_frequency_energy_ratio": 0.0,
        }
    left = samples[0::channels]
    start = min(frames - fft_size, int(sample_rate * 0.05))
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


def append_path(paths: list[Path], path: Path) -> None:
    if path not in paths:
        paths.append(path)


def main() -> None:
    additive_source_path = (
        ROOT / "examples" / "instruments" / "additive-generator-reference.json"
    )
    additive_source = json.loads(additive_source_path.read_text(encoding="utf-8"))
    source_path = ROOT / "examples" / "instruments" / "formant-generator-reference.json"
    source = json.loads(source_path.read_text(encoding="utf-8"))
    hybrid_source_path = (
        ROOT / "examples" / "instruments" / "harmonic-formant-hybrid-reference.json"
    )
    review_root = ROOT / "review-output" / "harmonic-formant-synthesis"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    midi_dir = review_root / "midi"
    asset_dir = review_root / "assets"
    technical_dir = review_root / "audio" / "technical"
    for directory in (definition_dir, event_dir, midi_dir, asset_dir, technical_dir):
        directory.mkdir(parents=True, exist_ok=True)

    additive_metrics = generate_additive_section(review_root)

    high_note = formant(
        source,
        vowel_position=0.0,
        formant_shift_cents=2400.0,
        throat=1.0,
        spectral_tilt_db_per_octave=0.0,
    )
    layer(high_note)["gain_db"] = -6.0
    definitions = {
        "vowel-a": formant(source, vowel_position=0.0),
        "vowel-i": formant(source, vowel_position=0.5),
        "vowel-u": formant(source, vowel_position=1.0),
        "vowel-e": formant(source, vowel_position=0.25),
        "vowel-o": formant(source, vowel_position=0.75),
        "vowel-morph": formant(source, vowel_position=0.0),
        "formant-shift": formant(source),
        "throat-sweep": formant(source),
        "formant-tilt-sweep": formant(source),
        "vowel-position-lfo": formant_with_lfo(source),
        "high-note-formant": high_note,
        "formant-noise-texture": formant_with_noise(source),
    }

    definition_paths: dict[str, Path] = {}
    for name, value in definitions.items():
        path = definition_dir / f"{name}.json"
        write_definition(path, value)
        definition_paths[name] = path
        run_cli(["instrument", "validate", str(path), "--json"])

    hybrid_value = json.loads(hybrid_source_path.read_text(encoding="utf-8"))
    hybrid_value["layers"][2]["generator"]["sample"]["zones"][0]["asset"][
        "path"
    ] = "../assets/metal-hit.wav"
    hybrid_path = definition_dir / hybrid_source_path.name
    write_definition(hybrid_path, hybrid_value)
    shutil.copy2(
        ROOT / "testdata" / "assets" / "metal-hit.wav",
        asset_dir / "metal-hit.wav",
    )
    run_cli(["instrument", "validate", str(hybrid_path), "--json"])

    hybrid_inspect = run_cli(
        ["instrument", "inspect", str(hybrid_path), "--json"]
    )
    hybrid_inspect_data = json.loads(hybrid_inspect)
    hybrid_generators = {
        layer_value["generator"]["kind"]
        for layer_value in hybrid_inspect_data["layers"]
    }
    if hybrid_generators != {"formant", "additive", "sample", "noise"}:
        raise RuntimeError(f"hybrid generator contract failed: {hybrid_generators}")
    if (
        len(hybrid_inspect_data["layers"]) != 4
        or {value["id"] for value in hybrid_inspect_data["voice_processors"]}
        != {"voice_tone", "voice_glue"}
        or {value["id"] for value in hybrid_inspect_data["global_processors"]}
        != {"echo", "space"}
    ):
        raise RuntimeError("hybrid processor contract failed")
    hybrid_parameter_ids = {
        parameter["id"] for parameter in hybrid_inspect_data["parameters"]
    }
    expected_hybrid_parameter_ids = {
        "layer.voice.generator.formant_vowel_position",
        "layer.voice.generator.formant_shift",
        "layer.voice.generator.formant_throat",
        "layer.voice.generator.formant_spectral_tilt",
        "voice.processor.voice_tone.cutoff",
        "voice.processor.voice_glue.mix",
        "global.processor.echo.mix",
        "global.processor.space.mix",
    }
    if not expected_hybrid_parameter_ids.issubset(hybrid_parameter_ids):
        raise RuntimeError("hybrid parameter contract failed")
    hybrid_route_targets = {route["target"] for route in hybrid_inspect_data["routes"]}
    expected_hybrid_route_targets = {
        "layer.voice.generator.formant_vowel_position",
        "layer.voice.generator.formant_shift",
        "layer.voice.generator.formant_throat",
        "layer.voice.generator.formant_spectral_tilt",
        "voice.processor.voice_tone.cutoff",
        "global.processor.space.mix",
        "global.processor.echo.mix",
    }
    if not expected_hybrid_route_targets.issubset(hybrid_route_targets):
        raise RuntimeError("hybrid modulation contract failed")
    write_utf8(review_root / "hybrid-inspect.json", hybrid_inspect)

    inspect = run_cli(
        ["instrument", "inspect", str(definition_paths["vowel-a"]), "--json"]
    )
    inspect_data = json.loads(inspect)
    inspected_generator = inspect_data["layers"][0]["generator"]
    if (
        inspected_generator["kind"] != "formant"
        or inspected_generator["profile_count"] != 5
        or inspected_generator["partial_count"] != 48
        or len(inspected_generator["profiles"]) != 5
    ):
        raise RuntimeError(f"formant inspect contract failed: {inspected_generator}")
    parameter_ids = {
        parameter["id"]
        for parameter in inspect_data["parameters"]
        if ".generator.formant_" in parameter["id"]
    }
    expected_parameter_ids = {
        "layer.voice.generator.formant_vowel_position",
        "layer.voice.generator.formant_shift",
        "layer.voice.generator.formant_throat",
        "layer.voice.generator.formant_spectral_tilt",
    }
    if parameter_ids != expected_parameter_ids:
        raise RuntimeError(f"formant parameter contract failed: {parameter_ids}")
    write_utf8(review_root / "inspect.json", inspect)

    event_values = {
        "vowel-morph": event_sequence("formant_vowel_position", 1.0, 1),
        "formant-shift": event_sequence("formant_shift", 0.75, 2),
        "throat-sweep": event_sequence("formant_throat", 1.0, 3),
        "formant-tilt-sweep": event_sequence("formant_spectral_tilt", 1.0, 4),
    }
    event_paths: dict[str, Path] = {}
    for name, events in event_values.items():
        path = event_dir / f"{name}.json"
        write_events(path, events)
        event_paths[name] = path
    hybrid_event_path = event_dir / "hybrid-controls.json"
    write_events(hybrid_event_path, hybrid_event_sequence())

    midi_source = ROOT / "testdata" / "midi" / "basic-poly-synth-phrase.mid"
    midi_path = midi_dir / midi_source.name
    shutil.copy2(midi_source, midi_path)

    generated_paths: list[Path] = []
    note_jobs = [
        ("12-vowel-a.wav", "vowel-a", 60),
        ("13-vowel-i.wav", "vowel-i", 60),
        ("14-vowel-u.wav", "vowel-u", 60),
        ("15-vowel-e.wav", "vowel-e", 60),
        ("16-vowel-o.wav", "vowel-o", 60),
        ("22-high-note-formant.wav", "high-note-formant", 84),
        ("23-formant-noise-texture.wav", "formant-noise-texture", 60),
    ]
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
        append_path(generated_paths, path)

    event_jobs = [
        ("17-vowel-morph.wav", "vowel-morph"),
        ("18-formant-shift-sweep.wav", "formant-shift"),
        ("19-throat-sweep.wav", "throat-sweep"),
        ("20-formant-tilt-sweep.wav", "formant-tilt-sweep"),
    ]
    for audio_name, event_name in event_jobs:
        path = technical_dir / audio_name
        render_event_file(
            definition_paths[event_name], event_paths[event_name], path
        )
        append_path(generated_paths, path)

    lfo_path = technical_dir / "21-vowel-position-lfo.wav"
    render_note(
        definition_paths["vowel-position-lfo"],
        60,
        lfo_path,
        BASE_BLOCK_SIZE,
        gate_seconds=0.5,
        tail_seconds=0.1,
    )
    append_path(generated_paths, lfo_path)

    hybrid_note_path = technical_dir / "24-harmonic-formant-hybrid.wav"
    render_note(
        hybrid_path,
        60,
        hybrid_note_path,
        BASE_BLOCK_SIZE,
        gate_seconds=0.25,
        tail_seconds=0.1,
    )
    append_path(generated_paths, hybrid_note_path)
    hybrid_midi_path = technical_dir / "25-harmonic-formant-hybrid-midi.wav"
    render_midi(
        hybrid_path,
        midi_path,
        hybrid_midi_path,
        BASE_BLOCK_SIZE,
        SAMPLE_RATE,
        tail_seconds=0.1,
    )
    append_path(generated_paths, hybrid_midi_path)
    hybrid_controls_path = technical_dir / "26-harmonic-formant-hybrid-controls.wav"
    render_event_file(
        hybrid_path,
        hybrid_event_path,
        hybrid_controls_path,
        duration_frames=16_801,
    )
    append_path(generated_paths, hybrid_controls_path)

    existing_review_definitions = [
        "basic-poly-synth.json",
        "moving-hybrid-pad.json",
        "processed-hybrid.json",
        "essential-hybrid-instrument.json",
        "granular-generator-reference.json",
        "wave-sequence-hybrid-reference.json",
        "digital-hybrid-reference.json",
    ]
    for name in existing_review_definitions:
        run_cli(
            [
                "instrument",
                "validate",
                str(ROOT / "examples" / "instruments" / name),
                "--json",
            ]
        )
    existing_render_jobs = [
        ("27-existing-processor-chain.wav", "processed-hybrid.json"),
        ("28-existing-digital-hybrid.wav", "digital-hybrid-reference.json"),
    ]
    existing_render_paths: dict[str, Path] = {}
    for audio_name, definition_name in existing_render_jobs:
        path = technical_dir / audio_name
        render_note(
            ROOT / "examples" / "instruments" / definition_name,
            60,
            path,
            BASE_BLOCK_SIZE,
            gate_seconds=0.25,
            tail_seconds=0.5,
        )
        existing_render_paths[definition_name] = path
        append_path(generated_paths, path)

    regression_paths: dict[str, Path] = {}
    for block_size in BLOCK_SIZES:
        path = technical_dir / f"regression-block-{block_size}.wav"
        render_note(
            definition_paths["vowel-a"],
            60,
            path,
            block_size,
            gate_seconds=0.25,
            tail_seconds=0.1,
        )
        regression_paths[str(block_size)] = path
        append_path(generated_paths, path)

    sample_rate_paths: dict[str, Path] = {}
    for sample_rate in (44_100, SAMPLE_RATE, 96_000):
        path = technical_dir / f"sample-rate-{sample_rate}.wav"
        render_note(
            definition_paths["vowel-a"],
            60,
            path,
            BASE_BLOCK_SIZE,
            sample_rate,
            gate_seconds=0.25,
            tail_seconds=0.1,
        )
        sample_rate_paths[str(sample_rate)] = path
        append_path(generated_paths, path)

    fresh_a = technical_dir / "fresh-a.wav"
    fresh_b = technical_dir / "fresh-b.wav"
    render_note(
        definition_paths["vowel-a"],
        60,
        fresh_a,
        BASE_BLOCK_SIZE,
        gate_seconds=0.25,
        tail_seconds=0.1,
    )
    render_note(
        definition_paths["vowel-a"],
        60,
        fresh_b,
        BASE_BLOCK_SIZE,
        gate_seconds=0.25,
        tail_seconds=0.1,
    )
    append_path(generated_paths, fresh_a)
    append_path(generated_paths, fresh_b)

    hybrid_block_paths: dict[str, Path] = {}
    for block_size in BLOCK_SIZES:
        path = technical_dir / f"hybrid-block-{block_size}.wav"
        render_note(
            hybrid_path,
            60,
            path,
            block_size,
            gate_seconds=0.25,
            tail_seconds=0.5,
        )
        hybrid_block_paths[str(block_size)] = path
        append_path(generated_paths, path)

    hybrid_sample_rate_paths: dict[str, Path] = {}
    for sample_rate in (44_100, SAMPLE_RATE, 96_000):
        path = technical_dir / f"hybrid-sample-rate-{sample_rate}.wav"
        render_note(
            hybrid_path,
            60,
            path,
            BASE_BLOCK_SIZE,
            sample_rate,
            gate_seconds=0.25,
            tail_seconds=0.5,
        )
        hybrid_sample_rate_paths[str(sample_rate)] = path
        append_path(generated_paths, path)

    hybrid_fresh_a = technical_dir / "hybrid-fresh-a.wav"
    hybrid_fresh_b = technical_dir / "hybrid-fresh-b.wav"
    for path in (hybrid_fresh_a, hybrid_fresh_b):
        render_note(
            hybrid_path,
            60,
            path,
            BASE_BLOCK_SIZE,
            gate_seconds=0.25,
            tail_seconds=0.5,
        )
        append_path(generated_paths, path)

    performance_metrics = generate_performance_metrics(
        review_root,
        additive_source,
        source,
    )

    spectrum_names = {
        "12-vowel-a.wav",
        "13-vowel-i.wav",
        "14-vowel-u.wav",
        "15-vowel-e.wav",
        "16-vowel-o.wav",
        "22-high-note-formant.wav",
    }
    fundamental_by_name = {
        name: midi_note_frequency(note)
        for name, note in (
            [(f"{index:02d}-vowel-{vowel}.wav", 60) for index, vowel in ((12, "a"), (13, "i"), (14, "u"), (15, "e"), (16, "o"))]
            + [("22-high-note-formant.wav", 84)]
        )
    }
    audio_metrics: dict[str, object] = {}
    for path in sorted(generated_paths):
        values = measure(
            path,
            list(BLOCK_SIZES),
            include_spectrum=path.name in spectrum_names,
            fundamental_frequency_hz=fundamental_by_name.get(path.name),
        )
        values.update(measure_stereo(path))
        audio_metrics[path.name] = values

    invalid_audio = [
        name
        for name, values in audio_metrics.items()
        if not values["finite"] or values["rms"] <= MINIMUM_AUDIO_RMS
    ]
    if invalid_audio:
        raise RuntimeError(f"formant audio checks failed: {invalid_audio}")

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
        raise RuntimeError(f"formant block-size mismatch: {invalid_block_comparisons}")

    hybrid_block_comparisons = {
        block_size: compare_wav(
            hybrid_block_paths["257"], hybrid_block_paths[str(block_size)]
        )
        for block_size in BLOCK_SIZES
    }
    invalid_hybrid_block_comparisons = {
        block_size: comparison
        for block_size, comparison in hybrid_block_comparisons.items()
        if not comparison.get("compatible")
        or comparison.get("max_abs_difference", 1.0) > BLOCK_SIZE_MAX_DIFFERENCE
    }
    if invalid_hybrid_block_comparisons:
        raise RuntimeError(
            f"hybrid block-size mismatch: {invalid_hybrid_block_comparisons}"
        )

    fresh_comparison = compare_wav(fresh_a, fresh_b)
    if (
        not fresh_comparison.get("compatible")
        or fresh_comparison.get("max_abs_difference", 1.0) != 0.0
    ):
        raise RuntimeError(f"formant fresh render is not reproducible: {fresh_comparison}")

    hybrid_fresh_comparison = compare_wav(hybrid_fresh_a, hybrid_fresh_b)
    if (
        not hybrid_fresh_comparison.get("compatible")
        or hybrid_fresh_comparison.get("max_abs_difference", 1.0) != 0.0
    ):
        raise RuntimeError(
            f"hybrid fresh render is not reproducible: {hybrid_fresh_comparison}"
        )

    parameter_comparisons = {
        "vowel_a_to_i": compare_wav(
            technical_dir / "12-vowel-a.wav", technical_dir / "13-vowel-i.wav"
        ),
        "vowel_a_to_u": compare_wav(
            technical_dir / "12-vowel-a.wav", technical_dir / "14-vowel-u.wav"
        ),
        "vowel_morph_to_shift": compare_wav(
            technical_dir / "17-vowel-morph.wav",
            technical_dir / "18-formant-shift-sweep.wav",
        ),
        "throat_to_tilt": compare_wav(
            technical_dir / "19-throat-sweep.wav",
            technical_dir / "20-formant-tilt-sweep.wav",
        ),
    }
    invalid_parameter_comparisons = {
        name: comparison
        for name, comparison in parameter_comparisons.items()
        if not comparison.get("compatible")
        or comparison.get("max_abs_difference", 0.0) <= BLOCK_SIZE_MAX_DIFFERENCE
    }
    if invalid_parameter_comparisons:
        raise RuntimeError(
            f"formant parameter changes produced no measurable difference: {invalid_parameter_comparisons}"
        )

    high_frequency_metrics = {
        "vowel_a": high_frequency_energy(technical_dir / "12-vowel-a.wav"),
        "high_note_formant": high_frequency_energy(
            technical_dir / "22-high-note-formant.wav"
        ),
    }
    if high_frequency_metrics["high_note_formant"]["high_frequency_energy_ratio"] <= 1.0e-4:
        raise RuntimeError(
            f"high-note formant fixture has insufficient high-frequency energy: {high_frequency_metrics}"
        )

    metrics: dict[str, object] = {
        "additive": additive_metrics,
        "sample_rate": SAMPLE_RATE,
        "base_block_size": BASE_BLOCK_SIZE,
        "block_sizes": list(BLOCK_SIZES),
        "profile_count": 5,
        "partial_count": 48,
        "formant_parameters": {
            "vowel_position": 0.0,
            "formant_shift_cents": 0.0,
            "throat": 0.5,
            "spectral_tilt_db_per_octave": -6.0,
        },
        "parameter_ids": sorted(expected_parameter_ids),
        "audio": audio_metrics,
        "block_size_comparisons": block_comparisons,
        "parameter_comparisons": parameter_comparisons,
        "sample_rate_metrics": {
            sample_rate: audio_metrics[path.name]
            for sample_rate, path in sample_rate_paths.items()
        },
        "fresh_render_comparison": {
            **fresh_comparison,
            "first_sha256": sha256_file(fresh_a),
            "second_sha256": sha256_file(fresh_b),
        },
        "hybrid_block_size_comparisons": hybrid_block_comparisons,
        "hybrid_sample_rate_metrics": {
            sample_rate: audio_metrics[path.name]
            for sample_rate, path in hybrid_sample_rate_paths.items()
        },
        "hybrid_fresh_render_comparison": {
            **hybrid_fresh_comparison,
            "first_sha256": sha256_file(hybrid_fresh_a),
            "second_sha256": sha256_file(hybrid_fresh_b),
        },
        "hybrid_control_comparison": compare_wav(
            hybrid_note_path, hybrid_controls_path
        ),
        "existing_review_regression": {
            "validated_definitions": existing_review_definitions,
            "rendered_definitions": list(existing_render_paths),
        },
        "high_frequency_energy": high_frequency_metrics,
        "performance": performance_metrics,
    }
    write_utf8(
        review_root / "metrics.json",
        json.dumps(metrics, ensure_ascii=False, indent=2) + "\n",
    )

    summary = """# Harmonic / Formant Synthesis Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Performance Render：Release Build、48,000 Hz、257 frames、2 seconds

## 入力と再生成

Additive、Formant、HybridのDefinitionは`definitions/`、Eventは`events/`、MIDIは`midi/`、Assetは`assets/`、通常のWAVは`audio/technical/`、性能計測のWAVは`audio/performance/`へ保存しています。`additive-inspect.json`、`inspect.json`、`hybrid-inspect.json`には各Generatorと4 LayerのCLI Inspect結果を保存しています。

```bash
python scripts/review/generate_harmonic_formant_package.py
```

## 音声一覧

### Additive

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

### Formant

| WAV | 目的 |
|---|---|
| `12-vowel-a.wav` | Vowel A |
| `13-vowel-i.wav` | Vowel I |
| `14-vowel-u.wav` | Vowel U |
| `15-vowel-e.wav` | Vowel E |
| `16-vowel-o.wav` | Vowel O |
| `17-vowel-morph.wav` | Vowel Position Morph |
| `18-formant-shift-sweep.wav` | Formant Shift |
| `19-throat-sweep.wav` | Throat |
| `20-formant-tilt-sweep.wav` | Spectral Tilt |
| `21-vowel-position-lfo.wav` | Vowel Position LFO |
| `22-high-note-formant.wav` | High-note Alias Fade |
| `23-formant-noise-texture.wav` | Formant and Noise Texture |

### Hybrid and regression

| WAV | 目的 |
|---|---|
| `24-harmonic-formant-hybrid.wav` | Formant and Additive Hybrid |
| `25-harmonic-formant-hybrid-midi.wav` | Hybrid MIDI Phrase |
| `26-harmonic-formant-hybrid-controls.wav` | Hybrid Parameter and External Control |
| `27-existing-processor-chain.wav` | Existing Processor Chain Regression |
| `28-existing-digital-hybrid.wav` | Existing Digital Hybrid Regression |

## 性能計測

`metrics.json`の`performance`には、Additiveの1 / 16 / 32 / 64 Partial × 1 / 4 / 8 / 16 Voice、Formantの32 / 64 Partial × 1 / 5 / 8 Profile、64 Partial × 5 Profile × 16 Voiceを記録しています。各Caseは`audio_duration_seconds`、`elapsed_seconds`、`realtime_ratio`（`elapsed_seconds / audio_duration_seconds`）、`work_units`、有限値、Peak、RMSを持ちます。Partial、Voice、Profileごとの相対Realtime比も同じJSONへ保存しています。絶対的な合格閾値は設けず、計算量に対する増加傾向と16 Voice × 64 Partialの実測値を確認します。Timingは実行環境に依存します。

## 機械検査

`metrics.json`はAdditiveのSine Table、Partial、Spectrum差分、Formant Profile / Band / Parameter、Hybrid Layer / Processor / Route、全WAVのFinite性、Peak、RMS、DC、Stereo、Parameter差分、High-frequency Energy、Sample Rate別値、Block Size比較、Fresh Runtime再現性、既存Reference回帰、Release Performanceを記録します。WAVは正規化せず、Metricsと試聴で同じ生出力を使用します。

## 人間の確認

- [ ] Harmonic Organで基音と整数倍Partialが明確に聞こえ、Clickがない
- [ ] Inharmonic BellでInteger Harmonicとの差と金属的な質感が聞き取れる
- [ ] Spectrum Morphが連続し、中間値で音量が急落・急増しない
- [ ] Partial Envelope終了時に残りPartialのGainが段差変化しない
- [ ] Additive High-noteで高域Partialが主音として折り返さず、自然に薄くなる
- [ ] Additive Polyphonyで音量、Pitch、Reset、Voice Stealingが安定している
- [ ] A / I / U / E / Oの共鳴位置を聞き分けられる
- [ ] Vowel Morphが連続し、Profile境界にClickやZipper Noiseがない
- [ ] Formant Shiftで基音のPitchを保ったままVocal Characterが変化する
- [ ] ThroatでResonanceの幅が変化し、端点で急増しない
- [ ] Spectral Tiltで明るさが連続して変化する
- [ ] Formant High-noteで高次Aliasが主音として支配的にならない
- [ ] Vowel Position LFOの動きが連続している
- [ ] Noise TextureがFormantの共鳴を隠さず、Hybridの一体感がある
- [ ] Layer Filter / DriveとVoice Filter / Driveの作用範囲が分かれ、Delay / ReverbのTailが自然である
- [ ] MIDI PhraseとParameter / External Control Eventが音色の動きへ連続して反映される
- [ ] Polyphony、Voice Stealing、Reset後の発音が安定している
"""
    write_utf8(review_root / "review-summary.md", summary)


if __name__ == "__main__":
    main()
