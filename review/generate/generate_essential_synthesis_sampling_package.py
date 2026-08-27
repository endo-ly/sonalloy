#!/usr/bin/env python3
"""Generate the deterministic Sample Instrument sound review package."""

from __future__ import annotations

import copy
import hashlib
import json
import math
import shutil
import struct
import subprocess
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SAMPLE_RATE = 48_000
SOURCE_SAMPLE_RATE = 44_100
BASE_BLOCK_SIZE = 257
BLOCK_SIZES = (32, 64, 257, 1024)
MAX_BLOCK_DIFFERENCE = 1.0e-5


from common import record_render_report  # noqa: E402
from measure_wav import compare_wav, measure, read_float_wav  # noqa: E402


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
    record_render_report(arguments, result.stdout)
    return result.stdout


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(
        (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    )


def write_events(path: Path, events: list[dict[str, object]]) -> None:
    write_json(path, {"events": events})


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_synthetic_wav(
    path: Path,
    duration_seconds: float,
    signal: str,
    frequency: float = 220.0,
) -> None:
    frames = int(round(duration_seconds * SOURCE_SAMPLE_RATE))
    samples: list[int] = []
    for frame in range(frames):
        time = frame / SOURCE_SAMPLE_RATE
        if signal == "hit":
            body = 0.32 * math.sin(2.0 * math.pi * frequency * time) * math.exp(-time * 5.0)
            attack = 0.38 * math.sin(2.0 * math.pi * frequency * 3.7 * time) * math.exp(
                -time * 42.0
            )
            value = body + attack
        elif signal == "soft":
            value = 0.24 * math.sin(2.0 * math.pi * frequency * time) * math.exp(-time * 3.0)
        elif signal == "hard":
            value = (
                0.58 * math.sin(2.0 * math.pi * frequency * time)
                + 0.11 * math.sin(2.0 * math.pi * frequency * 2.0 * time)
            ) * math.exp(-time * 3.0)
        elif signal == "loop":
            value = (
                0.34 * math.sin(2.0 * math.pi * frequency * time)
                + 0.08 * math.sin(2.0 * math.pi * frequency * 1.5 * time)
            )
        elif signal == "slice":
            bursts = (
                (0.14, 180.0, 0.52),
                (0.54, 440.0, 0.46),
                (0.94, 700.0, 0.40),
            )
            value = 0.0
            for center, burst_frequency, amplitude in bursts:
                offset = time - center
                value += amplitude * math.sin(2.0 * math.pi * burst_frequency * offset) * math.exp(
                    -(offset / 0.018) ** 2
                )
        else:
            raise ValueError(f"unknown synthetic signal: {signal}")
        samples.append(max(-32767, min(32767, round(value * 32767))))

    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(SOURCE_SAMPLE_RATE)
        output.writeframes(struct.pack(f"<{len(samples)}h", *samples))


def write_synthetic_stereo_wav(path: Path, duration_seconds: float) -> None:
    frames = int(round(duration_seconds * SOURCE_SAMPLE_RATE))
    samples: list[int] = []
    for frame in range(frames):
        time = frame / SOURCE_SAMPLE_RATE
        envelope = math.exp(-time * 3.0)
        left = (
            0.42 * math.sin(2.0 * math.pi * 220.0 * time)
            + 0.08 * math.sin(2.0 * math.pi * 330.0 * time)
        ) * envelope
        right = (
            0.28 * math.sin(2.0 * math.pi * 220.0 * time)
            + 0.16 * math.sin(2.0 * math.pi * 440.0 * time)
        ) * envelope
        samples.extend(
            (
                max(-32767, min(32767, round(left * 32767))),
                max(-32767, min(32767, round(right * 32767))),
            )
        )

    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(2)
        output.setsampwidth(2)
        output.setframerate(SOURCE_SAMPLE_RATE)
        output.writeframes(struct.pack(f"<{len(samples)}h", *samples))


def asset_reference(asset_dir: Path, asset_name: str) -> dict[str, str]:
    asset = asset_dir / asset_name
    return {"path": f"../assets/{asset_name}", "sha256": sha256_file(asset)}


def zone(
    asset_dir: Path,
    zone_id: str,
    asset_name: str,
    root_note: int,
    key_min: int,
    key_max: int,
    velocity_min: int = 1,
    velocity_max: int = 127,
    group: str | None = None,
    playback: dict[str, object] | None = None,
) -> dict[str, object]:
    return {
        "id": zone_id,
        "asset": asset_reference(asset_dir, asset_name),
        "root_note": root_note,
        "key_min": key_min,
        "key_max": key_max,
        "velocity_min": velocity_min,
        "velocity_max": velocity_max,
        "round_robin_group": group,
        "playback": playback
        or {
            "region": {"start_seconds": 0.0, "end_seconds": None},
            "direction": "forward",
            "loop": None,
            "time": {"mode": "resample"},
        },
    }


def sample_instrument(
    name: str,
    zones: list[dict[str, object]],
    polyphony: int = 8,
    gain_db: float = -3.0,
    release_seconds: float = 0.08,
    trigger_event: str = "note_on",
) -> dict[str, object]:
    return {
        "schema_version": 4,
        "metadata": {
            "name": name,
            "author": "Sonalloy",
            "description": "Deterministic sample mapping review fixture",
        },
        "performance": {
            "mode": "polyphonic",
            "polyphony": polyphony,
            "voice_stealing": "quietest_releasing_then_oldest",
        },
        "macros": [],
        "vectors": [],
        "layers": [
            {
                "id": "sample",
                "enabled": True,
                "trigger": {
                    "event": trigger_event,
                    "key_min": 0,
                    "key_max": 127,
                    "velocity_min": 1,
                    "velocity_max": 127,
                },
                "gain_db": gain_db,
                "pan": 0.0,
                "tuning_cents": 0.0,
                "envelope": {
                    "attack_seconds": 0.0,
                    "decay_seconds": 0.0,
                    "sustain_level": 1.0,
                    "release_seconds": release_seconds,
                },
                "generator": {"sample": {"interpolation": "cubic", "zones": zones}},
                "processors": [],
            }
        ],
        "voice_processors": [],
        "global_processors": [],
        "modulation": {"sources": [], "routes": []},
    }


def event_sequence(
    notes: list[int],
    velocities: list[int],
    onset_spacing: int,
    note_length: int,
) -> list[dict[str, object]]:
    events: list[dict[str, object]] = []
    for index, (note, velocity) in enumerate(zip(notes, velocities)):
        note_id = index + 1
        onset = index * onset_spacing
        events.append(
            {
                "absolute_frame": onset,
                "type": "note_on",
                "note_id": note_id,
                "note": note,
                "velocity": velocity,
            }
        )
        events.append(
            {
                "absolute_frame": onset + note_length,
                "type": "note_off",
                "note_id": note_id,
            }
        )
    return events


def render_events(
    definition: Path,
    events: Path,
    output: Path,
    duration_frames: int,
    block_size: int = BASE_BLOCK_SIZE,
    sample_rate: int = SAMPLE_RATE,
    tempo_bpm: float = 120.0,
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
            "--tempo",
            str(tempo_bpm),
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


def layer_report(report: dict[str, object], layer_id: str) -> dict[str, object]:
    for layer in report["layers"]:
        if layer["id"] == layer_id:
            return layer
    raise RuntimeError(f"missing inspected layer {layer_id}")


def frames(path: Path) -> tuple[int, int, list[float]]:
    return read_float_wav(path)


def segment_rms(path: Path, start_frame: int, frame_count: int) -> float:
    sample_rate, channels, samples = frames(path)
    del sample_rate
    begin = start_frame * channels
    end = min(len(samples), (start_frame + frame_count) * channels)
    values = samples[begin:end]
    return math.sqrt(sum(value * value for value in values) / len(values)) if values else 0.0


def segment_frequency(path: Path, start_frame: int, frame_count: int) -> float:
    sample_rate, channels, samples = frames(path)
    begin = start_frame * channels
    end = min(len(samples), (start_frame + frame_count) * channels)
    values = samples[begin:end:channels]
    crossings = sum(a <= 0.0 < b for a, b in zip(values, values[1:]))
    return crossings * sample_rate / len(values) if values else 0.0


def assert_audio_is_finite(audio_metrics: dict[str, dict[str, object]]) -> None:
    invalid = [name for name, values in audio_metrics.items() if not values["finite"]]
    if invalid:
        raise RuntimeError(f"sample review audio is not finite: {invalid}")


def render_job(
    definitions: dict[str, Path],
    events: dict[str, Path],
    audio_dir: Path,
    name: str,
    duration_frames: int,
    definition_name: str,
    event_name: str,
    block_size: int = BASE_BLOCK_SIZE,
    sample_rate: int = SAMPLE_RATE,
    tempo_bpm: float = 120.0,
) -> Path:
    output = audio_dir / f"{name}.wav"
    render_events(
        definitions[definition_name],
        events[event_name],
        output,
        duration_frames,
        block_size,
        sample_rate,
        tempo_bpm,
    )
    return output


def main() -> None:
    review_root = ROOT / "review" / "essential-synthesis-sampling"
    if review_root.exists():
        shutil.rmtree(review_root)
    audio_dir = review_root / "audio" / "technical"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    midi_dir = review_root / "midi"
    asset_dir = review_root / "assets"
    for directory in (audio_dir, definition_dir, event_dir, midi_dir, asset_dir):
        directory.mkdir(parents=True, exist_ok=True)

    assets = {
        "key-low.wav": (1.4, "hit", 130.0),
        "key-low-hard.wav": (1.4, "hard", 130.0),
        "key-mid.wav": (1.4, "hit", 220.0),
        "key-high.wav": (1.4, "hit", 440.0),
        "velocity-soft.wav": (1.2, "soft", 220.0),
        "velocity-hard.wav": (1.2, "hard", 220.0),
        "rr-a.wav": (0.72, "hit", 180.0),
        "rr-b.wav": (0.72, "hit", 310.0),
        "loop-sustain.wav": (2.4, "loop", 220.0),
        "slice-transients.wav": (1.35, "slice", 220.0),
    }
    for asset_name, (duration, signal, frequency) in assets.items():
        write_synthetic_wav(asset_dir / asset_name, duration, signal, frequency)
    write_synthetic_stereo_wav(asset_dir / "stereo-pad.wav", 1.4)

    definitions: dict[str, Path] = {}
    key_zones = [
        zone(asset_dir, "low", "key-low.wav", 36, 0, 47),
        zone(asset_dir, "mid", "key-mid.wav", 60, 48, 83),
        zone(asset_dir, "high", "key-high.wav", 84, 84, 127),
    ]
    velocity_zones = [
        zone(asset_dir, "soft", "velocity-soft.wav", 60, 0, 127, 1, 70),
        zone(asset_dir, "hard", "velocity-hard.wav", 60, 0, 127, 71, 127),
    ]
    round_robin_zones = [
        zone(asset_dir, "hit_a", "rr-a.wav", 60, 0, 127, group="hits"),
        zone(asset_dir, "hit_b", "rr-b.wav", 60, 0, 127, group="hits"),
    ]
    loop_zones = [
        zone(
            asset_dir,
            "sustain",
            "loop-sustain.wav",
            60,
            0,
            127,
            playback={
                "region": {"start_seconds": 0.0, "end_seconds": 2.0},
                "direction": "forward",
                "loop": {
                    "start_seconds": 0.4,
                    "end_seconds": 1.2,
                    "crossfade_seconds": 0.05,
                },
                "time": {"mode": "resample"},
            },
        )
    ]
    reverse_zones = [
        zone(
            asset_dir,
            "reverse",
            "slice-transients.wav",
            60,
            0,
            127,
            playback={
                "region": {"start_seconds": 0.04, "end_seconds": 1.10},
                "direction": "reverse",
                "loop": None,
                "time": {"mode": "resample"},
            },
        )
    ]
    reverse_loop_zones = [
        zone(
            asset_dir,
            "reverse_loop",
            "loop-sustain.wav",
            60,
            0,
            127,
            playback={
                "region": {"start_seconds": 0.0, "end_seconds": 2.0},
                "direction": "reverse",
                "loop": {
                    "start_seconds": 0.4,
                    "end_seconds": 1.2,
                    "crossfade_seconds": 0.05,
                },
                "time": {"mode": "resample"},
            },
        )
    ]
    stereo_zones = [zone(asset_dir, "stereo", "stereo-pad.wav", 60, 0, 127)]
    fixed_stretch_zones = [
        zone(
            asset_dir,
            "fixed_stretch",
            "loop-sustain.wav",
            60,
            0,
            127,
            playback={
                "region": {"start_seconds": 0.0, "end_seconds": 1.8},
                "direction": "forward",
                "loop": {
                    "start_seconds": 0.4,
                    "end_seconds": 1.2,
                    "crossfade_seconds": 0.05,
                },
                "time": {"mode": "fixed_stretch", "ratio": 2.0},
            },
        )
    ]
    tempo_sync_loop_zones = [
        zone(
            asset_dir,
            "tempo_sync_loop",
            "stereo-pad.wav",
            60,
            0,
            127,
            playback={
                "region": {"start_seconds": 0.0, "end_seconds": 1.2},
                "direction": "forward",
                "loop": {
                    "start_seconds": 0.2,
                    "end_seconds": 1.0,
                    "crossfade_seconds": 0.04,
                },
                "time": {"mode": "tempo_sync", "source_bpm": 120.0},
            },
        )
    ]
    tempo_sync_one_shot_zones = [
        zone(
            asset_dir,
            "tempo_sync_one_shot",
            "stereo-pad.wav",
            60,
            0,
            127,
            playback={
                "region": {"start_seconds": 0.0, "end_seconds": 1.2},
                "direction": "forward",
                "loop": None,
                "time": {"mode": "tempo_sync", "source_bpm": 120.0},
            },
        )
    ]
    release_zones = [zone(asset_dir, "release", "rr-a.wav", 60, 0, 127)]
    slice_zones = [
        zone(
            asset_dir,
            "slice_a",
            "slice-transients.wav",
            36,
            36,
            36,
            playback={
                "region": {"start_seconds": 0.04, "end_seconds": 0.28},
                "direction": "forward",
                "loop": None,
                "time": {"mode": "resample"},
            },
        ),
        zone(
            asset_dir,
            "slice_b",
            "slice-transients.wav",
            38,
            38,
            38,
            playback={
                "region": {"start_seconds": 0.43, "end_seconds": 0.70},
                "direction": "forward",
                "loop": None,
                "time": {"mode": "resample"},
            },
        ),
        zone(
            asset_dir,
            "slice_c",
            "slice-transients.wav",
            40,
            40,
            40,
            playback={
                "region": {"start_seconds": 0.83, "end_seconds": 1.10},
                "direction": "forward",
                "loop": None,
                "time": {"mode": "resample"},
            },
        ),
    ]
    mapped_zones = [
        zone(asset_dir, "low_soft", "key-low.wav", 36, 0, 47, 1, 70),
        zone(asset_dir, "low_hard", "key-low.wav", 36, 0, 47, 71, 127),
        zone(asset_dir, "mid", "key-mid.wav", 60, 48, 83),
        zone(asset_dir, "high_a", "rr-a.wav", 84, 84, 127, group="high_hits"),
        zone(asset_dir, "high_b", "rr-b.wav", 84, 84, 127, group="high_hits"),
    ]
    definition_values = {
        "key-zone-scale.json": sample_instrument("Key Zone Scale", key_zones),
        "velocity-layer-soft-hard.json": sample_instrument(
            "Velocity Layer Soft Hard", velocity_zones
        ),
        "round-robin-repeated-hit.json": sample_instrument(
            "Round Robin Repeated Hit", round_robin_zones
        ),
        "forward-loop-hold.json": sample_instrument("Forward Loop Hold", loop_zones),
        "forward-loop-release.json": sample_instrument("Forward Loop Release", loop_zones),
        "stereo-one-shot.json": sample_instrument("Stereo One Shot", stereo_zones),
        "fixed-stretch-loop.json": sample_instrument(
            "Fixed Stretch Loop", fixed_stretch_zones
        ),
        "tempo-sync-stereo-loop.json": sample_instrument(
            "Tempo Sync Stereo Loop", tempo_sync_loop_zones
        ),
        "tempo-sync-one-shot.json": sample_instrument(
            "Tempo Sync One Shot", tempo_sync_one_shot_zones
        ),
        "reverse-one-shot.json": sample_instrument("Reverse One Shot", reverse_zones),
        "reverse-loop.json": sample_instrument("Reverse Loop", reverse_loop_zones),
        "release-trigger.json": sample_instrument(
            "Release Trigger", release_zones, trigger_event="note_off"
        ),
        "explicit-slice-sequence.json": sample_instrument(
            "Explicit Slice Sequence", slice_zones
        ),
        "multi-sample-melody.json": sample_instrument("Multi Sample Melody", key_zones),
        "full-mapped-sample-instrument.json": sample_instrument(
            "Mapped Sample Instrument", mapped_zones
        ),
    }

    processed_source = json.loads(
        (ROOT / "testdata" / "instruments" / "processed-hybrid.json").read_text(
            encoding="utf-8"
        )
    )
    essential = copy.deepcopy(processed_source)
    essential["metadata"]["name"] = "Essential Hybrid Instrument"
    essential["metadata"]["description"] = "Mapped sample attack, oscillator body, and processors"
    sample_layer = next(layer for layer in essential["layers"] if "sample" in layer["generator"])
    sample_layer["generator"]["sample"]["zones"] = copy.deepcopy(round_robin_zones)
    definition_values["essential-hybrid-instrument.json"] = essential

    for name, value in definition_values.items():
        destination = definition_dir / name
        write_json(destination, value)
        definitions[name[:-5]] = destination

    event_values = {
        "key-zone-scale": event_sequence([36, 60, 84], [100, 100, 100], 12_000, 8_000),
        "velocity-layer-soft-hard": event_sequence([60, 60], [45, 112], 12_000, 8_000),
        "round-robin-repeated-hit": event_sequence(
            [60, 60, 60, 60], [110, 110, 110, 110], 12_000, 8_000
        ),
        "forward-loop-hold": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 76_800, "type": "note_off", "note_id": 1},
        ],
        "forward-loop-release": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 38_400, "type": "note_off", "note_id": 1},
        ],
        "stereo-one-shot": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 38_400, "type": "note_off", "note_id": 1},
        ],
        "fixed-stretch-loop": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 76_800, "type": "note_off", "note_id": 1},
        ],
        "tempo-sync-stereo-loop": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 96_000, "type": "note_off", "note_id": 1},
        ],
        "tempo-sync-one-shot": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
        ],
        "reverse-one-shot": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 50_000, "type": "note_off", "note_id": 1},
        ],
        "reverse-loop": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 76_800, "type": "note_off", "note_id": 1},
        ],
        "release-trigger": [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 60, "velocity": 100},
            {"absolute_frame": 19_200, "type": "note_off", "note_id": 1},
        ],
        "explicit-slice-sequence": event_sequence([36, 38, 40], [110, 110, 110], 12_000, 8_000),
        "multi-sample-melody": event_sequence(
            [36, 48, 60, 72, 84], [96, 96, 104, 104, 112], 9_600, 6_000
        ),
        "full-mapped-sample-instrument": event_sequence(
            [36, 36, 60, 90], [45, 112, 96, 110], 12_000, 8_000
        ),
        "essential-hybrid-instrument": event_sequence(
            [48, 60, 72, 60], [88, 104, 112, 96], 14_400, 10_000
        ),
    }
    events: dict[str, Path] = {}
    for name, value in event_values.items():
        destination = event_dir / f"{name}.json"
        write_events(destination, value)
        events[name] = destination

    midi_sources = {
        "mapped-sample-phrase.mid": ROOT / "testdata" / "midi" / "metallic-hybrid-phrase.mid",
        "essential-hybrid-phrase.mid": ROOT / "testdata" / "midi" / "metallic-hybrid-velocity.mid",
    }
    for name, source in midi_sources.items():
        shutil.copy2(source, midi_dir / name)

    render_jobs = [
        ("23-key-zone-scale", 40_000, "key-zone-scale", "key-zone-scale"),
        ("24-velocity-layer-soft-hard", 30_000, "velocity-layer-soft-hard", "velocity-layer-soft-hard"),
        ("25-round-robin-repeated-hit", 56_000, "round-robin-repeated-hit", "round-robin-repeated-hit"),
        ("26-forward-loop-hold", 80_000, "forward-loop-hold", "forward-loop-hold"),
        ("27-forward-loop-release", 70_000, "forward-loop-release", "forward-loop-release"),
        ("28-stereo-one-shot", 50_000, "stereo-one-shot", "stereo-one-shot"),
        ("40-fixed-stretch-loop", 100_000, "fixed-stretch-loop", "fixed-stretch-loop", 120.0),
        (
            "41-tempo-sync-stereo-loop-120",
            120_000,
            "tempo-sync-stereo-loop",
            "tempo-sync-stereo-loop",
            120.0,
        ),
        (
            "42-tempo-sync-stereo-loop-60",
            120_000,
            "tempo-sync-stereo-loop",
            "tempo-sync-stereo-loop",
            60.0,
        ),
        (
            "43-tempo-sync-one-shot-120",
            150_000,
            "tempo-sync-one-shot",
            "tempo-sync-one-shot",
            120.0,
        ),
        (
            "44-tempo-sync-one-shot-60",
            150_000,
            "tempo-sync-one-shot",
            "tempo-sync-one-shot",
            60.0,
        ),
        ("29-reverse-one-shot", 55_000, "reverse-one-shot", "reverse-one-shot"),
        ("30-reverse-loop", 80_000, "reverse-loop", "reverse-loop"),
        ("31-release-trigger", 50_000, "release-trigger", "release-trigger"),
        ("32-explicit-slice-sequence", 42_000, "explicit-slice-sequence", "explicit-slice-sequence"),
        ("33-multi-sample-melody", 55_000, "multi-sample-melody", "multi-sample-melody"),
        ("34-full-mapped-sample-instrument", 55_000, "full-mapped-sample-instrument", "full-mapped-sample-instrument"),
        ("35-essential-hybrid-instrument", 70_000, "essential-hybrid-instrument", "essential-hybrid-instrument"),
    ]
    audio_paths: dict[str, Path] = {}
    for job in render_jobs:
        audio_name, duration, definition_name, event_name, *tempo = job
        audio_paths[f"{audio_name}.wav"] = render_job(
            definitions,
            events,
            audio_dir,
            audio_name,
            duration,
            definition_name,
            event_name,
            tempo_bpm=tempo[0] if tempo else 120.0,
        )

    regression_definition = definitions["full-mapped-sample-instrument"]
    regression_events = events["full-mapped-sample-instrument"]
    regression_paths: dict[str, Path] = {}
    for block_size in BLOCK_SIZES:
        path = audio_dir / f"36-regression-block-{block_size}.wav"
        render_events(regression_definition, regression_events, path, 55_000, block_size)
        regression_paths[str(block_size)] = path
        audio_paths[path.name] = path

    sample_rate_paths: dict[str, Path] = {}
    for sample_rate in (44_100, SAMPLE_RATE, 96_000):
        path = audio_dir / f"37-sample-rate-{sample_rate}.wav"
        render_events(
            regression_definition,
            regression_events,
            path,
            55_000,
            BASE_BLOCK_SIZE,
            sample_rate,
        )
        sample_rate_paths[str(sample_rate)] = path
        audio_paths[path.name] = path

    repeat_a = audio_dir / "38-repeat-a.wav"
    repeat_b = audio_dir / "38-repeat-b.wav"
    render_events(regression_definition, regression_events, repeat_a, 55_000)
    render_events(regression_definition, regression_events, repeat_b, 55_000)
    audio_paths[repeat_a.name] = repeat_a
    audio_paths[repeat_b.name] = repeat_b

    stealing_value = copy.deepcopy(definition_values["full-mapped-sample-instrument.json"])
    stealing_value["performance"]["polyphony"] = 1
    stealing_definition = definition_dir / "voice-stealing-pending-zone.json"
    write_json(stealing_definition, stealing_value)
    definitions["voice-stealing-pending-zone"] = stealing_definition
    stealing_events_path = event_dir / "voice-stealing-pending-zone.json"
    write_events(
        stealing_events_path,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 90, "velocity": 110},
            {"absolute_frame": 96, "type": "note_on", "note_id": 2, "note": 90, "velocity": 110},
            {"absolute_frame": 9_600, "type": "note_off", "note_id": 2},
        ],
    )
    events["voice-stealing-pending-zone"] = stealing_events_path
    stealing_audio = audio_dir / "39-voice-stealing-pending-zone.wav"
    render_events(
        stealing_definition,
        stealing_events_path,
        stealing_audio,
        22_000,
        BASE_BLOCK_SIZE,
    )
    audio_paths[stealing_audio.name] = stealing_audio

    mapped_report = inspect_definition(regression_definition)
    mapped_layer = layer_report(mapped_report, "sample")
    generator = mapped_layer["generator"]
    expected_codes = {diagnostic["code"] for diagnostic in mapped_report["diagnostics"]}
    if expected_codes != {"ASSET_RESAMPLED"}:
        raise RuntimeError(f"unexpected mapped sample diagnostics: {expected_codes}")
    if (
        generator.get("sample_zone_count") != 5
        or generator.get("sample_enabled_zone_count") != 5
        or generator.get("sample_disabled_zone_count") != 0
        or generator.get("sample_asset_count") != 4
    ):
        raise RuntimeError(f"mapped sample inspect does not show the expected cache: {generator}")
    write_json(review_root / "inspect.json", mapped_report)

    fixed_report = inspect_definition(definitions["fixed-stretch-loop"])
    fixed_zone = layer_report(fixed_report, "sample")["generator"]["sample_zones"][0]
    tempo_loop_report = inspect_definition(definitions["tempo-sync-stereo-loop"])
    tempo_loop_generator = layer_report(tempo_loop_report, "sample")["generator"]
    tempo_one_shot_report = inspect_definition(definitions["tempo-sync-one-shot"])
    tempo_one_shot_zone = layer_report(tempo_one_shot_report, "sample")["generator"][
        "sample_zones"
    ][0]
    if (
        fixed_zone.get("time_mode") != "fixed_stretch"
        or fixed_zone.get("duration_ratio") != 2.0
        or fixed_report.get("reported_latency_frames", 0) <= 0
        or tempo_loop_generator.get("output_mode") != "stereo"
        or tempo_loop_generator["sample_zones"][0].get("time_mode") != "tempo_sync"
        or tempo_one_shot_zone.get("time_mode") != "tempo_sync"
    ):
        raise RuntimeError("time-stretch inspect output does not show the expected modes")

    audio_metrics = {
        path.name: measure(path, list(BLOCK_SIZES), include_spectrum=False)
        for path in sorted(audio_paths.values())
    }
    assert_audio_is_finite(audio_metrics)
    rendered_paths = [path for path in audio_paths.values() if path.exists()]
    if any(audio_metrics[path.name]["peak"] > 1.0 for path in rendered_paths):
        raise RuntimeError("sample review output exceeds the float WAV range")

    block_comparisons = {
        block_size: compare_wav(regression_paths["257"], regression_paths[str(block_size)])
        for block_size in BLOCK_SIZES
    }
    if any(
        not comparison.get("compatible")
        or comparison.get("max_abs_difference", 1.0) > MAX_BLOCK_DIFFERENCE
        for comparison in block_comparisons.values()
    ):
        raise RuntimeError(f"sample block-size comparison failed: {block_comparisons}")

    repeat_comparison = compare_wav(repeat_a, repeat_b)
    if not repeat_comparison.get("compatible") or repeat_comparison.get("max_abs_difference", 1.0) != 0.0:
        raise RuntimeError(f"sample repeat comparison failed: {repeat_comparison}")

    rr_audio = audio_paths["25-round-robin-repeated-hit.wav"]
    rr_order = ["hit_a", "hit_b", "hit_a", "hit_b"]
    rr_rms = [segment_rms(rr_audio, index * 12_000, 2_048) for index in range(4)]
    if abs(rr_rms[0] - rr_rms[1]) < 1.0e-4 or abs(rr_rms[0] - rr_rms[2]) > 1.0e-4:
        raise RuntimeError(f"round robin output does not match the expected order: {rr_rms}")

    key_audio = audio_paths["23-key-zone-scale.wav"]
    key_rms = [segment_rms(key_audio, index * 12_000, 2_048) for index in range(3)]
    velocity_audio = audio_paths["24-velocity-layer-soft-hard.wav"]
    velocity_rms = [segment_rms(velocity_audio, index * 12_000, 2_048) for index in range(2)]
    stereo_audio = audio_paths["28-stereo-one-shot.wav"]
    stereo_sample_rate, stereo_channels, stereo_samples = frames(stereo_audio)
    if stereo_sample_rate != SAMPLE_RATE or stereo_channels != 2:
        raise RuntimeError(
            f"stereo sample review output has unexpected format: {stereo_sample_rate} Hz, {stereo_channels} channels"
        )
    stereo_pairs = list(zip(stereo_samples[0::2], stereo_samples[1::2]))
    stereo_difference = math.sqrt(
        sum((left - right) ** 2 for left, right in stereo_pairs) / len(stereo_pairs)
    )
    reverse_audio = audio_paths["29-reverse-one-shot.wav"]
    reverse_rms = [
        segment_rms(reverse_audio, start, 1_024) for start in (7_200, 26_400, 45_600)
    ]
    reverse_loop_audio = audio_paths["30-reverse-loop.wav"]
    reverse_loop_rms = [
        segment_rms(reverse_loop_audio, start, 2_048) for start in (24_000, 48_000, 72_000)
    ]
    release_audio = audio_paths["31-release-trigger.wav"]
    release_pre_note_off_rms = segment_rms(release_audio, 4_000, 8_000)
    release_post_note_off_rms = segment_rms(release_audio, 20_000, 2_048)
    slice_audio = audio_paths["32-explicit-slice-sequence.wav"]
    slice_rms = [
        segment_rms(slice_audio, index * 12_000 + 5_000, 2_048) for index in range(3)
    ]
    loop_audio = audio_paths["26-forward-loop-hold.wav"]
    loop_rms = [segment_rms(loop_audio, start, 2_048) for start in (24_000, 48_000, 72_000)]
    fixed_stretch_audio = audio_paths["40-fixed-stretch-loop.wav"]
    tempo_loop_120_audio = audio_paths["41-tempo-sync-stereo-loop-120.wav"]
    tempo_loop_60_audio = audio_paths["42-tempo-sync-stereo-loop-60.wav"]
    tempo_one_shot_120_audio = audio_paths["43-tempo-sync-one-shot-120.wav"]
    tempo_one_shot_60_audio = audio_paths["44-tempo-sync-one-shot-60.wav"]
    tempo_loop_rms = {
        "120": segment_rms(tempo_loop_120_audio, 48_000, 2_048),
        "60": segment_rms(tempo_loop_60_audio, 48_000, 2_048),
    }
    tempo_loop_frequency = {
        "120": segment_frequency(tempo_loop_120_audio, 20_000, 10_000),
        "60": segment_frequency(tempo_loop_60_audio, 20_000, 10_000),
    }
    tempo_one_shot_rms = {
        "120_early": segment_rms(tempo_one_shot_120_audio, 24_000, 2_048),
        "120_late": segment_rms(tempo_one_shot_120_audio, 90_000, 2_048),
        "60_early": segment_rms(tempo_one_shot_60_audio, 24_000, 2_048),
        "60_late": segment_rms(tempo_one_shot_60_audio, 90_000, 2_048),
    }
    voice_stealing_audio = audio_paths["39-voice-stealing-pending-zone.wav"]
    automatic_checks = {
        "all_audio_finite": all(values["finite"] for values in audio_metrics.values()),
        "rendered_peaks_within_float_wav_range": all(
            values["peak"] <= 1.0 for values in audio_metrics.values()
        ),
        "block_sizes_reproducible": all(
            comparison.get("compatible")
            and comparison.get("max_abs_difference", 1.0) <= MAX_BLOCK_DIFFERENCE
            for comparison in block_comparisons.values()
        ),
        "repeat_render_reproducible": repeat_comparison.get("max_abs_difference") == 0.0,
        "key_zones_are_non_silent": all(value > 1.0e-3 for value in key_rms),
        "velocity_layers_differ": velocity_rms[1] > velocity_rms[0] * 1.2,
        "round_robin_order_is_definition_ordered": True,
        "round_robin_variants_are_audibly_distinct": abs(rr_rms[0] - rr_rms[1]) > 1.0e-4,
        "stereo_one_shot_has_independent_channels": stereo_difference > 1.0e-4,
        "reverse_one_shot_has_reversed_transient_order": (
            reverse_rms[0] < reverse_rms[1] < reverse_rms[2]
        ),
        "forward_loop_remains_active": all(value > 1.0e-3 for value in loop_rms),
        "reverse_loop_remains_active": all(value > 1.0e-3 for value in reverse_loop_rms),
        "release_trigger_stays_silent_until_note_off": release_pre_note_off_rms < 1.0e-8,
        "release_trigger_renders_after_note_off": release_post_note_off_rms > 1.0e-5,
        "slice_regions_are_non_silent": all(value > 1.0e-3 for value in slice_rms),
        "mapped_sample_asset_cache_is_shared": generator["sample_asset_count"] == 4,
        "voice_stealing_pending_render_is_non_silent": audio_metrics[voice_stealing_audio.name]["rms"] > 1.0e-5,
        "essential_hybrid_is_non_silent": audio_metrics["35-essential-hybrid-instrument.wav"]["rms"] > 1.0e-5,
        "fixed_stretch_loop_is_non_silent": audio_metrics[fixed_stretch_audio.name]["rms"] > 1.0e-5,
        "tempo_sync_stereo_loop_is_non_silent": all(value > 1.0e-5 for value in tempo_loop_rms.values()),
        "tempo_sync_keeps_pitch": (
            all(value > 100.0 for value in tempo_loop_frequency.values())
            and abs(tempo_loop_frequency["120"] - tempo_loop_frequency["60"]) <= 12.0
        ),
        "tempo_sync_one_shot_changes_duration": (
            tempo_one_shot_rms["120_early"] > 1.0e-5
            and tempo_one_shot_rms["60_early"] > 1.0e-5
            and tempo_one_shot_rms["120_late"] < 1.0e-6
            and tempo_one_shot_rms["60_late"] > 1.0e-5
        ),
    }
    if not all(automatic_checks.values()):
        raise RuntimeError(f"sample review automatic checks failed: {automatic_checks}")

    metrics = {
        "sample_rate": SAMPLE_RATE,
        "base_block_size": BASE_BLOCK_SIZE,
        "block_sizes": list(BLOCK_SIZES),
        "audio": audio_metrics,
        "block_size_comparisons": block_comparisons,
        "sample_rate_metrics": {
            sample_rate: audio_metrics[path.name]
            for sample_rate, path in sample_rate_paths.items()
        },
        "repeat_comparison": {
            **repeat_comparison,
            "reference_sha256": sha256_file(repeat_a),
            "repeat_sha256": sha256_file(repeat_b),
        },
        "round_robin_selection_order": rr_order,
        "round_robin_segment_rms": rr_rms,
        "stereo_sample_rate": stereo_sample_rate,
        "stereo_channels": stereo_channels,
        "stereo_channel_difference_rms": stereo_difference,
        "reverse_segment_rms": reverse_rms,
        "loop_period_frames": round((1.2 - 0.4) * SAMPLE_RATE),
        "loop_segment_rms": loop_rms,
        "fixed_stretch_latency_frames": fixed_report["reported_latency_frames"],
        "tempo_sync_stereo_loop_segment_rms": tempo_loop_rms,
        "tempo_sync_stereo_loop_segment_frequency_hz": tempo_loop_frequency,
        "tempo_sync_one_shot_segment_rms": tempo_one_shot_rms,
        "reverse_loop_segment_rms": reverse_loop_rms,
        "release_pre_note_off_rms": release_pre_note_off_rms,
        "release_post_note_off_rms": release_post_note_off_rms,
        "slice_region_durations_seconds": [0.24, 0.27, 0.27],
        "slice_segment_rms": slice_rms,
        "asset_cache": {
            "mapped_zone_count": generator["sample_zone_count"],
            "prepared_asset_count": generator["sample_asset_count"],
        },
        "automatic_checks": automatic_checks,
    }
    write_json(review_root / "metrics.json", metrics)

    summary = f"""# Essential Synthesis and Sampling Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample AssetのSource Sample Rate：44,100 Hz
- 比較Sample Rate：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Asset：Review Scriptが生成したMono / Stereo PCM16 Synthetic WAV

`audio/technical/`の生出力をMetricsと人間の試聴で共用します。試聴専用の正規化コピーは保存していません。

## 自動検査

- 全WAVがFinite：{"pass" if automatic_checks["all_audio_finite"] else "fail"}
- Float WAV範囲内：{"pass" if automatic_checks["rendered_peaks_within_float_wav_range"] else "fail"}
- Block Size再現：{"pass" if automatic_checks["block_sizes_reproducible"] else "fail"}
- 別Runtimeの同一入力再Render再現：{"pass" if automatic_checks["repeat_render_reproducible"] else "fail"}
- Key Zone切替：{"pass" if automatic_checks["key_zones_are_non_silent"] else "fail"}
- Velocity Layer差：{"pass" if automatic_checks["velocity_layers_differ"] else "fail"}
- Round Robin順序：{"pass" if automatic_checks["round_robin_order_is_definition_ordered"] else "fail"}
- Round Robin音源差：{"pass" if automatic_checks["round_robin_variants_are_audibly_distinct"] else "fail"}
- Stereo Channel分離：{"pass" if automatic_checks["stereo_one_shot_has_independent_channels"] else "fail"}
- Reverse One-shotのTransient順：{"pass" if automatic_checks["reverse_one_shot_has_reversed_transient_order"] else "fail"}
- Forward Crossfade Loop継続：{"pass" if automatic_checks["forward_loop_remains_active"] else "fail"}
- Reverse Loop継続：{"pass" if automatic_checks["reverse_loop_remains_active"] else "fail"}
- Release TriggerのNote Off前無音：{"pass" if automatic_checks["release_trigger_stays_silent_until_note_off"] else "fail"}
- Release TriggerのNote Off後発音：{"pass" if automatic_checks["release_trigger_renders_after_note_off"] else "fail"}
- Explicit Slice範囲：{"pass" if automatic_checks["slice_regions_are_non_silent"] else "fail"}
- Asset Cache共有：{"pass" if automatic_checks["mapped_sample_asset_cache_is_shared"] else "fail"}
- Voice Stealing Pending：{"pass" if automatic_checks["voice_stealing_pending_render_is_non_silent"] else "fail"}
- Essential Hybrid：{"pass" if automatic_checks["essential_hybrid_is_non_silent"] else "fail"}
- Fixed Stretch Loop：{"pass" if automatic_checks["fixed_stretch_loop_is_non_silent"] else "fail"}
- Tempo Sync Stereo Loop：{"pass" if automatic_checks["tempo_sync_stereo_loop_is_non_silent"] else "fail"}
- Tempo SyncのPitch保持：{"pass" if automatic_checks["tempo_sync_keeps_pitch"] else "fail"}
- Tempo Sync One-shotのDuration差：{"pass" if automatic_checks["tempo_sync_one_shot_changes_duration"] else "fail"}

`metrics.json`にはFinite性、Peak、RMS、DC、推定周波数、隣接Frame差分、左右Channel差分、Sample Rate別値、Block Size比較、再RenderSHA、Round Robin選択順、Reverse Transient順、Loop周期、Release Trigger前後、Slice Region長、Asset Cacheの共有数、StretchのMeasured Latency、Tempo SyncのBPM別継続時間とPitch保持を保存しています。

## 音声一覧

| WAV | 確認内容 |
|---|---|
| `23-key-zone-scale.wav` | Low / Mid / High Key Zoneの境界とPitch Mapping |
| `24-velocity-layer-soft-hard.wav` | Soft / Hard Velocity Layerの差 |
| `25-round-robin-repeated-hit.wav` | `hit_a → hit_b → hit_a → hit_b`の決定的選択 |
| `26-forward-loop-hold.wav` | Note保持中のForward Crossfade Loop周期と境界 |
| `27-forward-loop-release.wav` | Note Off後のCrossfade Loop継続とRelease |
| `28-stereo-one-shot.wav` | Stereo Channel分離と同一CursorのOne-shot |
| `29-reverse-one-shot.wav` | Reverse Regionと終端Fade |
| `30-reverse-loop.wav` | Reverse Crossfade Loop周期と境界 |
| `31-release-trigger.wav` | Note On時のArmed状態とNote Off後の発音 |
| `32-explicit-slice-sequence.wav` | 同一Assetの3つのOne-shot Region |
| `33-multi-sample-melody.wav` | 複数Key ZoneによるMelody |
| `34-full-mapped-sample-instrument.wav` | Key / Velocity / Round Robinを組み合わせたReference |
| `35-essential-hybrid-instrument.wav` | Sample、Oscillator、Processor ChainのHybrid |
| `36-regression-block-*.wav` | Block Size比較 |
| `37-sample-rate-*.wav` | Sample Rate比較 |
| `38-repeat-*.wav` | 別Runtimeの同一入力再Render再現性 |
| `39-voice-stealing-pending-zone.wav` | Pending NoteのZone選択保持 |
| `40-fixed-stretch-loop.wav` | Fixed StretchのPitch分離とLoop継続 |
| `41-tempo-sync-stereo-loop-120.wav` | Stereo Tempo Sync Loop（120 BPM、Pitch基準） |
| `42-tempo-sync-stereo-loop-60.wav` | Stereo Tempo Sync Loop（60 BPM、Pitch比較） |
| `43-tempo-sync-one-shot-120.wav` | Tempo Sync One-shot（120 BPM） |
| `44-tempo-sync-one-shot-60.wav` | Tempo Sync One-shot（60 BPM） |

## 人間の確認欄

| 確認項目 | 判定 | コメント |
|---|---|---|
| Key境界で意図したZoneへ切り替わる |  |  |
| Velocity Layerの音量・音色差が明確 |  |  |
| Round Robin順が聞き取れ、順番が崩れない |  |  |
| Pitch Mappingが自然 |  |  |
| Stereo Imageが左右に分離し、PitchとCursorが揃っている |  |  |
| ReverseのTransient順と終端が自然 |  |  |
| Forward Crossfade LoopにClickがなく周期が安定 |  |  |
| Reverse LoopにClickがなく周期が安定 |  |  |
| Release TriggerがNote Offで始まり、Note On中は無音 |  |  |
| Release中のCrossfade Loopが自然 |  |  |
| Sliceが指定Region外を再生しない |  |  |
| Missing Asset時も別Zone・別Layerが継続する |  |  |
| Voice Stealing後の音源が破綻しない |  |  |
| Essential Hybridが音色として成立する |  |  |
| Fixed StretchでPitchとDurationが独立する |  |  |
| Stereo Tempo Sync Loopの左右像、周期、BPM変更時のPitchが自然 |  |  |
| Tempo変更でOne-shotのDurationが変わりPitchが維持される |  |  |

## 再生成

```bash
python scripts/review/generate_essential_synthesis_sampling_package.py
```

同じDefinition、Event、Asset、Render条件からPackageを再生成できます。
"""
    (review_root / "review-summary.md").write_bytes(summary.encode("utf-8"))


if __name__ == "__main__":
    main()
