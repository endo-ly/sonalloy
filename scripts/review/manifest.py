#!/usr/bin/env python3
"""Shared manifest and render helpers for the deterministic sound review package."""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path


SAMPLE_RATE = 48_000
BASE_BLOCK_SIZE = 257
BLOCK_SIZES = (64, 257, 1024)


@dataclass(frozen=True)
class RenderJob:
    audio_name: str
    definition: str
    midi: str
    tail_seconds: float = 1.0


PRIMARY_RENDERS = (
    RenderJob(
        "01-sine-reference.wav",
        "examples/instruments/basic-poly-synth-sine.json",
        "testdata/midi/sine-reference.mid",
    ),
    RenderJob(
        "02-saw-registers.wav",
        "examples/instruments/basic-poly-synth-saw-open.json",
        "testdata/midi/saw-registers.mid",
    ),
    RenderJob(
        "03-attack-release.wav",
        "examples/instruments/basic-poly-synth.json",
        "testdata/midi/attack-release.mid",
    ),
    RenderJob(
        "04-repeated-notes.wav",
        "examples/instruments/basic-poly-synth.json",
        "testdata/midi/repeated-notes.mid",
    ),
    RenderJob(
        "05-polyphony-and-stealing.wav",
        "examples/instruments/basic-poly-synth-poly4.json",
        "testdata/midi/polyphony-stealing.mid",
    ),
    RenderJob(
        "06-filter-and-velocity.wav",
        "examples/instruments/basic-poly-synth.json",
        "testdata/midi/filter-velocity.mid",
    ),
    RenderJob(
        "07-musical-phrase.wav",
        "examples/instruments/basic-poly-synth.json",
        "testdata/midi/basic-poly-synth-phrase.mid",
    ),
)

COMPANION_RENDERS = (
    RenderJob(
        "02-saw-registers-filter-closed.wav",
        "examples/instruments/basic-poly-synth-saw-closed.json",
        "testdata/midi/saw-registers.mid",
    ),
    RenderJob(
        "03-attack-release-slow-attack.wav",
        "examples/instruments/basic-poly-synth-attack-slow.json",
        "testdata/midi/attack-release.mid",
    ),
)


def all_renders() -> tuple[RenderJob, ...]:
    return PRIMARY_RENDERS + COMPANION_RENDERS


def cli_command(root: Path) -> list[str]:
    """Use an existing binary when available, otherwise let Cargo build it."""

    candidates = (
        root / "target" / "debug" / "sonalloy.exe",
        root / "target" / "debug" / "sonalloy",
        root / "target" / "release" / "sonalloy.exe",
        root / "target" / "release" / "sonalloy",
    )
    for candidate in candidates:
        if candidate.exists():
            return [str(candidate)]
    return ["cargo", "run", "--quiet", "-p", "sonalloy-cli", "--"]


def render_job(root: Path, job: RenderJob, output: Path, block_size: int) -> None:
    command = cli_command(root) + [
        "render",
        "midi",
        str(root / job.definition),
        str(root / job.midi),
        "--sample-rate",
        str(SAMPLE_RATE),
        "--block-size",
        str(block_size),
        "--tail",
        str(job.tail_seconds),
        "--output",
        str(output),
    ]
    subprocess.run(command, cwd=root, check=True)
