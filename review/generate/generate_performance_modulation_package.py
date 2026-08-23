#!/usr/bin/env python3
"""Generate the Performance / Modulation sound-review package."""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

from common import ROOT, run_cli
from measure_wav import compare_wav, measure


SAMPLE_RATE = 48_000
SAMPLE_RATES = (44_100, 48_000, 96_000)
BASE_BLOCK_SIZE = 257
BLOCK_SIZES = (32, 64, 128, 257)
TAIL_SECONDS = 0.5


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def run_json(arguments: list[str]) -> dict[str, object]:
    return json.loads(run_cli(arguments))


def render(
    definition: Path,
    output: Path,
    duration_frames: int,
    trace: list[str],
    events: Path | None = None,
    pattern: Path | None = None,
    block_size: int = BASE_BLOCK_SIZE,
    sample_rate: int = SAMPLE_RATE,
) -> dict[str, object]:
    if (events is None) == (pattern is None):
        raise ValueError("exactly one event or pattern input is required")
    if events is not None:
        arguments = [
            "render",
            "events",
            str(definition),
            str(events),
            "--duration-frames",
            str(duration_frames),
        ]
    else:
        arguments = ["render", "pattern", str(definition), str(pattern)]
    arguments.extend(
        [
            "--tail",
            str(TAIL_SECONDS),
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
    for parameter in trace:
        arguments.extend(("--trace", parameter))
    if trace:
        arguments.extend(("--trace-every-frames", "2400"))
    report = run_json(arguments)
    if report.get("status") != "ok":
        raise RuntimeError(f"render did not succeed: {report}")
    return report


def render_case(
    package: Path,
    name: str,
    definition: Path,
    duration_frames: int,
    trace: list[str],
    events: Path | None = None,
    pattern: Path | None = None,
) -> tuple[dict[str, object], dict[str, object]]:
    output = package / "audio" / f"{name}.wav"
    report = render(definition, output, duration_frames, trace, events, pattern)
    write_json(package / "reports" / f"{name}.json", report)
    write_json(package / "analysis" / f"{name}.json", report.get("analysis"))
    write_json(package / "trace" / f"{name}.json", report.get("trace"))

    block_comparison: dict[str, object] = {}
    with tempfile.TemporaryDirectory(prefix="sonalloy-performance-modulation-") as directory:
        reference = output
        for block_size in BLOCK_SIZES:
            candidate = Path(directory) / f"{name}-{block_size}.wav"
            candidate_report = render(
                definition,
                candidate,
                duration_frames,
                trace,
                events,
                pattern,
                block_size,
            )
            block_comparison[str(block_size)] = {
                "frames": candidate_report["frames"],
                "comparison_to_base": compare_wav(reference, candidate),
            }
    technical = measure(output, list(BLOCK_SIZES))
    sample_rate_checks: dict[str, object] = {}
    with tempfile.TemporaryDirectory(prefix="sonalloy-performance-modulation-sample-rate-") as directory:
        for sample_rate in SAMPLE_RATES:
            candidate = Path(directory) / f"{name}-{sample_rate}.wav"
            candidate_duration_frames = round(duration_frames * sample_rate / SAMPLE_RATE)
            candidate_report = render(
                definition,
                candidate,
                candidate_duration_frames,
                [],
                events,
                pattern,
                BASE_BLOCK_SIZE,
                sample_rate,
            )
            analysis = candidate_report["analysis"]
            if (
                not analysis["finite"]
                or analysis["level"]["peak"] <= 0.0
                or analysis["level"]["over_full_scale"]
            ):
                raise RuntimeError(
                    f"sample-rate render failed finite/non-silent/full-scale checks: {candidate_report}"
                )
            sample_rate_checks[str(sample_rate)] = {
                "frames": candidate_report["frames"],
                "duration_seconds": analysis["duration_seconds"],
                "finite": analysis["finite"],
                "non_silent": analysis["level"]["peak"] > 0.0,
                "over_full_scale": analysis["level"]["over_full_scale"],
                "peak": analysis["level"]["peak"],
                "rms": analysis["level"]["rms"],
            }
    technical["sample_rate_checks"] = sample_rate_checks
    technical["block_size_comparison"] = block_comparison
    return report, technical


def main() -> None:
    package = ROOT / "review" / "performance-modulation"
    definitions = package / "definitions"
    events = package / "events"
    patterns = package / "patterns"
    validation: dict[str, object] = {}
    inspections: dict[str, object] = {}
    pattern_validation: dict[str, object] = {}
    pattern_inspections: dict[str, object] = {}
    analyses: dict[str, object] = {}
    technical: dict[str, object] = {}

    cases = [
        ("mono-portamento-lead", "mono-portamento-lead", 168_000, ["layer.lead.tuning"], events / "mono-portamento-lead.json", None),
        ("mseg-motion-pad", "mseg-motion-pad", 168_000, ["voice.processor.tone.cutoff"], events / "mseg-motion-pad.json", None),
        ("random-comparison", "random-comparison", 168_000, ["layer.body.tuning"], events / "random-comparison.json", None),
        ("macro-hybrid", "macro-hybrid", 144_000, ["macro.motion", "voice.processor.tone.cutoff"], events / "macro-hybrid.json", None),
        ("vector-hybrid", "vector-hybrid", 144_000, ["vector.character.x", "layer.analog.gain"], events / "vector-hybrid.json", None),
        ("tempo-step-bass", "tempo-step-bass", 0, ["voice.processor.tone.cutoff"], None, patterns / "tempo-step-bass.json"),
        ("vector-hybrid-pattern", "vector-hybrid", 0, ["vector.character.x", "layer.analog.gain"], None, patterns / "vector-hybrid.json"),
    ]

    for name, definition_name, duration_frames, trace, event_path, pattern_path in cases:
        definition = definitions / f"{definition_name}.json"
        validation[name] = run_json(["instrument", "validate", str(definition), "--json"])
        inspections[name] = run_json(["instrument", "inspect", str(definition), "--json"])
        write_json(package / "validation" / f"{name}.json", validation[name])
        write_json(package / "inspect" / f"{name}.json", inspections[name])
        if pattern_path is not None:
            pattern_validation[name] = run_json(
                ["pattern", "validate", str(pattern_path), "--json"]
            )
            pattern_inspections[name] = run_json(
                ["pattern", "inspect", str(pattern_path), "--json"]
            )
            write_json(
                package / "validation" / f"{name}-pattern.json", pattern_validation[name]
            )
            write_json(
                package / "inspect" / f"{name}-pattern.json", pattern_inspections[name]
            )
        report, metrics = render_case(
            package,
            name,
            definition,
            duration_frames,
            trace,
            events=event_path,
            pattern=pattern_path,
        )
        analyses[name] = report.get("analysis")
        technical[name] = metrics

    write_json(package / "analysis.json", analyses)
    write_json(
        package / "metrics.json",
        {
            "schema_version": 1,
            "sample_rate": SAMPLE_RATE,
            "base_block_size": BASE_BLOCK_SIZE,
            "block_sizes": list(BLOCK_SIZES),
            "validation": validation,
            "pattern_validation": pattern_validation,
            "pattern_inspections": pattern_inspections,
            "technical": technical,
        },
    )


if __name__ == "__main__":
    main()
