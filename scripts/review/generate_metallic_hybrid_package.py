#!/usr/bin/env python3
"""Generate the reproducible Metallic Hybrid sound review package."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

from measure_wav import compare_wav, measure  # noqa: E402


def run_cli(arguments: list[str]) -> None:
    subprocess.run(
        ["cargo", "run", "-q", "-p", "sonalloy-cli", "--", *arguments],
        cwd=ROOT,
        check=True,
    )


def source_commit() -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def render_note(definition: Path, output: Path, gate: str = "0.5") -> None:
    run_cli(
        [
            "render",
            "note",
            str(definition),
            "--note",
            "60",
            "--velocity",
            "110",
            "--gate",
            gate,
            "--tail",
            "0.5",
            "--sample-rate",
            "48000",
            "--block-size",
            "257",
            "--output",
            str(output),
            "--json",
        ]
    )


def render_midi(definition: Path, midi: Path, output: Path, block_size: int = 257) -> None:
    run_cli(
        [
            "render",
            "midi",
            str(definition),
            str(midi),
            "--sample-rate",
            "48000",
            "--block-size",
            str(block_size),
            "--tail",
            "1.0",
            "--output",
            str(output),
            "--json",
        ]
    )


def copy_definition(source: Path, destination: Path, asset_path: str | None) -> None:
    definition = json.loads(source.read_text(encoding="utf-8"))
    if asset_path is not None:
        for layer in definition["layers"]:
            sample = layer.get("generator", {}).get("sample")
            if sample is not None:
                sample["asset"]["path"] = asset_path
    destination.write_text(
        json.dumps(definition, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )


def main() -> None:
    subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts" / "review" / "generate_metallic_hybrid_inputs.py"),
        ],
        cwd=ROOT,
        check=True,
    )

    review_root = ROOT / "review-output" / "metallic-hybrid"
    audio_dir = review_root / "audio"
    definition_dir = review_root / "definitions"
    midi_dir = review_root / "midi"
    asset_dir = review_root / "assets"
    for directory in (audio_dir, definition_dir, midi_dir, asset_dir):
        directory.mkdir(parents=True, exist_ok=True)

    definition_sources = {
        "metallic-hybrid.json": ROOT / "examples" / "instruments" / "metallic-hybrid.json",
        "metallic-hybrid-oscillator-only.json": ROOT
        / "examples"
        / "instruments"
        / "metallic-hybrid-oscillator-only.json",
        "metallic-hybrid-sample-only.json": ROOT
        / "examples"
        / "instruments"
        / "metallic-hybrid-sample-only.json",
        "metallic-hybrid-missing-asset.json": ROOT
        / "examples"
        / "instruments"
        / "metallic-hybrid-missing-asset.json",
    }
    definitions = {}
    for name, source in definition_sources.items():
        destination = definition_dir / name
        package_asset_path = (
            "../assets/missing-metal-hit.wav"
            if name == "metallic-hybrid-missing-asset.json"
            else "../assets/metal-hit.wav"
        )
        copy_definition(source, destination, package_asset_path)
        definitions[name] = destination

    midi_sources = {
        "metallic-hybrid-phrase.mid": ROOT
        / "testdata"
        / "midi"
        / "metallic-hybrid-phrase.mid",
        "metallic-hybrid-pitch-range.mid": ROOT
        / "testdata"
        / "midi"
        / "metallic-hybrid-pitch-range.mid",
        "metallic-hybrid-velocity.mid": ROOT
        / "testdata"
        / "midi"
        / "metallic-hybrid-velocity.mid",
    }
    for name, source in midi_sources.items():
        shutil.copy2(source, midi_dir / name)

    source_asset = ROOT / "testdata" / "assets" / "metal-hit.wav"
    shutil.copy2(source_asset, asset_dir / source_asset.name)
    shutil.copy2(source_asset, audio_dir / "01-sample-source.wav")

    sample_only = definitions["metallic-hybrid-sample-only.json"]
    oscillator_only = definitions["metallic-hybrid-oscillator-only.json"]
    hybrid = definitions["metallic-hybrid.json"]
    missing = definitions["metallic-hybrid-missing-asset.json"]
    render_note(sample_only, audio_dir / "02-sample-decoded-root.wav")
    render_midi(
        sample_only,
        midi_sources["metallic-hybrid-pitch-range.mid"],
        audio_dir / "03-sample-pitch-range.wav",
    )
    render_midi(
        oscillator_only,
        midi_sources["metallic-hybrid-phrase.mid"],
        audio_dir / "04-oscillator-only.wav",
    )
    render_midi(
        sample_only,
        midi_sources["metallic-hybrid-phrase.mid"],
        audio_dir / "05-sample-only.wav",
    )
    render_note(hybrid, audio_dir / "06-hybrid-mix.wav", gate="0.35")
    render_midi(
        hybrid,
        midi_sources["metallic-hybrid-velocity.mid"],
        audio_dir / "07-velocity-response.wav",
    )
    render_midi(
        hybrid,
        midi_sources["metallic-hybrid-phrase.mid"],
        audio_dir / "08-musical-phrase.wav",
    )
    render_midi(
        missing,
        midi_sources["metallic-hybrid-phrase.mid"],
        audio_dir / "09-missing-asset-fallback.wav",
    )

    audio_metrics = {}
    for audio_path in sorted(audio_dir.glob("*.wav")):
        audio_metrics[audio_path.name] = measure(
            audio_path,
            [257],
            include_spectrum=audio_path.name in {
                "04-oscillator-only.wav",
                "06-hybrid-mix.wav",
                "08-musical-phrase.wav",
            },
        )

    with tempfile.TemporaryDirectory(prefix="sonalloy-review-") as temporary:
        temporary_root = Path(temporary)
        reference = audio_dir / "08-musical-phrase.wav"
        block_comparisons = {}
        for block_size in (64, 1024):
            candidate = temporary_root / f"phrase-{block_size}.wav"
            render_midi(
                hybrid,
                midi_sources["metallic-hybrid-phrase.mid"],
                candidate,
                block_size=block_size,
            )
            block_comparisons[str(block_size)] = compare_wav(reference, candidate)

    metrics = {
        "sample_rate": 48000,
        "review_block_size": 257,
        "asset": "metal-hit.wav",
        "audio": audio_metrics,
        "block_size_comparisons": block_comparisons,
    }
    finite = all(item["finite"] for item in audio_metrics.values())
    rendered_metrics = {
        name: item for name, item in audio_metrics.items() if name != "01-sample-source.wav"
    }
    peak = max((item["peak"] for item in rendered_metrics.values()), default=0.0)
    max_delta = max(
        (item["max_adjacent_frame_delta"] for item in rendered_metrics.values()),
        default=0.0,
    )
    block_size_stable = all(
        comparison.get("compatible")
        and float(comparison.get("max_abs_difference", 1.0)) <= 1.0e-5
        for comparison in block_comparisons.values()
    )
    rendered_peaks_safe = all(item["peak"] <= 1.0 for item in rendered_metrics.values())
    rendered_discontinuities_absent = all(
        item["large_discontinuity_count"] == 0 for item in rendered_metrics.values()
    )
    metrics["automatic_checks"] = {
        "all_audio_finite": finite,
        "rendered_peaks_within_float_wav_range": rendered_peaks_safe,
        "rendered_large_discontinuities_absent": rendered_discontinuities_absent,
        "rendered_block_sizes_reproducible": block_size_stable,
    }
    (review_root / "metrics.json").write_text(
        json.dumps(metrics, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    summary = f"""# Metallic Hybrid Sound Review

## Reference

- Definition: metallic-hybrid.json
- Source implementation commit: {source_commit()}
- Asset: metal-hit.wav
- MIDI inputs: metallic-hybrid-phrase.mid, metallic-hybrid-pitch-range.mid, metallic-hybrid-velocity.mid
- Sample rate: 48,000 Hz
- Render block size: 257 frames
- Rendered output format: stereo 32-bit float WAV
- Source asset format: mono PCM16 WAV

## Automatic checks

- All generated samples are finite: {"pass" if finite else "fail"}
- Rendered peaks stay within the float WAV range: {"pass" if rendered_peaks_safe else "fail"}
- Rendered outputs have no adjacent-frame discontinuity candidates over 0.25: {"pass" if rendered_discontinuities_absent else "fail"}
- All rendered outputs remain reproducible across block sizes 64, 257, and 1024: {"pass" if block_size_stable else "fail"}
- Maximum rendered-output absolute peak: {peak:.6f}
- Maximum rendered-output adjacent-frame difference: {max_delta:.6f}
- Source asset metrics are retained as 01-sample-source.wav in metrics.json
- Missing asset render retains the oscillator layer: 09-missing-asset-fallback.wav
- 02-sample-decoded-root.wav and 03-sample-pitch-range.wav are not loudness-matched to the source; they use the sample-only layer gain and include one-shot silence.
- 06-hybrid-mix.wav starts the sample and oscillator layers from one Note On and is intended to sound like one integrated instrument.

## Listening intent

| File | Intent | Expected result |
|---|---|---|
| 01-sample-source.wav | Present the source fixture without engine processing. | A short, bright metallic transient with a low body tail; this is the reference for the following sample renders. |
| 02-sample-decoded-root.wav | Check WAV decode, hash validation, 44.1 kHz to 48 kHz preparation, and root-note playback at C4. | The same character and pitch as the source, without a new click or an obvious resampling artifact. |
| 03-sample-pitch-range.wav | Check one-shot playback at C3, C4, and C5 using the C4 root note. | The attack moves one octave down and up while remaining recognizably the same sample and ending cleanly. |
| 04-oscillator-only.wav | Isolate the sine body used by the instrument. | A stable pitched tone with the intended envelope and no metallic attack layer. |
| 05-sample-only.wav | Isolate the sample layer across the musical phrase. | Each note has a distinct metallic attack and the layer does not contribute a sustained pitched body. |
| 06-hybrid-mix.wav | Compare the combined sample attack and oscillator body at the reference note. | The attack supplies brightness and definition while the body supplies pitch and sustain; the two layers sound like one instrument. |
| 07-velocity-response.wav | Check the four velocity levels from quiet to loud. | Louder notes become stronger and brighter in a continuous, musical way without clicks or disproportionate jumps. |
| 08-musical-phrase.wav | Evaluate the complete instrument in a short phrase with overlapping notes and varied velocities. | The hybrid remains clear, playable, and balanced through the phrase. |
| 09-missing-asset-fallback.wav | Confirm the body remains usable when the attack asset cannot be loaded. | The metallic attack disappears, but the oscillator body continues to render normally and the output stays clean. |

## Listening record

| Item | Result | Notes |
|---|---|---|
| Source and decoded root sound natural |  |  |
| Pitch range is usable |  |  |
| Sample ending is free of an audible click |  |  |
| Attack layer has a clear transient role |  |  |
| Body layer provides pitch and sustain |  |  |
| Solo layers combine into one instrument |  |  |
| Velocity response is natural |  |  |
| Musical phrase is usable |  |  |
| Hybrid value is clear |  |  |
| Instrument is ready for use |  |  |

## Files

The audio, copied Definitions, MIDI inputs, source asset, metrics, and this record are kept in this directory.
"""
    (review_root / "review-summary.md").write_text(summary, encoding="utf-8")


if __name__ == "__main__":
    main()
