#!/usr/bin/env python3
"""Generate a deterministic review package for dynamic parameter behavior."""

from __future__ import annotations

import json
import hashlib
import shutil
import subprocess
import tempfile
from pathlib import Path

from common import render_midi
from measure_wav import compare_wav, measure

ROOT = Path(__file__).resolve().parents[2]
SAMPLE_RATE = 48_000
DURATION_FRAMES = 144_000
BLOCK_SIZES = (32, 64, 257, 1024)
BLOCK_SIZE_MAX_DIFFERENCE = 1.0e-3
BUILTIN_SOURCE_IDS = {
    "velocity",
    "key_tracking",
    "pitch_bend",
    "mod_wheel",
    "aftertouch",
}


def cli_command() -> list[str]:
    candidates = (
        ROOT / "target" / "debug" / "sonalloy.exe",
        ROOT / "target" / "debug" / "sonalloy",
    )
    for candidate in candidates:
        if candidate.exists():
            return [str(candidate)]
    return ["cargo", "run", "-q", "-p", "sonalloy-cli", "--"]


def write_utf8(path: Path, content: str) -> None:
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write(content)


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


def copy_definition(source: Path, destination: Path) -> None:
    value = json.loads(source.read_text(encoding="utf-8"))
    for layer in value["layers"]:
        sample = layer.get("generator", {}).get("sample")
        if sample is not None:
            sample["zones"][0]["asset"]["path"] = "../assets/metal-hit.wav"
    write_definition(destination, value)


def write_definition(destination: Path, value: dict[str, object]) -> None:
    write_utf8(
        destination,
        json.dumps(value, ensure_ascii=False, indent=2) + "\n",
    )


def copy_modulation_variant(
    source: Path,
    destination: Path,
    route_keys: set[tuple[str, str]],
    extra_routes: list[dict[str, object]] | None = None,
) -> None:
    value = json.loads(source.read_text(encoding="utf-8"))
    for layer in value["layers"]:
        sample = layer.get("generator", {}).get("sample")
        if sample is not None:
            sample["zones"][0]["asset"]["path"] = "../assets/metal-hit.wav"
    modulation = value.get("modulation")
    if modulation is None:
        raise RuntimeError(f"definition has no modulation block: {source}")
    routes = [
        route
        for route in modulation["routes"]
        if (route["source"], route["target"]) in route_keys
    ]
    routes.extend(extra_routes or [])
    user_source_ids = {
        route["source"]
        for route in routes
        if route["source"] not in BUILTIN_SOURCE_IDS
    }
    modulation["sources"] = [
        source_definition
        for source_definition in modulation["sources"]
        if source_definition["id"] in user_source_ids
    ]
    modulation["routes"] = routes
    write_definition(destination, value)


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


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
    moving_lfo_definition = definition_dir / "moving-hybrid-pad-lfo-filter.json"
    moving_envelope_definition = definition_dir / "moving-hybrid-pad-envelope-pitch.json"
    moving_random_definition = definition_dir / "moving-hybrid-pad-random-pan.json"
    moving_key_tracking_definition = definition_dir / "moving-hybrid-pad-key-tracking.json"
    moving_resonance_definition = definition_dir / "moving-hybrid-pad-resonance.json"
    velocity_gain_routes = {
        ("velocity", "layer.attack.gain"),
        ("velocity", "layer.body.gain"),
    }
    copy_modulation_variant(
        moving_source,
        moving_lfo_definition,
        velocity_gain_routes | {("filter_motion", "voice.processor.tone.cutoff")},
    )
    copy_modulation_variant(
        moving_source,
        moving_envelope_definition,
        velocity_gain_routes | {("pitch_motion", "layer.body.tuning")},
    )
    copy_modulation_variant(
        moving_source,
        moving_random_definition,
        velocity_gain_routes | {("voice_pan", "layer.attack.pan")},
    )
    copy_modulation_variant(
        moving_source,
        moving_key_tracking_definition,
        velocity_gain_routes | {("key_tracking", "voice.processor.tone.cutoff")},
    )
    copy_modulation_variant(
        moving_source,
        moving_resonance_definition,
        velocity_gain_routes,
        extra_routes=[
            {
                "source": "mod_wheel",
                "target": "voice.processor.tone.resonance",
                "amount": 0.5,
                "curve": "linear",
            }
        ],
    )
    shutil.copy2(ROOT / "testdata" / "assets" / "metal-hit.wav", asset_dir / "metal-hit.wav")

    stealing_definition = definition_dir / "moving-hybrid-pad-stealing.json"
    stealing_value = json.loads(moving_source.read_text(encoding="utf-8"))
    stealing_value["performance"]["polyphony"] = 2
    for layer in stealing_value["layers"]:
        sample = layer.get("generator", {}).get("sample")
        if sample is not None:
            sample["zones"][0]["asset"]["path"] = "../assets/metal-hit.wav"
    write_utf8(
        stealing_definition,
        json.dumps(stealing_value, ensure_ascii=False, indent=2) + "\n",
    )

    event_source = ROOT / "testdata" / "events" / "expressive-hybrid-lead.json"
    event_fixture = event_dir / event_source.name
    shutil.copy2(event_source, event_fixture)
    random_event_source = ROOT / "testdata" / "events" / "dynamic-parameters-random-pan.json"
    random_event_fixture = event_dir / random_event_source.name
    shutil.copy2(random_event_source, random_event_fixture)
    resonance_event_source = ROOT / "testdata" / "events" / "dynamic-parameters-resonance.json"
    resonance_event_fixture = event_dir / resonance_event_source.name
    shutil.copy2(resonance_event_source, resonance_event_fixture)
    midi_source = ROOT / "testdata" / "midi" / "expressive-hybrid-controls.mid"
    midi_fixture = midi_dir / midi_source.name
    shutil.copy2(midi_source, midi_fixture)
    phrase_source = ROOT / "testdata" / "midi" / "basic-poly-synth-phrase.mid"
    phrase_fixture = midi_dir / phrase_source.name
    shutil.copy2(phrase_source, phrase_fixture)
    stealing_midi_source = ROOT / "testdata" / "midi" / "polyphony-stealing.mid"
    stealing_midi_fixture = midi_dir / stealing_midi_source.name
    shutil.copy2(stealing_midi_source, stealing_midi_fixture)
    key_tracking_midi_source = ROOT / "testdata" / "midi" / "saw-registers.mid"
    key_tracking_midi_fixture = midi_dir / "dynamic-parameters-key-tracking.mid"
    shutil.copy2(key_tracking_midi_source, key_tracking_midi_fixture)

    jobs = {
        "01-parameter-cutoff.wav": lambda output: render_events(
            expressive_definition, event_fixture, output, 257
        ),
        "02-lfo-filter.wav": lambda output: render_note(
            moving_lfo_definition, output, 257
        ),
        "03-envelope-pitch.wav": lambda output: render_note(
            moving_envelope_definition, output, 257
        ),
        "04-random-pan.wav": lambda output: render_events(
            moving_random_definition, random_event_fixture, output, 257
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
        "08-key-tracking.wav": lambda output: render_midi(
            moving_key_tracking_definition, key_tracking_midi_fixture, output, 257
        ),
        "09-resonance-control.wav": lambda output: render_events(
            moving_resonance_definition, resonance_event_fixture, output, 257
        ),
    }
    for name, render in jobs.items():
        render(audio_dir / name)

    block_outputs = {}
    for block_size in BLOCK_SIZES:
        output = audio_dir / f"external-controls-block-{block_size}.wav"
        render_events(expressive_definition, event_fixture, output, block_size)
        block_outputs[str(block_size)] = output

    audio_metrics = {
        name: measure(audio_dir / name, list(BLOCK_SIZES))
        for name in jobs
    }
    invalid_audio = [
        name
        for name, values in audio_metrics.items()
        if not values["finite"] or values["large_discontinuity_count"] != 0
    ]
    if invalid_audio:
        raise RuntimeError(f"dynamic review audio checks failed: {invalid_audio}")

    block_size_comparison = {}
    for block_size in BLOCK_SIZES:
        comparison = compare_wav(
            block_outputs["257"], block_outputs[str(block_size)]
        )
        if (
            not comparison.get("compatible")
            or comparison.get("max_abs_difference", 1.0)
            > BLOCK_SIZE_MAX_DIFFERENCE
        ):
            raise RuntimeError(
                f"dynamic review block-size mismatch at {block_size}: {comparison}"
            )
        block_size_comparison[str(block_size)] = comparison

    metrics = {
        "sample_rate": SAMPLE_RATE,
        "duration_frames": DURATION_FRAMES,
        "audio": audio_metrics,
        "block_size_comparison": block_size_comparison,
    }
    with tempfile.TemporaryDirectory(prefix="sonalloy-dynamic-review-") as temporary:
        repeated = Path(temporary) / "random-pan-repeat.wav"
        render_events(moving_random_definition, random_event_fixture, repeated, 257)
        random_comparison = compare_wav(audio_dir / "04-random-pan.wav", repeated)
        if (
            not random_comparison.get("compatible")
            or random_comparison.get("max_abs_difference", 1.0) != 0.0
        ):
            raise RuntimeError(
                f"random pan render is not reproducible: {random_comparison}"
            )
        metrics["random_pan_reproducibility"] = {
            "first_sha256": sha256_file(audio_dir / "04-random-pan.wav"),
            "repeat_sha256": sha256_file(repeated),
            "wav_comparison": random_comparison,
        }
    write_utf8(
        review_root / "metrics.json",
        json.dumps(metrics, ensure_ascii=False, indent=2) + "\n",
    )
    write_utf8(
        review_root / "review-summary.md",
        """# Dynamic Parameter Review

## Inputs

- Moving Hybrid Pad
- Expressive Hybrid Lead
- Event Sequence with Parameter Change, Pitch Bend, Mod Wheel, Aftertouch, Random Pan, and Resonance Control
- MIDI fixture with Pitch Bend, CC1, Channel Aftertouch, and Key Tracking notes

## Human listening items

- `01-parameter-cutoff.wav`: Parameter Changeの位置、Smoothing、Click、変化量
- `02-lfo-filter.wav`: LFOの周期、位相、滑らかさ、Block Size差
- `03-envelope-pitch.wav`: Attack、Decay、Release、Pitchの連続性
- `04-random-pan.wav`: Noteごとの左右差、同じ入力での再現、極端な偏り
- `05-external-controls.wav`: Pitch Bend、Mod Wheel、Aftertouchの反映とSmoothing
- `06-voice-stealing.wav`: Steal Fade、Pending Note、LFO / Envelope初期化、Click
- `07-musical-phrase.wav`: 4〜8小節相当の音色としての使いやすさ
- `08-key-tracking.wav`: 低音から高音までのCutoff変化と音域の自然さ
- `09-resonance-control.wav`: Resonance変化の安定性、発散、Click

## 判定基準

- すべての音源で明確なClick、NaN、Infinity、異常な音量落ちがない
- LFOとEnvelopeが階段状や不連続に聞こえない
- Pitch BendでSampleとOscillatorの音程変化が一致する
- Random PanがNoteごとに変化し、同じ入力では再現する
- Key TrackingとResonanceが意図したTargetだけを変化させる
- Voice Stealing後に新しいNoteのSource Stateが前のVoiceから混ざらない
- Reference Instrumentを実際の音色として使用できる

自動測定値は同じディレクトリの`metrics.json`に保存する。音質の判定はこの記録だけでは完了しない。
""",
    )


if __name__ == "__main__":
    main()
