#!/usr/bin/env python3
"""Measure the float32 WAV emitted by the CLI review render."""

from __future__ import annotations

import argparse
import json
import math
import struct
from pathlib import Path


def read_float_wav(path: Path) -> tuple[int, int, list[float]]:
    data = path.read_bytes()
    if data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError(f"not a RIFF/WAVE file: {path}")

    fmt: bytes | None = None
    audio_data: bytes | None = None
    offset = 12
    while offset + 8 <= len(data):
        chunk_id = data[offset : offset + 4]
        chunk_size = struct.unpack_from("<I", data, offset + 4)[0]
        chunk_start = offset + 8
        chunk_end = chunk_start + chunk_size
        if chunk_end > len(data):
            raise ValueError("WAV chunk extends beyond the file")
        if chunk_id == b"fmt ":
            fmt = data[chunk_start:chunk_end]
        elif chunk_id == b"data":
            audio_data = data[chunk_start:chunk_end]
        offset = chunk_end + (chunk_size & 1)

    if fmt is None or audio_data is None or len(fmt) < 16:
        raise ValueError("WAV is missing a valid fmt or data chunk")

    format_tag, channels, sample_rate, _, block_align, bits_per_sample = struct.unpack_from(
        "<HHIIHH", fmt
    )
    if format_tag == 0xFFFE:
        if len(fmt) < 40:
            raise ValueError("WAVE_FORMAT_EXTENSIBLE fmt chunk is truncated")
        subformat_tag = struct.unpack_from("<I", fmt, 24)[0]
        if subformat_tag != 3:
            raise ValueError("only IEEE float32 WAV input is supported")
    elif format_tag != 3:
        raise ValueError("only IEEE float32 WAV input is supported")
    if bits_per_sample != 32 or block_align != channels * 4:
        raise ValueError("expected packed float32 samples")
    if channels == 0 or len(audio_data) % block_align != 0:
        raise ValueError("invalid WAV channel layout or data length")

    sample_count = len(audio_data) // 4
    samples = list(struct.unpack(f"<{sample_count}f", audio_data))
    return sample_rate, channels, samples


def positive_zero_crossings(samples: list[float]) -> int:
    return sum(
        left <= 0.0 and right > 0.0
        for left, right in zip(samples, samples[1:])
    )


def measure(path: Path, block_sizes: list[int]) -> dict[str, object]:
    sample_rate, channels, samples = read_float_wav(path)
    if len(samples) % channels != 0:
        raise ValueError("sample count is not divisible by channel count")
    frames = len(samples) // channels
    left = samples[0::channels]
    finite = all(math.isfinite(sample) for sample in samples)
    peak = max((abs(sample) for sample in samples), default=0.0)
    rms = math.sqrt(sum(sample * sample for sample in samples) / len(samples)) if samples else 0.0
    dc = sum(samples) / len(samples) if samples else 0.0
    crossings = positive_zero_crossings(left)
    estimated_frequency = crossings * sample_rate / frames if frames else 0.0
    return {
        "sample_rate": sample_rate,
        "channels": channels,
        "frames": frames,
        "duration_seconds": frames / sample_rate if sample_rate else 0.0,
        "finite": finite,
        "peak": peak,
        "rms": rms,
        "dc": dc,
        "positive_zero_crossings_left": crossings,
        "estimated_frequency_hz": estimated_frequency,
        "block_sizes_checked": block_sizes,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--block-size", action="append", type=int, default=[])
    args = parser.parse_args()
    metrics = measure(args.input, args.block_size)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(metrics, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
