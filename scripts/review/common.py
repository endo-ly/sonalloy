"""Shared helpers for deterministic sound review packages."""

from __future__ import annotations

import hashlib
import json
import math
import subprocess
from pathlib import Path

from measure_wav import read_float_wav

ROOT = Path(__file__).resolve().parents[2]
SAMPLE_RATE = 48_000
BASE_BLOCK_SIZE = 257
BLOCK_SIZES = (32, 64, 257, 1024)
EVENT_DURATION_FRAMES = 16_384


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
    return result.stdout


def write_utf8(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content.encode("utf-8"))


def write_definition(path: Path, value: dict[str, object]) -> None:
    write_utf8(path, json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def write_events(path: Path, events: list[dict[str, object]]) -> None:
    write_utf8(path, json.dumps({"events": events}, ensure_ascii=False, indent=2) + "\n")


def render_note(
    definition: Path,
    note: int,
    output: Path,
    block_size: int,
    sample_rate: int = SAMPLE_RATE,
    gate_seconds: float = 0.15,
    tail_seconds: float = 0.1,
) -> None:
    run_cli(
        [
            "render",
            "note",
            str(definition),
            "--note",
            str(note),
            "--velocity",
            "112",
            "--gate",
            str(gate_seconds),
            "--tail",
            str(tail_seconds),
            "--sample-rate",
            str(sample_rate),
            "--block-size",
            str(block_size),
            "--output",
            str(output),
            "--json",
        ]
    )


def render_events(
    definition: Path,
    events: Path,
    output: Path,
    block_size: int,
    duration_frames: int = EVENT_DURATION_FRAMES,
    tail_seconds: float = 0.0,
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
            str(SAMPLE_RATE),
            "--block-size",
            str(block_size),
            "--tail",
            str(tail_seconds),
            "--output",
            str(output),
            "--json",
        ]
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def measure_stereo(path: Path) -> dict[str, object]:
    sample_rate, channels, samples = read_float_wav(path)
    if channels != 2:
        raise ValueError(f"expected stereo WAV: {path}")
    left = samples[0::2]
    right = samples[1::2]
    left_mean = sum(left) / len(left) if left else 0.0
    right_mean = sum(right) / len(right) if right else 0.0
    covariance = sum(
        (left_sample - left_mean) * (right_sample - right_mean)
        for left_sample, right_sample in zip(left, right)
    )
    left_variance = sum((sample - left_mean) ** 2 for sample in left)
    right_variance = sum((sample - right_mean) ** 2 for sample in right)
    denominator = math.sqrt(left_variance * right_variance)
    correlation = covariance / denominator if denominator > 0.0 else 1.0
    difference_rms = (
        math.sqrt(
            sum(
                (left_sample - right_sample) ** 2
                for left_sample, right_sample in zip(left, right)
            )
            / len(left)
        )
        if left
        else 0.0
    )
    return {
        "sample_rate": sample_rate,
        "stereo_rms_difference": difference_rms,
        "stereo_correlation": correlation,
    }
