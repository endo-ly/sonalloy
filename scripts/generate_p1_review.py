#!/usr/bin/env python3
"""Generate the complete deterministic sound review package."""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from p1_review import all_renders, render_job  # noqa: E402


def main() -> None:
    subprocess.run([sys.executable, str(ROOT / "scripts" / "generate_p1_midi.py")], check=True)

    review_root = ROOT / "review-output" / "p1"
    audio_dir = review_root / "audio"
    definition_dir = review_root / "definitions"
    midi_dir = review_root / "midi"
    for directory in (audio_dir, definition_dir, midi_dir):
        directory.mkdir(parents=True, exist_ok=True)

    jobs = all_renders()
    for job in jobs:
        source_definition = ROOT / job.definition
        shutil.copy2(source_definition, definition_dir / source_definition.name)
        source_midi = ROOT / job.midi
        shutil.copy2(source_midi, midi_dir / source_midi.name)
        render_job(ROOT, job, audio_dir / job.audio_name, block_size=257)

    subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "generate_p1_metrics.py")],
        check=True,
    )


if __name__ == "__main__":
    main()
