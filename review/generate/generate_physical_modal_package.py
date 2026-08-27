#!/usr/bin/env python3
"""Generate the Physical String / Modal sound review package."""

from __future__ import annotations

import json
import math
import tempfile
from pathlib import Path

from common import (
    BASE_BLOCK_SIZE,
    BLOCK_SIZES,
    ROOT,
    SAMPLE_RATE,
    midi_note_frequency,
    render_events,
    render_note,
    run_cli,
    sha256_file,
    timed_render,
    write_definition,
    write_events,
    write_utf8,
)
from measure_wav import compare_wav, measure, read_float_wav


REVIEW_ROOT = ROOT / "review" / "physical-modal"
DEFINITION_DIR = REVIEW_ROOT / "definitions"
VALIDATION_DIR = REVIEW_ROOT / "validation"
INSPECT_DIR = REVIEW_ROOT / "inspect"
TRACE_DIR = REVIEW_ROOT / "trace"
TECHNICAL_DIR = REVIEW_ROOT / "audio" / "technical"
MUSICAL_DIR = REVIEW_ROOT / "audio" / "musical"
EVENT_DIR = REVIEW_ROOT / "events"
TECHNICAL_NOTE = 60
MUSICAL_NOTES = {
    "physical_pluck": 60,
    "modal_mallet": 60,
    "imaginary_metal_body": 48,
}
BLOCK_SIZE_MAX_DIFFERENCE = 1.0e-4
PITCH_ACCURACY_LIMIT_CENTS = 20.0
REVIEW_SAMPLE_RATES = (44_100, SAMPLE_RATE, 96_000)
PERFORMANCE_SAMPLE_RATES = (SAMPLE_RATE, 96_000)
REVIEW_LAYER_GAIN_DB = 3.0
MIN_AUDITION_RMS = 1.0e-3
MIN_MODAL_DENSITY_DIFFERENCE_RATIO = 0.3
PITCH_ACCURACY_DEFINITIONS = {
    "string_impulse",
    "string_low_stiffness",
    "string_medium_stiffness",
    "string_high_stiffness",
    "physical_pluck",
}
PITCH_ACCURACY_FILENAMES = {f"{name}.wav" for name in PITCH_ACCURACY_DEFINITIONS}


def layer(
    layer_id: str,
    generator: dict[str, object],
    *,
    gain_db: float = REVIEW_LAYER_GAIN_DB,
    pan: float = 0.0,
    trigger_event: str = "note_on",
    release_seconds: float = 0.4,
) -> dict[str, object]:
    return {
        "id": layer_id,
        "enabled": True,
        "trigger": {
            "event": trigger_event,
            "key_min": 0,
            "key_max": 127,
            "velocity_min": 1,
            "velocity_max": 127,
        },
        "gain_db": gain_db,
        "pan": pan,
        "tuning_cents": 0.0,
        "envelope": {
            "attack_seconds": 0.0,
            "decay_seconds": 0.0,
            "sustain_level": 1.0,
            "release_seconds": release_seconds,
        },
        "generator": generator,
        "processors": [],
    }


def instrument(
    name: str,
    layers: list[dict[str, object]],
    *,
    voice_processors: list[dict[str, object]] | None = None,
    global_processors: list[dict[str, object]] | None = None,
    modulation: dict[str, object] | None = None,
) -> dict[str, object]:
    return {
        "schema_version": 4,
        "metadata": {
            "name": name,
            "author": "Sonalloy",
            "description": "Physical String and Modal synthesis review definition",
        },
        "performance": {
            "mode": "polyphonic",
            "polyphony": 8,
            "voice_stealing": "quietest_releasing_then_oldest",
        },
        "macros": [],
        "vectors": [],
        "layers": layers,
        "voice_processors": voice_processors or [],
        "global_processors": global_processors or [],
        "modulation": modulation,
    }


def physical_string(
    *,
    duration_seconds: float = 0.006,
    exciter_brightness: float = 0.8,
    seed: int = 4001,
    decay_seconds: float = 2.4,
    brightness: float = 0.65,
    stiffness: float = 0.15,
) -> dict[str, object]:
    return {
        "physical_string": {
            "exciter": {
                "type": "noise_burst",
                "duration_seconds": duration_seconds,
                "brightness": exciter_brightness,
                "seed": seed,
            },
            "decay_seconds": decay_seconds,
            "brightness": brightness,
            "stiffness": stiffness,
        }
    }


def modal(
    *,
    mode_count: int = 12,
    structure: float = 0.5,
    brightness: float = 0.65,
    decay: float = 0.65,
    exciter: dict[str, object] | None = None,
) -> dict[str, object]:
    return {
        "modal": {
            "exciter": exciter or {"type": "noise_burst", "duration_seconds": 0.008, "brightness": 0.6, "seed": 9102},
            "mode_count": mode_count,
            "structure": structure,
            "brightness": brightness,
            "decay": decay,
        }
    }


def processor(processor_type: str, processor_id: str, **fields: object) -> dict[str, object]:
    value: dict[str, object] = {"type": processor_type, "id": processor_id}
    value.update(fields)
    return value


def nearest_spectral_peak_error_cents(
    values: dict[str, object], fundamental_frequency_hz: float
) -> float | None:
    spectrum = values.get("spectrum_reference")
    if not isinstance(spectrum, dict):
        return None
    peaks = spectrum.get("peaks")
    if not isinstance(peaks, list):
        return None
    frequencies = [
        float(peak["frequency_hz"])
        for peak in peaks
        if isinstance(peak, dict) and "frequency_hz" in peak
    ]
    if not frequencies:
        return None
    nearest = min(frequencies, key=lambda frequency: abs(frequency - fundamental_frequency_hz))
    fft_size = int(spectrum.get("fft_size", 0))
    sample_rate = float(values.get("sample_rate", 0.0))
    tolerance = max(
        sample_rate / fft_size * 1.5 if fft_size > 0 else 0.0,
        fundamental_frequency_hz * 0.03,
    )
    if abs(nearest - fundamental_frequency_hz) > tolerance:
        return None
    return 1_200.0 * math.log2(nearest / fundamental_frequency_hz)


def autocorrelation_pitch_error_cents(path: Path, fundamental_frequency_hz: float) -> float | None:
    sample_rate, channels, samples = read_float_wav(path)
    if channels == 0 or not samples:
        return None
    left = samples[0::channels]
    expected_lag = sample_rate / fundamental_frequency_hz
    start = sample_rate // 50
    end = min(len(left), start + sample_rate // 2)
    if end - start <= expected_lag * 2.0:
        return None
    window = left[start:end]
    minimum_lag = max(2, round(expected_lag * 0.8))
    maximum_lag = round(expected_lag * 1.2)
    best_correlation = -1.0
    best_lag: int | None = None
    for lag in range(minimum_lag, maximum_lag + 1):
        first = window[:-lag]
        second = window[lag:]
        normalization = math.sqrt(
            sum(sample * sample for sample in first)
            * sum(sample * sample for sample in second)
        )
        if normalization <= 0.0:
            continue
        correlation = (
            sum(
                left_sample * right_sample
                for left_sample, right_sample in zip(first, second)
            )
            / normalization
        )
        if correlation > best_correlation:
            best_correlation = correlation
            best_lag = lag
    if best_lag is None:
        return None
    estimated_frequency = sample_rate / best_lag
    return 1_200.0 * math.log2(estimated_frequency / fundamental_frequency_hz)


def definitions() -> dict[str, dict[str, object]]:
    values: dict[str, dict[str, object]] = {}
    values["string_impulse"] = instrument(
        "Physical String Impulse",
        [layer("string", {"physical_string": {"exciter": {"type": "impulse"}, "decay_seconds": 2.0, "brightness": 0.55, "stiffness": 0.1}})],
    )
    values["string_noise_soft"] = instrument(
        "Physical String Soft Exciter",
        [layer("string", physical_string(exciter_brightness=0.2, seed=4002))],
    )
    values["string_noise_bright"] = instrument(
        "Physical String Bright Exciter",
        [layer("string", physical_string(exciter_brightness=1.0, seed=4003))],
    )
    values["string_short_decay"] = instrument(
        "Physical String Short Decay",
        [layer("string", physical_string(decay_seconds=0.35, seed=4004))],
    )
    values["string_long_decay"] = instrument(
        "Physical String Long Decay",
        [layer("string", physical_string(decay_seconds=5.0, seed=4005))],
    )
    values["string_soft"] = instrument(
        "Physical String Dark Loop",
        [layer("string", physical_string(brightness=0.05, seed=4006), gain_db=6.0)],
    )
    values["string_bright"] = instrument(
        "Physical String Bright Loop",
        [layer("string", physical_string(brightness=1.0, seed=4007))],
    )
    values["string_low_stiffness"] = instrument(
        "Physical String Low Stiffness",
        [layer("string", physical_string(stiffness=0.0, seed=4008))],
    )
    values["string_medium_stiffness"] = instrument(
        "Physical String Medium Stiffness",
        [layer("string", physical_string(stiffness=0.5, seed=4010))],
    )
    values["string_high_stiffness"] = instrument(
        "Physical String High Stiffness",
        [layer("string", physical_string(stiffness=1.0, seed=4009))],
    )

    density_exciter = {
        "type": "noise_burst",
        "duration_seconds": 0.008,
        "brightness": 1.0,
        "seed": 9102,
    }
    values["modal_4_modes"] = instrument(
        "Modal Four Modes",
        [layer("body", modal(mode_count=4, brightness=0.92, exciter=density_exciter))],
    )
    values["modal_8_modes"] = instrument(
        "Modal Eight Modes",
        [layer("body", modal(mode_count=8, brightness=0.92, exciter=density_exciter))],
    )
    values["modal_12_modes"] = instrument(
        "Modal Twelve Modes",
        [layer("body", modal(mode_count=12, brightness=0.92, exciter=density_exciter))],
    )
    values["modal_16_modes"] = instrument(
        "Modal Sixteen Modes",
        [layer("body", modal(mode_count=16, brightness=0.92, exciter=density_exciter))],
    )
    values["modal_20_modes"] = instrument(
        "Modal Twenty Modes",
        [layer("body", modal(mode_count=20, brightness=0.92, exciter=density_exciter))],
    )
    values["modal_24_modes"] = instrument(
        "Modal Twenty Four Modes",
        [layer("body", modal(mode_count=24, brightness=0.92, exciter=density_exciter))],
    )
    values["modal_harmonic_structure"] = instrument(
        "Modal Harmonic Structure",
        [layer("body", modal(mode_count=12, structure=0.28, brightness=0.5, decay=0.55))],
    )
    values["modal_stretched_structure"] = instrument(
        "Modal Stretched Structure",
        [layer("body", modal(mode_count=12, structure=0.82, brightness=0.75, decay=0.65))],
    )
    values["modal_dark"] = instrument(
        "Modal Dark Body",
        [layer("body", modal(mode_count=24, brightness=0.05), gain_db=6.0)],
    )
    values["modal_bright"] = instrument(
        "Modal Bright Body",
        [layer("body", modal(mode_count=24, brightness=1.0))],
    )
    values["modal_short_decay"] = instrument(
        "Modal Short Decay",
        [layer("body", modal(mode_count=12, decay=0.2), gain_db=6.0)],
    )
    values["modal_long_decay"] = instrument(
        "Modal Long Decay",
        [layer("body", modal(mode_count=24, decay=1.0))],
    )
    values["modal_impulse"] = instrument(
        "Modal Impulse",
        [layer("body", modal(mode_count=12, exciter={"type": "impulse"}))],
    )
    values["modal_noise_burst"] = instrument(
        "Modal Noise Burst",
        [layer("body", modal(mode_count=12))],
    )

    values["physical_pluck"] = instrument(
        "Physical Pluck",
        [
            layer(
                "string",
                physical_string(
                    duration_seconds=0.004,
                    exciter_brightness=0.8,
                    seed=4201,
                    decay_seconds=2.8,
                    brightness=0.62,
                    stiffness=0.08,
                ),
                gain_db=0.0,
            )
        ],
        voice_processors=[
            processor("filter", "pluck_tone", mode="low_pass", cutoff_hz=9_000.0, resonance=0.16)
        ],
        global_processors=[
            processor("reverb", "pluck_space", pre_delay_seconds=0.012, decay=0.35, damping=0.45, width=0.8, mix=0.18),
            processor("limiter", "pluck_ceiling", ceiling_db=-1.0, release_ms=80.0, input_gain_db=0.0),
        ],
    )
    values["modal_mallet"] = instrument(
        "Modal Mallet",
        [
            layer(
                "body",
                modal(
                    mode_count=12,
                    structure=0.34,
                    brightness=0.58,
                    decay=0.62,
                    exciter={"type": "noise_burst", "duration_seconds": 0.010, "brightness": 0.42, "seed": 4301},
                ),
                gain_db=0.0,
            )
        ],
        voice_processors=[
            processor("eq", "mallet_body", low_frequency_hz=180.0, low_gain_db=3.0, mid_frequency_hz=1_100.0, mid_gain_db=-1.5, mid_q=1.0, high_frequency_hz=7_000.0, high_gain_db=-2.0)
        ],
        global_processors=[
            processor("reverb", "mallet_space", pre_delay_seconds=0.008, decay=0.3, damping=0.5, width=0.85, mix=0.16),
            processor("limiter", "mallet_ceiling", ceiling_db=-1.0, release_ms=80.0, input_gain_db=0.0),
        ],
    )
    values["imaginary_metal_body"] = instrument(
        "Imaginary Metal Body",
        [
            layer("string", physical_string(seed=4401, decay_seconds=2.2, brightness=0.9, stiffness=0.78), gain_db=-1.0, pan=-0.08),
            layer("body", modal(mode_count=24, structure=0.78, brightness=0.88, decay=0.72, exciter={"type": "impulse"}), gain_db=-2.0, pan=0.08),
        ],
        voice_processors=[
            processor("filter", "metal_tone", mode="low_pass", cutoff_hz=13_000.0, resonance=0.22),
            processor("drive", "metal_edge", amount=0.12, mix=0.2),
            processor("compressor", "metal_glue", threshold_db=-20.0, ratio=2.5, attack_ms=8.0, release_ms=120.0, knee_db=6.0, makeup_gain_db=1.0, mix=0.7),
        ],
        global_processors=[
            processor("chorus", "metal_width", delay_ms=14.0, rate_hz=0.22, depth=0.35, feedback=0.08, width=0.75, mix=0.16),
            processor("reverb", "metal_space", pre_delay_seconds=0.015, decay=0.42, damping=0.32, width=0.95, mix=0.2),
            processor("limiter", "metal_ceiling", ceiling_db=-1.0, release_ms=80.0, input_gain_db=0.0),
        ],
        modulation={
            "sources": [],
            "routes": [
                {
                    "source": "mod_wheel",
                    "target": "layer.body.generator.modal_decay",
                    "depth": {"value": 0.35, "unit": "normalized"},
                    "curve": "linear",
                }
            ],
        },
    )
    return values


def performance_events(voice_count: int, note_off_frame: int = 24_000) -> list[dict[str, object]]:
    events: list[dict[str, object]] = []
    for index in range(voice_count):
        events.append(
            {
                "absolute_frame": index * 32,
                "type": "note_on",
                "note_id": index + 1,
                "note": 48 + index % 24,
                "velocity": 96 + index % 32,
            }
        )
    for index in range(voice_count):
        events.append(
            {
                "absolute_frame": note_off_frame,
                "type": "note_off",
                "note_id": index + 1,
            }
        )
    return events


def render_events_with_trace(
    definition: Path,
    events: Path,
    output: Path,
    duration_frames: int,
    parameters: list[str],
) -> dict[str, object]:
    arguments = [
        "render",
        "events",
        str(definition),
        str(events),
        "--duration-frames",
        str(duration_frames),
        "--sample-rate",
        str(SAMPLE_RATE),
        "--block-size",
        str(BASE_BLOCK_SIZE),
        "--tail",
        "0",
        "--output",
        str(output),
        "--json",
        "--trace-every-frames",
        "256",
    ]
    for parameter in parameters:
        arguments.extend(("--trace", parameter))
    report = json.loads(run_cli(arguments))
    trace = report.get("trace")
    if not isinstance(trace, dict):
        raise RuntimeError(f"trace report missing from render: {report}")
    return trace


def render_events_with_reset_check(
    definition: Path,
    events: Path,
    output: Path,
    duration_frames: int,
) -> dict[str, object]:
    report = json.loads(
        run_cli(
            [
                "render",
                "events",
                str(definition),
                str(events),
                "--duration-frames",
                str(duration_frames),
                "--sample-rate",
                str(SAMPLE_RATE),
                "--block-size",
                str(BASE_BLOCK_SIZE),
                "--tail",
                "0",
                "--output",
                str(output),
                "--reset-check",
                "--json",
            ]
        )
    )
    comparison = report.get("reset_comparison")
    if not isinstance(comparison, dict):
        raise RuntimeError(f"reset comparison missing from render: {report}")
    return comparison


def main() -> None:
    for directory in (
        DEFINITION_DIR,
        VALIDATION_DIR,
        INSPECT_DIR,
        TRACE_DIR,
        TECHNICAL_DIR,
        MUSICAL_DIR,
        EVENT_DIR,
    ):
        directory.mkdir(parents=True, exist_ok=True)

    definition_values = definitions()
    definition_paths: dict[str, Path] = {}
    validation_reports: dict[str, object] = {}
    for name, value in definition_values.items():
        path = DEFINITION_DIR / f"{name}.json"
        write_definition(path, value)
        definition_paths[name] = path
        report = json.loads(run_cli(["instrument", "validate", str(path), "--json"]))
        validation_reports[name] = report
        if report.get("status") != "ok":
            raise RuntimeError(f"validation failed for {name}: {report}")
        write_utf8(VALIDATION_DIR / f"{name}.json", json.dumps(report, ensure_ascii=False, indent=2) + "\n")

    for name in ("physical_pluck", "modal_mallet", "imaginary_metal_body"):
        report = run_cli(["instrument", "inspect", str(definition_paths[name]), "--json"])
        write_utf8(INSPECT_DIR / f"{name}.json", report)

    physical_event_path = EVENT_DIR / "physical-modal-parameter-sweep.json"
    write_events(
        physical_event_path,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 48, "velocity": 112},
            {"absolute_frame": 8_000, "type": "parameter_change", "parameter": "layer.string.generator.physical_string_brightness", "native_value": 0.2},
            {"absolute_frame": 12_000, "type": "parameter_change", "parameter": "layer.string.generator.physical_string_stiffness", "native_value": 0.9},
            {"absolute_frame": 16_000, "type": "parameter_change", "parameter": "layer.body.generator.modal_structure", "native_value": 0.35},
            {"absolute_frame": 20_000, "type": "parameter_change", "parameter": "layer.body.generator.modal_decay", "native_value": 0.85},
            {"absolute_frame": 24_000, "type": "mod_wheel", "value": 1.0},
            {"absolute_frame": 32_000, "type": "note_off", "note_id": 1},
        ],
    )
    performance_event_path = EVENT_DIR / "performance-note.json"
    write_events(
        performance_event_path,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 2, "note": 60, "velocity": 112},
            {"absolute_frame": 24_000, "type": "note_off", "note_id": 2},
        ],
    )

    technical_jobs = [
        (name, TECHNICAL_NOTE)
        for name in definition_values
        if name not in MUSICAL_NOTES
    ]
    technical_paths: dict[str, Path] = {}
    for name, note in technical_jobs:
        path = TECHNICAL_DIR / f"{name}.wav"
        render_note(definition_paths[name], note, path, gate_seconds=0.12, tail_seconds=1.0)
        technical_paths[name] = path

    musical_paths: dict[str, Path] = {}
    for name, note in MUSICAL_NOTES.items():
        path = MUSICAL_DIR / f"{name}.wav"
        render_note(definition_paths[name], note, path, gate_seconds=0.18, tail_seconds=2.5)
        musical_paths[name] = path
    sweep_path = MUSICAL_DIR / "imaginary_metal_body-parameter-sweep.wav"
    render_events(definition_paths["imaginary_metal_body"], physical_event_path, sweep_path, BASE_BLOCK_SIZE, duration_frames=48_000)
    musical_paths["imaginary_metal_body-parameter-sweep"] = sweep_path

    trace_parameters = [
        "layer.string.generator.physical_string_brightness",
        "layer.string.generator.physical_string_stiffness",
        "layer.body.generator.modal_structure",
        "layer.body.generator.modal_decay",
    ]
    trace = render_events_with_trace(
        definition_paths["imaginary_metal_body"],
        physical_event_path,
        sweep_path,
        duration_frames=48_000,
        parameters=trace_parameters,
    )
    if len(trace.get("parameters", [])) != len(trace_parameters):
        raise RuntimeError(f"unexpected trace parameter count: {trace}")
    for parameter in trace["parameters"]:
        observations = parameter.get("observations", [])
        if not observations or any(
            not isinstance(observation.get("final"), (int, float))
            or not math.isfinite(float(observation["final"]))
            for observation in observations
        ):
            raise RuntimeError(f"trace final values are incomplete: {parameter}")
    write_utf8(
        TRACE_DIR / "parameter-sweep.json",
        json.dumps(trace, ensure_ascii=False, indent=2) + "\n",
    )

    all_audio = {**technical_paths, **musical_paths}
    technical_metrics: dict[str, object] = {}
    for name, path in sorted(all_audio.items()):
        note = MUSICAL_NOTES.get(name, TECHNICAL_NOTE)
        technical_metrics[path.name] = measure(
            path,
            list(BLOCK_SIZES),
            include_spectrum=name in {"string_impulse", "string_low_stiffness", "string_medium_stiffness", "string_bright", "string_high_stiffness", "modal_4_modes", "modal_12_modes", "modal_24_modes", "modal_stretched_structure", "physical_pluck", "modal_mallet", "imaginary_metal_body"},
            fundamental_frequency_hz=midi_note_frequency(note),
        )
        pitch_error = nearest_spectral_peak_error_cents(
            technical_metrics[path.name], midi_note_frequency(note)
        )
        if pitch_error is not None:
            technical_metrics[path.name]["nearest_spectral_peak_error_cents"] = pitch_error
        if name in PITCH_ACCURACY_DEFINITIONS:
            autocorrelation_error = autocorrelation_pitch_error_cents(
                path, midi_note_frequency(note)
            )
            if autocorrelation_error is None:
                raise RuntimeError(f"pitch accuracy could not be estimated for {name}")
            technical_metrics[path.name][
                "autocorrelation_pitch_error_cents"
            ] = autocorrelation_error
            technical_metrics[path.name][
                "pitch_accuracy_limit_cents"
            ] = PITCH_ACCURACY_LIMIT_CENTS
            technical_metrics[path.name]["pitch_accuracy_pass"] = (
                abs(autocorrelation_error) <= PITCH_ACCURACY_LIMIT_CENTS
            )
    invalid_audio = [name for name, values in technical_metrics.items() if not values["finite"]]
    if invalid_audio:
        raise RuntimeError(f"physical/modal audio checks failed: {invalid_audio}")
    invalid_pitch = [
        name
        for name, values in technical_metrics.items()
        if name in PITCH_ACCURACY_FILENAMES
        and not values.get("pitch_accuracy_pass", False)
    ]
    if invalid_pitch:
        raise RuntimeError(f"pitch accuracy checks failed: {invalid_pitch}")

    audition_level_checks = {}
    for name in ("modal_dark", "modal_short_decay", "string_soft"):
        values = technical_metrics[f"{name}.wav"]
        rms = float(values["rms"])
        audition_level_checks[name] = {
            "rms": rms,
            "minimum_rms": MIN_AUDITION_RMS,
            "pass": rms >= MIN_AUDITION_RMS,
        }
    failed_audition_levels = [
        name for name, values in audition_level_checks.items() if not values["pass"]
    ]
    if failed_audition_levels:
        raise RuntimeError(f"audition level checks failed: {failed_audition_levels}")

    modal_density_comparison = compare_wav(
        technical_paths["modal_12_modes"], technical_paths["modal_24_modes"]
    )
    if not modal_density_comparison.get("compatible"):
        raise RuntimeError(f"modal density renders are incompatible: {modal_density_comparison}")
    modal_density_rms = max(
        float(technical_metrics["modal_12_modes.wav"]["rms"]),
        float(technical_metrics["modal_24_modes.wav"]["rms"]),
    )
    modal_density_difference_ratio = (
        float(modal_density_comparison["rms_difference"]) / modal_density_rms
        if modal_density_rms > 0.0
        else 0.0
    )
    modal_density_check = {
        **modal_density_comparison,
        "rms_difference_ratio": modal_density_difference_ratio,
        "minimum_difference_ratio": MIN_MODAL_DENSITY_DIFFERENCE_RATIO,
        "pass": modal_density_difference_ratio >= MIN_MODAL_DENSITY_DIFFERENCE_RATIO,
    }
    if not modal_density_check["pass"]:
        raise RuntimeError(f"modal density difference is too small: {modal_density_check}")

    block_size_comparisons: dict[str, object] = {}
    parameter_change_block_size_comparisons: dict[str, object] = {}
    sample_rate_metrics: dict[str, object] = {}
    fresh_render_comparisons: dict[str, object] = {}
    reset_comparisons: dict[str, object] = {}
    performance_matrix: dict[str, object] = {}
    with tempfile.TemporaryDirectory(prefix="sonalloy-physical-modal-") as temporary:
        temporary_root = Path(temporary)
        for name in MUSICAL_NOTES:
            definition_path = definition_paths[name]
            reference = temporary_root / f"{name}-block-257.wav"
            render_note(definition_path, MUSICAL_NOTES[name], reference, BASE_BLOCK_SIZE, tail_seconds=1.0)
            comparisons: dict[str, object] = {}
            for block_size in BLOCK_SIZES:
                candidate = temporary_root / f"{name}-block-{block_size}.wav"
                render_note(definition_path, MUSICAL_NOTES[name], candidate, block_size, tail_seconds=1.0)
                comparisons[str(block_size)] = compare_wav(reference, candidate)
            invalid_comparisons = {
                block_size: comparison
                for block_size, comparison in comparisons.items()
                if not comparison.get("compatible")
                or float(comparison.get("max_abs_difference", 1.0)) > BLOCK_SIZE_MAX_DIFFERENCE
            }
            if invalid_comparisons:
                raise RuntimeError(f"{name} block-size mismatch: {invalid_comparisons}")
            block_size_comparisons[name] = comparisons

            rates: dict[str, object] = {}
            for sample_rate in REVIEW_SAMPLE_RATES:
                candidate = temporary_root / f"{name}-rate-{sample_rate}.wav"
                render_note(definition_path, MUSICAL_NOTES[name], candidate, BASE_BLOCK_SIZE, sample_rate, tail_seconds=1.0)
                values = measure(candidate, list(BLOCK_SIZES), include_spectrum=True, fundamental_frequency_hz=midi_note_frequency(MUSICAL_NOTES[name]))
                if not values["finite"]:
                    raise RuntimeError(f"{name} sample-rate render is non-finite at {sample_rate} Hz")
                rates[str(sample_rate)] = values
            sample_rate_metrics[name] = rates

            first = temporary_root / f"{name}-fresh-a.wav"
            second = temporary_root / f"{name}-fresh-b.wav"
            render_note(definition_path, MUSICAL_NOTES[name], first, BASE_BLOCK_SIZE, tail_seconds=1.0)
            render_note(definition_path, MUSICAL_NOTES[name], second, BASE_BLOCK_SIZE, tail_seconds=1.0)
            comparison = compare_wav(first, second)
            if not comparison.get("compatible") or comparison.get("max_abs_difference") != 0.0:
                raise RuntimeError(f"{name} fresh render is not reproducible: {comparison}")
            fresh_render_comparisons[name] = {
                **comparison,
                "first_sha256": sha256_file(first),
                "second_sha256": sha256_file(second),
            }

            reset_events = temporary_root / f"{name}-reset-events.json"
            write_events(
                reset_events,
                [
                    {
                        "absolute_frame": 0,
                        "type": "note_on",
                        "note_id": 1,
                        "note": MUSICAL_NOTES[name],
                        "velocity": 112,
                    },
                    {
                        "absolute_frame": 7_200,
                        "type": "note_off",
                        "note_id": 1,
                    },
                ],
            )
            reset_output = temporary_root / f"{name}-reset.wav"
            same_runtime_comparison = render_events_with_reset_check(
                definition_path,
                reset_events,
                reset_output,
                duration_frames=48_000,
            )
            fresh_output = temporary_root / f"{name}-fresh-events.wav"
            render_events(
                definition_path,
                reset_events,
                fresh_output,
                BASE_BLOCK_SIZE,
                duration_frames=48_000,
            )
            fresh_runtime_comparison = compare_wav(fresh_output, reset_output)
            if (
                not same_runtime_comparison.get("compatible")
                or same_runtime_comparison.get("max_abs_difference") != 0.0
                or not fresh_runtime_comparison.get("compatible")
                or fresh_runtime_comparison.get("max_abs_difference") != 0.0
            ):
                raise RuntimeError(
                    f"{name} reset is not reproducible: "
                    f"same_runtime={same_runtime_comparison}, "
                    f"fresh_runtime={fresh_runtime_comparison}"
                )
            reset_comparisons[name] = {
                "same_runtime": same_runtime_comparison,
                "fresh_runtime": fresh_runtime_comparison,
                "reset_sha256": sha256_file(reset_output),
                "fresh_sha256": sha256_file(fresh_output),
            }

        dynamic_reference = temporary_root / "parameter-change-block-257.wav"
        render_events(
            definition_paths["imaginary_metal_body"],
            physical_event_path,
            dynamic_reference,
            BASE_BLOCK_SIZE,
            duration_frames=48_000,
        )
        dynamic_comparisons: dict[str, object] = {}
        for block_size in BLOCK_SIZES:
            candidate = temporary_root / f"parameter-change-block-{block_size}.wav"
            render_events(
                definition_paths["imaginary_metal_body"],
                physical_event_path,
                candidate,
                block_size,
                duration_frames=48_000,
            )
            comparison = compare_wav(dynamic_reference, candidate)
            if (
                not comparison.get("compatible")
                or float(comparison.get("max_abs_difference", 1.0)) > BLOCK_SIZE_MAX_DIFFERENCE
                or float(comparison.get("rms_difference", 1.0)) > 1.0e-5
            ):
                raise RuntimeError(
                    f"parameter-change block-size mismatch at {block_size}: {comparison}"
                )
            dynamic_comparisons[str(block_size)] = comparison
        parameter_change_block_size_comparisons["imaginary_metal_body"] = dynamic_comparisons

        performance: dict[str, object] = {}
        for name in ("physical_pluck", "modal_mallet"):
            performance[name] = timed_render(
                definition_paths[name],
                performance_event_path,
                temporary_root / f"{name}-performance.wav",
                duration_frames=48_000,
                block_size=BASE_BLOCK_SIZE,
                sample_rate=SAMPLE_RATE,
                release=True,
            )
            if float(performance[name]["realtime_ratio"]) >= 1.0:
                raise RuntimeError(f"{name} is slower than realtime: {performance[name]}")

        performance_jobs = {
            "physical_string": ("physical_pluck", (1, 8, 16, 32)),
            "modal_12_modes": ("modal_mallet", (1, 8, 16)),
            "modal_24_modes": ("modal_24_modes", (1, 8, 16)),
        }
        for matrix_name, (source_name, voice_counts) in performance_jobs.items():
            sample_rate_metrics: dict[str, object] = {}
            for sample_rate in PERFORMANCE_SAMPLE_RATES:
                voice_metrics: dict[str, object] = {}
                for voice_count in voice_counts:
                    definition_path = temporary_root / f"{matrix_name}-{sample_rate}-{voice_count}-voices.json"
                    value = json.loads(json.dumps(definition_values[source_name]))
                    value["performance"]["polyphony"] = voice_count
                    write_definition(definition_path, value)
                    events_path = temporary_root / f"{matrix_name}-{sample_rate}-{voice_count}-voices-events.json"
                    write_events(events_path, performance_events(voice_count))
                    output_path = temporary_root / f"{matrix_name}-{sample_rate}-{voice_count}-voices.wav"
                    metrics = timed_render(
                        definition_path,
                        events_path,
                        output_path,
                        duration_frames=48_000,
                        block_size=BASE_BLOCK_SIZE,
                        sample_rate=sample_rate,
                        release=True,
                    )
                    audio_metrics = measure(output_path, [], include_spectrum=False)
                    if audio_metrics["sample_rate"] != sample_rate or not audio_metrics["finite"]:
                        raise RuntimeError(
                            f"{matrix_name} at {sample_rate} Hz / {voice_count} voices is invalid: {audio_metrics}"
                        )
                    voice_metrics[str(voice_count)] = {
                        **metrics,
                        "finite": audio_metrics["finite"],
                        "rms": audio_metrics["rms"],
                    }
                sample_rate_metrics[str(sample_rate)] = {"voices": voice_metrics}
            performance_matrix[matrix_name] = {
                "source_definition": source_name,
                "sample_rates": sample_rate_metrics,
            }

    metrics = {
        "sample_rate": SAMPLE_RATE,
        "base_block_size": BASE_BLOCK_SIZE,
        "block_sizes": list(BLOCK_SIZES),
        "sample_rates": list(REVIEW_SAMPLE_RATES),
        "performance_sample_rates": list(PERFORMANCE_SAMPLE_RATES),
        "validation": validation_reports,
        "technical": technical_metrics,
        "block_size_comparisons": block_size_comparisons,
        "parameter_change_block_size_comparisons": parameter_change_block_size_comparisons,
        "sample_rate_metrics": sample_rate_metrics,
        "fresh_render_comparisons": fresh_render_comparisons,
        "reset_comparisons": reset_comparisons,
        "performance": performance,
        "performance_matrix": performance_matrix,
        "trace": trace,
        "audio_sha256": {path.name: sha256_file(path) for path in all_audio.values()},
        "audition_level_checks": audition_level_checks,
        "modal_density_check": modal_density_check,
    }
    write_utf8(REVIEW_ROOT / "metrics.json", json.dumps(metrics, ensure_ascii=False, indent=2) + "\n")

    summary = """# Physical / Modal Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Performance Matrix：48,000 / 96,000 Hz、Physical String 1 / 8 / 16 / 32 voices、Modal 12 / 24 modes × 1 / 8 / 16 voices
- Output：Stereo、32-bit float WAV
- Backend：DaisySP V1.0.0 (`a0494a3adb67f549e18dfd71a35fa656f65b38b6`)
- Technical Definitionの基準Layer Gain：+3 dB
- `modal_dark`、`modal_short_decay`、`string_soft`：比較用Layer Gain +6 dB

## 生成物

`definitions/`にTechnical Definitionと3つのMusical Definition、`validation/`に全DefinitionのCLI Validate JSON、`inspect/`に3つのMusical DefinitionのInspect JSON、`audio/technical/`と`audio/musical/`に同じ生出力、`trace/`にParameter Traceを保存しています。`metrics.json`はCLIの`--analyze --json`を基礎にFinite性、Level、DC、Continuity、Spectrum、Block Size、Sample Rate、Fresh Runtime、Reset、SHA-256、Performanceを記録し、Physical StringはStiffness 0 / 0.5 / 1の時間領域自己相関でPitch Errorを20 cents以内へ検証します。Dynamic ParameterのBlock Size比較、Trace Final Value、同一Prepared RuntimeのReset再現性、Fresh Runtimeとの一致、48,000 / 96,000 HzのVoice数別Performance Matrixも記録します。FFTのNearest Bin値は分解能の参考値として併記します。

再生成：

```bash
python3 review/generate/generate_physical_modal_package.py
```

## Musical Definition

| Definition | 目的 | WAV |
|---|---|---|
| `physical_pluck` | StringのPitch、Natural Decay、Brightness、既存Processorとの組み合わせ | `audio/musical/physical_pluck.wav` |
| `modal_mallet` | Wood / Bar方向のAttack、Mode Density、Body Decay | `audio/musical/modal_mallet.wav` |
| `imaginary_metal_body` | Physical String + Modal + Processorによる架空の金属Body | `audio/musical/imaginary_metal_body.wav` |

Parameter Changeを含むHybridの出力は`audio/musical/imaginary_metal_body-parameter-sweep.wav`です。

## Technical Definition

String：Impulse、Noise BurstのSoft / Bright、Short / Long Decay、Loop Brightness、Low / Medium / High Stiffnessを含みます。Modal：4 / 8 / 12 / 16 / 20 / 24 Mode、Harmonic / Stretched Structure、Dark / Bright、Short / Long Decay、Impulse / Noise Burstを含みます。Technical Definitionは基準Layer Gainを揃え、Dark / Short / Soft Loopだけ比較用Gainを加えて、発音とTailを確認できるようにしています。

## 人間の試聴欄

- [ ] StringのPitchがNote間で安定し、Stiffness 0 / 0.5 / 1で基音が保たれる
- [ ] StringのShort / Long Decayが単なる音量差ではなくTailの長さとして聞こえる
- [ ] StringのLoop Brightnessで高域Lossが変化する
- [ ] StringのStiffnessを上げると高次成分が硬く、Metallic方向へ変化する
- [ ] Modalの4 / 12 / 24 Modeで共鳴密度の差が聞こえる
- [ ] ModalのStructureでMode配置のCharacterが変わる
- [ ] ModalのBrightnessで高次Modeの存在感が変わる
- [ ] ModalのDecayで共鳴Tailの長さが変わる
- [ ] `physical_pluck`が撥弦系の実用的な基準音色として成立する
- [ ] `modal_mallet`が木質・棒状のBody方向として成立する
- [ ] `imaginary_metal_body`が既存Processorと混ぜても破綻しない架空音色として成立する
- [ ] Block SizeやSample Rateを変えてClick・Timing差・大きな音色破綻がない
- [ ] `audio/musical/imaginary_metal_body-parameter-sweep.wav`でParameter Changeが連続的に聞こえる
- [ ] `trace/parameter-sweep.json`のFinal Valueが演奏中のParameter変化を反映している
- [ ] `metrics.json`のReset比較が同一Prepared RuntimeとFresh Runtimeの両方で一致している
- [ ] `metrics.json`のPhysical / Modal Performance測定結果を実行環境の基準として確認した

### 人間の回答

- 判定：
- 修正指示：
- 確認者：
- 確認日：
"""
    write_utf8(REVIEW_ROOT / "review-summary.md", summary)


if __name__ == "__main__":
    main()
