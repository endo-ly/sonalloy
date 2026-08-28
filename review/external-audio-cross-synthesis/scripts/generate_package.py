#!/usr/bin/env python3
"""Generate the deterministic external-audio cross-synthesis review package."""

from __future__ import annotations

import copy
import json
import math
import struct
import sys
import wave
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parents[3] / "review" / "generate"
sys.path.insert(0, str(SCRIPT_DIR))

from common import (  # noqa: E402
    BASE_BLOCK_SIZE,
    BLOCK_SIZES,
    ROOT,
    SAMPLE_RATE,
    run_cli,
    sha256_file,
    write_definition,
    write_events,
    write_utf8,
)
from measure_wav import compare_wav, measure, read_float_wav  # noqa: E402


PACKAGE = ROOT / "review" / "external-audio-cross-synthesis"
ASSET_DIR = PACKAGE / "assets"
AUDIO_DIR = PACKAGE / "audio"
DEFINITION_DIR = PACKAGE / "definitions"
EVENT_DIR = PACKAGE / "events"
PATTERN_DIR = PACKAGE / "patterns"
ANALYSIS_DIR = PACKAGE / "analysis"
TRACE_DIR = PACKAGE / "trace"
FRAME_COUNT = SAMPLE_RATE * 2
SAMPLE_RATE_96K = 96_000
FRAME_COUNT_96K = SAMPLE_RATE_96K * 2


def write_pcm16(path: Path, channels: list[list[float]], sample_rate: int) -> None:
    if not channels or any(len(channel) != len(channels[0]) for channel in channels):
        raise ValueError("all WAV channels must have the same non-zero length")
    interleaved = bytearray()
    for frame in range(len(channels[0])):
        for channel in channels:
            sample = max(-1.0, min(1.0, channel[frame]))
            interleaved.extend(struct.pack("<h", round(sample * 30_000.0)))
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(len(channels))
        output.setsampwidth(2)
        output.setframerate(sample_rate)
        output.writeframes(interleaved)


def write_assets(
    *,
    destination: Path = ASSET_DIR,
    sample_rate: int = SAMPLE_RATE,
    frame_count: int = FRAME_COUNT,
    suffix: str = "",
    selected: set[str] | None = None,
) -> dict[str, Path]:
    rhythmic_left: list[float] = []
    sidechain_left: list[float] = []
    speech_left: list[float] = []
    speech_right: list[float] = []
    spectral_left: list[float] = []
    spectral_right: list[float] = []

    for frame in range(frame_count):
        time = frame / sample_rate
        pulse_phase = time % 0.5
        pulse = math.exp(-pulse_phase * 38.0) if pulse_phase < 0.12 else 0.0
        rhythmic = 0.76 * pulse * math.sin(2.0 * math.pi * 150.0 * time)
        rhythmic += 0.08 * math.sin(2.0 * math.pi * 3.0 * time)
        rhythmic_left.append(rhythmic)

        kick_phase = time % 0.5
        kick = math.exp(-kick_phase * 32.0) if kick_phase < 0.16 else 0.0
        kick_frequency = 92.0 - 38.0 * min(kick_phase / 0.16, 1.0)
        sidechain_left.append(0.9 * kick * math.sin(2.0 * math.pi * kick_frequency * time))

        syllable = 0.5 + 0.5 * math.sin(2.0 * math.pi * 2.5 * time - math.pi / 2.0)
        syllable = syllable * syllable
        f1 = 430.0 + 170.0 * (0.5 + 0.5 * math.sin(2.0 * math.pi * 0.7 * time))
        f2 = 1_250.0 + 420.0 * (0.5 + 0.5 * math.sin(2.0 * math.pi * 0.43 * time + 0.8))
        f3 = 2_650.0 + 260.0 * math.sin(2.0 * math.pi * 0.31 * time)
        speech = syllable * (
            0.48 * math.sin(2.0 * math.pi * f1 * time)
            + 0.30 * math.sin(2.0 * math.pi * f2 * time)
            + 0.18 * math.sin(2.0 * math.pi * f3 * time)
        )
        speech_left.append(speech)
        speech_right.append(
            syllable
            * (
                0.46 * math.sin(2.0 * math.pi * (f1 + 35.0) * time + 0.18)
                + 0.30 * math.sin(2.0 * math.pi * (f2 - 80.0) * time + 0.4)
                + 0.18 * math.sin(2.0 * math.pi * (f3 + 110.0) * time + 0.2)
            )
        )

        left_motion = 0.5 * math.sin(2.0 * math.pi * 0.22 * time)
        right_motion = 0.5 * math.sin(2.0 * math.pi * 0.17 * time + 1.1)
        spectral_left.append(
            0.34 * math.sin(2.0 * math.pi * 220.0 * time)
            + 0.22 * math.sin(2.0 * math.pi * (440.0 + 90.0 * left_motion) * time)
            + 0.14 * math.sin(2.0 * math.pi * (880.0 + 130.0 * left_motion) * time)
        )
        spectral_right.append(
            0.34 * math.sin(2.0 * math.pi * 330.0 * time + 0.2)
            + 0.22 * math.sin(2.0 * math.pi * (550.0 + 100.0 * right_motion) * time)
            + 0.14 * math.sin(2.0 * math.pi * (1_100.0 + 160.0 * right_motion) * time)
        )

    assets = {
        "rhythmic-pulse": (rhythmic_left, rhythmic_left),
        "synthetic-speech": (speech_left, speech_right),
        "stereo-spectral-motion": (spectral_left, spectral_right),
        "sidechain-kick": (sidechain_left, sidechain_left),
    }
    paths: dict[str, Path] = {}
    for name, channels in assets.items():
        if selected is not None and name not in selected:
            continue
        asset_name = f"{name}{suffix}"
        path = destination / f"{asset_name}.wav"
        write_pcm16(path, list(channels), sample_rate)
        paths[asset_name] = path
    return paths


def base_definition(name: str, channels: str = "stereo") -> dict[str, object]:
    source = json.loads(
        (ROOT / "testdata" / "instruments" / "basic-poly-synth.json").read_text(
            encoding="utf-8"
        )
    )
    source["metadata"]["name"] = name
    source["metadata"]["author"] = "Sonalloy deterministic review"
    source["metadata"]["description"] = "External audio cross-synthesis review fixture"
    source["external_audio"] = {"channels": channels}
    return source


def follower_source(definition: dict[str, object]) -> None:
    modulation = copy.deepcopy(definition["modulation"])
    modulation["sources"].append(
        {
            "type": "envelope_follower",
            "id": "input_env",
            "attack_ms": 2.0,
            "release_ms": 120.0,
            "input_gain_db": 0.0,
        }
    )
    modulation["routes"].append(
        {
            "source": "input_env",
            "target": "voice.processor.tone.cutoff",
            "depth": {"value": 2.2, "unit": "octaves"},
            "curve": "smooth_step",
        }
    )
    definition["modulation"] = modulation


def processor(kind: str, identifier: str) -> dict[str, object]:
    if kind == "compressor":
        return {
            "type": kind,
            "id": identifier,
            "threshold_db": -24.0,
            "ratio": 6.0,
            "attack_ms": 8.0,
            "release_ms": 180.0,
            "knee_db": 6.0,
            "makeup_gain_db": 0.0,
            "mix": 1.0,
            "detector": "external_audio",
        }
    if kind == "frequency_shifter":
        return {
            "type": kind,
            "id": identifier,
            "shift_hz": 180.0,
            "mix": 1.0,
        }
    if kind == "vocoder":
        return {
            "type": kind,
            "id": identifier,
            "attack_ms": 8.0,
            "release_ms": 80.0,
            "modulator_gain_db": 0.0,
            "output_gain_db": -3.0,
            "mix": 1.0,
        }
    if kind == "envelope_transfer":
        return {
            "type": kind,
            "id": identifier,
            "attack_ms": 2.0,
            "release_ms": 120.0,
            "input_gain_db": 0.0,
            "floor_db": -54.0,
            "mix": 1.0,
        }
    if kind == "spectral_morph":
        return {
            "type": kind,
            "id": identifier,
            "morph": 0.5,
            "output_gain_db": -3.0,
        }
    raise ValueError(f"unknown processor kind: {kind}")


def event_sequence(
    parameter_changes: list[dict[str, object]] | None = None,
    sample_rate: int = SAMPLE_RATE,
) -> list[dict[str, object]]:
    events: list[dict[str, object]] = [
        {
            "absolute_frame": 0,
            "type": "note_on",
            "note_id": 1,
            "note": 60,
            "velocity": 112,
        }
    ]
    if parameter_changes:
        events.extend(parameter_changes)
    events.append(
        {"absolute_frame": sample_rate + sample_rate // 2, "type": "note_off", "note_id": 1}
    )
    return events


def write_pattern(path: Path) -> None:
    write_utf8(
        path,
        json.dumps(
            {
                "schema_version": 1,
                "name": "Full Cross Synthesis",
                "ticks_per_beat": 480,
                "length_ticks": 1920,
                "tempo_changes": [{"tick": 0, "bpm": 120.0}],
                "time_signature_changes": [
                    {"tick": 0, "numerator": 4, "denominator": 4}
                ],
                "events": [
                    {
                        "type": "note",
                        "tick": 0,
                        "duration_ticks": 1440,
                        "note": 60,
                        "velocity": 112,
                    }
                ],
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
    )


def render_external(
    definition: Path,
    event_or_pattern: Path,
    asset: Path,
    output: Path,
    *,
    pattern: bool = False,
    trace: bool = False,
    reset_check: bool = False,
    block_size: int = BASE_BLOCK_SIZE,
    sample_rate: int = SAMPLE_RATE,
    frame_count: int = FRAME_COUNT,
) -> dict[str, object]:
    command = ["render", "pattern" if pattern else "events", str(definition), str(event_or_pattern)]
    command.extend(
        [
            "--audio-input",
            str(asset),
            "--sample-rate",
            str(sample_rate),
            "--block-size",
            str(block_size),
            "--tail",
            "0",
            "--output",
            str(output),
            "--analyze",
            "--json",
        ]
    )
    if not pattern:
        command.extend(["--duration-frames", str(frame_count)])
    if trace:
        command.extend(["--trace", "voice.processor.tone.cutoff", "--trace-every-frames", "480"])
    if reset_check:
        command.append("--reset-check")
    return json.loads(run_cli(command))


def inspect_definition(path: Path) -> dict[str, object]:
    return json.loads(run_cli(["instrument", "inspect", str(path), "--json"]))


def external_consumer(inspect: dict[str, object], identifier: str) -> dict[str, object]:
    external_audio = inspect.get("external_audio")
    if not isinstance(external_audio, dict):
        raise ValueError("definition does not expose external audio inspection")
    consumers = external_audio.get("consumers")
    if not isinstance(consumers, list):
        raise ValueError("external audio inspection does not expose consumers")
    for consumer in consumers:
        if isinstance(consumer, dict) and consumer.get("id") == identifier:
            return consumer
    raise ValueError(f"external consumer is missing: {identifier}")


def spectral_morph_runtime_buffer_bytes(alignment_frames: int) -> int:
    float_count = 3 * 1_024 + 2 * 4_096 + 2 * 1_024
    complex_count = 5 * (1_024 // 2 + 1)
    return float_count * 4 + complex_count * 8 + alignment_frames * 2 * 4


def envelope_correlation(input_path: Path, output_path: Path, window: int = 480) -> float:
    input_rate, input_channels, input_samples = read_float_wav(input_path)
    output_rate, output_channels, output_samples = read_float_wav(output_path)
    if input_rate != output_rate or output_channels != 2:
        raise ValueError("input and output rates or channels are incompatible")
    input_frames = len(input_samples) // input_channels
    output_frames = len(output_samples) // output_channels
    count = min(input_frames, output_frames) // window
    input_values: list[float] = []
    output_values: list[float] = []
    for index in range(count):
        start = index * window
        end = start + window
        input_values.append(
            sum(
                max(
                    abs(input_samples[frame * input_channels + channel])
                    for channel in range(input_channels)
                )
                for frame in range(start, end)
            )
            / window
        )
        output_values.append(
            sum(
                max(
                    abs(output_samples[frame * output_channels]),
                    abs(output_samples[frame * output_channels + 1]),
                )
                for frame in range(start, end)
            )
            / window
        )
    input_mean = sum(input_values) / len(input_values)
    output_mean = sum(output_values) / len(output_values)
    covariance = sum(
        (left - input_mean) * (right - output_mean)
        for left, right in zip(input_values, output_values)
    )
    input_variance = sum((value - input_mean) ** 2 for value in input_values)
    output_variance = sum((value - output_mean) ** 2 for value in output_values)
    denominator = math.sqrt(input_variance * output_variance)
    return covariance / denominator if denominator else 0.0


def main() -> None:
    for directory in (
        ASSET_DIR,
        AUDIO_DIR,
        DEFINITION_DIR,
        EVENT_DIR,
        PATTERN_DIR,
        ANALYSIS_DIR,
        TRACE_DIR,
    ):
        directory.mkdir(parents=True, exist_ok=True)

    assets = write_assets()
    assets.update(
        write_assets(
            sample_rate=SAMPLE_RATE_96K,
            frame_count=FRAME_COUNT_96K,
            suffix="-96k",
            selected={"synthetic-speech"},
        )
    )
    definitions = {
        "envelope-follower-motion": base_definition("Envelope Follower Motion"),
        "sidechain-ducking": base_definition("Sidechain Ducking"),
        "vocoder-speech": base_definition("Vocoder Speech"),
        "vocoder-stereo": base_definition("Vocoder Stereo"),
        "envelope-transfer-rhythm": base_definition("Envelope Transfer Rhythm"),
        "spectral-morph-texture": base_definition("Spectral Morph Texture"),
        "full-cross-synthesis": base_definition("Full Cross Synthesis"),
    }
    follower_source(definitions["envelope-follower-motion"])
    definitions["sidechain-ducking"]["global_processors"] = [
        processor("frequency_shifter", "shift"),
        processor("compressor", "duck"),
    ]
    definitions["vocoder-speech"]["global_processors"] = [processor("vocoder", "speech")]
    definitions["vocoder-stereo"]["global_processors"] = [processor("vocoder", "stereo")]
    definitions["envelope-transfer-rhythm"]["global_processors"] = [
        processor("envelope_transfer", "transfer")
    ]
    definitions["spectral-morph-texture"]["global_processors"] = [
        processor("spectral_morph", "morph")
    ]
    follower_source(definitions["full-cross-synthesis"])
    definitions["full-cross-synthesis"]["global_processors"] = [
        processor("vocoder", "vocoder"),
        processor("spectral_morph", "morph"),
        processor("compressor", "post_morph_duck"),
        {
            "type": "delay",
            "id": "space",
            "time": {"value": 0.18, "unit": "seconds"},
            "feedback_mode": "stereo",
            "feedback": 0.18,
            "taps": [],
            "mix": 0.12,
        },
        {
            "type": "limiter",
            "id": "ceiling",
            "ceiling_db": -1.0,
            "release_ms": 80.0,
            "input_gain_db": -3.0,
        },
    ]

    definition_paths: dict[str, Path] = {}
    for name, definition in definitions.items():
        path = DEFINITION_DIR / f"{name}.json"
        write_definition(path, definition)
        definition_paths[name] = path

    events_by_fixture = {
        "envelope-follower-motion": event_sequence(),
        "sidechain-ducking": event_sequence(),
        "vocoder-speech": event_sequence(),
        "vocoder-stereo": event_sequence(),
        "envelope-transfer-rhythm": event_sequence(),
        "spectral-morph-texture": event_sequence(
            [
                {
                    "absolute_frame": FRAME_COUNT // 3,
                    "type": "parameter_change",
                    "parameter": "global.processor.morph.morph",
                    "native_value": 0.0,
                },
                {
                    "absolute_frame": FRAME_COUNT // 2,
                    "type": "parameter_change",
                    "parameter": "global.processor.morph.morph",
                    "native_value": 0.5,
                },
                {
                    "absolute_frame": FRAME_COUNT * 2 // 3,
                    "type": "parameter_change",
                    "parameter": "global.processor.morph.morph",
                    "native_value": 1.0,
                },
            ]
        ),
        "full-cross-synthesis": event_sequence(),
    }
    event_paths: dict[str, Path] = {}
    for name, events in events_by_fixture.items():
        path = EVENT_DIR / f"{name}.json"
        write_events(path, events)
        event_paths[name] = path
    pattern_path = PATTERN_DIR / "full-cross-synthesis.json"
    write_pattern(pattern_path)
    full_cross_96k_events = EVENT_DIR / "full-cross-synthesis-96k.json"
    write_events(full_cross_96k_events, event_sequence(sample_rate=SAMPLE_RATE_96K))

    render_jobs = {
        "envelope-follower-motion": ("rhythmic-pulse", False, True, False),
        "sidechain-ducking": ("sidechain-kick", False, False, True),
        "vocoder-speech": ("synthetic-speech", False, False, False),
        "vocoder-stereo": ("synthetic-speech", False, False, False),
        "envelope-transfer-rhythm": ("rhythmic-pulse", False, False, False),
        "spectral-morph-texture": ("stereo-spectral-motion", False, False, False),
        "full-cross-synthesis": ("synthetic-speech", True, False, False),
    }
    reports: dict[str, dict[str, object]] = {}
    output_paths: dict[str, Path] = {}
    for name, (asset_name, use_pattern, trace, reset_check) in render_jobs.items():
        output = AUDIO_DIR / f"{name}.wav"
        reports[name] = render_external(
            definition_paths[name],
            pattern_path if use_pattern else event_paths[name],
            assets[asset_name],
            output,
            pattern=use_pattern,
            trace=trace,
            reset_check=reset_check,
        )
        output_paths[name] = output
        write_utf8(
            ANALYSIS_DIR / f"{name}.json",
            json.dumps(reports[name].get("analysis"), ensure_ascii=False, indent=2) + "\n",
        )

    full_cross_96k_output = AUDIO_DIR / "full-cross-synthesis-96k.wav"
    reports["full-cross-synthesis-96k"] = render_external(
        definition_paths["full-cross-synthesis"],
        full_cross_96k_events,
        assets["synthetic-speech-96k"],
        full_cross_96k_output,
        sample_rate=SAMPLE_RATE_96K,
        frame_count=FRAME_COUNT_96K,
    )
    output_paths["full-cross-synthesis-96k"] = full_cross_96k_output
    write_utf8(
        ANALYSIS_DIR / "full-cross-synthesis-96k.json",
        json.dumps(
            reports["full-cross-synthesis-96k"].get("analysis"),
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
    )

    inspect_reports: dict[str, dict[str, object]] = {}
    for name, path in definition_paths.items():
        inspect_reports[name] = inspect_definition(path)
        write_utf8(
            ANALYSIS_DIR / f"{name}-inspect.json",
            json.dumps(inspect_reports[name], ensure_ascii=False, indent=2) + "\n",
        )

    write_utf8(
        TRACE_DIR / "envelope-follower.json",
        json.dumps(reports["envelope-follower-motion"].get("trace"), ensure_ascii=False, indent=2)
        + "\n",
    )

    metrics: dict[str, object] = {
        "package": "external-audio-cross-synthesis",
        "sample_rate": SAMPLE_RATE,
        "sample_rates": [SAMPLE_RATE, SAMPLE_RATE_96K],
        "assets": {},
        "outputs": {},
        "cross_synthesis": {},
        "block_comparison": {},
        "alignment": {},
        "resources": {},
    }
    for name, path in assets.items():
        rate, channels, samples = read_float_wav(path)
        metrics["assets"][name] = {
            "sample_rate": rate,
            "channels": channels,
            "frames": len(samples) // channels,
            "sha256": sha256_file(path),
        }
    for name, output in output_paths.items():
        metrics["outputs"][name] = measure(
            output,
            list(BLOCK_SIZES),
            include_spectrum=True,
        )
    for name, asset_name in (
        ("envelope-follower-motion", "rhythmic-pulse"),
        ("sidechain-ducking", "sidechain-kick"),
        ("envelope-transfer-rhythm", "rhythmic-pulse"),
        ("full-cross-synthesis", "synthetic-speech"),
    ):
        metrics["cross_synthesis"][name] = {
            "input_sha256": metrics["assets"][asset_name]["sha256"],
            "input_channels": metrics["assets"][asset_name]["channels"],
            "input_frames": metrics["assets"][asset_name]["frames"],
            "output_input_envelope_correlation": envelope_correlation(
                assets[asset_name], output_paths[name]
            ),
        }

    alignment_cases = {
        "sidechain-ducking": ("duck", 127),
        "full-cross-synthesis": ("post_morph_duck", 1_024),
    }
    for name, (consumer_id, expected_frames) in alignment_cases.items():
        consumer = external_consumer(inspect_reports[name], consumer_id)
        actual_frames = int(consumer["alignment_frames"])
        if actual_frames != expected_frames:
            raise RuntimeError(
                f"{name} alignment is {actual_frames}, expected {expected_frames}"
            )
        alignment_bytes = actual_frames * 2 * 4
        metrics["alignment"][name] = {
            "consumer": consumer_id,
            "preceding_latency_frames": expected_frames,
            "actual_alignment_frames": actual_frames,
            "stereo_delay_buffer_bytes": alignment_bytes,
        }

    metrics["resources"] = {
        "spectral_morph_runtime_buffer_bytes": spectral_morph_runtime_buffer_bytes(
            0
        ),
        "full_chain_alignment_delay_bytes": metrics["alignment"][
            "full-cross-synthesis"
        ]["stereo_delay_buffer_bytes"],
        "full_chain_audio_thread_allocations_after_prepare": 0,
        "full_chain_allocation_check": {
            "covered_by": "sonalloy-core runtime allocation test",
            "test": "external_cross_synthesis_render_does_not_allocate_after_prepare",
        },
    }

    comparison_directory = PACKAGE / ".generated-block-comparison"
    comparison_directory.mkdir(parents=True, exist_ok=True)
    try:
        reference = output_paths["full-cross-synthesis"]
        comparisons: dict[str, object] = {}
        for block_size in (32, 64, 128):
            candidate = comparison_directory / f"full-cross-synthesis-{block_size}.wav"
            render_external(
                definition_paths["full-cross-synthesis"],
                pattern_path,
                assets["synthetic-speech"],
                candidate,
                pattern=True,
                block_size=block_size,
            )
            comparisons[f"{block_size}_vs_{BASE_BLOCK_SIZE}"] = compare_wav(
                reference, candidate
            )
        metrics["block_comparison"]["full-cross-synthesis"] = comparisons
    finally:
        for candidate in comparison_directory.glob("*.wav"):
            candidate.unlink()
        comparison_directory.rmdir()

    reset = reports["sidechain-ducking"].get("reset_comparison")
    if reset is not None:
        metrics["reset_comparison"] = reset
    write_utf8(
        PACKAGE / "metrics.json",
        json.dumps(metrics, ensure_ascii=False, indent=2) + "\n",
    )

    write_utf8(
        PACKAGE / "SUMMARY.md",
        """# External Audio Cross Synthesis Review

## Automated Review

- 7 fixture definitions were validated and rendered through the CLI external-audio path.
- Generated inputs are deterministic PCM16 WAV files at 48 kHz and 96 kHz. Their frame counts and SHA-256 values are recorded in metrics.json.
- The package records product Analysis and Inspect JSON, Envelope Follower Trace JSON, Full Cross Synthesis block-size differences, Reset comparison data, External alignment, and runtime resource metrics.
- The automated checks cover Envelope Follower, External Sidechain, Vocoder mono/stereo behavior, Envelope Transfer, Spectral Morph startup and parameter stages, a combined chain, and a 96 kHz Full Cross Synthesis render.

## Human Review

未試聴。音質、Speech-like articulation、Sidechainの聴感、Stereo定位、Morphの連続性は、人間が同じ再生条件で試聴して確認してください。
""",
    )
    write_utf8(
        PACKAGE / "README.md",
        """# External Audio Cross Synthesis

このReview Packageは、外部Audioを使う7つの固定条件をCLIから再生成します。assets/は入力、audio/は出力として分離しています。Full Cross Synthesisは48 kHzと96 kHzで生成します。

    python3 review/external-audio-cross-synthesis/scripts/generate_package.py

生成Scriptはreview/generate/common.pyのCLI実行・WAV測定・Block比較を利用します。入力WAVは録音素材ではなく、固定された数式から生成します。analysis/にはCLIのAnalysisとInspect、metrics.jsonには入力整列と固定長Bufferの測定結果を記録します。
""",
    )


if __name__ == "__main__":
    main()
