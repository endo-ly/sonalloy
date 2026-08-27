#!/usr/bin/env python3
"""Generate the deterministic Granular Generator sound review package."""

from __future__ import annotations

import copy
import hashlib
import json
import math
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SAMPLE_RATE = 48_000
DURATION_FRAMES = 96_000
BLOCK_SIZES = (32, 64, 257, 1024)
SAMPLE_RATES = (44_100, 48_000, 96_000)
MAX_BLOCK_DIFFERENCE = 1.0e-5


from common import record_render_report  # noqa: E402
from measure_wav import compare_wav, measure, read_float_wav  # noqa: E402


def cli_command() -> list[str]:
    candidates = (
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
        details = "\n".join(part for part in (result.stdout, result.stderr) if part).strip()
        raise RuntimeError(f"CLI failed with exit code {result.returncode}: {details}")
    record_render_report(arguments, result.stdout)
    return result.stdout


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def write_events(path: Path, events: list[dict[str, object]]) -> None:
    write_json(path, {"events": events})


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def granular_definition(
    asset_name: str,
    name: str,
    grain_size: float,
    density: float,
    position: float,
    randomness: float,
    pan_spread: float,
    seed: int = 8128,
    modulation: dict[str, object] | None = None,
) -> dict[str, object]:
    value: dict[str, object] = {
        "schema_version": 4,
        "metadata": {
            "name": name,
            "author": "Sonalloy",
            "description": "A deterministic granular texture source",
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
                "id": "texture",
                "enabled": True,
                "trigger": {
                    "event": "note_on",
                    "key_min": 0,
                    "key_max": 127,
                    "velocity_min": 1,
                    "velocity_max": 127,
                },
                "gain_db": -3.0,
                "pan": 0.0,
                "tuning_cents": 0.0,
                "envelope": {
                    "attack_seconds": 0.01,
                    "decay_seconds": 0.04,
                    "sustain_level": 1.0,
                    "release_seconds": 0.18,
                },
                "generator": {
                    "granular": {
                        "asset": {
                            "path": f"../assets/{asset_name}",
                            "sha256": "<SHA-256>",
                        },
                        "root_note": 60,
                        "region": {"start_seconds": 0.1, "end_seconds": 1.8},
                        "position": position,
                        "grain_size": grain_size,
                        "density": density,
                        "pitch": 0.0,
                        "randomness": randomness,
                        "pan_spread": pan_spread,
                        "seed": seed,
                    }
                },
                "processors": [],
            }
        ],
        "voice_processors": [],
        "global_processors": [],
    }
    if modulation is not None:
        value["modulation"] = modulation
    return value


def render_events(
    definition: Path,
    events: Path,
    output: Path,
    block_size: int,
    sample_rate: int = SAMPLE_RATE,
) -> None:
    run_cli(
        [
            "render",
            "events",
            str(definition),
            str(events),
            "--duration-frames",
            str(DURATION_FRAMES),
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


def inspect_definition(definition: Path) -> dict[str, object]:
    report = json.loads(run_cli(["instrument", "inspect", str(definition), "--json"]))
    if report.get("status") != "ok":
        raise RuntimeError(f"inspect failed for {definition}: {report}")
    return report


def layer_report(report: dict[str, object]) -> dict[str, object]:
    layers = report.get("layers", [])
    if not layers:
        raise RuntimeError("inspect report has no layers")
    return layers[0]


def main() -> None:
    review_root = ROOT / "review" / "granular-generator"
    if review_root.exists():
        shutil.rmtree(review_root)
    audio_dir = review_root / "audio" / "technical"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    asset_dir = review_root / "assets"
    for directory in (audio_dir, definition_dir, event_dir, asset_dir):
        directory.mkdir(parents=True, exist_ok=True)

    for name in ("stereo-texture.wav", "mono-texture.wav"):
        shutil.copy2(ROOT / "testdata" / "assets" / name, asset_dir / name)
    assets = {
        "stereo-texture.wav": sha256_file(asset_dir / "stereo-texture.wav"),
        "mono-texture.wav": sha256_file(asset_dir / "mono-texture.wav"),
    }

    definitions = {
        "granular-pad.json": granular_definition(
            "stereo-texture.wav", "Granular Pad", 0.08, 24.0, 0.45, 0.3, 0.8
        ),
        "vocal-freeze.json": granular_definition(
            "mono-texture.wav", "Vocal Freeze", 0.18, 48.0, 0.58, 0.0, 0.25
        ),
        "percussion-cloud.json": granular_definition(
            "mono-texture.wav", "Percussion Cloud", 0.015, 92.0, 0.3, 0.9, 1.0
        ),
        "position-scrub.json": granular_definition(
            "stereo-texture.wav",
            "Position Scrub",
            0.06,
            30.0,
            0.5,
            0.15,
            0.65,
            modulation={
                "sources": [
                    {
                        "type": "lfo",
                        "id": "position_lfo",
                        "waveform": "sine",
                        "rate": {"value": 0.18, "unit": "per_second"},
                        "phase": 0.0,
                    }
                ],
                "routes": [
                    {
                        "source": "position_lfo",
                        "target": "layer.texture.generator.granular_position",
                        "depth": {"value": 0.45, "unit": "normalized"},
                        "curve": "linear",
                    }
                ],
            },
        ),
    }
    parameter_variants = {
        "position": ("position-variant.json", 0.12, "Granular Position Variant"),
        "grain_size": ("grain-size-variant.json", 0.2, "Granular Grain Size Variant"),
        "density": ("density-variant.json", 76.0, "Granular Density Variant"),
        "pitch": ("pitch-variant.json", 700.0, "Granular Pitch Variant"),
        "randomness": ("randomness-variant.json", 0.0, "Granular Randomness Variant"),
        "pan_spread": ("pan-spread-variant.json", 0.0, "Granular Pan Spread Variant"),
    }
    for field, (filename, value, name) in parameter_variants.items():
        variant = copy.deepcopy(definitions["granular-pad.json"])
        variant["metadata"]["name"] = name
        variant["layers"][0]["generator"]["granular"][field] = value
        definitions[filename] = variant
    seed_variant = copy.deepcopy(definitions["granular-pad.json"])
    seed_variant["metadata"]["name"] = "Granular Seed Variant"
    seed_variant["layers"][0]["generator"]["granular"]["seed"] += 1
    definitions["seed-variant.json"] = seed_variant
    scrub_static = copy.deepcopy(definitions["position-scrub.json"])
    scrub_static.pop("modulation", None)
    scrub_static["metadata"]["name"] = "Position Scrub Static"
    definitions["position-scrub-static.json"] = scrub_static
    definitions["pool-stress.json"] = granular_definition(
        "stereo-texture.wav", "Granular Pool Stress", 0.5, 100.0, 0.5, 1.0, 1.0
    )
    for filename, value in definitions.items():
        asset_name = Path(
            value["layers"][0]["generator"]["granular"]["asset"]["path"]
        ).name
        value["layers"][0]["generator"]["granular"]["asset"]["sha256"] = assets[asset_name]
        write_json(definition_dir / filename, value)

    events = {
        "single-note.json": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 112},
            {"absolute_frame": 72_000, "type": "note_off", "note_id": 1},
        ],
        "polyphony.json": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 48, "velocity": 100},
            {"absolute_frame": 6_000, "type": "note_on", "note_id": 2, "note": 55, "velocity": 105},
            {"absolute_frame": 12_000, "type": "note_on", "note_id": 3, "note": 64, "velocity": 110},
            {"absolute_frame": 48_000, "type": "note_off", "note_id": 1},
            {"absolute_frame": 54_000, "type": "note_off", "note_id": 2},
            {"absolute_frame": 60_000, "type": "note_off", "note_id": 3},
        ],
    }
    event_paths: dict[str, Path] = {}
    for filename, value in events.items():
        path = event_dir / filename
        write_events(path, value)
        event_paths[filename] = path

    definition_paths = {name: definition_dir / name for name in definitions}
    audio_paths: dict[str, Path] = {}
    for definition_name, output_name in (
        ("granular-pad.json", "01-granular-pad.wav"),
        ("vocal-freeze.json", "02-vocal-freeze.wav"),
        ("percussion-cloud.json", "03-percussion-cloud.wav"),
        ("position-scrub.json", "04-position-scrub.wav"),
    ):
        output = audio_dir / output_name
        render_events(
            definition_paths[definition_name], event_paths["single-note.json"], output, 257
        )
        audio_paths[output.name] = output
    polyphony = audio_dir / "05-polyphony.wav"
    render_events(definition_paths["granular-pad.json"], event_paths["polyphony.json"], polyphony, 257)
    audio_paths[polyphony.name] = polyphony
    pool_stress = audio_dir / "06-pool-stress.wav"
    render_events(definition_paths["pool-stress.json"], event_paths["single-note.json"], pool_stress, 257)
    audio_paths[pool_stress.name] = pool_stress

    block_paths: dict[int, Path] = {}
    for block_size in BLOCK_SIZES:
        output = audio_dir / f"07-block-{block_size}.wav"
        render_events(
            definition_paths["granular-pad.json"],
            event_paths["single-note.json"],
            output,
            block_size,
        )
        block_paths[block_size] = output
        audio_paths[output.name] = output

    sample_rate_paths: dict[int, Path] = {}
    for sample_rate in SAMPLE_RATES:
        output = audio_dir / f"08-sample-rate-{sample_rate}.wav"
        render_events(
            definition_paths["granular-pad.json"],
            event_paths["single-note.json"],
            output,
            257,
            sample_rate,
        )
        sample_rate_paths[sample_rate] = output
        audio_paths[output.name] = output

    parameter_audio_paths: dict[str, Path] = {}
    parameter_outputs = {
        "position": "09-position-variant.wav",
        "grain_size": "10-grain-size-variant.wav",
        "density": "11-density-variant.wav",
        "pitch": "12-pitch-variant.wav",
        "randomness": "13-randomness-variant.wav",
        "pan_spread": "14-pan-spread-variant.wav",
    }
    for field, output_name in parameter_outputs.items():
        output = audio_dir / output_name
        filename = parameter_variants[field][0]
        render_events(
            definition_paths[filename], event_paths["single-note.json"], output, 257
        )
        parameter_audio_paths[field] = output
        audio_paths[output.name] = output
    seed_output = audio_dir / "15-seed-variant.wav"
    render_events(
        definition_paths["seed-variant.json"], event_paths["single-note.json"], seed_output, 257
    )
    audio_paths[seed_output.name] = seed_output
    scrub_static_output = audio_dir / "16-position-scrub-static.wav"
    render_events(
        definition_paths["position-scrub-static.json"],
        event_paths["single-note.json"],
        scrub_static_output,
        257,
    )
    audio_paths[scrub_static_output.name] = scrub_static_output

    repeat_a = audio_dir / "17-repeat-a.wav"
    repeat_b = audio_dir / "17-repeat-b.wav"
    render_events(definition_paths["granular-pad.json"], event_paths["single-note.json"], repeat_a, 257)
    render_events(definition_paths["granular-pad.json"], event_paths["single-note.json"], repeat_b, 257)
    audio_paths[repeat_a.name] = repeat_a
    audio_paths[repeat_b.name] = repeat_b

    report = inspect_definition(definition_paths["granular-pad.json"])
    layer = layer_report(report)
    generator = layer["generator"]
    parameter_ids = {
        parameter["id"] for parameter in report.get("parameters", [])
    }
    expected_parameters = {
        "layer.texture.generator.granular_position",
        "layer.texture.generator.grain_size",
        "layer.texture.generator.grain_density",
        "layer.texture.generator.grain_pitch",
        "layer.texture.generator.grain_randomness",
        "layer.texture.generator.grain_pan_spread",
    }
    write_json(review_root / "inspect.json", report)

    metrics = {
        path.name: measure(path, list(BLOCK_SIZES), include_spectrum=False)
        for path in sorted(audio_paths.values())
    }
    block_comparisons = {
        str(block_size): compare_wav(block_paths[257], block_paths[block_size])
        for block_size in BLOCK_SIZES
    }
    repeat_comparison = compare_wav(repeat_a, repeat_b)
    parameter_comparisons = {
        field: compare_wav(audio_paths["01-granular-pad.wav"], output)
        for field, output in parameter_audio_paths.items()
    }
    seed_comparison = compare_wav(audio_paths["01-granular-pad.wav"], seed_output)
    scrub_comparison = compare_wav(audio_paths["04-position-scrub.wav"], scrub_static_output)

    _, channels, pad_samples = read_float_wav(audio_paths["01-granular-pad.wav"])
    stereo_difference = 0.0
    if channels == 2:
        stereo_difference = math.sqrt(
            sum(
                (pad_samples[index] - pad_samples[index + 1]) ** 2
                for index in range(0, len(pad_samples), 2)
            )
            / (len(pad_samples) // 2)
        )

    automatic_checks = {
        "all_audio_finite": all(item["finite"] for item in metrics.values()),
        "rendered_peaks_within_float_wav_range": all(
            item["peak"] <= 1.0 for item in metrics.values()
        ),
        "granular_inspect_kind": generator.get("kind") == "granular",
        "granular_prepared_stereo": generator.get("prepared") and generator.get("output_mode") == "stereo",
        "granular_region_compiled": generator.get("region_start_frame") == 4_800
        and generator.get("region_end_frame") == 86_400,
        "parameter_catalog_complete": expected_parameters <= parameter_ids,
        "grain_pool_limit_is_64": generator.get("grain_pool_limit") == 64,
        "block_sizes_reproducible": all(
            comparison.get("compatible")
            and comparison.get("max_abs_difference", 1.0) <= MAX_BLOCK_DIFFERENCE
            for comparison in block_comparisons.values()
        ),
        "seed_reproducible": repeat_comparison.get("compatible")
        and repeat_comparison.get("max_abs_difference") == 0.0,
        "seed_changes_randomized_output": seed_comparison.get("compatible")
        and seed_comparison.get("different_sample_count", 0) > 0,
        "parameter_variants_change_output": all(
            comparison.get("compatible")
            and comparison.get("different_sample_count", 0) > 0
            for comparison in parameter_comparisons.values()
        ),
        "scrub_changes_position": scrub_comparison.get("compatible")
        and scrub_comparison.get("different_sample_count", 0) > 0,
        "stereo_source_is_separated": stereo_difference > 1.0e-4,
        "mono_source_outputs_stereo": metrics["02-vocal-freeze.wav"]["channels"] == 2,
        "sample_rates_rendered": all(
            metrics[path.name]["sample_rate"] == sample_rate
            and metrics[path.name]["finite"]
            and metrics[path.name]["rms"] > 1.0e-4
            for sample_rate, path in sample_rate_paths.items()
        ),
        "pad_non_silent": metrics["01-granular-pad.wav"]["rms"] > 1.0e-4,
        "freeze_non_silent": metrics["02-vocal-freeze.wav"]["rms"] > 1.0e-4,
        "percussion_cloud_non_silent": metrics["03-percussion-cloud.wav"]["rms"] > 1.0e-4,
        "scrub_non_silent": metrics["04-position-scrub.wav"]["rms"] > 1.0e-4,
        "polyphony_non_silent": metrics["05-polyphony.wav"]["rms"] > 1.0e-4,
        "pool_stress_non_silent": metrics["06-pool-stress.wav"]["rms"] > 1.0e-4,
    }
    failed_checks = [name for name, passed in automatic_checks.items() if not passed]
    if failed_checks:
        raise RuntimeError(f"Granular automatic checks failed: {failed_checks}")

    metrics["block_comparisons"] = block_comparisons
    metrics["repeat_comparison"] = repeat_comparison
    metrics["parameter_comparisons"] = parameter_comparisons
    metrics["seed_comparison"] = seed_comparison
    metrics["scrub_comparison"] = scrub_comparison
    metrics["sample_rate_metrics"] = {
        str(sample_rate): metrics[path.name]
        for sample_rate, path in sample_rate_paths.items()
    }
    metrics["stereo_difference_rms"] = stereo_difference
    metrics["automatic_checks"] = automatic_checks
    metrics["failed_checks"] = failed_checks
    write_json(review_root / "metrics.json", metrics)
    (review_root / "review-summary.md").write_text(
        "# Granular Generator Review\n\n"
        "自動検証ではFinite性、固定Pool、Stereo、Deterministic Random、Block Size再現、"
        "Sample Rate、各Granular Parameter、Position Scrub、Freeze、Percussion Cloud、Polyphonyを確認した。\n\n"
        "試聴時はGrain境界のClick、Density、Grain Size、Pitch、Randomness、Pan Spread、"
        "Scrub、Freeze、Vocal Texture、Percussion Cloudを確認する。\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
