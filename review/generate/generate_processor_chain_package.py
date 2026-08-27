#!/usr/bin/env python3
"""Generate the deterministic Processor Chain review package."""

from __future__ import annotations

import copy
import hashlib
import json
import math
import shutil
import subprocess
from pathlib import Path

from common import record_render_report, render_midi
from measure_wav import compare_wav, measure, read_float_wav

ROOT = Path(__file__).resolve().parents[2]
SAMPLE_RATE = 48_000
DURATION_FRAMES = 144_000
BLOCK_SIZES = (32, 64, 257, 1024)
BLOCK_SIZE_MAX_DIFFERENCE = 1.0e-3


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
    path.write_bytes(content.encode("utf-8"))


def run_cli(arguments: list[str]) -> str:
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
    record_render_report(arguments, result.stdout)
    return result.stdout


def render_note(
    definition: Path,
    output: Path,
    block_size: int,
    sample_rate: int = SAMPLE_RATE,
) -> None:
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
            str(sample_rate),
            "--block-size",
            str(block_size),
            "--output",
            str(output),
            "--analyze",
            "--json",
        ]
    )


def render_events(
    definition: Path,
    events: Path,
    output: Path,
    block_size: int,
) -> None:
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
            "--analyze",
            "--json",
        ]
    )


def load_definition(path: Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def prepare_asset_paths(value: dict[str, object]) -> None:
    for layer in value["layers"]:
        generator = layer.get("generator", {})
        sample = generator.get("sample")
        if sample is not None:
            sample["zones"][0]["asset"]["path"] = "../assets/metal-hit.wav"


def processor(
    processor_type: str,
    processor_id: str,
    **fields: float,
) -> dict[str, object]:
    value: dict[str, object] = {"type": processor_type, "id": processor_id}
    value.update(fields)
    return value


def base_value(source: Path) -> dict[str, object]:
    value = load_definition(source)
    prepare_asset_paths(value)
    return value


def empty_value(source: Path) -> dict[str, object]:
    value = base_value(source)
    for layer in value["layers"]:
        layer["processors"] = []
    value["voice_processors"] = []
    value["global_processors"] = []
    value["modulation"] = None
    return value


def impulse_value(source: Path) -> dict[str, object]:
    value = empty_value(source)
    value["layers"] = [value["layers"][0]]
    return value


def write_definition(path: Path, value: dict[str, object]) -> None:
    write_utf8(path, json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def tail_rms(path: Path, start_frame: int) -> float:
    _, channels, samples = read_float_wav(path)
    start = min(start_frame * channels, len(samples))
    tail = samples[start:]
    if not tail:
        return 0.0
    return math.sqrt(sum(sample * sample for sample in tail) / len(tail))


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    review_root = ROOT / "review" / "processor-chain"
    audio_dir = review_root / "audio"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    midi_dir = review_root / "midi"
    asset_dir = review_root / "assets"
    for directory in (audio_dir, definition_dir, event_dir, midi_dir, asset_dir):
        directory.mkdir(parents=True, exist_ok=True)

    source = ROOT / "testdata" / "instruments" / "processed-hybrid.json"
    definitions: dict[str, Path] = {}

    variants: dict[str, dict[str, object]] = {}
    variants["baseline"] = empty_value(source)

    value = empty_value(source)
    value["layers"][0]["processors"] = [
        processor("filter", "attack_tone", cutoff_hz=10_000.0, resonance=0.08)
    ]
    variants["layer-filter"] = value

    value = empty_value(source)
    value["layers"][1]["processors"] = [
        processor("drive", "body_drive", amount=0.28, mix=0.45)
    ]
    variants["layer-drive"] = value

    value = empty_value(source)
    value["voice_processors"] = [
        processor("filter", "tone", cutoff_hz=6_800.0, resonance=0.14)
    ]
    variants["voice-filter"] = value

    value = empty_value(source)
    value["voice_processors"] = [
        processor("drive", "glue", amount=0.12, mix=0.3)
    ]
    variants["voice-drive"] = value

    value = empty_value(source)
    value["global_processors"] = [
        processor("filter", "master_tone", cutoff_hz=7_000.0, resonance=0.1)
    ]
    variants["global-filter"] = value

    value = empty_value(source)
    value["global_processors"] = [
        processor("drive", "master_drive", amount=0.16, mix=0.3)
    ]
    variants["global-drive"] = value

    value = impulse_value(source)
    value["global_processors"] = [
        processor(
            "delay",
            "echo",
            time={"value": 0.24, "unit": "seconds"},
            feedback_mode="stereo",
            feedback=0.34,
            taps=[],
            mix=0.18,
        )
    ]
    variants["delay"] = value

    value = impulse_value(source)
    value["global_processors"] = [
        processor(
            "reverb",
            "space",
            pre_delay_seconds=0.012,
            decay=0.7,
            damping=0.32,
            width=0.9,
            mix=0.24,
        )
    ]
    variants["reverb"] = value

    variants["processed-hybrid"] = base_value(source)

    for name, value in variants.items():
        destination = definition_dir / f"{name}.json"
        write_definition(destination, value)
        definitions[name] = destination

    event_source = ROOT / "testdata" / "events" / "processed-hybrid.json"
    event_fixture = event_dir / event_source.name
    shutil.copy2(event_source, event_fixture)
    event_value = json.loads(event_source.read_text(encoding="utf-8"))
    parameter_event_fixture = event_dir / "parameter-change.json"
    write_definition(
        parameter_event_fixture,
        {
            "events": [
                event
                for event in event_value["events"]
                if event["type"] != "mod_wheel"
            ],
        },
    )
    global_event_fixture = event_dir / "global-mod-wheel.json"
    write_definition(
        global_event_fixture,
        {
            "events": [
                event
                for event in event_value["events"]
                if event["type"] != "parameter_change"
            ],
        },
    )
    phrase_source = ROOT / "testdata" / "midi" / "basic-poly-synth-phrase.mid"
    phrase_fixture = midi_dir / phrase_source.name
    shutil.copy2(phrase_source, phrase_fixture)
    stealing_source = ROOT / "testdata" / "midi" / "polyphony-stealing.mid"
    stealing_fixture = midi_dir / stealing_source.name
    shutil.copy2(stealing_source, stealing_fixture)
    shutil.copy2(ROOT / "testdata" / "assets" / "metal-hit.wav", asset_dir / "metal-hit.wav")

    jobs = {
        "01-baseline.wav": lambda output: render_note(definitions["baseline"], output, 257),
        "02-layer-filter.wav": lambda output: render_note(definitions["layer-filter"], output, 257),
        "03-layer-drive.wav": lambda output: render_note(definitions["layer-drive"], output, 257),
        "04-voice-filter.wav": lambda output: render_note(definitions["voice-filter"], output, 257),
        "05-voice-drive.wav": lambda output: render_note(definitions["voice-drive"], output, 257),
        "06-global-filter.wav": lambda output: render_note(definitions["global-filter"], output, 257),
        "07-global-drive.wav": lambda output: render_note(definitions["global-drive"], output, 257),
        "08-delay-impulse.wav": lambda output: render_note(definitions["delay"], output, 257),
        "09-reverb-impulse.wav": lambda output: render_note(definitions["reverb"], output, 257),
        "10-processed-hybrid.wav": lambda output: render_events(
            definitions["processed-hybrid"], event_fixture, output, 257
        ),
        "11-parameter-change.wav": lambda output: render_events(
            definitions["processed-hybrid"], parameter_event_fixture, output, 257
        ),
        "12-global-mod-wheel.wav": lambda output: render_events(
            definitions["processed-hybrid"], global_event_fixture, output, 257
        ),
        "13-voice-stealing.wav": lambda output: render_midi(
            definitions["processed-hybrid"], stealing_fixture, output, 257
        ),
    }
    for name, render in jobs.items():
        render(audio_dir / name)

    block_outputs: dict[str, Path] = {}
    for block_size in BLOCK_SIZES:
        output = audio_dir / f"processed-hybrid-block-{block_size}.wav"
        render_events(definitions["processed-hybrid"], event_fixture, output, block_size)
        block_outputs[str(block_size)] = output

    sample_rate_outputs: dict[str, Path] = {}
    for sample_rate in (44_100, 48_000, 96_000):
        output = audio_dir / f"processed-hybrid-sample-rate-{sample_rate}.wav"
        render_note(definitions["processed-hybrid"], output, 257, sample_rate)
        sample_rate_outputs[str(sample_rate)] = output

    audio_metrics = {
        name: measure(audio_dir / name, list(BLOCK_SIZES))
        for name in jobs
    }
    block_size_comparison = {
        block_size: compare_wav(block_outputs["257"], block_outputs[block_size])
        for block_size in map(str, BLOCK_SIZES)
    }
    for block_size, comparison in block_size_comparison.items():
        if (
            not comparison.get("compatible")
            or comparison.get("max_abs_difference", 1.0) > BLOCK_SIZE_MAX_DIFFERENCE
        ):
            raise RuntimeError(
                f"processor review block-size mismatch at {block_size}: {comparison}"
            )

    invalid_audio = [
        name
        for name, values in audio_metrics.items()
        if not values["finite"]
    ]
    if invalid_audio:
        raise RuntimeError(f"processor review audio checks failed: {invalid_audio}")

    repeated = audio_dir / "processed-hybrid-repeat.wav"
    render_events(definitions["processed-hybrid"], event_fixture, repeated, 257)
    reset_comparison = compare_wav(audio_dir / "10-processed-hybrid.wav", repeated)
    if (
        not reset_comparison.get("compatible")
        or reset_comparison.get("max_abs_difference", 1.0) != 0.0
    ):
        raise RuntimeError(f"processed hybrid render is not reproducible: {reset_comparison}")

    metrics = {
        "sample_rate": SAMPLE_RATE,
        "duration_frames": DURATION_FRAMES,
        "audio": audio_metrics,
        "block_size_comparison": block_size_comparison,
        "sample_rate_audio": {
            rate: measure(path, list(BLOCK_SIZES))
            for rate, path in sample_rate_outputs.items()
        },
        "processed_hybrid_tail_rms_after_frame": 96_000,
        "processed_hybrid_tail_rms": tail_rms(audio_dir / "10-processed-hybrid.wav", 96_000),
        "reset_reproducibility": {
            "sha256": sha256_file(audio_dir / "10-processed-hybrid.wav"),
            "repeat_sha256": sha256_file(repeated),
            "wav_comparison": reset_comparison,
        },
    }
    write_utf8(review_root / "metrics.json", json.dumps(metrics, ensure_ascii=False, indent=2) + "\n")
    write_utf8(
        review_root / "review-summary.md",
        """# Processor Chain Review

## Inputs

- ProcessorなしのBaseline
- Layer Filter / Drive
- Voice Filter / Drive
- Global Filter / Drive
- Global Delay / Reverb
- Processed Hybrid（Sample Attack、Saw Body、Layer / Voice / Global Processor）
- Processor Parameter Change、Global Mod Wheel、Voice Stealing、Reset、Block Size、Sample Rate

## Human listening items

- `02-layer-filter.wav`: Attack LayerだけへのCutoff / Resonanceの作用
- `03-layer-drive.wav`: Body LayerだけへのAmount / Mix、低Amountの自然さ、高AmountのAliasing
- `04-voice-filter.wav` / `05-voice-drive.wav`: Layer Mix全体への作用とParameter ChangeのClick
- `06-global-filter.wav` / `07-global-drive.wav`: Voice Sum後の一回だけの処理とLevel Balance
- `08-delay-impulse.wav`: Echo間隔、Feedback減衰、左右独立、Dry / Wet、Tail
- `09-reverb-impulse.wav`: 初期反射、金属的なRing、Tail、Damping、Width、Mix
- `10-processed-hybrid.wav`: AttackとBodyの一体感、Global Effectの量、楽曲での実用性
- `13-voice-stealing.wav`: Steal Fade、Tail、Note間のState分離
- `processed-hybrid-block-*.wav`: Block SizeによるClickや時間軸の差
- `processed-hybrid-sample-rate-*.wav`: Sample Rateごとの音色と安定性

## Automated checks

- すべてのWAVがStereoでFiniteである
- Block Size 32 / 64 / 257 / 1024の出力差が閾値以内である
- Sample Rate 44.1 / 48 / 96 kHzの出力がFiniteである
- 同じ入力を二度Renderした出力が一致する
- `metrics.json`へ測定値を保存する

音質の判定はMetricsだけでは完了しない。人間の試聴結果を`review-summary.md`へ追記する。
""",
    )


if __name__ == "__main__":
    main()
