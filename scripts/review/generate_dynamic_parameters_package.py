#!/usr/bin/env python3
"""Generate a deterministic review package for dynamic parameter behavior."""

from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path

from measure_wav import compare_wav, measure

ROOT = Path(__file__).resolve().parents[2]
SAMPLE_RATE = 48_000
DURATION_FRAMES = 144_000
BLOCK_SIZES = (32, 64, 257, 1024)


def cli_command() -> list[str]:
    candidates = (
        ROOT / "target" / "debug" / "sonalloy.exe",
        ROOT / "target" / "debug" / "sonalloy",
    )
    for candidate in candidates:
        if candidate.exists():
            return [str(candidate)]
    return ["cargo", "run", "-q", "-p", "sonalloy-cli", "--"]


def run_cli(arguments: list[str]) -> None:
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


def render_events(definition: Path, events: Path, output: Path, block_size: int) -> None:
    run_cli(
        [
            "render",
            "events",
            str(definition),
            str(events),
            "--duration-frames",
            str(DURATION_FRAMES),
            "--sample-rate",
            str(SAMPLE_RATE),
            "--block-size",
            str(block_size),
            "--tail",
            "0.5",
            "--output",
            str(output),
            "--json",
        ]
    )


def render_note(definition: Path, output: Path, block_size: int) -> None:
    run_cli(
        [
            "render",
            "note",
            str(definition),
            "--note",
            "60",
            "--velocity",
            "112",
            "--gate",
            "1.5",
            "--tail",
            "1.0",
            "--sample-rate",
            str(SAMPLE_RATE),
            "--block-size",
            str(block_size),
            "--output",
            str(output),
            "--json",
        ]
    )


def render_midi(definition: Path, midi: Path, output: Path, block_size: int) -> None:
    run_cli(
        [
            "render",
            "midi",
            str(definition),
            str(midi),
            "--sample-rate",
            str(SAMPLE_RATE),
            "--block-size",
            str(block_size),
            "--tail",
            "1.0",
            "--output",
            str(output),
            "--json",
        ]
    )


def copy_definition(source: Path, destination: Path) -> None:
    value = json.loads(source.read_text(encoding="utf-8"))
    for layer in value["layers"]:
        sample = layer.get("generator", {}).get("sample")
        if sample is not None:
            sample["asset"]["path"] = "../assets/metal-hit.wav"
    destination.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def main() -> None:
    review_root = ROOT / "review-output" / "dynamic-parameters"
    audio_dir = review_root / "audio"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    midi_dir = review_root / "midi"
    asset_dir = review_root / "assets"
    for directory in (audio_dir, definition_dir, event_dir, midi_dir, asset_dir):
        directory.mkdir(parents=True, exist_ok=True)

    moving_source = ROOT / "examples" / "instruments" / "moving-hybrid-pad.json"
    expressive_source = ROOT / "examples" / "instruments" / "expressive-hybrid-lead.json"
    moving_definition = definition_dir / moving_source.name
    expressive_definition = definition_dir / expressive_source.name
    copy_definition(moving_source, moving_definition)
    copy_definition(expressive_source, expressive_definition)
    shutil.copy2(ROOT / "testdata" / "assets" / "metal-hit.wav", asset_dir / "metal-hit.wav")

    stealing_definition = definition_dir / "moving-hybrid-pad-stealing.json"
    stealing_value = json.loads(moving_source.read_text(encoding="utf-8"))
    stealing_value["performance"]["polyphony"] = 2
    for layer in stealing_value["layers"]:
        sample = layer.get("generator", {}).get("sample")
        if sample is not None:
            sample["asset"]["path"] = "../assets/metal-hit.wav"
    stealing_definition.write_text(
        json.dumps(stealing_value, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )

    event_source = ROOT / "testdata" / "events" / "expressive-hybrid-lead.json"
    event_fixture = event_dir / event_source.name
    shutil.copy2(event_source, event_fixture)
    midi_source = ROOT / "testdata" / "midi" / "expressive-hybrid-controls.mid"
    midi_fixture = midi_dir / midi_source.name
    shutil.copy2(midi_source, midi_fixture)
    phrase_source = ROOT / "testdata" / "midi" / "basic-poly-synth-phrase.mid"
    phrase_fixture = midi_dir / phrase_source.name
    shutil.copy2(phrase_source, phrase_fixture)
    stealing_midi_source = ROOT / "testdata" / "midi" / "polyphony-stealing.mid"
    stealing_midi_fixture = midi_dir / stealing_midi_source.name
    shutil.copy2(stealing_midi_source, stealing_midi_fixture)

    jobs = {
        "01-parameter-cutoff.wav": lambda output: render_events(
            expressive_definition, event_fixture, output, 257
        ),
        "02-lfo-filter.wav": lambda output: render_note(
            moving_definition, output, 257
        ),
        "03-envelope-pitch.wav": lambda output: render_note(
            moving_definition, output, 257
        ),
        "04-random-pan.wav": lambda output: render_note(
            moving_definition, output, 257
        ),
        "05-external-controls.wav": lambda output: render_midi(
            expressive_definition, midi_fixture, output, 257
        ),
        "06-voice-stealing.wav": lambda output: render_midi(
            stealing_definition, stealing_midi_fixture, output, 257
        ),
        "07-musical-phrase.wav": lambda output: render_midi(
            expressive_definition, phrase_fixture, output, 257
        ),
    }
    for name, render in jobs.items():
        render(audio_dir / name)

    block_outputs = {}
    for block_size in BLOCK_SIZES:
        output = audio_dir / f"external-controls-block-{block_size}.wav"
        render_events(expressive_definition, event_fixture, output, block_size)
        block_outputs[str(block_size)] = output

    metrics = {
        "sample_rate": SAMPLE_RATE,
        "duration_frames": DURATION_FRAMES,
        "audio": {
            name: measure(audio_dir / name, list(BLOCK_SIZES))
            for name in jobs
        },
        "block_size_comparison": {
            block_size: compare_wav(
                block_outputs["257"], block_outputs[str(block_size)]
            )
            for block_size in BLOCK_SIZES
        },
    }
    (review_root / "metrics.json").write_text(
        json.dumps(metrics, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    (review_root / "review-summary.md").write_text(
        """# Dynamic Parameter Review

## Inputs

- Moving Hybrid Pad
- Expressive Hybrid Lead
- Event Sequence with Parameter Change, Pitch Bend, Mod Wheel, and Aftertouch
- MIDI fixture with Pitch Bend, CC1, and Channel Aftertouch

## Human listening items

- Parameter Changeの位置、Smoothing、Click
- LFO FilterとModulation Envelope Pitchの周期・境界
- Random Panの左右差と再現性
- Pitch BendでSampleとOscillatorが一致すること
- Mod Wheel / AftertouchのFilter・Gain反映
- Voice Stealing中のFade、Pending Note、Source初期化
- Musical Phraseでの音色としての使いやすさ

Metricsは同じディレクトリの`metrics.json`に保存する。
""",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
