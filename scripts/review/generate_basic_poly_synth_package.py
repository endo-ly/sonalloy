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


def source_commit() -> str:
    result = subprocess.run(
        [
            "git",
            "log",
            "-1",
            "--format=%H",
            "--",
            "crates/sonalloy-core/src/compiler.rs",
            "crates/sonalloy-core/src/runtime/instrument.rs",
            "crates/sonalloy-core/src/runtime/sample.rs",
            "crates/sonalloy-core/src/runtime/voice.rs",
            "crates/sonalloy-dsp-sys",
            "native/daisysp-wrapper",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def refresh_review_summary(review_root: Path) -> None:
    summary_path = review_root / "review-summary.md"
    content = summary_path.read_text(encoding="utf-8")
    marker = "- Source implementation commit："
    lines = content.splitlines(keepends=True)
    for index, line in enumerate(lines):
        if line.startswith(marker):
            line_ending = "\n" if line.endswith("\n") else ""
            lines[index] = f"{marker}{source_commit()}{line_ending}"
            summary_path.write_bytes("".join(lines).encode("utf-8"))
            return
    raise RuntimeError(f"review summary is missing the source revision marker: {summary_path}")


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
    refresh_review_summary(review_root)


if __name__ == "__main__":
    main()
