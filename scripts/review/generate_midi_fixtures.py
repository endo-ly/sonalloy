#!/usr/bin/env python3
"""Generate deterministic MIDI fixtures used by the sound review packages."""

from __future__ import annotations

import struct
from pathlib import Path


def vlq(value: int) -> bytes:
    encoded = [value & 0x7F]
    value >>= 7
    while value:
        encoded.append((value & 0x7F) | 0x80)
        value >>= 7
    return bytes(reversed(encoded))


def track(events: list[tuple[int, bytes]]) -> bytes:
    data = bytearray()
    for delta, payload in events:
        data.extend(vlq(delta))
        data.extend(payload)
    data.extend(b"\x00\xff\x2f\x00")
    return b"MTrk" + struct.pack(">I", len(data)) + data


def make_midi() -> bytes:
    tempo = track([
        (0, b"\xff\x51\x03\x07\xa1\x20"),
        (1920, b"\xff\x51\x03\x09\x27\xc0"),
    ])
    notes = [
        (0, 240, 36, 92),
        (0, 480, 48, 100),
        (0, 480, 55, 86),
        (480, 240, 40, 78),
        (480, 480, 52, 94),
        (960, 480, 43, 88),
        (960, 480, 55, 92),
        (1440, 240, 41, 76),
        (1440, 480, 53, 96),
        (1920, 240, 36, 92),
        (1920, 480, 48, 100),
        (1920, 480, 55, 86),
        (2400, 240, 40, 78),
        (2400, 480, 52, 94),
        (2880, 480, 43, 88),
        (2880, 480, 55, 92),
        (3360, 240, 41, 76),
        (3360, 480, 53, 96),
        (3840, 240, 38, 92),
        (3840, 480, 50, 100),
        (3840, 480, 57, 86),
        (4320, 240, 41, 78),
        (4320, 480, 53, 94),
        (4800, 480, 45, 88),
        (4800, 480, 57, 92),
        (5280, 240, 43, 76),
        (5280, 480, 55, 96),
        (5760, 240, 36, 92),
        (5760, 480, 48, 100),
        (5760, 480, 55, 86),
        (6240, 240, 40, 78),
        (6240, 480, 52, 94),
        (6720, 480, 43, 88),
        (6720, 480, 55, 92),
        (7200, 240, 41, 76),
        (7200, 480, 53, 96),
    ]
    events: list[tuple[int, int, bytes]] = []
    for start, duration, note, velocity in notes:
        events.append((start, 1, bytes((0x90, note, velocity))))
        events.append((start + duration, 0, bytes((0x80, note, 0))))
    events.sort(key=lambda event: (event[0], event[1], event[2][1]))
    last_tick = 0
    note_events: list[tuple[int, bytes]] = []
    for tick, _, payload in events:
        note_events.append((tick - last_tick, payload))
        last_tick = tick
    return (
        b"MThd"
        + struct.pack(">IHHH", 6, 1, 2, 480)
        + tempo
        + track(note_events)
    )


def make_note_midi(
    notes: list[tuple[int, int, int, int]],
    tempo_changes: list[tuple[int, int]] | None = None,
) -> bytes:
    tempo_events: list[tuple[int, bytes]] = [(0, b"\xff\x51\x03\x07\xa1\x20")]
    for tick, microseconds in tempo_changes or []:
        tempo_events.append((tick, b"\xff\x51\x03" + microseconds.to_bytes(3, "big")))
    tempo_events.sort(key=lambda event: event[0])
    last_tempo_tick = 0
    tempo_track_events: list[tuple[int, bytes]] = []
    for tick, payload in tempo_events:
        tempo_track_events.append((tick - last_tempo_tick, payload))
        last_tempo_tick = tick
    events: list[tuple[int, int, bytes]] = []
    for start, duration, note, velocity in notes:
        events.append((start, 1, bytes((0x90, note, velocity))))
        events.append((start + duration, 0, bytes((0x80, note, 0))))
    events.sort(key=lambda event: (event[0], event[1], event[2][1]))
    last_tick = 0
    note_track_events: list[tuple[int, bytes]] = []
    for tick, _, payload in events:
        note_track_events.append((tick - last_tick, payload))
        last_tick = tick
    return b"MThd" + struct.pack(">IHHH", 6, 1, 2, 480) + track(tempo_track_events) + track(note_track_events)


def main() -> None:
    root = Path(__file__).resolve().parents[2]
    output_dir = root / "testdata" / "midi"
    output_dir.mkdir(parents=True, exist_ok=True)
    outputs = {
        "basic-poly-synth-phrase.mid": make_midi(),
        "sine-reference.mid": make_note_midi(
            [(0, 800, 48, 100), (960, 800, 69, 100), (1920, 800, 84, 100)]
        ),
        "saw-registers.mid": make_note_midi(
            [(index * 960, 800, note, 100) for index, note in enumerate((36, 48, 60, 72, 84))]
        ),
        "attack-release.mid": make_note_midi(
            [
                (0, 120, 60, 100),
                (720, 960, 62, 100),
                (2400, 120, 64, 100),
                (3600, 2400, 65, 100),
            ]
        ),
        "repeated-notes.mid": make_note_midi(
            [(index * 240, 120, 60, 100) for index in range(16)]
        ),
        "polyphony-stealing.mid": make_note_midi(
            [
                (0, 480, 48, 96),
                (120, 480, 52, 96),
                (240, 480, 55, 96),
                (360, 480, 59, 96),
                (500, 480, 62, 96),
                (620, 480, 65, 96),
                (740, 480, 67, 96),
                (860, 480, 71, 96),
            ]
        ),
        "filter-velocity.mid": make_note_midi(
            [(index * 480, 360, 60, velocity) for index, velocity in enumerate((32, 64, 96, 127))]
        ),
    }
    for name, data in outputs.items():
        (output_dir / name).write_bytes(data)


if __name__ == "__main__":
    main()
