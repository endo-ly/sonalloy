#!/usr/bin/env python3
"""Generate the stereo source fixtures used by the spectral reference instruments."""

from __future__ import annotations

import math
import struct
import wave
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
ASSET_DIR = ROOT / "examples" / "assets"


def write_fixture(path: Path, sample_rate: int, duration_seconds: float, variant: int) -> None:
    frame_count = round(sample_rate * duration_seconds)
    frames = bytearray()
    noise_state = 0x13579BDF + variant
    for index in range(frame_count):
        time = index / sample_rate
        envelope = min(1.0, index / (sample_rate * 0.02))
        envelope *= min(1.0, (frame_count - index) / (sample_rate * 0.08))
        carrier = math.sin(math.tau * (220.0 + 18.0 * variant) * time)
        if variant == 0:
            harmonic = math.sin(math.tau * 440.0 * time + 0.31)
        else:
            harmonic = math.sin(
                math.tau * (430.0 * time + 60.0 * time * time) + 0.31
            )
        texture = math.sin(math.tau * (1_760.0 + 113.0 * variant) * time)
        noise_state = (1_664_525 * noise_state + 1_013_904_223) & 0xFFFFFFFF
        noise = 2.0 * noise_state / 0xFFFFFFFF - 1.0
        left = envelope * (
            0.42 * carrier + 0.18 * harmonic + 0.08 * texture + 0.025 * noise
        )
        right = envelope * (
            0.36 * math.sin(math.tau * (220.0 + 18.0 * variant) * time + 0.43)
            + 0.21
            * math.sin(
                math.tau
                * (
                    440.0 * time
                    if variant == 0
                    else 430.0 * time + 60.0 * time * time
                )
                + 0.67
            )
            + 0.06 * math.sin(math.tau * (1_760.0 + 113.0 * variant) * time + 0.19)
            + 0.02 * noise
        )
        frames.extend(struct.pack("<hh", round(left * 30_000), round(right * 30_000)))

    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(2)
        output.setsampwidth(2)
        output.setframerate(sample_rate)
        output.writeframes(frames)


def write_impulse_fixture(path: Path) -> None:
    sample_rate = 48_000
    frame_count = 8_192
    impulse_frame = 2_048
    frames = bytearray(frame_count * 4)
    struct.pack_into("<hh", frames, impulse_frame * 4, 20_000, 20_000)

    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(2)
        output.setsampwidth(2)
        output.setframerate(sample_rate)
        output.writeframes(frames)


def main() -> None:
    write_fixture(ASSET_DIR / "spectral-reference-a.wav", 44_100, 1.6, 0)
    write_fixture(ASSET_DIR / "spectral-reference-b.wav", 48_000, 1.6, 1)
    write_impulse_fixture(ASSET_DIR / "spectral-reference-impulse.wav")


if __name__ == "__main__":
    main()
