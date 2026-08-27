#!/usr/bin/env python3
"""Generate the Extended Processing definitions and reproducible measurements."""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
PACKAGE = ROOT / "review" / "extended-processing"
REVIEW_TAIL_SECONDS = "4.0"
sys.path.insert(0, str(ROOT / "review" / "generate"))
from common import run_cli  # noqa: E402
from measure_wav import compare_wav  # noqa: E402


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def asset_reference(path: Path) -> dict[str, object]:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return {"path": f"../assets/{path.name}", "sha256": digest}


def profile(profile_id: str, frequencies: tuple[float, ...]) -> dict[str, object]:
    return {
        "id": profile_id,
        "formants": [
            {"frequency_hz": frequency, "bandwidth_hz": 80.0 + index * 12.0, "gain_db": -index * 3.0}
            for index, frequency in enumerate(frequencies)
        ],
    }


def processor(kind: str, processor_id: str, **fields: object) -> dict[str, object]:
    return {"type": kind, "id": processor_id, **fields}


def base() -> dict[str, object]:
    source = json.loads((ROOT / "testdata" / "instruments" / "basic-poly-synth.json").read_text(encoding="utf-8"))
    source["metadata"] = {
        "name": "Extended Processing Review",
        "author": "Sonalloy",
        "description": "Deterministic review fixture for the extended processor set",
    }
    source["voice_processors"] = []
    source["global_processors"] = []
    source["modulation"] = None
    source["macros"] = []
    source["vectors"] = []
    source["layers"][0]["gain_db"] = -9.0
    source["layers"][0]["envelope"] = {
        "attack_seconds": 0.005,
        "decay_seconds": 0.25,
        "sustain_level": 0.7,
        "release_seconds": 0.35,
    }
    return source


def definitions(body: Path, room: Path) -> dict[str, dict[str, object]]:
    formants = [
        profile("a", (800.0, 1_150.0, 2_900.0, 3_900.0, 4_950.0)),
        profile("i", (350.0, 1_700.0, 2_700.0, 3_700.0, 4_950.0)),
    ]
    ladder = base()
    ladder["metadata"]["name"] = "Ladder Acid Bass"
    ladder["layers"][0]["processors"] = [
        processor("ladder_filter", "acid_filter", cutoff_hz=850.0, resonance=0.72, drive=0.38),
        processor("drive", "edge", amount=0.18, mix=0.3),
    ]
    ladder["macros"] = [{"id": "motion", "name": "Motion", "default": 0.0}]
    ladder["modulation"] = {
        "sources": [],
        "routes": [
            {
                "source": "macro.motion",
                "target": "layer.body.processor.acid_filter.cutoff",
                "depth": {"value": 2.0, "unit": "octaves"},
                "curve": "linear",
            }
        ],
    }

    formant_value = base()
    formant_value["metadata"]["name"] = "Formant Filter Sweep"
    formant_value["layers"][0]["processors"] = [
        processor(
            "formant",
            "vowel_filter",
            vowel_position=0.0,
            formant_shift_cents=0.0,
            throat=0.5,
            profiles=formants,
            mix=0.75,
        )
    ]

    shift = base()
    shift["metadata"]["name"] = "Frequency Shift Bell"
    shift["global_processors"] = [processor("frequency_shifter", "metal_shift", shift_hz=420.0, mix=0.8)]

    convolution = base()
    convolution["metadata"]["name"] = "Convolution Body"
    convolution["global_processors"] = [
        processor("convolution", "body_ir", ir=asset_reference(body), gain_db=-3.0, mix=0.65)
    ]

    gate = base()
    gate["metadata"]["name"] = "Gate Dynamics"
    gate["voice_processors"] = [
        processor(
            "gate",
            "tight_gate",
            threshold_db=-35.0,
            hysteresis_db=4.0,
            attack_ms=2.0,
            hold_ms=35.0,
            release_ms=90.0,
            range_db=-72.0,
        )
    ]

    transient = base()
    transient["metadata"]["name"] = "Transient Drum"
    transient["voice_processors"] = [
        processor("transient_shaper", "drum_punch", attack=0.55, sustain=-0.35, mix=1.0)
    ]

    ping_pong = base()
    ping_pong["metadata"]["name"] = "Tempo Ping-Pong Delay"
    ping_pong["global_processors"] = [
        processor(
            "delay",
            "tempo_echo",
            time={"value": 0.75, "unit": "beats"},
            feedback_mode="ping_pong",
            feedback=0.45,
            taps=[],
            mix=0.35,
        )
    ]

    multi_tap = base()
    multi_tap["metadata"]["name"] = "Multi-Tap Delay"
    multi_tap["global_processors"] = [
        processor(
            "delay",
            "multi_echo",
            time={"value": 0.25, "unit": "beats"},
            feedback_mode="stereo",
            feedback=0.25,
            taps=[
                {"time": {"value": 0.5, "unit": "beats"}, "gain_db": -6.0},
                {"time": {"value": 0.75, "unit": "beats"}, "gain_db": -9.0},
                {"time": {"value": 1.25, "unit": "beats"}, "gain_db": -12.0},
            ],
            mix=0.4,
        )
    ]

    full = base()
    full["metadata"]["name"] = "Full Extended Processing Hybrid"
    full["layers"][0]["processors"] = [
        processor("ladder_filter", "layer_ladder", cutoff_hz=1_200.0, resonance=0.55, drive=0.2),
        processor(
            "formant",
            "layer_vowel",
            vowel_position=0.25,
            formant_shift_cents=0.0,
            throat=0.5,
            profiles=formants,
            mix=0.35,
        ),
    ]
    full["voice_processors"] = [
        processor(
            "gate",
            "voice_gate",
            threshold_db=-45.0,
            hysteresis_db=3.0,
            attack_ms=1.0,
            hold_ms=25.0,
            release_ms=120.0,
            range_db=-48.0,
        ),
        processor("transient_shaper", "voice_shape", attack=0.35, sustain=-0.2, mix=0.7),
        processor(
            "compressor",
            "voice_glue",
            threshold_db=-18.0,
            ratio=2.0,
            attack_ms=10.0,
            release_ms=140.0,
            knee_db=6.0,
            makeup_gain_db=1.0,
            mix=0.65,
        ),
    ]
    full["global_processors"] = [
        processor("frequency_shifter", "shift", shift_hz=180.0, mix=0.25),
        processor(
            "delay",
            "echo",
            time={"value": 0.75, "unit": "beats"},
            feedback_mode="ping_pong",
            feedback=0.35,
            taps=[{"time": {"value": 1.5, "unit": "beats"}, "gain_db": -9.0}],
            mix=0.22,
        ),
        processor("convolution", "room", ir=asset_reference(room), gain_db=-6.0, mix=0.3),
        processor("limiter", "ceiling", ceiling_db=-1.0, release_ms=80.0, input_gain_db=0.0),
    ]
    full["macros"] = [
        {"id": "motion", "name": "Motion", "default": 0.0},
        {"id": "space", "name": "Space", "default": 0.0},
    ]
    full["modulation"] = {
        "sources": [
            {
                "type": "lfo",
                "id": "filter_lfo",
                "waveform": "sine",
                "phase": 0.0,
                "rate": {"value": 0.35, "unit": "per_second"},
            },
            {
                "type": "mseg",
                "id": "vowel_motion",
                "initial_value": 0.0,
                "segments": [
                    {
                        "duration": {"value": 1.0, "unit": "beats"},
                        "target": 1.0,
                        "curve": "smooth_step",
                    },
                    {
                        "duration": {"value": 1.0, "unit": "beats"},
                        "target": 0.0,
                        "curve": "smooth_step",
                    },
                ],
                "loop_range": {"start_segment": 0, "end_segment": 1},
            },
            {
                "type": "envelope",
                "id": "gate_motion",
                "attack_seconds": 0.02,
                "decay_seconds": 0.35,
                "sustain_level": 0.0,
                "release_seconds": 0.2,
            },
        ],
        "routes": [
            {
                "source": "filter_lfo",
                "target": "layer.body.processor.layer_ladder.cutoff",
                "depth": {"value": 1.5, "unit": "octaves"},
                "curve": "linear",
            },
            {
                "source": "vowel_motion",
                "target": "layer.body.processor.layer_vowel.vowel_position",
                "depth": {"value": 1.0, "unit": "normalized"},
                "curve": "linear",
            },
            {
                "source": "gate_motion",
                "target": "voice.processor.voice_gate.threshold_db",
                "depth": {"value": 12.0, "unit": "decibels"},
                "curve": "linear",
            },
            {
                "source": "macro.motion",
                "target": "global.processor.shift.shift_hz",
                "depth": {"value": 800.0, "unit": "hertz"},
                "curve": "linear",
            },
            {
                "source": "macro.space",
                "target": "global.processor.room.mix",
                "depth": {"value": 0.4, "unit": "normalized"},
                "curve": "linear",
            },
            {
                "source": "transport_beat_phase",
                "target": "global.processor.echo.feedback",
                "depth": {"value": 0.15, "unit": "normalized"},
                "curve": "linear",
            },
        ],
    }

    return {
        "ladder-acid-bass": ladder,
        "formant-filter-sweep": formant_value,
        "frequency-shift-bell": shift,
        "convolution-body": convolution,
        "gate-dynamics": gate,
        "transient-drum": transient,
        "tempo-ping-pong-delay": ping_pong,
        "multi-tap-delay": multi_tap,
        "full-extended-processing-hybrid": full,
    }


def main() -> None:
    subprocess.run([sys.executable, str(Path(__file__).with_name("generate_ir.py"))], cwd=ROOT, check=True)
    for directory in ("definitions", "events", "patterns", "analysis", "trace", "audio"):
        (PACKAGE / directory).mkdir(parents=True, exist_ok=True)
    body = PACKAGE / "assets" / "body-short.wav"
    room = PACKAGE / "assets" / "room-medium.wav"
    values = definitions(body, room)
    paths = {}
    for name, value in values.items():
        path = PACKAGE / "definitions" / f"{name}.json"
        write_json(path, value)
        paths[name] = path

    full = paths["full-extended-processing-hybrid"]
    events = {
        "events": [
            {"absolute_frame": 0, "type": "note_on", "note": 48, "velocity": 112, "note_id": 1},
            {"absolute_frame": 12_000, "type": "parameter_change", "parameter": "layer.body.processor.layer_ladder.cutoff", "native_value": 620.0},
            {"absolute_frame": 24_000, "type": "parameter_change", "parameter": "layer.body.processor.layer_vowel.vowel_position", "native_value": 0.75},
            {"absolute_frame": 36_000, "type": "parameter_change", "parameter": "global.processor.shift.shift_hz", "native_value": -240.0},
            {"absolute_frame": 48_000, "type": "parameter_change", "parameter": "global.processor.room.mix", "native_value": 0.5},
            {"absolute_frame": 60_000, "type": "parameter_change", "parameter": "voice.processor.voice_gate.threshold_db", "native_value": -32.0},
            {"absolute_frame": 72_000, "type": "parameter_change", "parameter": "voice.processor.voice_shape.attack", "native_value": -0.25},
            {"absolute_frame": 84_000, "type": "parameter_change", "parameter": "global.processor.echo.feedback", "native_value": 0.2},
            {"absolute_frame": 96_000, "type": "note_off", "note_id": 1},
        ]
    }
    event_path = PACKAGE / "events" / "full-extended-processing.json"
    write_json(event_path, events)
    pattern = ROOT / "review" / "performance-modulation" / "patterns" / "tempo-step-bass.json"
    shutil.copy2(pattern, PACKAGE / "patterns" / "tempo-ping-pong.json")

    analyses: dict[str, object] = {}
    convolution_resources: list[dict[str, object]] = []
    for name, path in paths.items():
        validate = json.loads(run_cli(["instrument", "validate", str(path), "--json"]))
        write_json(PACKAGE / "analysis" / f"{name}-validate.json", validate)
        inspect = json.loads(run_cli(["instrument", "inspect", str(path), "--json"]))
        write_json(PACKAGE / "analysis" / f"{name}-inspect.json", inspect)
        for processor_info in inspect.get("global_processors", []):
            if processor_info.get("kind") != "convolution":
                continue
            static_fields = {
                field["id"]: field["value"] for field in processor_info["static_fields"]
            }
            channels = int(static_fields["source_channels"])
            partitions = int(static_fields["partition_count"])
            complex_bytes = 8
            prepared_bytes = channels * partitions * 512 * complex_bytes
            runtime_bytes = 2 * (partitions * 512 * complex_bytes + 2 * 512 * complex_bytes)
            convolution_resources.append(
                {
                    "fixture": name,
                    "source_channels": channels,
                    "source_frames": int(static_fields["source_frames"]),
                    "prepared_frames": int(static_fields["prepared_frames"]),
                    "partition_count": partitions,
                    "estimated_prepared_ir_bytes": prepared_bytes,
                    "estimated_stereo_runtime_bytes": runtime_bytes,
                }
            )
        render = json.loads(
            run_cli(
                [
                    "render",
                    "note",
                    str(path),
                    "--note",
                    "48",
                    "--velocity",
                    "112",
                    "--gate",
                    "0.12",
                    "--tail",
                    REVIEW_TAIL_SECONDS,
                    "--sample-rate",
                    "48000",
                    "--block-size",
                    "257",
                    "--output",
                    str(PACKAGE / "audio" / f"{name}.wav"),
                    "--analyze",
                    "--json",
                ]
            )
        )
        analyses[name] = render.get("analysis", {})
        write_json(PACKAGE / "analysis" / f"{name}.json", analyses[name])

    trace = json.loads(
        run_cli(
            [
                "render",
                "events",
                str(full),
                str(event_path),
                "--duration-frames",
                "108000",
                "--tempo",
                "120",
                "--sample-rate",
                "48000",
                "--block-size",
                "257",
                "--tail",
                REVIEW_TAIL_SECONDS,
                "--output",
                str(PACKAGE / "audio" / "full-extended-processing-events.wav"),
                "--trace",
                "layer.body.processor.layer_ladder.cutoff",
                "--trace",
                "layer.body.processor.layer_vowel.vowel_position",
                "--trace",
                "global.processor.shift.shift_hz",
                "--trace",
                "global.processor.room.mix",
                "--trace",
                "voice.processor.voice_gate.threshold_db",
                "--trace",
                "voice.processor.voice_shape.attack",
                "--trace",
                "global.processor.echo.feedback",
                "--trace-every-frames",
                "1024",
                "--analyze",
                "--json",
            ]
        )
    )
    write_json(PACKAGE / "trace" / "full-extended-processing.json", trace.get("trace", {}))
    write_json(PACKAGE / "analysis" / "full-extended-processing-events.json", trace.get("analysis", {}))

    block_diffs: dict[str, object] = {}
    sample_rate_matrix: dict[str, object] = {}
    reference = PACKAGE / "audio" / "full-extended-processing-hybrid.wav"
    with tempfile.TemporaryDirectory(prefix="sonalloy-extended-processing-") as temporary:
        temporary_path = Path(temporary)
        for sample_rate in (44_100, 48_000, 96_000):
            for block_size in (32, 64, 128, 257):
                output = temporary_path / f"full-{sample_rate}-{block_size}.wav"
                result = json.loads(
                    run_cli(
                        [
                            "render",
                            "note",
                            str(full),
                            "--note",
                            "48",
                            "--velocity",
                            "112",
                            "--gate",
                            "0.12",
                            "--tail",
                            REVIEW_TAIL_SECONDS,
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
                )
                sample_rate_matrix[f"{sample_rate}/{block_size}"] = result.get("analysis", {})
                if sample_rate == 48_000 and block_size != 257:
                    block_diffs[str(block_size)] = compare_wav(reference, output)
    write_json(
        PACKAGE / "metrics.json",
        {
            "sample_rate": 48_000,
            "block_sizes": [32, 64, 128, 257],
            "fixtures": analyses,
            "full_extended_processing_events": trace.get("analysis", {}),
            "sample_rate_matrix": sample_rate_matrix,
            "full_hybrid_block_differences_from_257": block_diffs,
            "resource_summary": {
                "convolution": convolution_resources,
                "delay": {
                    "max_seconds": 16.0,
                    "stereo_buffer_bytes_per_processor_at_48000": (16 * 48_000 + 4) * 2 * 4,
                    "stereo_buffer_bytes_per_processor_at_96000": (16 * 96_000 + 4) * 2 * 4,
                    "maximum_processors": 4,
                },
            },
        },
    )


if __name__ == "__main__":
    main()
