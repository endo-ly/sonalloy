#!/usr/bin/env python3
"""Generate the machine-readable metrics for the P1 review package."""

from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from measure_wav import compare_wav, measure  # noqa: E402
from p1_review import BASE_BLOCK_SIZE, BLOCK_SIZES, PRIMARY_RENDERS, render_job  # noqa: E402


def main() -> None:
    audio_dir = ROOT / "review-output" / "p1" / "audio"
    names = [job.audio_name for job in PRIMARY_RENDERS]
    companion_names = [
        path.name
        for path in sorted(audio_dir.iterdir())
        if path.is_file() and path.suffix == ".wav" and path.name not in names
    ]
    all_names = names + companion_names
    metrics = {
        "sample_rate": 48000,
        "block_size": BASE_BLOCK_SIZE,
        "files": {
            name: measure(
                audio_dir / name,
                list(BLOCK_SIZES),
                include_spectrum=name == "02-saw-registers.wav",
            )
            for name in all_names
        },
    }
    with tempfile.TemporaryDirectory(prefix="sonalloy-p1-blocks-") as temporary:
        temporary_dir = Path(temporary)
        block_comparisons = {}
        for job in PRIMARY_RENDERS:
            reference = audio_dir / job.audio_name
            block_comparisons[job.audio_name] = {}
            for block_size in BLOCK_SIZES:
                candidate = temporary_dir / f"{block_size}-{job.audio_name}"
                render_job(ROOT, job, candidate, block_size)
                comparison = compare_wav(reference, candidate)
                max_difference = comparison.get("max_abs_difference", 1.0)
                if not comparison.get("compatible") or max_difference > 1.0e-5:
                    raise RuntimeError(
                        f"block-size render mismatch for {job.audio_name} at {block_size}: "
                        f"{comparison}"
                    )
                block_comparisons[job.audio_name][str(block_size)] = comparison
        metrics["block_size_comparisons"] = block_comparisons
    output = ROOT / "review-output" / "p1" / "metrics.json"
    output.write_bytes((json.dumps(metrics, indent=2) + "\n").encode("utf-8"))


if __name__ == "__main__":
    main()
