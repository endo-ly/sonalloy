"""Generate the single-cycle wavetable frames used by Rhythmic Wave Sequence."""

import hashlib
import json
import math
import wave
from pathlib import Path

RATE = 48000
FRAME_LENGTH = 2048
C4_FREQUENCY = 440 * 2 ** ((60 - 69) / 12)
ROOT = Path(__file__).resolve().parent

spectra = [
    [(h, 1 / h) for h in range(1, 25)],
    [(h, 1 / h) for h in range(1, 24, 2)],
    [(h, math.sin(math.pi * 0.27 * h) / h) for h in range(1, 25)],
    [(h, (-1) ** ((h - 1) // 2) / h**2) for h in range(1, 16, 2)],
    [(1, 1), (3, 0.48), (5, 0.18), (7, 0.06)],
    [(1, 1), (2, 0.6), (3, 0.24), (4, 0.16)],
    [(1, 0.7), (2, 0.4), (4, 0.35), (7, 0.14)],
    [
        (
            h,
            (
                math.exp(-(((h * C4_FREQUENCY - 700) / 240) ** 2))
                + 0.6 * math.exp(-(((h * C4_FREQUENCY - 1200) / 340) ** 2))
                + 0.08
            )
            / h**0.6,
        )
        for h in range(1, 17)
    ],
]
frames = []
for spectrum in spectra:
    values = []
    for frame in range(FRAME_LENGTH):
        phase = frame / FRAME_LENGTH
        values.append(
            sum(
                amplitude * math.sin(2 * math.pi * harmonic * phase)
                for harmonic, amplitude in spectrum
            )
        )
    scale = 0.6 / max(abs(x) for x in values)
    frames.extend(round(x * scale * 8388607) for x in values)
path = ROOT / "wave-seq-steps.wav"
with wave.open(str(path), "wb") as audio:
    audio.setparams((1, 3, RATE, 0, "NONE", "not compressed"))
    audio.writeframes(b"".join(x.to_bytes(3, "little", signed=True) for x in frames))
definition = ROOT.parent / "50-rhythmic-wave-sequence/definition.json"
data = json.loads(definition.read_text())
checksum = hashlib.sha256(path.read_bytes()).hexdigest()
data["layers"][0]["generator"]["wavetable"]["asset"]["sha256"] = checksum
definition.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
print(path.name, checksum)
