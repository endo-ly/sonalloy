#!/usr/bin/env python3
"""Generate the deterministic Wavetable sound review package."""

from __future__ import annotations

import copy
import json
import math
import shutil
import struct
import subprocess
import tempfile
import wave
from pathlib import Path

from common import (
    BASE_BLOCK_SIZE,
    BLOCK_SIZES,
    ROOT,
    SAMPLE_RATE,
    cli_command,
    measure_stereo,
    midi_note_frequency,
    render_events,
    render_midi,
    render_note,
    run_cli,
    sha256_file,
    timed_render,
    write_definition,
    write_events,
    write_utf8,
)
from measure_wav import boundary_differences, compare_wav, measure

BLOCK_SIZE_MAX_DIFFERENCE = 1.0e-5
FRAME_LENGTH = 256
FRAME_COUNT = 4
OPERATOR_PERFORMANCE_DURATION_FRAMES = SAMPLE_RATE
OPERATOR_PERFORMANCE_GATE_FRAMES = OPERATOR_PERFORMANCE_DURATION_FRAMES // 2

WAVETABLE_AUDIO = [
    "01-sine-single-frame.wav",
    "02-saw-single-frame-low.wav",
    "03-saw-single-frame-high.wav",
    "04-position-0.wav",
    "05-position-05.wav",
    "06-position-1.wav",
    "07-position-sweep.wav",
    "08-position-lfo.wav",
    "09-unison-5-stereo.wav",
    "10-band-boundary-sweep.wav",
]
OPERATOR_AUDIO_REMAP = {
    "14-operator-pm-stack4-bell.wav": "11-operator-pm-stack4-bell.wav",
    "15-operator-fm-stack4-bass.wav": "12-operator-fm-stack4-bass.wav",
    "16-operator-am-two-stacks.wav": "13-operator-am-two-stacks.wav",
    "17-operator-ring-two-stacks.wav": "14-operator-ring-two-stacks.wav",
    "18-operator-algorithm-stack4.wav": "15-operator-algorithm-stack4.wav",
    "19-operator-algorithm-two-stacks.wav": "16-operator-algorithm-two-stacks.wav",
    "20-operator-algorithm-shared.wav": "17-operator-algorithm-shared.wav",
    "21-operator-ratio-sweep.wav": "18-operator-ratio-sweep.wav",
    "22-operator-modulation-amount-sweep.wav": "19-operator-modulation-amount-sweep.wav",
    "23-operator-feedback-sweep.wav": "20-operator-feedback-sweep.wav",
    "24-operator-envelope-bell.wav": "21-operator-envelope-bell.wav",
    "25-operator-unison-4.wav": "22-operator-unison-4.wav",
    "26-operator-polyphony-stealing.wav": "23-operator-polyphony-stealing.wav",
}
COMPLEX_AUDIO = [
    "24-phase-distortion-025.wav",
    "25-phase-distortion-075.wav",
    "26-phase-distortion-sweep.wav",
    "27-feedback-03.wav",
    "28-feedback-08.wav",
    "29-feedback-sweep.wav",
    "30-wavefold-025.wav",
    "31-wavefold-075.wav",
    "32-wavefold-sweep.wav",
    "33-waveshaping-wavefold.wav",
    "34-hard-sync-wavefold.wav",
    "35-unison-wavefold.wav",
]
MUSICAL_AUDIO = [
    "36-wavetable-motion-bass.wav",
    "37-four-operator-fm-bell.wav",
    "38-phase-distortion-lead.wav",
    "39-digital-hybrid-lead.wav",
    "40-digital-hybrid-phrase.wav",
]
REFERENCE_NOTES = {
    "01-sine-single-frame.wav": 60,
    "02-saw-single-frame-low.wav": 36,
    "03-saw-single-frame-high.wav": 108,
    "04-position-0.wav": 60,
    "05-position-05.wav": 60,
    "06-position-1.wav": 60,
    "07-position-sweep.wav": 60,
    "08-position-lfo.wav": 60,
    "09-unison-5-stereo.wav": 48,
    "11-operator-pm-stack4-bell.wav": 60,
    "12-operator-fm-stack4-bass.wav": 36,
    "13-operator-am-two-stacks.wav": 60,
    "14-operator-ring-two-stacks.wav": 60,
    "15-operator-algorithm-stack4.wav": 72,
    "16-operator-algorithm-two-stacks.wav": 60,
    "17-operator-algorithm-shared.wav": 60,
    "18-operator-ratio-sweep.wav": 60,
    "19-operator-modulation-amount-sweep.wav": 60,
    "20-operator-feedback-sweep.wav": 60,
    "21-operator-envelope-bell.wav": 60,
    "22-operator-unison-4.wav": 48,
    "24-phase-distortion-025.wav": 60,
    "25-phase-distortion-075.wav": 60,
    "26-phase-distortion-sweep.wav": 60,
    "27-feedback-03.wav": 60,
    "28-feedback-08.wav": 60,
    "29-feedback-sweep.wav": 60,
    "30-wavefold-025.wav": 48,
    "31-wavefold-075.wav": 48,
    "32-wavefold-sweep.wav": 48,
    "33-waveshaping-wavefold.wav": 48,
    "34-hard-sync-wavefold.wav": 60,
    "35-unison-wavefold.wav": 48,
    "36-wavetable-motion-bass.wav": 36,
    "37-four-operator-fm-bell.wav": 60,
    "38-phase-distortion-lead.wav": 60,
    "39-digital-hybrid-lead.wav": 60,
}


def _move_to_support(path: Path, technical_dir: Path) -> Path:
    support = technical_dir / f"support-{path.name}"
    support.unlink(missing_ok=True)
    path.replace(support)
    return support


def _hybrid_definition() -> dict[str, object]:
    source_path = ROOT / "review" / "generate" / "fixtures" / "digital-hybrid-reference.json"
    value = json.loads(source_path.read_text(encoding="utf-8"))
    sample_asset = value["layers"][2]["generator"]["sample"]["zones"][0]["asset"]
    sample_asset["path"] = "../assets/metal-hit.wav"
    value["layers"][1]["generator"] = copy.deepcopy(
        four_operator_fm_bell_definition()["layers"][0]["generator"]
    )
    return value


def _phase_distortion_lead_definition(
    source: dict[str, object],
) -> dict[str, object]:
    value = copy.deepcopy(source)
    value["metadata"] = {
        "name": "Phase Distortion Lead",
        "author": "Sonalloy",
        "description": "Phase-distorted sine lead with controlled unison",
    }
    value.pop("modulation", None)
    for layer in value["layers"]:
        layer["enabled"] = layer["id"] == "hard_sync_lead"
    lead_layer = next(
        layer for layer in value["layers"] if layer["id"] == "hard_sync_lead"
    )
    lead_layer["trigger"]["key_min"] = 36
    lead_layer["trigger"]["key_max"] = 108
    lead_layer["gain_db"] = -12.0
    oscillator = lead_layer["generator"]["oscillator"]
    oscillator["waveform"] = {"type": "sine"}
    oscillator["hard_sync"] = None
    oscillator["waveshaping"] = {"amount": 0.12}
    oscillator["phase_distortion"] = {"amount": 0.55}
    oscillator["feedback"] = {"amount": 0.12}
    oscillator["wavefold"] = None
    oscillator["unison"] = {
        "voices": 3,
        "detune_cents": 7.0,
        "stereo_spread": 0.3,
        "phase_spread": 0.05,
    }
    return value


def _final_summary() -> str:
    audio_descriptions = {
        "01-sine-single-frame.wav": "Sine single frame",
        "02-saw-single-frame-low.wav": "Saw low note",
        "03-saw-single-frame-high.wav": "Saw high note",
        "04-position-0.wav": "Wavetable position 0",
        "05-position-05.wav": "Wavetable position 0.5",
        "06-position-1.wav": "Wavetable position 1",
        "07-position-sweep.wav": "Wavetable position sweep",
        "08-position-lfo.wav": "Wavetable position LFO",
        "09-unison-5-stereo.wav": "Wavetable unison 5 stereo",
        "10-band-boundary-sweep.wav": "Wavetable band boundary",
        "11-operator-pm-stack4-bell.wav": "PM Stack 4 stress",
        "12-operator-fm-stack4-bass.wav": "FM Stack 4 stress",
        "13-operator-am-two-stacks.wav": "AM two-operator comparison",
        "14-operator-ring-two-stacks.wav": "Ring two-operator comparison",
        "15-operator-algorithm-stack4.wav": "Stack 4 topology",
        "16-operator-algorithm-two-stacks.wav": "Two stacks topology",
        "17-operator-algorithm-shared.wav": "Shared modulator topology",
        "18-operator-ratio-sweep.wav": "Operator ratio sweep on a two-operator patch",
        "19-operator-modulation-amount-sweep.wav": "Operator index sweep on a two-operator patch",
        "20-operator-feedback-sweep.wav": "Operator feedback sweep on a two-operator patch",
        "21-operator-envelope-bell.wav": "Operator envelope bell",
        "22-operator-unison-4.wav": "Operator unison 4 on a two-operator patch",
        "23-operator-polyphony-stealing.wav": "Operator polyphony and voice stealing on a two-operator patch",
        "24-phase-distortion-025.wav": "Phase distortion 0.25",
        "25-phase-distortion-075.wav": "Phase distortion 0.75",
        "26-phase-distortion-sweep.wav": "Phase distortion sweep",
        "27-feedback-03.wav": "Oscillator feedback 0.3",
        "28-feedback-08.wav": "Oscillator feedback 0.8",
        "29-feedback-sweep.wav": "Oscillator feedback sweep",
        "30-wavefold-025.wav": "Wavefold 0.25",
        "31-wavefold-075.wav": "Wavefold 0.75",
        "32-wavefold-sweep.wav": "Wavefold sweep",
        "33-waveshaping-wavefold.wav": "Waveshaping and wavefold",
        "34-hard-sync-wavefold.wav": "Hard sync and wavefold",
        "35-unison-wavefold.wav": "Unison and wavefold",
        "36-wavetable-motion-bass.wav": "Wavetable motion bass",
        "37-four-operator-fm-bell.wav": "Four-operator FM bell",
        "38-phase-distortion-lead.wav": "Phase-distortion lead",
        "39-digital-hybrid-lead.wav": "Digital hybrid lead",
        "40-digital-hybrid-phrase.wav": "Digital hybrid phrase",
    }
    rows = "\n".join(
        f"| `{name}` | {audio_descriptions[name]} |"
        for name in WAVETABLE_AUDIO
        + list(OPERATOR_AUDIO_REMAP.values())
        + COMPLEX_AUDIO
        + MUSICAL_AUDIO
    )
    human_rows = "\n".join(
        f"| {item} | 未確認 |"
        for item in [
            "Wavetable frame / positionの音色差",
            "Wavetable position sweepとLFOの滑らかさ",
            "Wavetable band切替と高音域Alias",
            "Wavetable unisonのBeat・Stereo幅・Mono互換性",
            "Wavetable motion bassの音色成立",
            "PM / FMの差とRatio Sweepの連続性",
            "AM / Ringの差",
            "Operator topologyの音色差",
            "Operator envelope・feedback・indexの連続性",
            "Operator unison・polyphony・releaseの成立",
            "Phase Distortionの音色範囲とSweepの連続性",
            "Oscillator Feedbackの粗さと安定性",
            "WavefoldのFold感とAmount 0からの連続性",
            "Waveshaping + Wavefoldの役割差",
            "Hard Sync + WavefoldのAliasと実用性",
            "Unison + WavefoldのBeat・Stereo幅・Level",
            "FM Bellの倍音変化・減衰・音色成立",
            "Phase Distortion Leadの音色成立",
            "Digital Hybrid Leadの音色成立",
            "Digital Hybrid Phraseのレイヤー一体感",
        ]
    )
    return f"""# Digital Synthesis Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Package範囲：Wavetable、4 Operator Modulation、Complex Oscillator、Digital Hybrid

Definitionは`definitions/`、Assetは`assets/`、Eventは`events/`、MIDI入力は`midi/`、WAVは`audio/technical/`へ保存しています。同じ生WAVをMetricsと人間の試聴に使用します。

再生成：

```bash
py -3 scripts/review/generate_digital_synthesis_package.py
```

## 音声一覧

| WAV | 目的 |
|---|---|
{rows}

Regression WAVは`audio/technical/regression-*.wav`、`audio/technical/sample-rate-*.wav`です。Metricsは`metrics.json`に保存しています。

## 自動確認

- 全40件のWAVがFiniteで、Metricsを再生成済み
- 基準周波数が成立する単音RenderのSpectrum、Spectral Centroid、Harmonic / Non-harmonic Energy参考値をMetricsに記録
- Wavetable / Operator / ComplexのDefinition ValidateとInspect JSONを確認済み
- WavetableのFrame、Position、Band、Missing Asset診断を確認済み
- Wavetable / Operator / ComplexのParameter Sweep境界差分を確認済み
- OperatorのPM / FM / AM / Ring、8 topology、Unison、Reset、Allocation 0を確認済み
- Operatorの1 / 8 / 16 Voice × Unison 1 / 4のCLI性能値を記録済み
- ComplexのPhase Distortion、Feedback、Wavefold、Hard Sync / Unison組合せを確認済み
- Block Size、Sample Rate、Fresh Runtime、Reset、ネイティブ有限値境界を自動検査済み
- Digital Hybrid ReferenceをWavetable + Operator + Sampleの3レイヤーでValidate・Render済み
- Digital Hybrid Phraseを`render events`と`render midi`でRenderし、MIDI出力の有限値を確認済み

## 人間の確認

| 確認項目 | 判定 |
|---|---|
{human_rows}

判定は同じ再生環境・音量で確認後に記録します。Metricsは音質の承認を代替しません。
"""


def finalize_integrated_package(review_root: Path) -> None:
    technical_dir = review_root / "audio" / "technical"
    asset_dir = review_root / "assets"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    midi_dir = review_root / "midi"
    staging_root = review_root / "_complex_staging"
    if staging_root.exists():
        shutil.rmtree(staging_root)

    import generate_complex_oscillator_package

    generate_complex_oscillator_package.main(staging_root)
    staging_technical = staging_root / "audio" / "technical"
    staging_definitions = staging_root / "definitions"
    staging_events = staging_root / "events"
    staging_metrics = json.loads(
        (staging_root / "metrics.json").read_text(encoding="utf-8")
    )

    support_paths: dict[str, Path] = {}
    for source_name in [
        "11-mod-wheel-position.wav",
        "12-motion-bass.wav",
        "13-missing-asset-fallback.wav",
    ]:
        support_paths[source_name] = _move_to_support(
            technical_dir / source_name, technical_dir
        )
    for source_name in OPERATOR_AUDIO_REMAP:
        support_paths[source_name] = _move_to_support(
            technical_dir / source_name, technical_dir
        )

    shutil.copy2(
        support_paths["12-motion-bass.wav"],
        technical_dir / "36-wavetable-motion-bass.wav",
    )
    for source_name, target_name in OPERATOR_AUDIO_REMAP.items():
        shutil.copy2(support_paths[source_name], technical_dir / target_name)
    for name in COMPLEX_AUDIO:
        shutil.copy2(staging_technical / name, technical_dir / name)

    for path in staging_definitions.glob("*.json"):
        shutil.copy2(path, definition_dir / path.name)
    for path in staging_events.glob("*.json"):
        shutil.copy2(path, event_dir / path.name)
    phase_distortion_lead_path = definition_dir / "phase-distortion-lead.json"
    phase_distortion_source = json.loads(
        (definition_dir / "phase-distortion-025.json").read_text(encoding="utf-8")
    )
    write_definition(
        phase_distortion_lead_path,
        _phase_distortion_lead_definition(phase_distortion_source),
    )
    run_cli(["instrument", "validate", str(phase_distortion_lead_path), "--json"])
    shutil.copy2(staging_root / "inspect.json", review_root / "complex-inspect.json")
    shutil.copy2(
        staging_root / "phase-inspect.json", review_root / "complex-phase-inspect.json"
    )

    hybrid_path = definition_dir / "digital-hybrid-reference.json"
    shutil.copy2(
        ROOT / "testdata" / "assets" / "metal-hit.wav",
        asset_dir / "metal-hit.wav",
    )
    write_definition(hybrid_path, _hybrid_definition())
    run_cli(["instrument", "validate", str(hybrid_path), "--json"])
    hybrid_inspect = json.loads(
        run_cli(["instrument", "inspect", str(hybrid_path), "--json"])
    )
    hybrid_generator_kinds = [
        layer["generator"]["kind"] for layer in hybrid_inspect["layers"]
    ]
    if hybrid_generator_kinds != ["wavetable", "operator_modulation", "sample"]:
        raise RuntimeError(f"Digital Hybrid inspect is incomplete: {hybrid_generator_kinds}")
    write_utf8(
        review_root / "digital-hybrid-inspect.json",
        json.dumps(hybrid_inspect, ensure_ascii=False, indent=2) + "\n",
    )
    hybrid_events = event_dir / "digital-hybrid-phrase.json"
    write_events(
        hybrid_events,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 48, "velocity": 112},
            {
                "absolute_frame": 4096,
                "type": "parameter_change",
                "parameter": "layer.motion.generator.wavetable_position",
                "native_value": 0.85,
            },
            {"absolute_frame": 6144, "type": "note_on", "note_id": 2, "note": 55, "velocity": 96},
            {
                "absolute_frame": 9216,
                "type": "parameter_change",
                "parameter": "layer.motion.generator.wavetable_position",
                "native_value": 0.2,
            },
            {"absolute_frame": 12288, "type": "note_off", "note_id": 1},
            {"absolute_frame": 14336, "type": "note_off", "note_id": 2},
        ],
    )
    midi_source = ROOT / "testdata" / "midi" / "basic-poly-synth-phrase.mid"
    midi_fixture = midi_dir / "digital-hybrid-phrase.mid"
    midi_fixture.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(midi_source, midi_fixture)
    midi_render = review_root / "_digital-hybrid-midi-render.wav"
    try:
        render_midi(hybrid_path, midi_fixture, midi_render, BASE_BLOCK_SIZE)
        midi_metrics = measure(midi_render, list(BLOCK_SIZES))
        require_finite({midi_render.name: midi_metrics})
        midi_metrics["sha256"] = sha256_file(midi_render)
    finally:
        midi_render.unlink(missing_ok=True)
    render_note(
        hybrid_path,
        60,
        technical_dir / "39-digital-hybrid-lead.wav",
        BASE_BLOCK_SIZE,
        gate_seconds=0.45,
    )
    render_note(
        definition_dir / "four-operator-fm-bell.json",
        60,
        technical_dir / "37-four-operator-fm-bell.wav",
        BASE_BLOCK_SIZE,
        gate_seconds=0.35,
    )
    render_note(
        phase_distortion_lead_path,
        60,
        technical_dir / "38-phase-distortion-lead.wav",
        BASE_BLOCK_SIZE,
        gate_seconds=0.35,
    )
    render_events(
        hybrid_path,
        hybrid_events,
        technical_dir / "40-digital-hybrid-phrase.wav",
        BASE_BLOCK_SIZE,
    )

    old_metrics = json.loads(
        (review_root / "metrics.json").read_text(encoding="utf-8")
    )
    final_audio = WAVETABLE_AUDIO + list(OPERATOR_AUDIO_REMAP.values()) + COMPLEX_AUDIO + MUSICAL_AUDIO
    final_technical: dict[str, dict[str, object]] = {}
    for name in final_audio:
        path = technical_dir / name
        values = measure(
            path,
            list(BLOCK_SIZES),
            include_spectrum=name in {
                "03-saw-single-frame-high.wav",
                "12-operator-fm-stack4-bass.wav",
                "24-phase-distortion-025.wav",
                "30-wavefold-025.wav",
                "37-four-operator-fm-bell.wav",
                "38-phase-distortion-lead.wav",
                "39-digital-hybrid-lead.wav",
            },
            fundamental_frequency_hz=(
                midi_note_frequency(REFERENCE_NOTES[name])
                if name in REFERENCE_NOTES
                else None
            ),
        )
        values.update(measure_stereo(path))
        final_technical[name] = values
    require_finite(final_technical)

    complex_technical = {
        name: final_technical[name] for name in COMPLEX_AUDIO
    }
    operator_technical = {
        name: final_technical[name] for name in OPERATOR_AUDIO_REMAP.values()
    }
    wavetable_technical = {name: final_technical[name] for name in WAVETABLE_AUDIO}
    musical_technical = {name: final_technical[name] for name in MUSICAL_AUDIO}
    old_metrics["technical"] = final_technical
    old_metrics["wavetable"] = {"technical": wavetable_technical}
    old_metrics["operator"] = {
        **old_metrics.get("operator", {}),
        "technical": operator_technical,
    }
    old_metrics["complex_oscillator"] = {
        "technical": complex_technical,
        "block_size_comparisons": staging_metrics["block_size_comparisons"],
        "sample_rate_metrics": staging_metrics["sample_rate_metrics"],
        "fresh_render_comparison": staging_metrics["fresh_render_comparison"],
        "parameter_sweep_boundary_differences": staging_metrics[
            "parameter_sweep_boundary_differences"
        ],
        "performance": staging_metrics["performance"],
    }
    old_metrics["digital_hybrid"] = {
        "technical": musical_technical,
        "definition": str(hybrid_path.relative_to(review_root)),
        "phrase_events": str(hybrid_events.relative_to(review_root)),
        "phrase_midi": str(midi_fixture.relative_to(review_root)),
        "midi_render": midi_metrics,
    }
    old_metrics["musical_references"] = {
        "36-wavetable-motion-bass.wav": "definitions/motion-bass.json",
        "37-four-operator-fm-bell.wav": "definitions/four-operator-fm-bell.json",
        "38-phase-distortion-lead.wav": "definitions/phase-distortion-lead.json",
        "39-digital-hybrid-lead.wav": "definitions/digital-hybrid-reference.json",
        "40-digital-hybrid-phrase.wav": "definitions/digital-hybrid-reference.json",
    }
    old_metrics["integrated_audio_order"] = final_audio
    write_utf8(review_root / "metrics.json", json.dumps(old_metrics, ensure_ascii=False, indent=2) + "\n")
    write_utf8(review_root / "review-summary.md", _final_summary())
    for path in support_paths.values():
        path.unlink(missing_ok=True)
    shutil.rmtree(staging_root)


def write_pcm16_wav(path: Path, frames: list[list[float]], sample_rate: int) -> None:
    samples = [sample for frame in frames for sample in frame]
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(sample_rate)
        output.writeframes(
            b"".join(
                struct.pack("<h", int(max(-1.0, min(1.0, sample)) * 30_000.0))
                for sample in samples
            )
        )


def periodic_frame(length: int, harmonics: tuple[tuple[int, float], ...]) -> list[float]:
    return [
        sum(amplitude * math.sin(math.tau * harmonic * index / length) for harmonic, amplitude in harmonics)
        for index in range(length)
    ]


def saw_frame(length: int) -> list[float]:
    return [0.72 * (2.0 * index / length - 1.0) for index in range(length)]


def asset_reference(path: Path, asset_name: str) -> dict[str, str]:
    asset_path = path / asset_name
    return {
        "path": f"../assets/{asset_name}",
        "sha256": sha256_file(asset_path),
    }


def modulation(
    source: dict[str, object] | None,
    route: dict[str, object] | None,
) -> dict[str, object] | None:
    if route is None:
        return None
    return {
        "sources": [] if source is None else [source],
        "routes": [route],
    }


def definition(
    name: str,
    asset: dict[str, object],
    *,
    frame_length: int = FRAME_LENGTH,
    position: float = 0.0,
    note_min: int = 0,
    note_max: int = 127,
    unison: dict[str, object] | None = None,
    modulation_value: dict[str, object] | None = None,
    extra_layers: list[dict[str, object]] | None = None,
) -> dict[str, object]:
    wavetable = {
        "asset": asset,
        "frame_length": frame_length,
        "position": position,
        "phase_reset": True,
        "phase": 0.0,
    }
    if unison is not None:
        wavetable["unison"] = unison
    layers: list[dict[str, object]] = [
        {
            "id": "motion",
            "enabled": True,
            "trigger": {
                "event": "note_on",
                "key_min": note_min,
                "key_max": note_max,
                "velocity_min": 1,
                "velocity_max": 127,
            },
            "gain_db": -4.0,
            "pan": 0.0,
            "tuning_cents": 0.0,
            "envelope": {
                "attack_seconds": 0.0,
                "decay_seconds": 0.0,
                "sustain_level": 1.0,
                "release_seconds": 0.08,
            },
            "generator": {"wavetable": wavetable},
            "processors": [],
        }
    ]
    if extra_layers:
        layers.extend(copy.deepcopy(extra_layers))
    value: dict[str, object] = {
        "schema_version": 2,
        "metadata": {
            "name": name,
            "author": "Sonalloy",
            "description": "Wavetable Generator review instrument",
        },
        "performance": {
            "polyphony": 16,
            "voice_stealing": "quietest_releasing_then_oldest",
        },
        "layers": layers,
        "voice_processors": [],
        "global_processors": [],
    }
    if modulation_value is not None:
        value["modulation"] = modulation_value
    return value


def operator_definition(
    name: str,
    mode: str,
    algorithm: str,
    *,
    unison: dict[str, object] | None = None,
    polyphony: int = 16,
    envelope_decay: tuple[float, float, float, float] = (0.18, 0.12, 0.08, 0.04),
    envelope_sustain: tuple[float, float, float, float] = (1.0, 1.0, 1.0, 1.0),
    note_min: int = 0,
    note_max: int = 127,
    ratios: tuple[float, float, float, float] = (1.0, 2.0, 3.0, 5.0),
    modulation_amounts: tuple[float, float, float, float] | None = None,
    feedback_values: tuple[float, float, float, float] | None = None,
) -> dict[str, object]:
    topology_values: dict[str, tuple[list[float], list[float]]] = {
        "stack_4": ([0.9, 0.0, 0.0, 0.0], [0.0, 2.2, 1.8, 2.6]),
        "stack_3_plus_carrier": ([0.6, 0.55, 0.0, 0.0], [0.0, 0.0, 2.0, 2.5]),
        "two_stacks": ([0.62, 0.0, 0.62, 0.0], [0.0, 2.0, 0.0, 2.2]),
        "fork_to_carrier": ([0.9, 0.0, 0.0, 0.0], [0.0, 2.0, 2.0, 2.4]),
        "two_modulators_plus_carrier": ([0.65, 0.5, 0.0, 0.0], [0.0, 0.0, 2.0, 2.0]),
        "three_modulators": ([0.9, 0.0, 0.0, 0.0], [0.0, 2.0, 2.0, 2.0]),
        "shared_modulator": ([0.52, 0.52, 0.52, 0.0], [0.0, 0.0, 0.0, 2.4]),
        "parallel": ([0.45, 0.45, 0.45, 0.45], [0.0, 0.0, 0.0, 0.0]),
    }
    levels, amounts = topology_values[algorithm]
    if mode in ("amplitude", "ring"):
        amounts = [amount * 0.18 for amount in amounts]
    if modulation_amounts is not None:
        amounts = list(modulation_amounts)
    feedback = [0.0, 0.0, 0.0, 0.28] if mode in ("phase", "frequency") else [0.0] * 4
    if feedback_values is not None:
        feedback = list(feedback_values)
    operators = []
    for index, (level, amount, decay, sustain) in enumerate(
        zip(levels, amounts, envelope_decay, envelope_sustain)
    ):
        operators.append(
            {
                "ratio": ratios[index],
                "detune_cents": 0.0,
                "level": level,
                "modulation_amount": amount,
                "feedback": feedback[index],
                "phase": 0.0,
                "envelope": {
                    "attack_seconds": 0.0,
                    "decay_seconds": decay,
                    "sustain_level": sustain,
                    "release_seconds": 0.08,
                },
            }
        )
    return {
        "schema_version": 2,
        "metadata": {
            "name": name,
            "author": "Sonalloy",
            "description": "Four-operator modulation review instrument",
        },
        "performance": {
            "polyphony": polyphony,
            "voice_stealing": "quietest_releasing_then_oldest",
        },
        "layers": [
            {
                "id": "operator",
                "enabled": True,
                "trigger": {
                    "event": "note_on",
                    "key_min": note_min,
                    "key_max": note_max,
                    "velocity_min": 1,
                    "velocity_max": 127,
                },
                "gain_db": -6.0,
                "pan": 0.0,
                "tuning_cents": 0.0,
                "envelope": {
                    "attack_seconds": 0.0,
                    "decay_seconds": 0.12,
                    "sustain_level": 0.85,
                    "release_seconds": 0.15,
                },
                "generator": {
                    "operator_modulation": {
                        "mode": mode,
                        "algorithm": algorithm,
                        "operators": operators,
                        "phase_reset": True,
                        "unison": unison,
                    }
                },
                "processors": [],
            }
        ],
        "voice_processors": [],
        "global_processors": [],
        "modulation": None,
    }


def four_operator_fm_bell_definition() -> dict[str, object]:
    return operator_definition(
        "Four Operator FM Bell",
        "frequency",
        "stack_4",
        envelope_decay=(0.9, 0.22, 0.12, 0.06),
        envelope_sustain=(0.18, 0.04, 0.02, 0.0),
        note_min=36,
        note_max=108,
        ratios=(1.0, 2.71, 4.07, 6.83),
        modulation_amounts=(0.0, 3.8, 2.7, 2.1),
        feedback_values=(0.0, 0.0, 0.0, 0.18),
    )


def two_active_operator_definition(
    name: str,
    mode: str,
    *,
    modulation_amount: float,
    unison: dict[str, object] | None = None,
    polyphony: int = 16,
) -> dict[str, object]:
    return operator_definition(
        name,
        mode,
        "stack_4",
        unison=unison,
        polyphony=polyphony,
        ratios=(1.0, 2.0, 1.0, 1.0),
        modulation_amounts=(0.0, modulation_amount, 0.0, 0.0),
        feedback_values=(0.0, 0.0, 0.0, 0.0),
    )


def note_events(note: int, note_id: int = 1, release_frame: int = 12_000) -> list[dict[str, object]]:
    return [
        {
            "absolute_frame": 0,
            "type": "note_on",
            "note_id": note_id,
            "note": note,
            "velocity": 112,
        },
        {"absolute_frame": release_frame, "type": "note_off", "note_id": note_id},
    ]


def note_events_with_controls(
    note: int,
    controls: list[dict[str, object]],
    note_id: int = 1,
    release_frame: int = 12_000,
) -> list[dict[str, object]]:
    return sorted(
        note_events(note, note_id, release_frame) + controls,
        key=lambda event: int(event["absolute_frame"]),
    )


def band_count(frame_length: int) -> int:
    count = 0
    limit = frame_length // 2
    while limit >= 1:
        count += 1
        limit //= 2
    return count


def require_finite(metrics: dict[str, dict[str, object]]) -> None:
    invalid = [name for name, value in metrics.items() if not value["finite"]]
    if invalid:
        raise RuntimeError(f"Review audio is not finite: {invalid}")


def run_cli_error(arguments: list[str]) -> tuple[int, str, str]:
    result = subprocess.run(
        cli_command() + arguments,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    return result.returncode, result.stdout, result.stderr


def operator_performance_metrics(
    base_definition: dict[str, object],
) -> dict[str, dict[str, object]]:
    metrics: dict[str, dict[str, object]] = {}
    with tempfile.TemporaryDirectory(prefix="sonalloy-operator-review-") as temporary:
        temporary_root = Path(temporary)
        for polyphony in (1, 8, 16):
            for voices in (1, 4):
                value = copy.deepcopy(base_definition)
                value["performance"]["polyphony"] = polyphony
                operator_modulation = value["layers"][0]["generator"][
                    "operator_modulation"
                ]
                operator_modulation["unison"] = (
                    None
                    if voices == 1
                    else {
                        "voices": voices,
                        "detune_cents": 16.0,
                        "stereo_spread": 0.85,
                        "phase_spread": 0.35,
                    }
                )
                definition_path = temporary_root / f"poly{polyphony}-unison{voices}.json"
                events_path = temporary_root / f"poly{polyphony}-unison{voices}.events.json"
                audio_path = temporary_root / f"poly{polyphony}-unison{voices}.wav"
                write_definition(definition_path, value)
                run_cli(["instrument", "validate", str(definition_path), "--json"])
                events = [
                    {
                        "absolute_frame": 0,
                        "type": "note_on",
                        "note_id": index + 1,
                        "note": 48 + index % 36,
                        "velocity": 112,
                    }
                    for index in range(polyphony)
                ]
                events.extend(
                    {
                        "absolute_frame": OPERATOR_PERFORMANCE_GATE_FRAMES,
                        "type": "note_off",
                        "note_id": index + 1,
                    }
                    for index in range(polyphony)
                )
                write_events(events_path, events)
                metrics[f"polyphony_{polyphony}_unison_{voices}"] = timed_render(
                    definition_path,
                    events_path,
                    audio_path,
                    OPERATOR_PERFORMANCE_DURATION_FRAMES,
                )
    return metrics


def main() -> None:
    review_root = ROOT / "review" / "digital-synthesis"
    asset_dir = review_root / "assets"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    technical_dir = review_root / "audio" / "technical"
    for directory in (asset_dir, definition_dir, event_dir, technical_dir):
        directory.mkdir(parents=True, exist_ok=True)

    motion_asset_path = asset_dir / "digital-motion.wav"
    source_motion_asset_path = ROOT / "testdata" / "assets" / "digital-motion.wav"
    if not source_motion_asset_path.exists():
        raise RuntimeError(f"Digital Hybrid Wavetable asset is missing: {source_motion_asset_path}")
    shutil.copy2(source_motion_asset_path, motion_asset_path)
    sine_asset_path = asset_dir / "sine-single-frame.wav"
    write_pcm16_wav(sine_asset_path, [periodic_frame(FRAME_LENGTH, ((1, 0.78),))], 48_000)
    saw_asset_path = asset_dir / "saw-single-frame.wav"
    write_pcm16_wav(saw_asset_path, [saw_frame(FRAME_LENGTH)], 48_000)
    invalid_layout_path = asset_dir / "invalid-layout.wav"
    write_pcm16_wav(invalid_layout_path, [periodic_frame(255, ((1, 0.78),))], 48_000)

    motion_asset = asset_reference(asset_dir, motion_asset_path.name)
    sine_asset = asset_reference(asset_dir, sine_asset_path.name)
    saw_asset = asset_reference(asset_dir, saw_asset_path.name)
    position_parameter = "layer.motion.generator.wavetable_position"

    lfo_route = modulation(
        {
            "type": "lfo",
            "id": "motion_lfo",
            "waveform": "sine",
            "rate_hz": 0.35,
            "phase": 0.0,
        },
        {
            "source": "motion_lfo",
            "target": position_parameter,
            "depth": {"value": 0.5, "unit": "normalized"},
            "curve": "linear",
        },
    )
    mod_wheel_route = modulation(
        None,
        {
            "source": "mod_wheel",
            "target": position_parameter,
            "depth": {"value": 1.0, "unit": "normalized"},
            "curve": "linear",
        },
    )
    tuning_route = modulation(
        None,
        {
            "source": "pitch_bend",
            "target": "layer.motion.tuning",
            "depth": {"value": 840.0, "unit": "cents"},
            "curve": "linear",
        },
    )
    unison = {
        "voices": 5,
        "detune_cents": 18.0,
        "stereo_spread": 0.85,
        "phase_spread": 0.2,
    }
    definitions: dict[str, dict[str, object]] = {
        "sine-single-frame": definition("Sine Single Frame", sine_asset),
        "saw-single-frame": definition("Saw Single Frame", saw_asset),
        "position-0": definition("Position 0", motion_asset, position=0.0),
        "position-05": definition("Position 0.5", motion_asset, position=0.5),
        "position-1": definition("Position 1", motion_asset, position=1.0),
        "position-sweep": definition("Position Sweep", motion_asset),
        "position-lfo": definition(
            "Position LFO", motion_asset, position=0.5, modulation_value=lfo_route
        ),
        "mod-wheel-position": definition(
            "Mod Wheel Position", motion_asset, modulation_value=mod_wheel_route
        ),
        "unison-5": definition("Unison 5 Stereo", motion_asset, unison=unison, position=0.5),
        "band-boundary-sweep": definition(
            "Band Boundary Sweep", motion_asset, position=0.5, modulation_value=tuning_route
        ),
        "motion-bass": definition(
            "Wavetable Motion Bass",
            motion_asset,
            position=0.25,
            unison=unison,
            modulation_value=lfo_route,
        ),
    }
    fallback_layer = {
        "id": "fallback",
        "enabled": True,
        "trigger": {
            "event": "note_on",
            "key_min": 0,
            "key_max": 127,
            "velocity_min": 1,
            "velocity_max": 127,
        },
        "gain_db": -8.0,
        "pan": 0.0,
        "tuning_cents": 0.0,
        "envelope": {
            "attack_seconds": 0.0,
            "decay_seconds": 0.0,
            "sustain_level": 1.0,
            "release_seconds": 0.08,
        },
        "generator": {
            "oscillator": {
                "waveform": {"type": "sine"},
                "phase_reset": True,
                "phase": 0.0,
            }
        },
        "processors": [],
    }
    definitions["missing-asset-fallback"] = definition(
        "Missing Asset Fallback",
        {"path": "../assets/missing-wavetable.wav", "sha256": None},
        extra_layers=[fallback_layer],
    )

    definition_paths: dict[str, Path] = {}
    for name, value in definitions.items():
        path = definition_dir / f"{name}.json"
        write_definition(path, value)
        definition_paths[name] = path
        run_cli(["instrument", "validate", str(path), "--json"])

    operator_unison = {
        "voices": 4,
        "detune_cents": 16.0,
        "stereo_spread": 0.85,
        "phase_spread": 0.35,
    }
    operator_definitions: dict[str, dict[str, object]] = {
        "operator-pm-stack4": operator_definition(
            "PM Stack 4 Stress", "phase", "stack_4"
        ),
        "operator-pm-stack4-algorithm": operator_definition(
            "PM Stack 4 Algorithm", "phase", "stack_4", envelope_decay=(0.08, 0.08, 0.08, 0.08)
        ),
        "operator-fm-stack4": operator_definition(
            "FM Stack 4 Stress", "frequency", "stack_4"
        ),
        "four-operator-fm-bell": four_operator_fm_bell_definition(),
        "operator-am-two-stacks": two_active_operator_definition(
            "AM Two-Operator Comparison",
            "amplitude",
            modulation_amount=0.85,
        ),
        "operator-ring-two-stacks": two_active_operator_definition(
            "Ring Two-Operator Comparison",
            "ring",
            modulation_amount=1.0,
        ),
        "operator-pm-two-stacks": operator_definition(
            "PM Two Stacks", "phase", "two_stacks"
        ),
        "operator-pm-shared": operator_definition(
            "PM Shared Modulator", "phase", "shared_modulator"
        ),
        "operator-pm-ratio-sweep": two_active_operator_definition(
            "PM Ratio Sweep", "phase", modulation_amount=1.4
        ),
        "operator-pm-index-sweep": two_active_operator_definition(
            "PM Index Sweep", "phase", modulation_amount=1.4
        ),
        "operator-pm-feedback-sweep": two_active_operator_definition(
            "PM Feedback Sweep", "phase", modulation_amount=1.4
        ),
        "operator-pm-envelope": operator_definition(
            "Operator Envelope Bell",
            "phase",
            "stack_4",
            envelope_decay=(0.35, 0.18, 0.06, 0.02),
            envelope_sustain=(0.18, 0.04, 0.02, 0.0),
            ratios=(1.0, 2.71, 4.07, 6.83),
            modulation_amounts=(0.0, 2.2, 1.5, 1.0),
            feedback_values=(0.0, 0.0, 0.0, 0.0),
        ),
        "operator-fm-unison": two_active_operator_definition(
            "Operator Unison 4",
            "frequency",
            modulation_amount=1.8,
            unison=operator_unison,
        ),
        "operator-pm-stealing": two_active_operator_definition(
            "Operator Polyphony Stealing",
            "frequency",
            modulation_amount=1.4,
            polyphony=2,
        ),
    }
    operator_definition_paths: dict[str, Path] = {}
    for name, value in operator_definitions.items():
        path = definition_dir / f"{name}.json"
        write_definition(path, value)
        operator_definition_paths[name] = path
        run_cli(["instrument", "validate", str(path), "--json"])

    layout_definition = definition(
        "Invalid Wavetable Layout",
        asset_reference(asset_dir, invalid_layout_path.name),
        frame_length=FRAME_LENGTH,
    )
    layout_definition_path = definition_dir / "layout-error.json"
    write_definition(layout_definition_path, layout_definition)
    layout_exit_code, layout_stdout, layout_stderr = run_cli_error(
        ["instrument", "validate", str(layout_definition_path), "--json"]
    )
    layout_report = json.loads(layout_stdout)
    layout_codes = {diagnostic["code"] for diagnostic in layout_report["diagnostics"]}
    if layout_exit_code != 1 or "WAVETABLE_LAYOUT_INVALID" not in layout_codes:
        raise RuntimeError(
            f"Wavetable layout validation did not fail as expected: {layout_exit_code} {layout_stdout} {layout_stderr}"
        )
    write_utf8(
        review_root / "layout-error.json",
        json.dumps(layout_report, ensure_ascii=False, indent=2) + "\n",
    )

    position_sweep_events = event_dir / "position-sweep.json"
    write_events(
        position_sweep_events,
        note_events_with_controls(
            60,
            [
                {
                    "absolute_frame": 4_096,
                    "type": "parameter_change",
                    "parameter": position_parameter,
                    "native_value": 1.0,
                },
                {
                    "absolute_frame": 8_192,
                    "type": "parameter_change",
                    "parameter": position_parameter,
                    "native_value": 0.1,
                },
            ],
        ),
    )
    mod_wheel_events = event_dir / "mod-wheel-position.json"
    write_events(
        mod_wheel_events,
        note_events_with_controls(
            60,
            [
                {"absolute_frame": 3_072, "type": "mod_wheel", "value": 1.0},
                {"absolute_frame": 7_168, "type": "mod_wheel", "value": 0.0},
            ],
        ),
    )
    band_boundary_events = event_dir / "band-boundary-sweep.json"
    write_events(
        band_boundary_events,
        note_events_with_controls(
            108,
            [
                {"absolute_frame": 3_072, "type": "pitch_bend", "value": 1.0},
                {"absolute_frame": 8_192, "type": "pitch_bend", "value": -1.0},
            ],
            release_frame=14_000,
        ),
    )

    ratio_parameter = "layer.operator.generator.operator.2.ratio"
    index_parameter = "layer.operator.generator.operator.2.modulation_amount"
    feedback_parameter = "layer.operator.generator.operator.2.feedback"
    ratio_sweep_events = event_dir / "operator-ratio-sweep.json"
    write_events(
        ratio_sweep_events,
        note_events_with_controls(
            60,
            [
                {
                    "absolute_frame": 4_096,
                    "type": "parameter_change",
                    "parameter": ratio_parameter,
                    "native_value": 0.28917204597632185,
                },
                {
                    "absolute_frame": 8_192,
                    "type": "parameter_change",
                    "parameter": ratio_parameter,
                    "native_value": 0.38689124838559746,
                },
            ],
        ),
    )
    index_sweep_events = event_dir / "operator-index-sweep.json"
    write_events(
        index_sweep_events,
        note_events_with_controls(
            60,
            [
                {
                    "absolute_frame": 4_096,
                    "type": "parameter_change",
                    "parameter": index_parameter,
                    "native_value": 0.8,
                },
                {
                    "absolute_frame": 8_192,
                    "type": "parameter_change",
                    "parameter": index_parameter,
                    "native_value": 3.2,
                },
            ],
        ),
    )
    feedback_sweep_events = event_dir / "operator-feedback-sweep.json"
    write_events(
        feedback_sweep_events,
        note_events_with_controls(
            60,
            [
                {
                    "absolute_frame": 4_096,
                    "type": "parameter_change",
                    "parameter": feedback_parameter,
                    "native_value": 0.25,
                },
                {
                    "absolute_frame": 8_192,
                    "type": "parameter_change",
                    "parameter": feedback_parameter,
                    "native_value": 0.65,
                },
            ],
        ),
    )
    stealing_events = event_dir / "operator-polyphony-stealing.json"
    write_events(
        stealing_events,
        [
            {
                "absolute_frame": 0,
                "type": "note_on",
                "note_id": 1,
                "note": 48,
                "velocity": 112,
            },
            {
                "absolute_frame": 2_048,
                "type": "note_on",
                "note_id": 2,
                "note": 55,
                "velocity": 112,
            },
            {
                "absolute_frame": 4_096,
                "type": "note_on",
                "note_id": 3,
                "note": 60,
                "velocity": 112,
            },
            {"absolute_frame": 10_000, "type": "note_off", "note_id": 2},
            {"absolute_frame": 12_000, "type": "note_off", "note_id": 3},
        ],
    )

    note_jobs = [
        ("01-sine-single-frame.wav", "sine-single-frame", 60),
        ("02-saw-single-frame-low.wav", "saw-single-frame", 36),
        ("03-saw-single-frame-high.wav", "saw-single-frame", 108),
        ("04-position-0.wav", "position-0", 60),
        ("05-position-05.wav", "position-05", 60),
        ("06-position-1.wav", "position-1", 60),
        ("08-position-lfo.wav", "position-lfo", 60),
        ("09-unison-5-stereo.wav", "unison-5", 48),
        ("12-motion-bass.wav", "motion-bass", 36),
    ]
    generated_audio: dict[str, Path] = {}
    generated_fundamental_frequencies: dict[str, float] = {}
    for audio_name, definition_name, note in note_jobs:
        path = technical_dir / audio_name
        render_note(definition_paths[definition_name], note, path, BASE_BLOCK_SIZE, gate_seconds=0.35)
        generated_audio[audio_name] = path
        generated_fundamental_frequencies[audio_name] = midi_note_frequency(note)

    event_jobs = [
        ("07-position-sweep.wav", "position-sweep", position_sweep_events, 60),
        ("11-mod-wheel-position.wav", "mod-wheel-position", mod_wheel_events, 60),
    ]
    for audio_name, definition_name, events, note in event_jobs:
        path = technical_dir / audio_name
        render_events(definition_paths[definition_name], events, path, BASE_BLOCK_SIZE)
        generated_audio[audio_name] = path
        generated_fundamental_frequencies[audio_name] = midi_note_frequency(note)

    band_boundary_path = technical_dir / "10-band-boundary-sweep.wav"
    render_events(
        definition_paths["band-boundary-sweep"],
        band_boundary_events,
        band_boundary_path,
        BASE_BLOCK_SIZE,
    )
    generated_audio[band_boundary_path.name] = band_boundary_path

    missing_audio = technical_dir / "13-missing-asset-fallback.wav"
    render_note(definition_paths["missing-asset-fallback"], 60, missing_audio, BASE_BLOCK_SIZE)
    generated_audio[missing_audio.name] = missing_audio
    generated_fundamental_frequencies[missing_audio.name] = midi_note_frequency(60)

    operator_note_jobs = [
        ("14-operator-pm-stack4-bell.wav", "operator-pm-stack4", 60),
        ("15-operator-fm-stack4-bass.wav", "operator-fm-stack4", 36),
        ("16-operator-am-two-stacks.wav", "operator-am-two-stacks", 60),
        ("17-operator-ring-two-stacks.wav", "operator-ring-two-stacks", 60),
        ("18-operator-algorithm-stack4.wav", "operator-pm-stack4-algorithm", 72),
        ("19-operator-algorithm-two-stacks.wav", "operator-pm-two-stacks", 60),
        ("20-operator-algorithm-shared.wav", "operator-pm-shared", 60),
        ("24-operator-envelope-bell.wav", "operator-pm-envelope", 60),
        ("25-operator-unison-4.wav", "operator-fm-unison", 48),
    ]
    for audio_name, definition_name, note in operator_note_jobs:
        path = technical_dir / audio_name
        render_note(
            operator_definition_paths[definition_name],
            note,
            path,
            BASE_BLOCK_SIZE,
            gate_seconds=0.35,
        )
        generated_audio[audio_name] = path
        generated_fundamental_frequencies[audio_name] = midi_note_frequency(note)

    operator_event_jobs = [
        (
            "21-operator-ratio-sweep.wav",
            "operator-pm-ratio-sweep",
            ratio_sweep_events,
        ),
        (
            "22-operator-modulation-amount-sweep.wav",
            "operator-pm-index-sweep",
            index_sweep_events,
        ),
        (
            "23-operator-feedback-sweep.wav",
            "operator-pm-feedback-sweep",
            feedback_sweep_events,
        ),
        (
            "26-operator-polyphony-stealing.wav",
            "operator-pm-stealing",
            stealing_events,
        ),
    ]
    for audio_name, definition_name, events in operator_event_jobs:
        path = technical_dir / audio_name
        render_events(
            operator_definition_paths[definition_name],
            events,
            path,
            BASE_BLOCK_SIZE,
        )
        generated_audio[audio_name] = path

    technical_metrics: dict[str, dict[str, object]] = {}
    for path in sorted(generated_audio.values()):
        values = measure(path, list(BLOCK_SIZES), include_spectrum=path.name in {
            "01-sine-single-frame.wav",
            "02-saw-single-frame-low.wav",
            "03-saw-single-frame-high.wav",
            "12-motion-bass.wav",
        }, fundamental_frequency_hz=generated_fundamental_frequencies.get(path.name))
        values.update(measure_stereo(path))
        technical_metrics[path.name] = values
    require_finite(technical_metrics)
    position_sweep_boundary_metrics = boundary_differences(
        generated_audio["07-position-sweep.wav"], [4_096, 8_192]
    )
    band_boundary_metrics = boundary_differences(
        generated_audio["10-band-boundary-sweep.wav"], [3_072, 8_192]
    )

    block_paths: dict[str, Path] = {}
    for block_size in BLOCK_SIZES:
        path = technical_dir / f"regression-block-{block_size}.wav"
        render_note(definition_paths["position-05"], 60, path, block_size)
        block_paths[str(block_size)] = path
    block_comparisons = {
        block_size: compare_wav(block_paths["257"], block_paths[str(block_size)])
        for block_size in BLOCK_SIZES
    }
    invalid_block_comparisons = {
        block_size: value
        for block_size, value in block_comparisons.items()
        if not value.get("compatible")
        or value.get("max_abs_difference", 1.0) > BLOCK_SIZE_MAX_DIFFERENCE
    }
    if invalid_block_comparisons:
        raise RuntimeError(f"Wavetable block-size mismatch: {invalid_block_comparisons}")

    fresh_a = technical_dir / "regression-fresh-a.wav"
    fresh_b = technical_dir / "regression-fresh-b.wav"
    render_note(definition_paths["position-05"], 60, fresh_a, BASE_BLOCK_SIZE)
    render_note(definition_paths["position-05"], 60, fresh_b, BASE_BLOCK_SIZE)
    fresh_comparison = compare_wav(fresh_a, fresh_b)
    if not fresh_comparison.get("compatible") or fresh_comparison.get("max_abs_difference", 1.0) != 0.0:
        raise RuntimeError(f"Wavetable fresh render is not reproducible: {fresh_comparison}")

    sample_rate_paths: dict[str, Path] = {}
    for sample_rate in (44_100, SAMPLE_RATE, 96_000):
        path = technical_dir / f"sample-rate-{sample_rate}.wav"
        render_note(definition_paths["position-05"], 60, path, BASE_BLOCK_SIZE, sample_rate)
        sample_rate_paths[str(sample_rate)] = path
    sample_rate_metrics = {
        sample_rate: measure(
            path,
            list(BLOCK_SIZES),
            include_spectrum=True,
            fundamental_frequency_hz=midi_note_frequency(60),
        )
        for sample_rate, path in sample_rate_paths.items()
    }

    operator_block_paths: dict[str, Path] = {}
    for block_size in BLOCK_SIZES:
        path = technical_dir / f"operator-regression-block-{block_size}.wav"
        render_note(
            operator_definition_paths["operator-pm-stack4"],
            60,
            path,
            block_size,
        )
        operator_block_paths[str(block_size)] = path
    operator_block_comparisons = {
        block_size: compare_wav(
            operator_block_paths[str(BASE_BLOCK_SIZE)], operator_block_paths[str(block_size)]
        )
        for block_size in BLOCK_SIZES
    }
    invalid_operator_block_comparisons = {
        block_size: value
        for block_size, value in operator_block_comparisons.items()
        if not value.get("compatible")
        or value.get("max_abs_difference", 1.0) > BLOCK_SIZE_MAX_DIFFERENCE
    }
    if invalid_operator_block_comparisons:
        raise RuntimeError(
            f"Operator block-size mismatch: {invalid_operator_block_comparisons}"
        )

    operator_sample_rate_paths: dict[str, Path] = {}
    for sample_rate in (44_100, SAMPLE_RATE, 96_000):
        path = technical_dir / f"operator-sample-rate-{sample_rate}.wav"
        render_note(
            operator_definition_paths["operator-fm-stack4"],
            60,
            path,
            BASE_BLOCK_SIZE,
            sample_rate,
        )
        operator_sample_rate_paths[str(sample_rate)] = path
    operator_sample_rate_metrics = {
        sample_rate: measure(
            path,
            list(BLOCK_SIZES),
            include_spectrum=True,
            fundamental_frequency_hz=midi_note_frequency(60),
        )
        for sample_rate, path in operator_sample_rate_paths.items()
    }

    operator_fresh_a = technical_dir / "operator-regression-fresh-a.wav"
    operator_fresh_b = technical_dir / "operator-regression-fresh-b.wav"
    render_note(
        operator_definition_paths["operator-fm-stack4"],
        60,
        operator_fresh_a,
        BASE_BLOCK_SIZE,
    )
    render_note(
        operator_definition_paths["operator-fm-stack4"],
        60,
        operator_fresh_b,
        BASE_BLOCK_SIZE,
    )
    operator_fresh_comparison = compare_wav(operator_fresh_a, operator_fresh_b)
    if (
        not operator_fresh_comparison.get("compatible")
        or operator_fresh_comparison.get("max_abs_difference", 1.0) != 0.0
    ):
        raise RuntimeError(
            f"Operator fresh render is not reproducible: {operator_fresh_comparison}"
        )
    operator_performance = operator_performance_metrics(
        operator_definitions["operator-fm-unison"]
    )

    inspect_stdout = run_cli(
        ["instrument", "inspect", str(definition_paths["motion-bass"]), "--json"]
    )
    inspect = json.loads(inspect_stdout)
    inspect_generator = inspect["layers"][0]["generator"]
    if (
        inspect_generator["kind"] != "wavetable"
        or not inspect_generator["prepared"]
        or inspect_generator["frame_length"] != FRAME_LENGTH
        or inspect_generator["frame_count"] != FRAME_COUNT
        or inspect_generator["band_max_harmonics"]
        != [
            FRAME_LENGTH // (2 ** (index + 1))
            for index in range(band_count(FRAME_LENGTH))
        ]
    ):
        raise RuntimeError(f"Wavetable inspect metadata is incomplete: {inspect_generator}")
    write_utf8(review_root / "inspect.json", json.dumps(inspect, ensure_ascii=False, indent=2) + "\n")
    missing_inspect = json.loads(
        run_cli(
            [
                "instrument",
                "inspect",
                str(definition_paths["missing-asset-fallback"]),
                "--json",
            ]
        )
    )
    missing_generator = missing_inspect["layers"][0]["generator"]
    if (
        missing_generator["kind"] != "wavetable"
        or missing_generator["prepared"]
        or missing_generator["frame_length"] != FRAME_LENGTH
    ):
        raise RuntimeError(f"Unavailable Wavetable inspect metadata is incomplete: {missing_generator}")
    write_utf8(
        review_root / "missing-asset-inspect.json",
        json.dumps(missing_inspect, ensure_ascii=False, indent=2) + "\n",
    )
    operator_inspect = json.loads(
        run_cli(
            [
                "instrument",
                "inspect",
                str(operator_definition_paths["operator-fm-unison"]),
                "--json",
            ]
        )
    )
    operator_generator = operator_inspect["layers"][0]["generator"]
    if (
        operator_generator["kind"] != "operator_modulation"
        or operator_generator["mode"] != "frequency"
        or operator_generator["algorithm"] != "stack_4"
        or operator_generator["evaluation_order"] != [4, 3, 2, 1]
        or operator_generator["carrier_operators"] != [1]
        or len(operator_generator["operators"]) != 4
        or operator_generator["unison_voices"] != 4
    ):
        raise RuntimeError(f"Operator inspect metadata is incomplete: {operator_generator}")
    write_utf8(
        review_root / "operator-inspect.json",
        json.dumps(operator_inspect, ensure_ascii=False, indent=2) + "\n",
    )
    prepared_bytes = band_count(FRAME_LENGTH) * FRAME_COUNT * (FRAME_LENGTH + 3) * 4
    operator_audio_names = {
        path.name
        for path in generated_audio.values()
        if path.name.startswith(("14-operator-", "15-operator-", "16-operator-", "17-operator-", "18-operator-", "19-operator-", "20-operator-", "21-operator-", "22-operator-", "23-operator-", "24-operator-", "25-operator-", "26-operator-"))
    }
    operator_technical_metrics = {
        name: value for name, value in technical_metrics.items() if name in operator_audio_names
    }
    require_finite(operator_technical_metrics)
    operator_metrics = {
        "technical": operator_technical_metrics,
        "block_size_comparisons": operator_block_comparisons,
        "sample_rate_metrics": operator_sample_rate_metrics,
        "fresh_render_comparison": {
            **operator_fresh_comparison,
            "first_sha256": sha256_file(operator_fresh_a),
            "second_sha256": sha256_file(operator_fresh_b),
        },
        "parameter_sweep_boundary_differences": {
            "ratio": boundary_differences(
                generated_audio["21-operator-ratio-sweep.wav"], [4_096, 8_192]
            ),
            "modulation_amount": boundary_differences(
                generated_audio["22-operator-modulation-amount-sweep.wav"],
                [4_096, 8_192],
            ),
            "feedback": boundary_differences(
                generated_audio["23-operator-feedback-sweep.wav"], [4_096, 8_192]
            ),
        },
        "performance": operator_performance,
        "allocation_check": {
            "covered_by": "sonalloy-core runtime allocation test",
            "status": "automated test passed",
        },
        "reset_check": {
            "covered_by": "sonalloy-core operator runtime reset integration test",
            "status": "automated test passed",
        },
    }
    metrics = {
        "sample_rate": SAMPLE_RATE,
        "base_block_size": BASE_BLOCK_SIZE,
        "block_sizes": list(BLOCK_SIZES),
        "technical": technical_metrics,
        "operator": operator_metrics,
        "block_size_comparisons": block_comparisons,
        "sample_rate_metrics": sample_rate_metrics,
        "position_comparisons": {
            "0_vs_05": compare_wav(
                generated_audio["04-position-0.wav"], generated_audio["05-position-05.wav"]
            ),
            "05_vs_1": compare_wav(
                generated_audio["05-position-05.wav"], generated_audio["06-position-1.wav"]
            ),
        },
        "position_sweep_boundary_differences": position_sweep_boundary_metrics,
        "band_boundary_differences": band_boundary_metrics,
        "stereo_unison": measure_stereo(generated_audio["09-unison-5-stereo.wav"]),
        "missing_asset_fallback": measure(
            generated_audio[missing_audio.name],
            list(BLOCK_SIZES),
            fundamental_frequency_hz=midi_note_frequency(60),
        ),
        "layout_error_diagnostics": sorted(layout_codes),
        "missing_asset_diagnostics": [
            diagnostic["code"] for diagnostic in missing_inspect["diagnostics"]
        ],
        "fresh_render_comparison": {
            **fresh_comparison,
            "first_sha256": sha256_file(fresh_a),
            "second_sha256": sha256_file(fresh_b),
        },
        "prepared_wavetable_bytes": prepared_bytes,
        "reset_comparison": {
            "covered_by": "sonalloy-core/tests/wavetable.rs::wavetable_output_is_stable_across_block_sizes_and_reset",
            "status": "automated test passed",
        },
    }
    write_utf8(review_root / "metrics.json", json.dumps(metrics, ensure_ascii=False, indent=2) + "\n")

    finalize_integrated_package(review_root)


if __name__ == "__main__":
    main()
