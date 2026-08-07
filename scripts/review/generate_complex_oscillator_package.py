#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Generate the deterministic Complex Oscillator sound review package."""

from __future__ import annotations

import copy
import json
import tempfile
from pathlib import Path

from common import (
    BASE_BLOCK_SIZE,
    BLOCK_SIZES,
    ROOT,
    SAMPLE_RATE,
    measure_stereo,
    render_events,
    render_note as render_common_note,
    run_cli,
    sha256_file,
    timed_render,
    write_definition,
    write_events,
    write_utf8,
)
from measure_wav import boundary_differences, compare_wav, measure

BLOCK_SIZE_MAX_DIFFERENCE = 1.0e-5
COMPLEX_GATE_SECONDS = 0.35
PERFORMANCE_DURATION_FRAMES = SAMPLE_RATE
PERFORMANCE_GATE_FRAMES = PERFORMANCE_DURATION_FRAMES // 2


def layer(value: dict[str, object], layer_id: str) -> dict[str, object]:
    for candidate in value["layers"]:
        if candidate["id"] == layer_id:
            return candidate
    raise KeyError(layer_id)


def oscillator(value: dict[str, object], layer_id: str) -> dict[str, object]:
    return layer(value, layer_id)["generator"]["oscillator"]


def oscillator_variant(
    source: dict[str, object], layer_id: str
) -> dict[str, object]:
    value = copy.deepcopy(source)
    value.pop("modulation", None)
    for candidate in value["layers"]:
        candidate["enabled"] = candidate["id"] == layer_id
    return value


def set_hard_sync(value: dict[str, object], layer_id: str, ratio: float) -> None:
    oscillator(value, layer_id)["hard_sync"] = {"ratio": ratio}


def set_waveshaping(value: dict[str, object], layer_id: str, amount: float) -> None:
    oscillator(value, layer_id)["waveshaping"] = {"amount": amount}


def set_phase_domain(
    value: dict[str, object],
    layer_id: str,
    phase_distortion: float | None,
    wavefold: float | None,
    feedback: float | None,
) -> None:
    oscillator_value = oscillator(value, layer_id)
    oscillator_value["waveform"] = {"type": "sine"}
    oscillator_value["hard_sync"] = None
    oscillator_value["phase_distortion"] = (
        None
        if phase_distortion is None
        else {"amount": phase_distortion}
    )
    oscillator_value["wavefold"] = (
        None if wavefold is None else {"amount": wavefold}
    )
    oscillator_value["feedback"] = (
        None if feedback is None else {"amount": feedback}
    )


def set_unison(
    value: dict[str, object],
    layer_id: str,
    voices: int | None,
    detune_cents: float = 0.0,
    stereo_spread: float = 0.0,
    phase_spread: float = 0.0,
) -> None:
    oscillator_value = oscillator(value, layer_id)
    oscillator_value["unison"] = (
        None
        if voices is None
        else {
            "voices": voices,
            "detune_cents": detune_cents,
            "stereo_spread": stereo_spread,
            "phase_spread": phase_spread,
        }
    )


def set_performance(value: dict[str, object], polyphony: int) -> None:
    value["performance"]["polyphony"] = polyphony


def main(review_root: Path) -> None:
    source_path = ROOT / "examples" / "instruments" / "complex-oscillator-reference.json"
    source = json.loads(source_path.read_text(encoding="utf-8"))
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    technical_dir = review_root / "audio" / "technical"
    for directory in (definition_dir, event_dir, technical_dir):
        directory.mkdir(parents=True, exist_ok=True)

    definitions: dict[str, dict[str, object]] = {}
    ratio_two = oscillator_variant(source, "hard_sync_lead")
    set_hard_sync(ratio_two, "hard_sync_lead", 2.0)
    set_waveshaping(ratio_two, "hard_sync_lead", 0.0)
    set_unison(ratio_two, "hard_sync_lead", None)
    definitions["hard-sync-ratio-2"] = ratio_two

    ratio_six = copy.deepcopy(ratio_two)
    set_hard_sync(ratio_six, "hard_sync_lead", 6.0)
    definitions["hard-sync-ratio-6"] = ratio_six

    hard_sync_sweep = copy.deepcopy(ratio_two)
    definitions["hard-sync-sweep"] = hard_sync_sweep

    waveshape_half = oscillator_variant(source, "unison_body")
    set_waveshaping(waveshape_half, "unison_body", 0.5)
    set_unison(waveshape_half, "unison_body", None)
    definitions["waveshaping-amount-05"] = waveshape_half

    waveshape_sweep = copy.deepcopy(waveshape_half)
    set_waveshaping(waveshape_sweep, "unison_body", 0.0)
    definitions["waveshaping-sweep"] = waveshape_sweep

    unison_three = oscillator_variant(source, "unison_body")
    set_waveshaping(unison_three, "unison_body", 0.0)
    set_unison(unison_three, "unison_body", 3, 12.0, 0.4, 0.15)
    definitions["unison-3"] = unison_three

    unison_five = oscillator_variant(source, "unison_body")
    set_waveshaping(unison_five, "unison_body", 0.0)
    set_unison(unison_five, "unison_body", 5, 18.0, 0.85, 0.2)
    definitions["unison-5-stereo"] = unison_five

    unison_eight = oscillator_variant(source, "unison_body")
    set_waveshaping(unison_eight, "unison_body", 0.0)
    set_unison(unison_eight, "unison_body", 8, 25.0, 1.0, 0.3)
    definitions["unison-8"] = unison_eight

    hard_sync_unison = copy.deepcopy(ratio_two)
    set_hard_sync(hard_sync_unison, "hard_sync_lead", 3.0)
    set_unison(hard_sync_unison, "hard_sync_lead", 3, 8.0, 0.35, 0.0)
    definitions["hard-sync-unison"] = hard_sync_unison
    phase_distortion_025 = oscillator_variant(source, "hard_sync_lead")
    set_phase_domain(phase_distortion_025, "hard_sync_lead", 0.25, None, None)
    set_unison(phase_distortion_025, "hard_sync_lead", None)
    definitions["phase-distortion-025"] = phase_distortion_025

    phase_distortion_075 = copy.deepcopy(phase_distortion_025)
    set_phase_domain(phase_distortion_075, "hard_sync_lead", 0.75, None, None)
    definitions["phase-distortion-075"] = phase_distortion_075

    phase_distortion_sweep = copy.deepcopy(phase_distortion_025)
    definitions["phase-distortion-sweep"] = phase_distortion_sweep

    feedback_03 = oscillator_variant(source, "hard_sync_lead")
    set_phase_domain(feedback_03, "hard_sync_lead", None, None, 0.3)
    set_unison(feedback_03, "hard_sync_lead", None)
    definitions["feedback-03"] = feedback_03

    feedback_08 = copy.deepcopy(feedback_03)
    set_phase_domain(feedback_08, "hard_sync_lead", None, None, 0.8)
    definitions["feedback-08"] = feedback_08

    feedback_sweep = copy.deepcopy(feedback_03)
    definitions["feedback-sweep"] = feedback_sweep

    wavefold_025 = oscillator_variant(source, "unison_body")
    set_waveshaping(wavefold_025, "unison_body", 0.0)
    set_phase_domain(wavefold_025, "unison_body", None, 0.25, None)
    definitions["wavefold-025"] = wavefold_025

    wavefold_075 = copy.deepcopy(wavefold_025)
    set_phase_domain(wavefold_075, "unison_body", None, 0.75, None)
    definitions["wavefold-075"] = wavefold_075

    wavefold_sweep = copy.deepcopy(wavefold_025)
    definitions["wavefold-sweep"] = wavefold_sweep

    waveshaping_wavefold = copy.deepcopy(wavefold_025)
    set_waveshaping(waveshaping_wavefold, "unison_body", 0.45)
    definitions["waveshaping-wavefold"] = waveshaping_wavefold

    hard_sync_wavefold = copy.deepcopy(ratio_two)
    set_phase_domain(hard_sync_wavefold, "hard_sync_lead", None, 0.5, None)
    hard_sync_wavefold["layers"][1]["generator"]["oscillator"]["waveform"] = {
        "type": "saw"
    }
    hard_sync_wavefold["layers"][1]["generator"]["oscillator"]["hard_sync"] = {
        "ratio": 2.0
    }
    definitions["hard-sync-wavefold"] = hard_sync_wavefold

    unison_wavefold = copy.deepcopy(wavefold_025)
    set_unison(unison_wavefold, "unison_body", 5, 18.0, 0.8, 0.2)
    definitions["unison-wavefold"] = unison_wavefold
    definitions["full-essential-synth-patch"] = source

    definition_paths: dict[str, Path] = {}
    for name, value in definitions.items():
        path = definition_dir / f"{name}.json"
        write_definition(path, value)
        definition_paths[name] = path
        run_cli(["instrument", "validate", str(path), "--json"])

    inspect_json = run_cli(
        [
            "instrument",
            "inspect",
            str(definition_paths["full-essential-synth-patch"]),
            "--json",
        ]
    )
    write_utf8(review_root / "inspect.json", inspect_json)
    phase_inspect_json = run_cli(
        [
            "instrument",
            "inspect",
            str(definition_paths["phase-distortion-025"]),
            "--json",
        ]
    )
    write_utf8(review_root / "phase-inspect.json", phase_inspect_json)

    hard_sync_events = event_dir / "hard-sync-sweep.json"
    write_events(
        hard_sync_events,
        [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 1,
                "note": 60,
                "velocity": 112,
            },
            {
                "absolute_frame": 4096,
                "type": "parameter_change",
                "parameter": "layer.hard_sync_lead.generator.sync_ratio",
                "normalized": 0.8,
            },
            {
                "absolute_frame": 12_000,
                "type": "parameter_change",
                "parameter": "layer.hard_sync_lead.generator.sync_ratio",
                "normalized": 0.1,
            },
            {"absolute_frame": 14_000, "type": "note_off", "note_id": 1},
        ],
    )
    waveshape_events = event_dir / "waveshaping-sweep.json"
    write_events(
        waveshape_events,
        [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 2,
                "note": 48,
                "velocity": 112,
            },
            {
                "absolute_frame": 4096,
                "type": "parameter_change",
                "parameter": "layer.unison_body.generator.waveshape",
                "normalized": 0.75,
            },
            {
                "absolute_frame": 12_000,
                "type": "parameter_change",
                "parameter": "layer.unison_body.generator.waveshape",
                "normalized": 0.05,
            },
            {"absolute_frame": 14_000, "type": "note_off", "note_id": 2},
        ],
    )
    phase_distortion_events = event_dir / "phase-distortion-sweep.json"
    write_events(
        phase_distortion_events,
        [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 3,
                "note": 60,
                "velocity": 112,
            },
            {
                "absolute_frame": 4096,
                "type": "parameter_change",
                "parameter": "layer.hard_sync_lead.generator.phase_distortion",
                "normalized": 0.8,
            },
            {
                "absolute_frame": 12_000,
                "type": "parameter_change",
                "parameter": "layer.hard_sync_lead.generator.phase_distortion",
                "normalized": 0.1,
            },
            {"absolute_frame": 14_000, "type": "note_off", "note_id": 3},
        ],
    )
    feedback_events = event_dir / "feedback-sweep.json"
    write_events(
        feedback_events,
        [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 4,
                "note": 60,
                "velocity": 112,
            },
            {
                "absolute_frame": 4096,
                "type": "parameter_change",
                "parameter": "layer.hard_sync_lead.generator.oscillator_feedback",
                "normalized": 0.8,
            },
            {
                "absolute_frame": 12_000,
                "type": "parameter_change",
                "parameter": "layer.hard_sync_lead.generator.oscillator_feedback",
                "normalized": 0.1,
            },
            {"absolute_frame": 14_000, "type": "note_off", "note_id": 4},
        ],
    )
    wavefold_events = event_dir / "wavefold-sweep.json"
    write_events(
        wavefold_events,
        [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 5,
                "note": 48,
                "velocity": 112,
            },
            {
                "absolute_frame": 4096,
                "type": "parameter_change",
                "parameter": "layer.unison_body.generator.wavefold",
                "normalized": 0.75,
            },
            {
                "absolute_frame": 12_000,
                "type": "parameter_change",
                "parameter": "layer.unison_body.generator.wavefold",
                "normalized": 0.05,
            },
            {"absolute_frame": 14_000, "type": "note_off", "note_id": 5},
        ],
    )

    note_jobs = [
        ("13-hard-sync-ratio-2.wav", "hard-sync-ratio-2", 60),
        ("14-hard-sync-ratio-6.wav", "hard-sync-ratio-6", 84),
        ("16-waveshaping-amount-05.wav", "waveshaping-amount-05", 48),
        ("18-unison-3.wav", "unison-3", 48),
        ("19-unison-5-stereo.wav", "unison-5-stereo", 48),
        ("20-unison-8.wav", "unison-8", 48),
        ("21-hard-sync-unison.wav", "hard-sync-unison", 60),
        ("22-full-essential-synth-patch.wav", "full-essential-synth-patch", 48),
        ("24-phase-distortion-025.wav", "phase-distortion-025", 60),
        ("25-phase-distortion-075.wav", "phase-distortion-075", 60),
        ("27-feedback-03.wav", "feedback-03", 60),
        ("28-feedback-08.wav", "feedback-08", 60),
        ("30-wavefold-025.wav", "wavefold-025", 48),
        ("31-wavefold-075.wav", "wavefold-075", 48),
        ("33-waveshaping-wavefold.wav", "waveshaping-wavefold", 48),
        ("34-hard-sync-wavefold.wav", "hard-sync-wavefold", 60),
        ("35-unison-wavefold.wav", "unison-wavefold", 48),
    ]
    note_audio_paths: list[Path] = []
    for audio_name, definition_name, note in note_jobs:
        audio_path = technical_dir / audio_name
        render_common_note(
            definition_paths[definition_name],
            note,
            audio_path,
            BASE_BLOCK_SIZE,
            gate_seconds=COMPLEX_GATE_SECONDS,
        )
        note_audio_paths.append(audio_path)
    hard_sync_sweep_audio_path = technical_dir / "15-hard-sync-sweep.wav"
    render_events(
        definition_paths["hard-sync-sweep"],
        hard_sync_events,
        hard_sync_sweep_audio_path,
        BASE_BLOCK_SIZE,
    )
    waveshaping_sweep_audio_path = technical_dir / "17-waveshaping-sweep.wav"
    render_events(
        definition_paths["waveshaping-sweep"],
        waveshape_events,
        waveshaping_sweep_audio_path,
        BASE_BLOCK_SIZE,
    )
    phase_distortion_sweep_audio_path = technical_dir / "26-phase-distortion-sweep.wav"
    render_events(
        definition_paths["phase-distortion-sweep"],
        phase_distortion_events,
        phase_distortion_sweep_audio_path,
        BASE_BLOCK_SIZE,
    )
    feedback_sweep_audio_path = technical_dir / "29-feedback-sweep.wav"
    render_events(
        definition_paths["feedback-sweep"],
        feedback_events,
        feedback_sweep_audio_path,
        BASE_BLOCK_SIZE,
    )
    wavefold_sweep_audio_path = technical_dir / "32-wavefold-sweep.wav"
    render_events(
        definition_paths["wavefold-sweep"],
        wavefold_events,
        wavefold_sweep_audio_path,
        BASE_BLOCK_SIZE,
    )

    regression_definition = definition_paths["hard-sync-unison"]
    regression_paths: dict[str, Path] = {}
    for block_size in BLOCK_SIZES:
        path = technical_dir / f"regression-block-{block_size}.wav"
        render_common_note(
            regression_definition,
            60,
            path,
            block_size,
            gate_seconds=COMPLEX_GATE_SECONDS,
        )
        regression_paths[str(block_size)] = path
    fresh_a = technical_dir / "regression-fresh-a.wav"
    fresh_b = technical_dir / "regression-fresh-b.wav"
    render_common_note(
        regression_definition,
        60,
        fresh_a,
        BASE_BLOCK_SIZE,
        gate_seconds=COMPLEX_GATE_SECONDS,
    )
    render_common_note(
        regression_definition,
        60,
        fresh_b,
        BASE_BLOCK_SIZE,
        gate_seconds=COMPLEX_GATE_SECONDS,
    )

    sample_rate_paths: dict[str, Path] = {}
    for sample_rate in (44_100, SAMPLE_RATE, 96_000):
        path = technical_dir / f"sample-rate-{sample_rate}.wav"
        render_common_note(
            regression_definition,
            60,
            path,
            BASE_BLOCK_SIZE,
            sample_rate,
            COMPLEX_GATE_SECONDS,
        )
        sample_rate_paths[str(sample_rate)] = path

    generated_audio_paths = (
        note_audio_paths
        + [hard_sync_sweep_audio_path, waveshaping_sweep_audio_path]
        + [
            phase_distortion_sweep_audio_path,
            feedback_sweep_audio_path,
            wavefold_sweep_audio_path,
        ]
        + list(regression_paths.values())
        + [fresh_a, fresh_b]
        + list(sample_rate_paths.values())
    )
    technical_metrics: dict[str, dict[str, object]] = {}
    spectrum_names = {
        "13-hard-sync-ratio-2.wav",
        "14-hard-sync-ratio-6.wav",
        "16-waveshaping-amount-05.wav",
        "19-unison-5-stereo.wav",
        "21-hard-sync-unison.wav",
        "22-full-essential-synth-patch.wav",
        "24-phase-distortion-025.wav",
        "25-phase-distortion-075.wav",
        "27-feedback-03.wav",
        "28-feedback-08.wav",
        "30-wavefold-025.wav",
        "31-wavefold-075.wav",
        "33-waveshaping-wavefold.wav",
        "34-hard-sync-wavefold.wav",
        "35-unison-wavefold.wav",
    }
    for path in sorted(generated_audio_paths):
        values = measure(
            path,
            list(BLOCK_SIZES),
            include_spectrum=path.name in spectrum_names,
        )
        values.update(measure_stereo(path))
        technical_metrics[path.name] = values
    invalid_audio = [
        name for name, values in technical_metrics.items() if not values["finite"]
    ]
    if invalid_audio:
        raise RuntimeError(f"complex oscillator audio checks failed: {invalid_audio}")
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
        raise RuntimeError(f"complex oscillator block-size mismatch: {invalid_block_comparisons}")
    fresh_comparison = compare_wav(fresh_a, fresh_b)
    if (
        not fresh_comparison.get("compatible")
        or fresh_comparison.get("max_abs_difference", 1.0) != 0.0
    ):
        raise RuntimeError(
            f"complex oscillator fresh render is not reproducible: {fresh_comparison}"
        )
    parameter_sweep_boundaries = {
        "phase_distortion": boundary_differences(
            phase_distortion_sweep_audio_path, [4_096, 12_000]
        ),
        "feedback": boundary_differences(
            feedback_sweep_audio_path, [4_096, 12_000]
        ),
        "wavefold": boundary_differences(
            wavefold_sweep_audio_path, [4_096, 12_000]
        ),
    }

    performance: dict[str, dict[str, object]] = {}
    performance_modes = {
        "basic_saw": "unison_body",
        "hard_sync": "hard_sync_lead",
        "waveshaping": "unison_body",
        "phase_domain": "hard_sync_lead",
        "wavefold": "unison_body",
        "processor_chain": "unison_body",
    }
    with tempfile.TemporaryDirectory(prefix="sonalloy-complex-review-") as temporary:
        temporary_root = Path(temporary)
        for mode, layer_id in performance_modes.items():
            for polyphony in (1, 8, 16):
                for voices in (1, 4, 8):
                    value = oscillator_variant(source, layer_id)
                    oscillator_value = oscillator(value, layer_id)
                    oscillator_value["hard_sync"] = (
                        {"ratio": 3.0} if mode == "hard_sync" else None
                    )
                    oscillator_value["waveshaping"] = (
                        {"amount": 0.45} if mode == "waveshaping" else None
                    )
                    set_phase_domain(
                        value,
                        layer_id,
                        0.5 if mode == "phase_domain" else None,
                        0.5 if mode == "wavefold" else None,
                        0.3 if mode == "phase_domain" else None,
                    )
                    if mode == "hard_sync":
                        oscillator_value["waveform"] = {"type": "saw"}
                        oscillator_value["hard_sync"] = {"ratio": 3.0}
                    elif mode != "phase_domain":
                        oscillator_value["waveform"] = {"type": "saw"}
                        oscillator_value["phase_distortion"] = None
                        oscillator_value["feedback"] = None
                    set_unison(
                        value,
                        layer_id,
                        None if voices == 1 else voices,
                        18.0,
                        0.8,
                        0.0,
                    )
                    if mode != "processor_chain":
                        layer(value, layer_id)["processors"] = []
                    set_performance(value, polyphony)
                    path = temporary_root / f"{mode}-poly{polyphony}-voices{voices}.json"
                    events_path = temporary_root / (
                        f"{mode}-poly{polyphony}-voices{voices}.events.json"
                    )
                    audio_path = temporary_root / f"{mode}-poly{polyphony}-voices{voices}.wav"
                    write_definition(path, value)
                    run_cli(["instrument", "validate", str(path), "--json"])
                    note_on_events = []
                    note_off_events = []
                    for voice_index in range(polyphony):
                        note_id = voice_index + 1
                        note_on_events.append(
                            {
                                "absolute_frame": 0,
                                "type": "note_on",
                                "note_id": note_id,
                                "note": 48 + voice_index % 36,
                                "velocity": 112,
                            }
                        )
                        note_off_events.append(
                            {
                                "absolute_frame": PERFORMANCE_GATE_FRAMES,
                                "type": "note_off",
                                "note_id": note_id,
                            }
                        )
                    events = note_on_events + note_off_events
                    write_events(events_path, events)
                    performance[f"{mode}_polyphony_{polyphony}_unison_{voices}"] = (
                        timed_render(
                            path,
                            events_path,
                            audio_path,
                            PERFORMANCE_DURATION_FRAMES,
                        )
                    )

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
        "fresh_render_comparison": {
            **fresh_comparison,
            "first_sha256": sha256_file(fresh_a),
            "second_sha256": sha256_file(fresh_b),
        },
        "parameter_sweep_boundary_differences": parameter_sweep_boundaries,
        "performance": performance,
    }
    write_utf8(review_root / "metrics.json", json.dumps(metrics, ensure_ascii=False, indent=2) + "\n")
