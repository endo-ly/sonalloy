#!/usr/bin/env python3
"""Generate the deterministic granular texture fixtures used by the granular review package."""

from __future__ import annotations

import math
import struct
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ASSET_DIR = ROOT / "testdata" / "assets"
SAMPLE_RATE = 48_000


def write_source(path: Path, stereo: bool) -> None:
    frames = 96_000
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(2 if stereo else 1)
        output.setsampwidth(2)
        output.setframerate(SAMPLE_RATE)
        samples: list[int] = []
        for frame in range(frames):
            time = frame / SAMPLE_RATE
            movement = 0.7 + 0.3 * math.sin(2.0 * math.pi * 0.31 * time)
            left = movement * (
                0.26 * math.sin(2.0 * math.pi * 220.0 * time)
                + 0.11 * math.sin(2.0 * math.pi * 441.0 * time)
                + 0.06 * math.sin(2.0 * math.pi * 733.0 * time)
            )
            right = movement * (
                0.19 * math.sin(2.0 * math.pi * 277.0 * time)
                + 0.13 * math.sin(2.0 * math.pi * 554.0 * time)
                + 0.08 * math.sin(2.0 * math.pi * 831.0 * time)
            )
            if stereo:
                values = (left, right)
            else:
                values = ((left + right) * 0.5,)
            samples.extend(
                max(-32767, min(32767, round(value * 32767.0))) for value in values
            )
        output.writeframes(struct.pack(f"<{len(samples)}h", *samples))


if __name__ == "__main__":
    write_source(ASSET_DIR / "stereo-texture.wav", stereo=True)
    write_source(ASSET_DIR / "mono-texture.wav", stereo=False)
