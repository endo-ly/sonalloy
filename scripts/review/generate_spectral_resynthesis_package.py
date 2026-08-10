#!/usr/bin/env python3
"""Generate the deterministic spectral resynthesis sound review package."""

from __future__ import annotations

import json
import math
import shutil
import tempfile
from pathlib import Path

import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

from common import (  # noqa: E402
    BASE_BLOCK_SIZE,
    BLOCK_SIZES,
    SAMPLE_RATE,
    build_cli,
    measure_stereo,
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
from measure_wav import (  # noqa: E402
    boundary_differences,
    compare_wav,
    measure,
    read_float_wav,
)


REVIEW_ROOT = ROOT / "review-output" / "spectral-resynthesis"
TECHNICAL_AUDIO = REVIEW_ROOT / "audio" / "technical"
MINIMUM_AUDIO_RMS = 1.0e-8
MAX_BLOCK_DIFFERENCE = 1.0e-5
FREEZE_TRANSITION_FRAME = 8_192
MAX_FREEZE_TRANSITION_DELTA = 0.01


def definition_path(name: str) -> Path:
    return ROOT / "examples" / "instruments" / name


def read_definition(path: Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def inspect_definition(path: Path) -> dict[str, object]:
    report = json.loads(run_cli(["instrument", "inspect", str(path), "--json"]))
    if report.get("status") != "ok":
        raise RuntimeError(f"inspect failed for {path}: {report}")
    return report


def performance_audio_metrics(path: Path) -> dict[str, object]:
    sample_rate, channels, samples = read_float_wav(path)
    return {
        "sample_rate": sample_rate,
        "channels": channels,
        "frames": len(samples) // channels if channels else 0,
        "finite": all(math.isfinite(sample) for sample in samples),
        "peak": max((abs(sample) for sample in samples), default=0.0),
        "rms": math.sqrt(sum(sample * sample for sample in samples) / len(samples))
        if samples
        else 0.0,
    }


def spectrum_snapshot(
    path: Path, start_frame: int | None = None, fft_size: int = 512
) -> dict[str, float]:
    sample_rate, channels, samples = read_float_wav(path)
    frames = len(samples) // channels
    if frames < 4:
        return {
            "dominant_frequency_hz": 0.0,
            "spectral_centroid_hz": 0.0,
            "near_nyquist_energy_ratio": 0.0,
        }
    fft_size = min(fft_size, frames)
    start = (
        min(max(start_frame or 0, 0), frames - fft_size)
        if start_frame is not None
        else min(frames - fft_size, int(sample_rate * 0.2))
    )
    left = samples[0::channels]
    magnitudes: list[float] = []
    for bin_index in range(fft_size // 2 + 1):
        real = 0.0
        imaginary = 0.0
        for index in range(fft_size):
            window = 0.5 - 0.5 * math.cos(
                2.0 * math.pi * index / max(1, fft_size - 1)
            )
            angle = 2.0 * math.pi * bin_index * index / fft_size
            sample = left[start + index] * window
            real += sample * math.cos(angle)
            imaginary -= sample * math.sin(angle)
        magnitudes.append(math.hypot(real, imaginary))
    energies = [magnitude * magnitude for magnitude in magnitudes]
    total_energy = sum(energies[1:])
    if total_energy <= 0.0:
        return {
            "dominant_frequency_hz": 0.0,
            "spectral_centroid_hz": 0.0,
            "near_nyquist_energy_ratio": 0.0,
        }
    dominant_bin = max(range(1, len(magnitudes)), key=magnitudes.__getitem__)
    bin_width = sample_rate / fft_size
    centroid = sum(
        index * bin_width * energy for index, energy in enumerate(energies[1:], 1)
    ) / total_energy
    near_nyquist_start = max(1, int(0.8 * (fft_size // 2)))
    near_nyquist_energy = sum(energies[near_nyquist_start:])
    return {
        "dominant_frequency_hz": dominant_bin * bin_width,
        "spectral_centroid_hz": centroid,
        "near_nyquist_energy_ratio": near_nyquist_energy / total_energy,
    }


def spectral_flux(path: Path, start_frame: int, duration_frames: int) -> float:
    sample_rate, channels, samples = read_float_wav(path)
    del sample_rate
    frames = len(samples) // channels
    fft_size = 256
    hop_size = 128
    left = samples[0::channels]
    start = min(max(start_frame, 0), max(0, frames - fft_size))
    end = min(frames - fft_size, start + duration_frames)
    previous: list[float] | None = None
    flux_sum = 0.0
    flux_count = 0
    for frame in range(start, max(start, end) + 1, hop_size):
        magnitudes: list[float] = []
        for bin_index in range(1, fft_size // 2 + 1):
            real = 0.0
            imaginary = 0.0
            for index in range(fft_size):
                window = 0.5 - 0.5 * math.cos(
                    2.0 * math.pi * index / (fft_size - 1)
                )
                angle = 2.0 * math.pi * bin_index * index / fft_size
                sample = left[frame + index] * window
                real += sample * math.cos(angle)
                imaginary -= sample * math.sin(angle)
            magnitudes.append(math.hypot(real, imaginary))
        if previous is not None:
            scale = max(sum(magnitudes), 1.0e-20)
            flux_sum += sum(
                max(current - prior, 0.0)
                for current, prior in zip(magnitudes, previous)
            ) / scale
            flux_count += 1
        previous = magnitudes
    return flux_sum / flux_count if flux_count else 0.0


def aligned_identity_metrics(
    source_path: Path,
    rendered_path: Path,
    render_offset_frames: int,
    comparison_frames: int,
) -> dict[str, float | int]:
    source_rate, source_channels, source_samples = read_float_wav(source_path)
    rendered_rate, rendered_channels, rendered_samples = read_float_wav(rendered_path)
    if source_rate != rendered_rate or source_channels != rendered_channels:
        raise RuntimeError("identity source and render formats differ")
    source_frame_count = len(source_samples) // source_channels
    rendered_frame_count = len(rendered_samples) // rendered_channels
    start = 512
    end = min(
        source_frame_count,
        comparison_frames,
        max(0, rendered_frame_count - render_offset_frames),
    )
    if end <= start:
        raise RuntimeError("identity comparison has no usable frames")
    signal_power = 0.0
    error_power = 0.0
    maximum_error = 0.0
    expected_values: list[float] = []
    rendered_values: list[float] = []
    for source_frame in range(start, end):
        for channel in range(source_channels):
            expected = source_samples[source_frame * source_channels + channel]
            rendered = rendered_samples[
                (render_offset_frames + source_frame) * rendered_channels + channel
            ]
            signal_power += expected * expected
            error = rendered - expected
            error_power += error * error
            maximum_error = max(maximum_error, abs(error))
            expected_values.append(expected)
            rendered_values.append(rendered)
    expected_mean = sum(expected_values) / len(expected_values)
    rendered_mean = sum(rendered_values) / len(rendered_values)
    covariance = sum(
        (expected - expected_mean) * (rendered - rendered_mean)
        for expected, rendered in zip(expected_values, rendered_values)
    )
    expected_variance = sum((expected - expected_mean) ** 2 for expected in expected_values)
    rendered_variance = sum((rendered - rendered_mean) ** 2 for rendered in rendered_values)
    correlation_denominator = math.sqrt(expected_variance * rendered_variance)
    return {
        "render_offset_frames": render_offset_frames,
        "compared_frames": end - start,
        "snr_db": 10.0 * math.log10(signal_power / max(error_power, 1.0e-20)),
        "rms_error": math.sqrt(error_power / len(expected_values)),
        "max_error": maximum_error,
        "correlation": (
            covariance / correlation_denominator
            if correlation_denominator > 0.0
            else 1.0
        ),
    }


def maximum_frame(path: Path) -> int:
    _, channels, samples = read_float_wav(path)
    frames = len(samples) // channels
    return max(
        range(frames),
        key=lambda frame: max(
            abs(samples[frame * channels + channel]) for channel in range(channels)
        ),
    )


def assert_audio_metrics(metrics: dict[str, dict[str, object]]) -> None:
    invalid = [
        name
        for name, values in metrics.items()
        if not values["finite"] or values["rms"] <= MINIMUM_AUDIO_RMS
    ]
    if invalid:
        raise RuntimeError(f"spectral review audio checks failed: {invalid}")


def control_events() -> list[dict[str, object]]:
    return [
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
            "parameter": "layer.spectral.generator.spectral_position",
            "normalized": 0.8,
        },
        {
            "absolute_frame": 8_192,
            "type": "parameter_change",
            "parameter": "layer.spectral.generator.spectral_freeze",
            "normalized": 0.7,
        },
        {
            "absolute_frame": 12_288,
            "type": "parameter_change",
            "parameter": "layer.spectral.generator.spectral_blur",
            "normalized": 0.55,
        },
        {
            "absolute_frame": 16_384,
            "type": "parameter_change",
            "parameter": "layer.spectral.generator.spectral_shift",
            "normalized": 0.56,
        },
        {
            "absolute_frame": 20_480,
            "type": "parameter_change",
            "parameter": "layer.spectral.generator.spectral_morph",
            "normalized": 0.85,
        },
        {"absolute_frame": 24_576, "type": "mod_wheel", "value": 0.8},
        {"absolute_frame": 28_672, "type": "note_off", "note_id": 1},
    ]


def polyphony_events(voice_count: int, duration_frames: int) -> list[dict[str, object]]:
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
            "absolute_frame": duration_frames * 3 // 4 + index,
            "type": "note_off",
            "note_id": index + 1,
        }
        for index in range(voice_count)
    )
    return events


def copy_reference_inputs(
    definition_dir: Path, asset_dir: Path, midi_dir: Path
) -> dict[str, Path]:
    source_definitions = {
        "spectral": definition_path("spectral-generator-reference.json"),
        "hybrid": definition_path("spectral-hybrid-reference.json"),
    }
    definitions: dict[str, Path] = {}
    for name, source in source_definitions.items():
        value = read_definition(source)
        if name == "hybrid":
            for layer in value["layers"]:
                sample = layer.get("generator", {}).get("sample")
                if sample is not None:
                    sample["zones"][0]["asset"]["path"] = "../assets/metal-hit.wav"
        destination = definition_dir / source.name
        write_definition(destination, value)
        definitions[name] = destination

    for name in (
        "spectral-reference-a.wav",
        "spectral-reference-b.wav",
        "spectral-reference-impulse.wav",
    ):
        shutil.copy2(ROOT / "examples" / "assets" / name, asset_dir / name)
    shutil.copy2(
        ROOT / "testdata" / "assets" / "metal-hit.wav", asset_dir / "metal-hit.wav"
    )
    midi_source = ROOT / "testdata" / "midi" / "basic-poly-synth-phrase.mid"
    shutil.copy2(midi_source, midi_dir / midi_source.name)
    return definitions


def isolated_spectral_definition(
    source_definition: Path,
    asset_path: str,
    asset_sha256: str,
    destination: Path,
) -> Path:
    value = read_definition(source_definition)
    value["performance"]["polyphony"] = 1
    layer = value["layers"][0]
    layer["gain_db"] = 0.0
    layer["envelope"] = {
        "attack_seconds": 0.0,
        "decay_seconds": 0.0,
        "sustain_level": 1.0,
        "release_seconds": 0.0,
    }
    spectral = layer["generator"]["spectral"]
    spectral["asset_a"] = {"path": asset_path, "sha256": asset_sha256}
    spectral["asset_b"] = None
    spectral["position"] = 0.0
    spectral["freeze"] = 0.0
    spectral["blur_seconds"] = 0.0
    spectral["shift_hz"] = 0.0
    spectral["morph"] = 0.0
    layer["processors"] = []
    value["voice_processors"] = []
    value["global_processors"] = []
    value["modulation"] = None
    write_definition(destination, value)
    run_cli(["instrument", "validate", str(destination), "--json"])
    return destination


def create_special_definitions(
    definitions: dict[str, Path], asset_dir: Path
) -> dict[str, Path]:
    return {
        "identity": isolated_spectral_definition(
            definitions["spectral"],
            "../assets/spectral-reference-b.wav",
            sha256_file(asset_dir / "spectral-reference-b.wav"),
            REVIEW_ROOT / "definitions" / "identity-metric.json",
        ),
        "latency": isolated_spectral_definition(
            definitions["spectral"],
            "../assets/spectral-reference-impulse.wav",
            sha256_file(asset_dir / "spectral-reference-impulse.wav"),
            REVIEW_ROOT / "definitions" / "latency-impulse.json",
        ),
        "stealing": isolated_spectral_definition(
            definitions["spectral"],
            "../assets/spectral-reference-a.wav",
            sha256_file(asset_dir / "spectral-reference-a.wav"),
            REVIEW_ROOT / "definitions" / "spectral-voice-stealing.json",
        ),
    }


def regression_definition_paths() -> dict[str, Path]:
    names = {
        "oscillator": "basic-poly-synth-sine.json",
        "noise": "basic-generators-reference.json",
        "digital-hybrid": "digital-hybrid-reference.json",
        "granular": "granular-generator-reference.json",
        "wave-sequence": "wave-sequence-reference.json",
        "additive": "additive-generator-reference.json",
        "formant": "formant-generator-reference.json",
    }
    return {kind: definition_path(name) for kind, name in names.items()}


def render_technical_audio(
    definitions: dict[str, Path],
    regression_definitions: dict[str, Path],
    special_definitions: dict[str, Path],
    event_dir: Path,
    midi_dir: Path,
) -> tuple[dict[str, Path], dict[str, dict[str, object]], dict[str, object]]:
    audio_paths: dict[str, Path] = {}
    metrics: dict[str, dict[str, object]] = {}

    control_path = event_dir / "spectral-controls.json"
    write_events(control_path, control_events())
    polyphony_path = event_dir / "spectral-polyphony-16.json"
    write_events(polyphony_path, polyphony_events(16, 32_768))
    stealing_path = event_dir / "spectral-voice-stealing.json"
    write_events(
        stealing_path,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 48, "velocity": 112},
            {"absolute_frame": 64, "type": "note_on", "note_id": 2, "note": 60, "velocity": 112},
            {"absolute_frame": 8_192, "type": "note_off", "note_id": 2},
        ],
    )

    def add(name: str, path: Path) -> None:
        audio_paths[name] = path
        metrics[name] = {
            **measure(path, list(BLOCK_SIZES)),
            **measure_stereo(path),
        }

    def spectral_variant(name: str, **settings: object) -> Path:
        value = read_definition(definitions["spectral"])
        value["layers"][0]["generator"]["spectral"].update(settings)
        path = REVIEW_ROOT / "definitions" / f"{name}.json"
        write_definition(path, value)
        run_cli(["instrument", "validate", str(path), "--json"])
        return path

    static_cases = [
        ("01-identity-resynthesis", {"position": 0.0, "freeze": 0.0, "blur_seconds": 0.0, "shift_hz": 0.0, "morph": 0.0}, 60),
        ("02-position-quarter", {"position": 0.25}, 60),
        ("03-position-half", {"position": 0.5}, 60),
        ("04-freeze", {"freeze": 1.0}, 60),
        ("06-blur", {"blur_seconds": 0.25}, 60),
        ("07-shift-up", {"shift_hz": 900.0}, 60),
        ("08-shift-down", {"shift_hz": -900.0}, 60),
        ("09-root-note", {"root_note": 48}, 60),
        ("10-pitch-up-octave", {}, 72),
        ("11-pitch-down-octave", {}, 48),
        ("12-morph-a", {"morph": 0.0}, 60),
        ("13-morph-mid", {"morph": 0.5}, 60),
        ("14-morph-b", {"morph": 1.0}, 60),
        ("17-stereo-resynthesis", {}, 60),
        ("18-high-note-spectrum", {}, 108),
    ]
    for name, settings, note in static_cases:
        definition = spectral_variant(name, **settings)
        path = TECHNICAL_AUDIO / f"{name}.wav"
        render_note(definition, note, path, gate_seconds=0.3, tail_seconds=0.1)
        add(name, path)

    freeze_transition_events = event_dir / "freeze-transition.json"
    write_events(
        freeze_transition_events,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 112},
            {"absolute_frame": FREEZE_TRANSITION_FRAME, "type": "parameter_change", "parameter": "layer.spectral.generator.spectral_freeze", "normalized": 1.0},
            {"absolute_frame": 24_576, "type": "note_off", "note_id": 1},
        ],
    )
    freeze_transition = TECHNICAL_AUDIO / "05-freeze-transition.wav"
    render_events(
        definitions["spectral"],
        freeze_transition_events,
        freeze_transition,
        BASE_BLOCK_SIZE,
        duration_frames=28_672,
    )
    add("05-freeze-transition", freeze_transition)

    position_scrub_events = event_dir / "position-scrub.json"
    write_events(
        position_scrub_events,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 112},
            {"absolute_frame": 8_192, "type": "parameter_change", "parameter": "layer.spectral.generator.spectral_position", "normalized": 0.75},
            {"absolute_frame": 16_384, "type": "parameter_change", "parameter": "layer.spectral.generator.spectral_position", "normalized": 0.2},
            {"absolute_frame": 24_576, "type": "note_off", "note_id": 1},
        ],
    )
    position_scrub = TECHNICAL_AUDIO / "16-position-scrub.wav"
    render_events(
        definitions["spectral"],
        position_scrub_events,
        position_scrub,
        BASE_BLOCK_SIZE,
        duration_frames=28_672,
    )
    add("16-position-scrub", position_scrub)

    morph_sweep_events = event_dir / "morph-sweep.json"
    write_events(
        morph_sweep_events,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 112},
            {"absolute_frame": 8_192, "type": "parameter_change", "parameter": "layer.spectral.generator.spectral_morph", "normalized": 0.5},
            {"absolute_frame": 16_384, "type": "parameter_change", "parameter": "layer.spectral.generator.spectral_morph", "normalized": 1.0},
            {"absolute_frame": 24_576, "type": "note_off", "note_id": 1},
        ],
    )
    morph_sweep = TECHNICAL_AUDIO / "15-morph-sweep.wav"
    render_events(
        definitions["spectral"],
        morph_sweep_events,
        morph_sweep,
        BASE_BLOCK_SIZE,
        duration_frames=28_672,
    )
    add("15-morph-sweep", morph_sweep)

    stereo_resynthesis = TECHNICAL_AUDIO / "17-stereo-resynthesis.wav"
    spectral_note = stereo_resynthesis
    add("spectral-note", spectral_note)
    hybrid_note = TECHNICAL_AUDIO / "19-spectral-hybrid.wav"
    render_note(definitions["hybrid"], 60, hybrid_note, gate_seconds=0.3, tail_seconds=0.2)
    add("19-spectral-hybrid", hybrid_note)
    hybrid_midi = TECHNICAL_AUDIO / "20-spectral-hybrid-midi.wav"
    render_midi(
        definitions["hybrid"],
        midi_dir / "basic-poly-synth-phrase.mid",
        hybrid_midi,
        tail_seconds=0.2,
    )
    add("20-spectral-hybrid-midi", hybrid_midi)
    hybrid_controls = TECHNICAL_AUDIO / "spectral-hybrid-controls.wav"
    render_events(
        definitions["hybrid"],
        control_path,
        hybrid_controls,
        BASE_BLOCK_SIZE,
        duration_frames=32_768,
    )
    add("spectral-hybrid-controls", hybrid_controls)
    polyphony_audio = TECHNICAL_AUDIO / "21-spectral-polyphony.wav"
    render_events(
        definitions["spectral"],
        polyphony_path,
        polyphony_audio,
        BASE_BLOCK_SIZE,
        duration_frames=32_768,
    )
    add("21-spectral-polyphony", polyphony_audio)
    stealing_audio = TECHNICAL_AUDIO / "spectral-voice-stealing.wav"
    render_events(
        special_definitions["stealing"],
        stealing_path,
        stealing_audio,
        BASE_BLOCK_SIZE,
        duration_frames=16_384,
    )
    add("spectral-voice-stealing", stealing_audio)

    identity_metric = TECHNICAL_AUDIO / "identity-metric.wav"
    render_note(
        special_definitions["identity"],
        60,
        identity_metric,
        gate_seconds=0.25,
        tail_seconds=0.05,
    )
    add("identity-metric", identity_metric)
    latency_impulse = TECHNICAL_AUDIO / "latency-impulse.wav"
    render_note(
        special_definitions["latency"],
        60,
        latency_impulse,
        gate_seconds=0.3,
        tail_seconds=0.1,
    )
    add("latency-impulse", latency_impulse)

    fft_paths: dict[str, Path] = {"2048": stereo_resynthesis}
    for fft_size in (1024, 4096):
        definition = spectral_variant(f"spectral-fft-{fft_size}", fft_size=fft_size)
        path = TECHNICAL_AUDIO / f"fft-{fft_size}.wav"
        render_note(definition, 60, path, gate_seconds=0.3, tail_seconds=0.1)
        fft_paths[str(fft_size)] = path
    for fft_size in (1024, 2048, 4096):
        add(f"fft-{fft_size}", fft_paths[str(fft_size)])

    block_paths: dict[str, Path] = {str(BASE_BLOCK_SIZE): stereo_resynthesis}
    for block_size in BLOCK_SIZES:
        if block_size == BASE_BLOCK_SIZE:
            continue
        path = TECHNICAL_AUDIO / f"spectral-block-{block_size}.wav"
        render_note(
            definitions["spectral"],
            60,
            path,
            block_size=block_size,
            gate_seconds=0.3,
            tail_seconds=0.1,
        )
        block_paths[str(block_size)] = path
    for block_size in BLOCK_SIZES:
        add(f"spectral-block-{block_size}", block_paths[str(block_size)])

    sample_rate_paths: dict[str, Path] = {str(SAMPLE_RATE): stereo_resynthesis}
    for sample_rate in (44_100, 96_000):
        path = TECHNICAL_AUDIO / f"spectral-sample-rate-{sample_rate}.wav"
        render_note(
            definitions["spectral"],
            60,
            path,
            sample_rate=sample_rate,
            gate_seconds=0.3,
            tail_seconds=0.1,
        )
        sample_rate_paths[str(sample_rate)] = path
    for sample_rate in (44_100, SAMPLE_RATE, 96_000):
        add(f"spectral-sample-rate-{sample_rate}", sample_rate_paths[str(sample_rate)])

    fresh_a = TECHNICAL_AUDIO / "spectral-fresh-a.wav"
    fresh_b = TECHNICAL_AUDIO / "spectral-fresh-b.wav"
    for path in (fresh_a, fresh_b):
        render_note(definitions["hybrid"], 60, path, gate_seconds=0.3, tail_seconds=0.2)
        add(path.stem, path)

    regression_paths: dict[str, Path] = {}
    for kind, definition in regression_definitions.items():
        path = TECHNICAL_AUDIO / f"regression-{kind}.wav"
        run_cli(["instrument", "validate", str(definition), "--json"])
        render_note(definition, 60, path, gate_seconds=0.2, tail_seconds=0.1)
        regression_paths[kind] = path
        add(f"regression-{kind}", path)

    assert_audio_metrics(metrics)
    block_comparisons = {
        block_size: compare_wav(
            block_paths[str(BASE_BLOCK_SIZE)], block_paths[str(block_size)]
        )
        for block_size in BLOCK_SIZES
    }
    invalid_block_comparisons = {
        block_size: comparison
        for block_size, comparison in block_comparisons.items()
        if not comparison.get("compatible")
        or comparison.get("max_abs_difference", 1.0) > MAX_BLOCK_DIFFERENCE
    }
    if invalid_block_comparisons:
        raise RuntimeError(f"spectral block-size mismatch: {invalid_block_comparisons}")

    fresh_comparison = compare_wav(fresh_a, fresh_b)
    if not fresh_comparison.get("compatible") or fresh_comparison.get("max_abs_difference", 1.0) != 0.0:
        raise RuntimeError(f"spectral fresh render is not reproducible: {fresh_comparison}")

    latency_peak_frame = maximum_frame(latency_impulse)
    expected_latency_peak_frame = 2_048
    if latency_peak_frame != expected_latency_peak_frame:
        raise RuntimeError(
            "spectral latency impulse mismatch: "
            f"expected {expected_latency_peak_frame}, got {latency_peak_frame}"
        )
    freeze_transition_delta = boundary_differences(
        freeze_transition, [FREEZE_TRANSITION_FRAME]
    )
    invalid_freeze_transition = {
        boundary: delta
        for boundary, delta in freeze_transition_delta.items()
        if delta > MAX_FREEZE_TRANSITION_DELTA
    }
    if invalid_freeze_transition:
        raise RuntimeError(
            "spectral freeze transition exceeded the adjacent-sample delta limit: "
            f"{invalid_freeze_transition}"
        )
    feature_metrics = {
        "identity": {
            **aligned_identity_metrics(
                REVIEW_ROOT / "assets" / "spectral-reference-b.wav",
                identity_metric,
                render_offset_frames=0,
                comparison_frames=9_600,
            ),
            "reported_latency_frames": 1_536,
        },
        "freeze": {
            "transition_delta": freeze_transition_delta,
            "spectral_flux": spectral_flux(freeze_transition, 12_288, 8_192),
        },
        "blur": {
            "spectral_flux": spectral_flux(
                TECHNICAL_AUDIO / "06-blur.wav", 4_096, 8_192
            )
        },
        "shift": {
            "up": spectrum_snapshot(TECHNICAL_AUDIO / "07-shift-up.wav"),
            "down": spectrum_snapshot(TECHNICAL_AUDIO / "08-shift-down.wav"),
        },
        "pitch": {
            "up": {
                **spectrum_snapshot(TECHNICAL_AUDIO / "10-pitch-up-octave.wav"),
                "duration_seconds": metrics["10-pitch-up-octave"][
                    "duration_seconds"
                ],
            },
            "down": {
                **spectrum_snapshot(TECHNICAL_AUDIO / "11-pitch-down-octave.wav"),
                "duration_seconds": metrics["11-pitch-down-octave"][
                    "duration_seconds"
                ],
            },
        },
        "morph": {
            "endpoint_difference": compare_wav(
                TECHNICAL_AUDIO / "12-morph-a.wav",
                TECHNICAL_AUDIO / "14-morph-b.wav",
            ),
            "midpoint": spectrum_snapshot(TECHNICAL_AUDIO / "13-morph-mid.wav"),
            "sweep_boundary_delta": boundary_differences(
                morph_sweep, [8_192, 16_384]
            ),
        },
        "high_note": spectrum_snapshot(
            TECHNICAL_AUDIO / "18-high-note-spectrum.wav"
        ),
        "latency": {
            "reported_latency_frames": 1_536,
            "cli_compensated_peak_frame": latency_peak_frame,
            "expected_cli_compensated_peak_frame": expected_latency_peak_frame,
        },
    }
    return audio_paths, metrics, {
        "audio_outputs": {
            name: str(path.relative_to(REVIEW_ROOT)) for name, path in audio_paths.items()
        },
        "block_size_comparison": block_comparisons,
        "sample_rate_outputs": {
            sample_rate: str(path.relative_to(REVIEW_ROOT))
            for sample_rate, path in sample_rate_paths.items()
        },
        "fresh_comparison": fresh_comparison,
        "fresh_sha256": {
            "first": sha256_file(fresh_a),
            "second": sha256_file(fresh_b),
        },
        "feature_metrics": feature_metrics,
        "regression_outputs": {
            kind: str(path.relative_to(REVIEW_ROOT)) for kind, path in regression_paths.items()
        },
    }


def generate_performance_metrics() -> dict[str, object]:
    duration_frames = SAMPLE_RATE
    cases: dict[str, dict[str, object]] = {}
    build_cli(release=True)
    with tempfile.TemporaryDirectory(prefix="sonalloy-spectral-performance-") as temporary:
        temporary_root = Path(temporary)
        for fft_size in (1024, 2048, 4096):
            for voice_count in (1, 4, 8, 16):
                value = read_definition(definition_path("spectral-generator-reference.json"))
                value["performance"]["polyphony"] = voice_count
                spectral = value["layers"][0]["generator"]["spectral"]
                spectral["fft_size"] = fft_size
                spectral["asset_a"]["path"] = str(
                    ROOT / "examples" / "assets" / "spectral-reference-a.wav"
                )
                spectral["asset_b"]["path"] = str(
                    ROOT / "examples" / "assets" / "spectral-reference-b.wav"
                )
                case_name = f"fft_{fft_size}_voices_{voice_count}"
                definition = temporary_root / f"{case_name}.json"
                events = temporary_root / f"{case_name}-events.json"
                output = temporary_root / f"{case_name}.wav"
                write_definition(definition, value)
                write_events(events, polyphony_events(voice_count, duration_frames))
                run_cli(["instrument", "validate", str(definition), "--json"])
                generator = inspect_definition(definition)["layers"][0]["generator"]
                result = timed_render(
                    definition,
                    events,
                    output,
                    duration_frames,
                    BASE_BLOCK_SIZE,
                    SAMPLE_RATE,
                    release=True,
                )
                audio = performance_audio_metrics(output)
                if not audio["finite"] or audio["rms"] <= MINIMUM_AUDIO_RMS:
                    raise RuntimeError(f"performance audio checks failed: {audio}")
                cases[case_name] = {
                    "voice_count": voice_count,
                    "fft_size": fft_size,
                    "hop_size": generator["hop_size"],
                    "spectral_frames": generator["spectral_frame_count"],
                    "prepared_bytes": generator["prepared_bytes"],
                    "stereo": generator["output_mode"] == "stereo",
                    "morph_enabled": True,
                    **result,
                    "audio": audio,
                }
    return {
        "build": "release",
        "sample_rate": SAMPLE_RATE,
        "block_size": BASE_BLOCK_SIZE,
        "duration_frames": duration_frames,
        "duration_seconds": 1.0,
        "audio_output": "temporary directory",
        "cases": cases,
    }


def main() -> None:
    if REVIEW_ROOT.exists():
        shutil.rmtree(REVIEW_ROOT)
    for directory in (
        TECHNICAL_AUDIO,
        REVIEW_ROOT / "definitions",
        REVIEW_ROOT / "events",
        REVIEW_ROOT / "midi",
        REVIEW_ROOT / "assets",
    ):
        directory.mkdir(parents=True, exist_ok=True)

    definitions = copy_reference_inputs(
        REVIEW_ROOT / "definitions",
        REVIEW_ROOT / "assets",
        REVIEW_ROOT / "midi",
    )
    special_definitions = create_special_definitions(
        definitions, REVIEW_ROOT / "assets"
    )
    regression_definitions = regression_definition_paths()
    inspect = {name: inspect_definition(path) for name, path in definitions.items()}
    inspect.update(
        {
            name: inspect_definition(path)
            for name, path in special_definitions.items()
        }
    )
    write_utf8(REVIEW_ROOT / "inspect.json", json.dumps(inspect, ensure_ascii=False, indent=2) + "\n")

    audio_paths, audio_metrics, technical = render_technical_audio(
        definitions,
        regression_definitions,
        special_definitions,
        REVIEW_ROOT / "events",
        REVIEW_ROOT / "midi",
    )
    del audio_paths
    performance = generate_performance_metrics()
    metrics = {
        "references": {
            name: {
                "definition": str(path.relative_to(REVIEW_ROOT)),
                "asset_a_sha256": sha256_file(
                    REVIEW_ROOT / "assets" / "spectral-reference-a.wav"
                ),
                "asset_b_sha256": sha256_file(
                    REVIEW_ROOT / "assets" / "spectral-reference-b.wav"
                ),
                "impulse_sha256": sha256_file(
                    REVIEW_ROOT / "assets" / "spectral-reference-impulse.wav"
                ),
            }
            for name, path in definitions.items()
        },
        "technical_audio": audio_metrics,
        **technical,
        "performance": performance,
        "human_review": {
            "spectral_note": "listen for stable stereo image and natural resynthesis",
            "spectral_controls": "listen for position, freeze, blur, shift, and morph changes",
            "hybrid_balance": "listen for spectral body, additive body, sample attack, and noise air",
            "processor_chain": "listen for layer, voice, and global processor coloration",
            "midi": "listen for note timing, velocity response, and control changes",
        },
    }
    write_utf8(REVIEW_ROOT / "metrics.json", json.dumps(metrics, ensure_ascii=False, indent=2) + "\n")
    summary = """# Spectral resynthesis review package

The package contains the two reference Definitions, their source assets, inspect output, MIDI and absolute-frame event fixtures, technical renders, regression renders for the existing Generator families, and machine measurements.

Machine checks performed by the generator:

- Spectral A/B preparation, Stereo output, FFT 1024 / 2048 / 4096, Morph, and all five Spectral control parameters.
- Spectral plus Additive, Sample, and Noise with Layer, Voice, and Global Processor chains and Modulation routes.
- MIDI render, absolute-frame parameter changes, 16-voice rendering, one-voice stealing, supported block sizes, supported sample rates, Fresh Runtime reproducibility, and the reported latency impulse position.
- Existing Oscillator, Noise, Granular, Wave Sequence, Digital Hybrid (Sample, Wavetable, and Operator Modulation), Additive, and Formant reference renders.
- Identity SNR / error / correlation, Freeze boundary adjacent-sample assertion, transition and spectral-flux measurements, Shift and Pitch spectrum estimates, Morph boundary measurements, and high-note near-Nyquist energy are recorded in metrics.json.
- Release performance measurements for FFT 1024 / 2048 / 4096 with 1 / 4 / 8 / 16 Stereo voices and Morph enabled. Performance audio is kept outside the package.

Human listening checklist:

- [ ] Spectral note has a stable, clearly differentiated stereo image.
- [ ] Position and Freeze remain stable when held.
- [ ] Blur changes temporal definition without clicks.
- [ ] Shift changes pitch without changing scan duration.
- [ ] Morph moves smoothly between A and B.
- [ ] Hybrid layers remain distinguishable and the processor chain remains controlled.
- [ ] MIDI timing, velocity, and external controls are musically usable.
"""
    write_utf8(REVIEW_ROOT / "review-summary.md", summary)


if __name__ == "__main__":
    main()
