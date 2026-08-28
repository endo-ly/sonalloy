#!/usr/bin/env python3
"""Generate the deterministic Wave Sequence sound review package."""

from __future__ import annotations

import copy
import hashlib
import json
import math
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SAMPLE_RATE = 48_000
DURATION_FRAMES = 96_000
BLOCK_SIZES = (64, 257, 1024)
SAMPLE_RATES = (44_100, 48_000, 96_000)
MAX_BLOCK_DIFFERENCE = 1.0e-5


from common import render_events, render_midi, run_cli, write_definition, write_events  # noqa: E402
from measure_wav import compare_wav, measure, read_float_wav  # noqa: E402


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def inspect_definition(definition: Path) -> dict[str, object]:
    report = json.loads(run_cli(["instrument", "inspect", str(definition), "--json"]))
    if report.get("status") != "ok":
        raise RuntimeError(f"inspect failed for {definition}: {report}")
    return report


def validate_definition(definition: Path) -> dict[str, object]:
    report = json.loads(run_cli(["instrument", "validate", str(definition), "--json"]))
    if report.get("status") != "ok":
        raise RuntimeError(f"validate failed for {definition}: {report}")
    return report


def asset_reference(asset_name: str, assets: dict[str, str]) -> dict[str, str]:
    return {
        "path": f"../assets/{asset_name}",
        "sha256": assets[asset_name],
    }


def trigger(event: str = "note_on") -> dict[str, object]:
    return {
        "event": event,
        "key_min": 0,
        "key_max": 127,
        "velocity_min": 1,
        "velocity_max": 127,
    }


def envelope(release: float = 0.2) -> dict[str, float]:
    return {
        "attack_seconds": 0.0,
        "decay_seconds": 0.05,
        "sustain_level": 1.0,
        "release_seconds": release,
    }


def sequence_steps(assets: dict[str, str]) -> list[dict[str, object]]:
    regions = ((0.0, 0.08), (0.08, 0.16), (0.16, 0.24), (0.24, 0.32))
    values = (
        ("attack", "seconds", 0.18, "loop", "forward", 0.0, 0.0),
        ("body", "beats", 0.5, "one_shot", "forward", -3.0, 1200.0),
        ("reverse", "seconds", 0.18, "loop", "reverse", -6.0, -700.0),
        ("tail", "seconds", 0.22, "loop", "forward", -9.0, 500.0),
    )
    asset_names = ("metal-hit.wav", "metal-hit.wav", "stereo-texture.wav", "metal-hit.wav")
    steps: list[dict[str, object]] = []
    for (
        (step_id, mode, duration, playback, direction, gain, pitch),
        (start, end),
        asset_name,
    ) in zip(values, regions, asset_names):
        steps.append(
            {
                "id": step_id,
                "asset": asset_reference(asset_name, assets),
                "region": {"start_seconds": start, "end_seconds": end},
                "duration": {"mode": mode, "value": duration},
                "playback": playback,
                "playback_direction": direction,
                "gain_db": gain,
                "pitch_cents": pitch,
            }
        )
    return steps


def sequence_definition(
    assets: dict[str, str],
    name: str,
    direction: str = "forward",
    loop_sequence: bool = True,
    crossfade: float = 0.25,
    steps: list[dict[str, object]] | None = None,
) -> dict[str, object]:
    return {
        "schema_version": 5,
        "metadata": {
            "name": name,
            "author": "Sonalloy",
            "description": "A deterministic time-ordered material sequence",
        },
        "performance": {
            "mode": "polyphonic",
            "polyphony": 8,
            "voice_stealing": "quietest_releasing_then_oldest",
        },
        "macros": [],
        "vectors": [],
        "layers": [
            {
                "id": "sequence",
                "enabled": True,
                "trigger": trigger(),
                "gain_db": -6.0,
                "pan": 0.0,
                "tuning_cents": 0.0,
                "envelope": envelope(0.18),
                "generator": {
                    "wave_sequence": {
                        "root_note": 60,
                        "direction": direction,
                        "loop": loop_sequence,
                        "crossfade": crossfade,
                        "steps": steps if steps is not None else sequence_steps(assets),
                    }
                },
                "processors": [],
            }
        ],
        "voice_processors": [],
        "global_processors": [],
    }


def sample_zone(assets: dict[str, str], direction: str, start: float, end: float) -> dict[str, object]:
    return {
        "id": "material",
        "asset": asset_reference("metal-hit.wav", assets),
        "root_note": 60,
        "key_min": 0,
        "key_max": 127,
        "velocity_min": 1,
        "velocity_max": 127,
        "round_robin_group": None,
        "playback": {
            "region": {"start_seconds": start, "end_seconds": end},
            "direction": direction,
            "loop": None,
            "time": {"mode": "resample"},
        },
    }


def hybrid_definition(assets: dict[str, str]) -> dict[str, object]:
    return {
        "schema_version": 5,
        "metadata": {
            "name": "Wave Sequence Hybrid Reference",
            "author": "Sonalloy",
            "description": "Wavetable, granular, sequence, attack, and release layers",
        },
        "performance": {
            "mode": "polyphonic",
            "polyphony": 8,
            "voice_stealing": "quietest_releasing_then_oldest",
        },
        "macros": [],
        "vectors": [],
        "layers": [
            {
                "id": "motion",
                "enabled": True,
                "trigger": trigger(),
                "gain_db": -18.0,
                "pan": 0.0,
                "tuning_cents": 0.0,
                "envelope": envelope(0.4),
                "generator": {
                    "wavetable": {
                        "asset": asset_reference("digital-motion.wav", assets),
                        "frame_length": 256,
                        "position": 0.35,
                        "phase_reset": True,
                        "phase": 0.0,
                        "unison": {
                            "voices": 3,
                            "detune_cents": 12.0,
                            "stereo_spread": 0.65,
                            "phase_spread": 0.15,
                        },
                    }
                },
                "processors": [],
            },
            {
                "id": "texture",
                "enabled": True,
                "trigger": trigger(),
                "gain_db": -12.0,
                "pan": 0.0,
                "tuning_cents": 0.0,
                "envelope": envelope(0.5),
                "generator": {
                    "granular": {
                        "asset": asset_reference("stereo-texture.wav", assets),
                        "root_note": 60,
                        "region": {"start_seconds": 0.1, "end_seconds": 1.8},
                        "position": 0.5,
                        "grain_size": 0.08,
                        "density": 24.0,
                        "pitch": 0.0,
                        "randomness": 0.35,
                        "pan_spread": 0.75,
                        "seed": 8128,
                    }
                },
                "processors": [],
            },
            {
                "id": "sequence",
                "enabled": True,
                "trigger": trigger(),
                "gain_db": -18.0,
                "pan": 0.0,
                "tuning_cents": 0.0,
                "envelope": envelope(0.3),
                "generator": {
                    "wave_sequence": {
                        "root_note": 60,
                        "direction": "ping_pong",
                        "loop": True,
                        "crossfade": 0.2,
                        "steps": sequence_steps(assets),
                    }
                },
                "processors": [],
            },
            {
                "id": "attack",
                "enabled": True,
                "trigger": trigger(),
                "gain_db": -18.0,
                "pan": 0.0,
                "tuning_cents": 0.0,
                "envelope": {**envelope(0.12), "decay_seconds": 0.05, "sustain_level": 0.0},
                "generator": {
                    "sample": {
                        "interpolation": "cubic",
                        "zones": [sample_zone(assets, "reverse", 0.0, 0.08)],
                    }
                },
                "processors": [],
            },
            {
                "id": "release",
                "enabled": True,
                "trigger": trigger("note_off"),
                "gain_db": -21.0,
                "pan": 0.0,
                "tuning_cents": 0.0,
                "envelope": {**envelope(0.3), "decay_seconds": 0.12, "sustain_level": 0.0},
                "generator": {
                    "sample": {
                        "interpolation": "cubic",
                        "zones": [sample_zone(assets, "forward", 0.08, 0.18)],
                    }
                },
                "processors": [],
            },
        ],
        "voice_processors": [
            {
                "type": "filter",
                "id": "tone",
                "cutoff_hz": 4800.0,
                "resonance": 0.16,
            },
            {
                "type": "drive",
                "id": "glue",
                "amount": 0.12,
                "mix": 0.3,
            },
        ],
        "global_processors": [
            {
                "type": "delay",
                "id": "echo",
                "time": {"value": 0.24, "unit": "seconds"},
                "feedback_mode": "stereo",
                "feedback": 0.34,
                "taps": [],
                "mix": 0.18,
            },
            {
                "type": "reverb",
                "id": "space",
                "pre_delay_seconds": 0.012,
                "decay": 0.7,
                "damping": 0.32,
                "width": 0.9,
                "mix": 0.24,
            },
        ],
        "modulation": {
            "sources": [
                {
                    "type": "lfo",
                    "id": "texture_motion",
                    "waveform": "sine",
                    "rate": {"value": 0.18, "unit": "per_second"},
                    "phase": 0.0,
                }
            ],
            "routes": [
                {
                    "source": "texture_motion",
                    "target": "layer.texture.generator.granular_position",
                    "depth": {"value": 0.35, "unit": "normalized"},
                    "curve": "linear",
                },
                {
                    "source": "mod_wheel",
                    "target": "layer.texture.generator.grain_density",
                    "depth": {"value": 2.325349, "unit": "octaves"},
                    "curve": "linear",
                },
            ],
        },
    }


def render_sequence(
    definition: Path,
    events: Path,
    output: Path,
    block_size: int,
    sample_rate: int = SAMPLE_RATE,
    duration_frames: int = DURATION_FRAMES,
) -> None:
    run_cli(
        [
            "render",
            "events",
            str(definition),
            str(events),
            "--duration-frames",
            str(duration_frames),
            "--sample-rate",
            str(sample_rate),
            "--block-size",
            str(block_size),
            "--tail",
            "0",
            "--output",
            str(output),
            "--analyze",
            "--json",
        ]
    )


def main() -> None:
    review_root = ROOT / "review" / "wave-sequence"
    if review_root.exists():
        shutil.rmtree(review_root)
    audio_dir = review_root / "audio" / "technical"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    midi_dir = review_root / "midi"
    asset_dir = review_root / "assets"
    for directory in (audio_dir, definition_dir, event_dir, midi_dir, asset_dir):
        directory.mkdir(parents=True, exist_ok=True)

    source_paths = {
        "metal-hit.wav": ROOT / "testdata" / "assets" / "metal-hit.wav",
        "digital-motion.wav": ROOT / "testdata" / "assets" / "digital-motion.wav",
        "stereo-texture.wav": ROOT / "testdata" / "assets" / "stereo-texture.wav",
    }
    for name, source in source_paths.items():
        shutil.copy2(source, asset_dir / name)
    assets = {name: sha256_file(asset_dir / name) for name in source_paths}

    definitions: dict[str, dict[str, object]] = {
        "wave-sequence-reference.json": sequence_definition(
            assets, "Wave Sequence Reference"
        ),
        "reverse-reference.json": sequence_definition(
            assets, "Reverse Wave Sequence", direction="reverse", crossfade=0.0
        ),
        "ping-pong-reference.json": sequence_definition(
            assets, "Ping Pong Wave Sequence", direction="ping_pong", crossfade=0.25
        ),
        "single-step-reference.json": sequence_definition(
            assets,
            "Single Step Wave Sequence",
            crossfade=0.0,
            steps=[sequence_steps(assets)[0]],
        ),
    }
    missing_steps = copy.deepcopy(sequence_steps(assets))
    missing_steps[1]["asset"] = {"path": "missing-step.wav"}
    definitions["missing-step-reference.json"] = sequence_definition(
        assets, "Missing Step Wave Sequence", steps=missing_steps
    )
    all_missing = copy.deepcopy(sequence_steps(assets))
    for step in all_missing:
        step["asset"] = {"path": "missing-step.wav"}
    definitions["all-missing-reference.json"] = sequence_definition(
        assets, "All Missing Wave Sequence", steps=all_missing
    )
    pitch_variant = copy.deepcopy(definitions["wave-sequence-reference.json"])
    pitch_variant["metadata"]["name"] = "Wave Sequence Pitch Variant"
    pitch_variant["layers"][0]["generator"]["wave_sequence"]["steps"][0]["pitch_cents"] = 700.0
    definitions["pitch-variant.json"] = pitch_variant
    gain_variant = copy.deepcopy(definitions["wave-sequence-reference.json"])
    gain_variant["metadata"]["name"] = "Wave Sequence Gain Variant"
    gain_variant["layers"][0]["generator"]["wave_sequence"]["steps"][0]["gain_db"] = -18.0
    definitions["gain-variant.json"] = gain_variant
    tempo_definition = copy.deepcopy(definitions["wave-sequence-reference.json"])
    tempo_definition["metadata"]["name"] = "Wave Sequence Tempo Change Reference"
    tempo_definition["layers"][0]["gain_db"] = -12.0
    definitions["tempo-change-reference.json"] = tempo_definition
    definitions["wave-sequence-hybrid-reference.json"] = hybrid_definition(assets)
    definition_paths: dict[str, Path] = {}
    for name, value in definitions.items():
        path = definition_dir / name
        write_definition(path, value)
        definition_paths[name] = path

    single_note = event_dir / "single-note.json"
    write_events(
        single_note,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 112},
            {"absolute_frame": 72_000, "type": "note_off", "note_id": 1},
        ],
    )
    midi = midi_dir / "tempo-change.mid"
    shutil.copy2(ROOT / "testdata" / "midi" / "basic-poly-synth-phrase.mid", midi)

    audio_paths: dict[str, Path] = {}
    render_jobs = {
        "01-wave-sequence.wav": "wave-sequence-reference.json",
        "02-reverse.wav": "reverse-reference.json",
        "03-ping-pong.wav": "ping-pong-reference.json",
        "04-single-step.wav": "single-step-reference.json",
        "05-missing-step.wav": "missing-step-reference.json",
        "06-pitch-variant.wav": "pitch-variant.json",
        "07-gain-variant.wav": "gain-variant.json",
    }
    for output_name, definition_name in render_jobs.items():
        output = audio_dir / output_name
        render_sequence(definition_paths[definition_name], single_note, output, 257)
        audio_paths[output.name] = output

    block_paths: dict[int, Path] = {}
    for block_size in BLOCK_SIZES:
        output = audio_dir / f"08-block-{block_size}.wav"
        render_sequence(
            definition_paths["wave-sequence-reference.json"],
            single_note,
            output,
            block_size,
        )
        block_paths[block_size] = output
        audio_paths[output.name] = output

    sample_rate_paths: dict[int, Path] = {}
    for sample_rate in SAMPLE_RATES:
        output = audio_dir / f"09-sample-rate-{sample_rate}.wav"
        render_sequence(
            definition_paths["wave-sequence-reference.json"],
            single_note,
            output,
            257,
            sample_rate,
        )
        sample_rate_paths[sample_rate] = output
        audio_paths[output.name] = output

    tempo_output = audio_dir / "10-tempo-change.wav"
    render_midi(
        definition_paths["tempo-change-reference.json"],
        midi,
        tempo_output,
        257,
        SAMPLE_RATE,
        0.0,
    )
    audio_paths[tempo_output.name] = tempo_output

    hybrid_output = audio_dir / "11-wave-sequence-hybrid.wav"
    render_events(
        definition_paths["wave-sequence-hybrid-reference.json"],
        single_note,
        hybrid_output,
        257,
        DURATION_FRAMES,
        0.0,
    )
    audio_paths[hybrid_output.name] = hybrid_output
    hybrid_midi_output = audio_dir / "12-wave-sequence-hybrid-midi.wav"
    render_midi(
        definition_paths["wave-sequence-hybrid-reference.json"],
        midi,
        hybrid_midi_output,
        257,
        SAMPLE_RATE,
        0.0,
    )
    audio_paths[hybrid_midi_output.name] = hybrid_midi_output

    repeat = audio_dir / "13-repeat.wav"
    render_sequence(
        definition_paths["wave-sequence-reference.json"],
        single_note,
        repeat,
        257,
    )
    audio_paths[repeat.name] = repeat

    reference_report = inspect_definition(definition_paths["wave-sequence-reference.json"])
    missing_report = inspect_definition(definition_paths["missing-step-reference.json"])
    all_missing_report = inspect_definition(definition_paths["all-missing-reference.json"])
    hybrid_report = inspect_definition(definition_paths["wave-sequence-hybrid-reference.json"])
    validate_reports = {
        name: validate_definition(path)
        for name, path in definition_paths.items()
        if name != "all-missing-reference.json"
    }
    (review_root / "inspect.json").write_text(
        json.dumps(reference_report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    (review_root / "hybrid-inspect.json").write_text(
        json.dumps(hybrid_report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )

    metrics = {
        path.name: measure(path, list(BLOCK_SIZES), include_spectrum=False)
        for path in sorted(audio_paths.values())
    }
    block_comparisons = {
        str(block_size): compare_wav(block_paths[257], block_paths[block_size])
        for block_size in BLOCK_SIZES
    }
    sample_rate_metrics = {
        str(sample_rate): metrics[path.name]
        for sample_rate, path in sample_rate_paths.items()
    }
    pitch_comparison = compare_wav(audio_paths["01-wave-sequence.wav"], audio_paths["06-pitch-variant.wav"])
    gain_comparison = compare_wav(audio_paths["01-wave-sequence.wav"], audio_paths["07-gain-variant.wav"])
    reset_comparison = compare_wav(audio_paths["01-wave-sequence.wav"], audio_paths["13-repeat.wav"])

    _, hybrid_channels, hybrid_samples = read_float_wav(hybrid_output)
    hybrid_stereo_difference = 0.0
    if hybrid_channels == 2:
        hybrid_stereo_difference = math.sqrt(
            sum(
                (hybrid_samples[index] - hybrid_samples[index + 1]) ** 2
                for index in range(0, len(hybrid_samples), 2)
            )
            / (len(hybrid_samples) // 2)
        )
    reference_layer = reference_report["layers"][0]
    missing_layer = missing_report["layers"][0]
    all_missing_layer = all_missing_report["layers"][0]
    hybrid_kinds = {layer["generator"]["kind"] for layer in hybrid_report["layers"]}
    hybrid_voice_processor_kinds = {
        processor["kind"] for processor in hybrid_report["voice_processors"]
    }
    hybrid_global_processor_kinds = {
        processor["kind"] for processor in hybrid_report["global_processors"]
    }
    automatic_checks = {
        "all_audio_finite": all(item["finite"] for item in metrics.values()),
        "peaks_within_float_wav_range": all(item["peak"] <= 1.0 for item in metrics.values()),
        "reference_has_four_steps": reference_layer["generator"]["step_count"] == 4,
        "reference_direction_loop_crossfade": (
            reference_layer["generator"]["direction"] == "forward"
            and reference_layer["generator"]["loop_sequence"]
            and abs(reference_layer["generator"]["crossfade"] - 0.25) < 1.0e-6
        ),
        "reference_steps_enabled": reference_layer["generator"]["enabled_step_count"] == 4,
        "missing_step_preserves_sequence_length": (
            missing_layer["generator"]["step_count"] == 4
            and missing_layer["generator"]["enabled_step_count"] == 3
        ),
        "all_missing_layer_disabled": (
            all_missing_layer["generator"]["enabled_step_count"] == 0
            and all_missing_layer["asset_status"] == "disabled"
        ),
        "block_sizes_reproducible": all(
            comparison.get("compatible")
            and comparison.get("max_abs_difference", 1.0) <= MAX_BLOCK_DIFFERENCE
            for comparison in block_comparisons.values()
        ),
        "sample_rates_rendered": all(
            item["finite"] and item["rms"] > 1.0e-5 and item["sample_rate"] == int(sample_rate)
            for sample_rate, item in sample_rate_metrics.items()
        ),
        "pitch_changes_output": pitch_comparison.get("different_sample_count", 0) > 0,
        "gain_changes_output": gain_comparison.get("different_sample_count", 0) > 0,
        "reset_reproduces_output": reset_comparison.get("max_abs_difference") == 0.0,
        "tempo_change_rendered": metrics[tempo_output.name]["finite"]
        and metrics[tempo_output.name]["rms"] > 1.0e-5,
        "hybrid_contains_material_generators": {
            "wavetable",
            "granular",
            "wave_sequence",
            "sample",
        } <= hybrid_kinds,
        "hybrid_has_voice_filter_and_drive": {"filter", "drive"}
        <= hybrid_voice_processor_kinds,
        "hybrid_has_global_delay_and_reverb": {"delay", "reverb"}
        <= hybrid_global_processor_kinds,
        "hybrid_has_release_layer": any(
            layer["trigger"]["event"] == "note_off" for layer in hybrid_report["layers"]
        ),
        "hybrid_is_stereo": hybrid_channels == 2 and hybrid_stereo_difference > 1.0e-4,
        "hybrid_is_non_silent": metrics[hybrid_output.name]["rms"] > 1.0e-5,
        "hybrid_midi_is_non_silent": metrics[hybrid_midi_output.name]["rms"] > 1.0e-5,
        "validation_reports_succeeded": all(
            report.get("status") == "ok" for report in validate_reports.values()
        ),
    }
    failed_checks = [name for name, passed in automatic_checks.items() if not passed]
    if failed_checks:
        raise RuntimeError(f"Wave Sequence automatic checks failed: {failed_checks}")

    metrics["block_comparisons"] = block_comparisons
    metrics["sample_rate_metrics"] = sample_rate_metrics
    metrics["pitch_comparison"] = pitch_comparison
    metrics["gain_comparison"] = gain_comparison
    metrics["reset_comparison"] = reset_comparison
    metrics["hybrid_stereo_difference_rms"] = hybrid_stereo_difference
    metrics["automatic_checks"] = automatic_checks
    metrics["failed_checks"] = failed_checks
    metrics["validation_reports"] = validate_reports
    (review_root / "metrics.json").write_text(
        json.dumps(metrics, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    (review_root / "review-summary.md").write_text(
        "# Wave Sequence Review\n\n"
        "自動検証ではStep Count、Forward / Reverse / Ping Pong、Sequence Loop、One-shot / Loop、"
        "Seconds / Beats、Tempo Change、Crossfade、Pitch / Gain、Missing Step、All Missing、"
        "Stereo / Mono、Block Size、Sample Rate、Reset、Hybrid構成を確認した。\n\n"
        "試聴時はStep順序、端Stepの重複、One-shot終端、Loop境界、Crossfade、Step Pitch / Gain、"
        "Missing StepのTiming、Stereo定位、Tempo Change、Reset、Hybrid音色としての成立を確認する。\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
