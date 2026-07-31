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


def max_adjacent_frame_delta(
    samples: list[float], channels: int, frames: int
) -> tuple[float, int, list[int]]:
    threshold = 0.25
    maximum = 0.0
    large_count = 0
    candidate_frames: list[int] = []
    for frame in range(1, frames):
        previous = frame - 1
        frame_delta = max(
            abs(samples[frame * channels + channel] - samples[previous * channels + channel])
            for channel in range(channels)
        )
        maximum = max(maximum, frame_delta)
        if frame_delta > threshold:
            large_count += 1
            if len(candidate_frames) < 16:
                candidate_frames.append(frame)
    return maximum, large_count, candidate_frames


def spectrum_reference(
    left: list[float], sample_rate: int, frames: int
) -> dict[str, object]:
    fft_size = min(4096, frames)
    if fft_size < 4:
        return {"fft_size": fft_size, "peaks": []}
    start = min(frames - fft_size, int(sample_rate * 0.2))
    window = [
        left[start + index]
        * (0.5 - 0.5 * math.cos(2.0 * math.pi * index / (fft_size - 1)))
        for index in range(fft_size)
    ]
    magnitudes: list[tuple[float, int]] = []
    for bin_index in range(1, fft_size // 2 + 1):
        real = 0.0
        imaginary = 0.0
        angle_step = 2.0 * math.pi * bin_index / fft_size
        for index, sample in enumerate(window):
            angle = angle_step * index
            real += sample * math.cos(angle)
            imaginary -= sample * math.sin(angle)
        magnitudes.append((math.hypot(real, imaginary), bin_index))
    peak_magnitude = max((magnitude for magnitude, _ in magnitudes), default=0.0)
    peaks = []
    for magnitude, bin_index in sorted(magnitudes, reverse=True)[:8]:
        peaks.append(
            {
                "frequency_hz": bin_index * sample_rate / fft_size,
                "relative_amplitude": magnitude / peak_magnitude if peak_magnitude else 0.0,
            }
        )
    return {"fft_size": fft_size, "window_start_frame": start, "peaks": peaks}


def measure(
    path: Path, block_sizes: list[int], include_spectrum: bool = False
) -> dict[str, object]:
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
    max_delta, large_count, candidate_frames = max_adjacent_frame_delta(
        samples, channels, frames
    )
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
        "max_adjacent_frame_delta": max_delta,
        "large_discontinuity_threshold": 0.25,
        "large_discontinuity_count": large_count,
        "large_discontinuity_frames": candidate_frames,
        "block_sizes_checked": block_sizes,
        **(
            {"spectrum_reference": spectrum_reference(left, sample_rate, frames)}
            if include_spectrum
            else {}
        ),
    }


def compare_wav(reference: Path, candidate: Path) -> dict[str, object]:
    """Compare two rendered float32 WAVs sample by sample."""

    reference_rate, reference_channels, reference_samples = read_float_wav(reference)
    candidate_rate, candidate_channels, candidate_samples = read_float_wav(candidate)
    if (
        reference_rate != candidate_rate
        or reference_channels != candidate_channels
        or len(reference_samples) != len(candidate_samples)
    ):
        return {
            "compatible": False,
            "reference_sample_rate": reference_rate,
            "candidate_sample_rate": candidate_rate,
            "reference_channels": reference_channels,
            "candidate_channels": candidate_channels,
            "reference_samples": len(reference_samples),
            "candidate_samples": len(candidate_samples),
        }

    differences = [
        abs(reference - candidate)
        for reference, candidate in zip(reference_samples, candidate_samples)
    ]
    max_difference = max(differences, default=0.0)
    rms_difference = (
        math.sqrt(sum(difference * difference for difference in differences) / len(differences))
        if differences
        else 0.0
    )
    return {
        "compatible": True,
        "max_abs_difference": max_difference,
        "rms_difference": rms_difference,
        "different_sample_count": sum(difference != 0.0 for difference in differences),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--block-size", action="append", type=int, default=[])
    args = parser.parse_args()
    metrics = measure(args.input, args.block_size)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes((json.dumps(metrics, indent=2) + "\n").encode("utf-8"))


if __name__ == "__main__":
    main()
