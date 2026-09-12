"""Generate the C4 fragments used by Rhythmic Wave Sequence (Python 3)."""

import hashlib
import json
import math
import wave
from pathlib import Path

RATE = 48000
FRAGMENT_FRAMES = 11520
FREQUENCY = 440 * 2 ** ((60 - 69) / 12)
ROOT = Path(__file__).resolve().parent

spectra = [
    [(h, 1 / h) for h in range(1, 25)],
    [(h, 1 / h) for h in range(1, 24, 2)],
    [(h, math.sin(math.pi * 0.27 * h) / h) for h in range(1, 25)],
    [(h, (-1) ** ((h - 1) // 2) / h**2) for h in range(1, 16, 2)],
    [(1, 1), (3, 0.48), (5, 0.18), (7, 0.06)],
    [(1, 1), (2, 0.6), (3, 0.24), (4, 0.16)],
    [(1, 0.7), (2, 0.4), (4.013, 0.35), (7.027, 0.14)],
    [
        (
            h,
            (
                math.exp(-(((h * FREQUENCY - 700) / 240) ** 2))
                + 0.6 * math.exp(-(((h * FREQUENCY - 1200) / 340) ** 2))
                + 0.08
            )
            / h**0.6,
        )
        for h in range(1, 17)
    ],
]
fragments = []
for spectrum in spectra:
    values = []
    for frame in range(FRAGMENT_FRAMES):
        t = frame / RATE
        envelope = (
            min(t / 0.0025, 1)
            * math.exp(-t / 0.048)
            * min((FRAGMENT_FRAMES - 1 - frame) / (RATE * 0.015), 1)
        )
        values.append(
            envelope
            * sum(a * math.sin(2 * math.pi * FREQUENCY * h * t) for h, a in spectrum)
        )
    scale = 0.6 / max(abs(x) for x in values)
    fragments.extend(round(x * scale * 8388607) for x in values)
path = ROOT / "wave-seq-steps.wav"
with wave.open(str(path), "wb") as audio:
    audio.setparams((1, 3, RATE, 0, "NONE", "not compressed"))
    audio.writeframes(b"".join(x.to_bytes(3, "little", signed=True) for x in fragments))
definition = ROOT.parent / "50-rhythmic-wave-sequence/definition.json"
data = json.loads(definition.read_text())
checksum = hashlib.sha256(path.read_bytes()).hexdigest()
for index, step in enumerate(data["layers"][0]["generator"]["wave_sequence"]["steps"]):
    step["asset"]["sha256"] = checksum
    step["region"] = {
        "start_seconds": index * FRAGMENT_FRAMES / RATE,
        "end_seconds": (index + 1) * FRAGMENT_FRAMES / RATE,
    }
definition.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
print(path.name, checksum)
