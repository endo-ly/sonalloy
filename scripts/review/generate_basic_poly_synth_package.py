#!/usr/bin/env python3
"""Generate the complete deterministic sound review package."""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

from manifest import RenderJob, all_renders, render_job  # noqa: E402


def main() -> None:
    subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts" / "review" / "generate_midi_fixtures.py"),
        ],
        check=True,
    )

    review_root = ROOT / "review-output" / "basic-poly-synth"
    audio_dir = review_root / "audio"
    definition_dir = review_root / "definitions"
    midi_dir = review_root / "midi"
    for directory in (audio_dir, definition_dir, midi_dir):
        directory.mkdir(parents=True, exist_ok=True)

    jobs = all_renders()
    package_jobs = []
    for job in jobs:
        source_definition = ROOT / job.definition
        package_definition = definition_dir / source_definition.name
        shutil.copy2(source_definition, package_definition)
        source_midi = ROOT / job.midi
        package_midi = midi_dir / source_midi.name
        shutil.copy2(source_midi, package_midi)
        package_jobs.append(
            RenderJob(
                job.audio_name,
                str(package_definition.relative_to(ROOT)),
                str(package_midi.relative_to(ROOT)),
                job.tail_seconds,
            )
        )
    for job in package_jobs:
        render_job(ROOT, job, audio_dir / job.audio_name, block_size=257)

    subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts" / "review" / "generate_basic_poly_synth_metrics.py"),
        ],
        check=True,
    )


if __name__ == "__main__":
    main()
