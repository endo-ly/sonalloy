#!/usr/bin/env python3
"""Generate the deterministic impulse responses used by the review package."""

from __future__ import annotations

import math
import struct
import sys
import wave
from pathlib import Path


SAMPLE_RATE = 48_000


def write_wav(path: Path, channels: list[list[float]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    frame_count = len(channels[0])
    with wave.open(str(path), "wb") as writer:
        writer.setnchannels(len(channels))
        writer.setsampwidth(2)
        writer.setframerate(SAMPLE_RATE)
        frames = bytearray()
        for index in range(frame_count):
            for channel in channels:
                sample = max(-1.0, min(1.0, channel[index]))
                frames.extend(struct.pack("<h", round(sample * 32_767.0)))
        writer.writeframes(frames)


def body_ir() -> list[float]:
    length = round(SAMPLE_RATE * 0.18)
    result = []
    for index in range(length):
        time = index / SAMPLE_RATE
        envelope = math.exp(-time * 28.0)
        resonances = (
            0.65 * math.sin(2.0 * math.pi * 180.0 * time)
            + 0.25 * math.sin(2.0 * math.pi * 730.0 * time)
            + 0.12 * math.sin(2.0 * math.pi * 2_400.0 * time)
        )
        result.append((1.0 if index == 0 else 0.0) + envelope * resonances * 0.18)
    return result


def room_ir() -> list[list[float]]:
    length = round(SAMPLE_RATE * 1.0)
    left = [0.0] * length
    right = [0.0] * length
    reflections = ((0, 0.75, 0.75), (1_137, 0.36, 0.22), (2_491, 0.2, 0.34), (6_017, 0.11, 0.16))
    for offset, left_gain, right_gain in reflections:
        left[offset] = left_gain
        right[offset] = right_gain
    for index in range(8_000, length):
        time = index / SAMPLE_RATE
        envelope = math.exp(-time * 5.5)
        left[index] += envelope * 0.012 * math.sin(2.0 * math.pi * 1_700.0 * time)
        right[index] += envelope * 0.010 * math.sin(2.0 * math.pi * 1_930.0 * time)
    return [left, right]


def main() -> None:
    destination = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "assets"
    write_wav(destination / "body-short.wav", [body_ir()])
    write_wav(destination / "room-medium.wav", room_ir())


if __name__ == "__main__":
    main()
