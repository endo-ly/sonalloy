#!/usr/bin/env python3
"""Generate the deterministic asset and MIDI inputs for Metallic Hybrid."""

from __future__ import annotations

import math
import struct
import wave
from pathlib import Path

from generate_midi_fixtures import make_note_midi

ROOT = Path(__file__).resolve().parents[2]


def make_metal_hit(sample_rate: int = 44_100, duration_seconds: float = 0.45) -> bytes:
    frames = round(sample_rate * duration_seconds)
    samples: list[int] = []
    for index in range(frames):
        time = index / sample_rate
        transient = math.exp(-time * 24.0) * (
            0.52 * math.sin(2.0 * math.pi * 2_900.0 * time)
            + 0.28 * math.sin(2.0 * math.pi * 4_750.0 * time + 0.4)
            + 0.16 * math.sin(2.0 * math.pi * 7_300.0 * time + 1.1)
        )
        body = 0.18 * math.exp(-time * 8.0) * math.sin(
            2.0 * math.pi * 260.0 * time
        )
        value = max(-0.95, min(0.95, transient + body))
        samples.append(round(value * 32_767.0))
    payload = struct.pack(f"<{len(samples)}h", *samples)
    header = struct.pack(
        "<4sI4s4sIHHIIHH4sI",
        b"RIFF",
        36 + len(payload),
        b"WAVE",
        b"fmt ",
        16,
        1,
        1,
        sample_rate,
        sample_rate * 2,
        2,
        16,
        b"data",
        len(payload),
    )
    return header + payload


def write_outputs() -> None:
    asset_path = ROOT / "testdata" / "assets" / "metal-hit.wav"
    asset_path.parent.mkdir(parents=True, exist_ok=True)
    asset_path.write_bytes(make_metal_hit())

    midi_dir = ROOT / "testdata" / "midi"
    midi_dir.mkdir(parents=True, exist_ok=True)
    outputs = {
        "metallic-hybrid-phrase.mid": make_note_midi(
            [
                (0, 360, 60, 112),
                (0, 720, 48, 92),
                (480, 360, 64, 96),
                (960, 360, 67, 120),
                (960, 720, 52, 88),
                (1_440, 360, 65, 72),
                (1_920, 360, 60, 127),
                (1_920, 720, 55, 100),
                (2_400, 360, 64, 84),
                (2_880, 720, 67, 116),
                (3_840, 360, 60, 104),
                (3_840, 720, 48, 90),
            ]
        ),
        "metallic-hybrid-pitch-range.mid": make_note_midi(
            [(index * 720, 480, note, 108) for index, note in enumerate((48, 60, 72))]
        ),
        "metallic-hybrid-velocity.mid": make_note_midi(
            [
                (index * 480, 240, 60, velocity)
                for index, velocity in enumerate((32, 64, 96, 127))
            ]
        ),
    }
    for name, data in outputs.items():
        (midi_dir / name).write_bytes(data)


if __name__ == "__main__":
    write_outputs()
