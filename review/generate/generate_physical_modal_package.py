#!/usr/bin/env python3
"""Generate the Physical String / Modal sound review package."""

from __future__ import annotations

import json
import math
import tempfile
from pathlib import Path

from common import (
    BASE_BLOCK_SIZE,
    BLOCK_SIZES,
    ROOT,
    SAMPLE_RATE,
    midi_note_frequency,
    render_events,
    render_note,
    run_cli,
    sha256_file,
    timed_render,
    write_definition,
    write_events,
    write_utf8,
)
from measure_wav import compare_wav, measure, read_float_wav


REVIEW_ROOT = ROOT / "review" / "physical-modal"
DEFINITION_DIR = REVIEW_ROOT / "definitions"
VALIDATION_DIR = REVIEW_ROOT / "validation"
INSPECT_DIR = REVIEW_ROOT / "inspect"
TECHNICAL_DIR = REVIEW_ROOT / "audio" / "technical"
MUSICAL_DIR = REVIEW_ROOT / "audio" / "musical"
EVENT_DIR = REVIEW_ROOT / "events"
TECHNICAL_NOTE = 60
MUSICAL_NOTES = {
    "physical_pluck": 60,
    "modal_mallet": 60,
    "imaginary_metal_body": 48,
}
BLOCK_SIZE_MAX_DIFFERENCE = 1.0e-4
PITCH_ACCURACY_LIMIT_CENTS = 20.0
PITCH_ACCURACY_DEFINITIONS = {"string_impulse", "string_low_stiffness", "physical_pluck"}
PITCH_ACCURACY_FILENAMES = {f"{name}.wav" for name in PITCH_ACCURACY_DEFINITIONS}


def layer(
    layer_id: str,
    generator: dict[str, object],
    *,
    gain_db: float = -10.0,
    pan: float = 0.0,
    trigger_event: str = "note_on",
    release_seconds: float = 0.4,
) -> dict[str, object]:
    return {
        "id": layer_id,
        "enabled": True,
        "trigger": {
            "event": trigger_event,
            "key_min": 0,
            "key_max": 127,
            "velocity_min": 1,
            "velocity_max": 127,
        },
        "gain_db": gain_db,
        "pan": pan,
        "tuning_cents": 0.0,
        "envelope": {
            "attack_seconds": 0.0,
            "decay_seconds": 0.0,
            "sustain_level": 1.0,
            "release_seconds": release_seconds,
        },
        "generator": generator,
        "processors": [],
    }


def instrument(
    name: str,
    layers: list[dict[str, object]],
    *,
    voice_processors: list[dict[str, object]] | None = None,
    global_processors: list[dict[str, object]] | None = None,
) -> dict[str, object]:
    return {
        "schema_version": 2,
        "metadata": {
            "name": name,
            "author": "Sonalloy",
            "description": "Physical String and Modal synthesis review definition",
        },
        "performance": {
            "polyphony": 8,
            "voice_stealing": "quietest_releasing_then_oldest",
        },
        "layers": layers,
        "voice_processors": voice_processors or [],
        "global_processors": global_processors or [],
        "modulation": None,
    }


def physical_string(
    *,
    duration_seconds: float = 0.006,
    exciter_brightness: float = 0.8,
    seed: int = 4001,
    decay_seconds: float = 2.4,
    brightness: float = 0.65,
    stiffness: float = 0.15,
) -> dict[str, object]:
    return {
        "physical_string": {
            "exciter": {
                "type": "noise_burst",
                "duration_seconds": duration_seconds,
                "brightness": exciter_brightness,
                "seed": seed,
            },
            "decay_seconds": decay_seconds,
            "brightness": brightness,
            "stiffness": stiffness,
        }
    }


def modal(
    *,
    mode_count: int = 12,
    structure: float = 0.5,
    brightness: float = 0.65,
    decay: float = 0.65,
    exciter: dict[str, object] | None = None,
) -> dict[str, object]:
    return {
        "modal": {
            "exciter": exciter or {"type": "noise_burst", "duration_seconds": 0.008, "brightness": 0.6, "seed": 9102},
            "mode_count": mode_count,
            "structure": structure,
            "brightness": brightness,
            "decay": decay,
        }
    }


def processor(processor_type: str, processor_id: str, **fields: object) -> dict[str, object]:
    value: dict[str, object] = {"type": processor_type, "id": processor_id}
    value.update(fields)
    return value


def nearest_spectral_peak_error_cents(
    values: dict[str, object], fundamental_frequency_hz: float
) -> float | None:
    spectrum = values.get("spectrum_reference")
    if not isinstance(spectrum, dict):
        return None
    peaks = spectrum.get("peaks")
    if not isinstance(peaks, list):
        return None
    frequencies = [
        float(peak["frequency_hz"])
        for peak in peaks
        if isinstance(peak, dict) and "frequency_hz" in peak
    ]
    if not frequencies:
        return None
    nearest = min(frequencies, key=lambda frequency: abs(frequency - fundamental_frequency_hz))
    fft_size = int(spectrum.get("fft_size", 0))
    sample_rate = float(values.get("sample_rate", 0.0))
    tolerance = max(
        sample_rate / fft_size * 1.5 if fft_size > 0 else 0.0,
        fundamental_frequency_hz * 0.03,
    )
    if abs(nearest - fundamental_frequency_hz) > tolerance:
        return None
    return 1_200.0 * math.log2(nearest / fundamental_frequency_hz)


def autocorrelation_pitch_error_cents(path: Path, fundamental_frequency_hz: float) -> float | None:
    sample_rate, channels, samples = read_float_wav(path)
    if channels == 0 or not samples:
        return None
    left = samples[0::channels]
    expected_lag = sample_rate / fundamental_frequency_hz
    start = sample_rate // 50
    end = min(len(left), start + sample_rate // 2)
    if end - start <= expected_lag * 2.0:
        return None
    window = left[start:end]
    minimum_lag = max(2, round(expected_lag * 0.8))
    maximum_lag = round(expected_lag * 1.2)
    best_correlation = -1.0
    best_lag: int | None = None
    for lag in range(minimum_lag, maximum_lag + 1):
        first = window[:-lag]
        second = window[lag:]
        normalization = math.sqrt(
            sum(sample * sample for sample in first)
            * sum(sample * sample for sample in second)
        )
        if normalization <= 0.0:
            continue
        correlation = (
            sum(
                left_sample * right_sample
                for left_sample, right_sample in zip(first, second)
            )
            / normalization
        )
        if correlation > best_correlation:
            best_correlation = correlation
            best_lag = lag
    if best_lag is None:
        return None
    estimated_frequency = sample_rate / best_lag
    return 1_200.0 * math.log2(estimated_frequency / fundamental_frequency_hz)


def definitions() -> dict[str, dict[str, object]]:
    values: dict[str, dict[str, object]] = {}
    values["string_impulse"] = instrument(
        "Physical String Impulse",
        [layer("string", {"physical_string": {"exciter": {"type": "impulse"}, "decay_seconds": 2.0, "brightness": 0.55, "stiffness": 0.1}})],
    )
    values["string_noise_soft"] = instrument(
        "Physical String Soft Exciter",
        [layer("string", physical_string(exciter_brightness=0.2, seed=4002))],
    )
    values["string_noise_bright"] = instrument(
        "Physical String Bright Exciter",
        [layer("string", physical_string(exciter_brightness=1.0, seed=4003))],
    )
    values["string_short_decay"] = instrument(
        "Physical String Short Decay",
        [layer("string", physical_string(decay_seconds=0.35, seed=4004))],
    )
    values["string_long_decay"] = instrument(
        "Physical String Long Decay",
        [layer("string", physical_string(decay_seconds=5.0, seed=4005))],
    )
    values["string_soft"] = instrument(
        "Physical String Dark Loop",
        [layer("string", physical_string(brightness=0.05, seed=4006))],
    )
    values["string_bright"] = instrument(
        "Physical String Bright Loop",
        [layer("string", physical_string(brightness=1.0, seed=4007))],
    )
    values["string_low_stiffness"] = instrument(
        "Physical String Low Stiffness",
        [layer("string", physical_string(stiffness=0.0, seed=4008))],
    )
    values["string_high_stiffness"] = instrument(
        "Physical String High Stiffness",
        [layer("string", physical_string(stiffness=1.0, seed=4009))],
    )

    values["modal_4_modes"] = instrument(
        "Modal Four Modes",
        [layer("body", modal(mode_count=4))],
    )
    values["modal_8_modes"] = instrument("Modal Eight Modes", [layer("body", modal(mode_count=8))])
    values["modal_12_modes"] = instrument("Modal Twelve Modes", [layer("body", modal(mode_count=12))])
    values["modal_16_modes"] = instrument("Modal Sixteen Modes", [layer("body", modal(mode_count=16))])
    values["modal_20_modes"] = instrument("Modal Twenty Modes", [layer("body", modal(mode_count=20))])
    values["modal_24_modes"] = instrument("Modal Twenty Four Modes", [layer("body", modal(mode_count=24))])
    values["modal_harmonic_structure"] = instrument(
        "Modal Harmonic Structure",
        [layer("body", modal(mode_count=12, structure=0.28, brightness=0.5, decay=0.55))],
    )
    values["modal_stretched_structure"] = instrument(
        "Modal Stretched Structure",
        [layer("body", modal(mode_count=12, structure=0.82, brightness=0.75, decay=0.65))],
    )
    values["modal_dark"] = instrument(
        "Modal Dark Body",
        [layer("body", modal(mode_count=24, brightness=0.05))],
    )
    values["modal_bright"] = instrument(
        "Modal Bright Body",
        [layer("body", modal(mode_count=24, brightness=1.0))],
    )
    values["modal_short_decay"] = instrument(
        "Modal Short Decay",
        [layer("body", modal(mode_count=12, decay=0.05))],
    )
    values["modal_long_decay"] = instrument(
        "Modal Long Decay",
        [layer("body", modal(mode_count=24, decay=1.0))],
    )
    values["modal_impulse"] = instrument(
        "Modal Impulse",
        [layer("body", modal(mode_count=12, exciter={"type": "impulse"}))],
    )
    values["modal_noise_burst"] = instrument(
        "Modal Noise Burst",
        [layer("body", modal(mode_count=12))],
    )

    values["physical_pluck"] = instrument(
        "Physical Pluck",
        [
            layer(
                "string",
                physical_string(
                    duration_seconds=0.004,
                    exciter_brightness=0.8,
                    seed=4201,
                    decay_seconds=2.8,
                    brightness=0.62,
                    stiffness=0.08,
                ),
                gain_db=-7.0,
            )
        ],
        voice_processors=[
            processor("filter", "pluck_tone", mode="low_pass", cutoff_hz=9_000.0, resonance=0.16)
        ],
        global_processors=[
            processor("reverb", "pluck_space", pre_delay_seconds=0.012, decay=0.35, damping=0.45, width=0.8, mix=0.18),
            processor("limiter", "pluck_ceiling", ceiling_db=-1.0, release_ms=80.0, input_gain_db=0.0),
        ],
    )
    values["modal_mallet"] = instrument(
        "Modal Mallet",
        [
            layer(
                "body",
                modal(
                    mode_count=12,
                    structure=0.34,
                    brightness=0.58,
                    decay=0.62,
                    exciter={"type": "noise_burst", "duration_seconds": 0.010, "brightness": 0.42, "seed": 4301},
                ),
                gain_db=-9.0,
            )
        ],
        voice_processors=[
            processor("eq", "mallet_body", low_frequency_hz=180.0, low_gain_db=3.0, mid_frequency_hz=1_100.0, mid_gain_db=-1.5, mid_q=1.0, high_frequency_hz=7_000.0, high_gain_db=-2.0)
        ],
        global_processors=[
            processor("reverb", "mallet_space", pre_delay_seconds=0.008, decay=0.3, damping=0.5, width=0.85, mix=0.16),
            processor("limiter", "mallet_ceiling", ceiling_db=-1.0, release_ms=80.0, input_gain_db=0.0),
        ],
    )
    values["imaginary_metal_body"] = instrument(
        "Imaginary Metal Body",
        [
            layer("string", physical_string(seed=4401, decay_seconds=2.2, brightness=0.9, stiffness=0.78), gain_db=-11.0, pan=-0.08),
            layer("body", modal(mode_count=24, structure=0.78, brightness=0.88, decay=0.72, exciter={"type": "impulse"}), gain_db=-12.0, pan=0.08),
        ],
        voice_processors=[
            processor("filter", "metal_tone", mode="low_pass", cutoff_hz=13_000.0, resonance=0.22),
            processor("drive", "metal_edge", amount=0.12, mix=0.2),
            processor("compressor", "metal_glue", threshold_db=-20.0, ratio=2.5, attack_ms=8.0, release_ms=120.0, knee_db=6.0, makeup_gain_db=1.0, mix=0.7),
        ],
        global_processors=[
            processor("chorus", "metal_width", delay_ms=14.0, rate_hz=0.22, depth=0.35, feedback=0.08, width=0.75, mix=0.16),
            processor("reverb", "metal_space", pre_delay_seconds=0.015, decay=0.42, damping=0.32, width=0.95, mix=0.2),
            processor("limiter", "metal_ceiling", ceiling_db=-1.0, release_ms=80.0, input_gain_db=0.0),
        ],
    )
    return values


def main() -> None:
    for directory in (DEFINITION_DIR, VALIDATION_DIR, INSPECT_DIR, TECHNICAL_DIR, MUSICAL_DIR, EVENT_DIR):
        directory.mkdir(parents=True, exist_ok=True)

    definition_values = definitions()
    definition_paths: dict[str, Path] = {}
    validation_reports: dict[str, object] = {}
    for name, value in definition_values.items():
        path = DEFINITION_DIR / f"{name}.json"
        write_definition(path, value)
        definition_paths[name] = path
        report = json.loads(run_cli(["instrument", "validate", str(path), "--json"]))
        validation_reports[name] = report
        if report.get("status") != "ok":
            raise RuntimeError(f"validation failed for {name}: {report}")
        write_utf8(VALIDATION_DIR / f"{name}.json", json.dumps(report, ensure_ascii=False, indent=2) + "\n")

    for name in ("physical_pluck", "modal_mallet", "imaginary_metal_body"):
        report = run_cli(["instrument", "inspect", str(definition_paths[name]), "--json"])
        write_utf8(INSPECT_DIR / f"{name}.json", report)

    physical_event_path = EVENT_DIR / "physical-modal-parameter-sweep.json"
    write_events(
        physical_event_path,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 1, "note": 48, "velocity": 112},
            {"absolute_frame": 8_000, "type": "parameter_change", "parameter": "layer.string.generator.physical_string_brightness", "native_value": 0.2},
            {"absolute_frame": 12_000, "type": "parameter_change", "parameter": "layer.string.generator.physical_string_stiffness", "native_value": 0.9},
            {"absolute_frame": 16_000, "type": "parameter_change", "parameter": "layer.body.generator.modal_structure", "native_value": 0.35},
            {"absolute_frame": 20_000, "type": "parameter_change", "parameter": "layer.body.generator.modal_decay", "native_value": 0.85},
            {"absolute_frame": 32_000, "type": "note_off", "note_id": 1},
        ],
    )
    performance_event_path = EVENT_DIR / "performance-note.json"
    write_events(
        performance_event_path,
        [
            {"absolute_frame": 0, "type": "note_on", "note_id": 2, "note": 60, "velocity": 112},
            {"absolute_frame": 24_000, "type": "note_off", "note_id": 2},
        ],
    )

    technical_jobs = [
        (name, TECHNICAL_NOTE)
        for name in definition_values
        if name not in MUSICAL_NOTES
    ]
    technical_paths: dict[str, Path] = {}
    for name, note in technical_jobs:
        path = TECHNICAL_DIR / f"{name}.wav"
        render_note(definition_paths[name], note, path, gate_seconds=0.12, tail_seconds=1.0)
        technical_paths[name] = path

    musical_paths: dict[str, Path] = {}
    for name, note in MUSICAL_NOTES.items():
        path = MUSICAL_DIR / f"{name}.wav"
        render_note(definition_paths[name], note, path, gate_seconds=0.18, tail_seconds=2.5)
        musical_paths[name] = path
    sweep_path = MUSICAL_DIR / "imaginary_metal_body-parameter-sweep.wav"
    render_events(definition_paths["imaginary_metal_body"], physical_event_path, sweep_path, BASE_BLOCK_SIZE, duration_frames=48_000)
    musical_paths["imaginary_metal_body-parameter-sweep"] = sweep_path

    all_audio = {**technical_paths, **musical_paths}
    technical_metrics: dict[str, object] = {}
    for name, path in sorted(all_audio.items()):
        note = MUSICAL_NOTES.get(name, TECHNICAL_NOTE)
        technical_metrics[path.name] = measure(
            path,
            list(BLOCK_SIZES),
            include_spectrum=name in {"string_impulse", "string_low_stiffness", "string_bright", "string_high_stiffness", "modal_4_modes", "modal_24_modes", "modal_stretched_structure", "physical_pluck", "modal_mallet", "imaginary_metal_body"},
            fundamental_frequency_hz=midi_note_frequency(note),
        )
        pitch_error = nearest_spectral_peak_error_cents(
            technical_metrics[path.name], midi_note_frequency(note)
        )
        if pitch_error is not None:
            technical_metrics[path.name]["nearest_spectral_peak_error_cents"] = pitch_error
        if name in PITCH_ACCURACY_DEFINITIONS:
            autocorrelation_error = autocorrelation_pitch_error_cents(
                path, midi_note_frequency(note)
            )
            if autocorrelation_error is None:
                raise RuntimeError(f"pitch accuracy could not be estimated for {name}")
            technical_metrics[path.name][
                "autocorrelation_pitch_error_cents"
            ] = autocorrelation_error
            technical_metrics[path.name][
                "pitch_accuracy_limit_cents"
            ] = PITCH_ACCURACY_LIMIT_CENTS
            technical_metrics[path.name]["pitch_accuracy_pass"] = (
                abs(autocorrelation_error) <= PITCH_ACCURACY_LIMIT_CENTS
            )
    invalid_audio = [name for name, values in technical_metrics.items() if not values["finite"]]
    if invalid_audio:
        raise RuntimeError(f"physical/modal audio checks failed: {invalid_audio}")
    invalid_pitch = [
        name
        for name, values in technical_metrics.items()
        if name in PITCH_ACCURACY_FILENAMES
        and not values.get("pitch_accuracy_pass", False)
    ]
    if invalid_pitch:
        raise RuntimeError(f"pitch accuracy checks failed: {invalid_pitch}")

    block_size_comparisons: dict[str, object] = {}
    sample_rate_metrics: dict[str, object] = {}
    fresh_render_comparisons: dict[str, object] = {}
    with tempfile.TemporaryDirectory(prefix="sonalloy-physical-modal-") as temporary:
        temporary_root = Path(temporary)
        for name in MUSICAL_NOTES:
            definition_path = definition_paths[name]
            reference = temporary_root / f"{name}-block-257.wav"
            render_note(definition_path, MUSICAL_NOTES[name], reference, BASE_BLOCK_SIZE, tail_seconds=1.0)
            comparisons: dict[str, object] = {}
            for block_size in BLOCK_SIZES:
                candidate = temporary_root / f"{name}-block-{block_size}.wav"
                render_note(definition_path, MUSICAL_NOTES[name], candidate, block_size, tail_seconds=1.0)
                comparisons[str(block_size)] = compare_wav(reference, candidate)
            invalid_comparisons = {
                block_size: comparison
                for block_size, comparison in comparisons.items()
                if not comparison.get("compatible")
                or float(comparison.get("max_abs_difference", 1.0)) > BLOCK_SIZE_MAX_DIFFERENCE
            }
            if invalid_comparisons:
                raise RuntimeError(f"{name} block-size mismatch: {invalid_comparisons}")
            block_size_comparisons[name] = comparisons

            rates: dict[str, object] = {}
            for sample_rate in (44_100, SAMPLE_RATE, 96_000):
                candidate = temporary_root / f"{name}-rate-{sample_rate}.wav"
                render_note(definition_path, MUSICAL_NOTES[name], candidate, BASE_BLOCK_SIZE, sample_rate, tail_seconds=1.0)
                values = measure(candidate, list(BLOCK_SIZES), include_spectrum=True, fundamental_frequency_hz=midi_note_frequency(MUSICAL_NOTES[name]))
                if not values["finite"]:
                    raise RuntimeError(f"{name} sample-rate render is non-finite at {sample_rate} Hz")
                rates[str(sample_rate)] = values
            sample_rate_metrics[name] = rates

            first = temporary_root / f"{name}-fresh-a.wav"
            second = temporary_root / f"{name}-fresh-b.wav"
            render_note(definition_path, MUSICAL_NOTES[name], first, BASE_BLOCK_SIZE, tail_seconds=1.0)
            render_note(definition_path, MUSICAL_NOTES[name], second, BASE_BLOCK_SIZE, tail_seconds=1.0)
            comparison = compare_wav(first, second)
            if not comparison.get("compatible") or comparison.get("max_abs_difference") != 0.0:
                raise RuntimeError(f"{name} fresh render is not reproducible: {comparison}")
            fresh_render_comparisons[name] = {
                **comparison,
                "first_sha256": sha256_file(first),
                "second_sha256": sha256_file(second),
            }

        performance: dict[str, object] = {}
        for name in ("physical_pluck", "modal_mallet"):
            performance[name] = timed_render(
                definition_paths[name],
                performance_event_path,
                temporary_root / f"{name}-performance.wav",
                duration_frames=48_000,
                block_size=BASE_BLOCK_SIZE,
                release=True,
            )
            if float(performance[name]["realtime_ratio"]) >= 1.0:
                raise RuntimeError(f"{name} is slower than realtime: {performance[name]}")

    metrics = {
        "sample_rate": SAMPLE_RATE,
        "base_block_size": BASE_BLOCK_SIZE,
        "block_sizes": list(BLOCK_SIZES),
        "sample_rates": [44_100, SAMPLE_RATE, 96_000],
        "validation": validation_reports,
        "technical": technical_metrics,
        "block_size_comparisons": block_size_comparisons,
        "sample_rate_metrics": sample_rate_metrics,
        "fresh_render_comparisons": fresh_render_comparisons,
        "performance": performance,
        "audio_sha256": {path.name: sha256_file(path) for path in all_audio.values()},
    }
    write_utf8(REVIEW_ROOT / "metrics.json", json.dumps(metrics, ensure_ascii=False, indent=2) + "\n")

    summary = """# Physical / Modal Sound Review

## Render条件

- 基準Sample Rate：48,000 Hz
- Sample Rate比較：44,100 / 48,000 / 96,000 Hz
- 基準Block Size：257 frames
- 比較Block Size：32 / 64 / 257 / 1024 frames
- Output：Stereo、32-bit float WAV
- Backend：DaisySP V1.0.0 (`a0494a3adb67f549e18dfd71a35fa656f65b38b6`)

## 生成物

`definitions/`にTechnical Definitionと3つのMusical Definition、`validation/`に全DefinitionのCLI Validate JSON、`inspect/`に3つのMusical DefinitionのInspect JSON、`audio/technical/`と`audio/musical/`に同じ生出力を保存しています。`metrics.json`はCLIの`--analyze --json`を基礎にFinite性、Level、DC、Continuity、Spectrum、Block Size、Sample Rate、Fresh Runtime、SHA-256、Performanceを記録し、Stiffness 0のPhysical Stringは時間領域の自己相関でPitch Errorを20 cents以内へ検証します。FFTのNearest Bin値は分解能の参考値として併記します。

再生成：

```bash
python3 review/generate/generate_physical_modal_package.py
```

## Musical Definition

| Definition | 目的 | WAV |
|---|---|---|
| `physical_pluck` | StringのPitch、Natural Decay、Brightness、既存Processorとの組み合わせ | `audio/musical/physical_pluck.wav` |
| `modal_mallet` | Wood / Bar方向のAttack、Mode Density、Body Decay | `audio/musical/modal_mallet.wav` |
| `imaginary_metal_body` | Physical String + Modal + Processorによる架空の金属Body | `audio/musical/imaginary_metal_body.wav` |

Parameter Changeを含むHybridの出力は`audio/musical/imaginary_metal_body-parameter-sweep.wav`です。

## Technical Definition

String：Impulse、Noise BurstのSoft / Bright、Short / Long Decay、Loop Brightness、Low / High Stiffnessを含みます。Modal：4 / 8 / 12 / 16 / 20 / 24 Mode、Harmonic / Stretched Structure、Dark / Bright、Short / Long Decay、Impulse / Noise Burstを含みます。

## 人間の試聴欄

- [ ] StringのPitchがNote間で安定し、Stiffness 0でHarmonic寄りに聞こえる
- [ ] StringのShort / Long Decayが単なる音量差ではなくTailの長さとして聞こえる
- [ ] StringのLoop Brightnessで高域Lossが変化する
- [ ] StringのStiffnessを上げると高次成分が硬く、Metallic方向へ変化する
- [ ] Modalの4 / 12 / 24 Modeで共鳴密度の差が聞こえる
- [ ] ModalのStructureでMode配置のCharacterが変わる
- [ ] ModalのBrightnessで高次Modeの存在感が変わる
- [ ] ModalのDecayで共鳴Tailの長さが変わる
- [ ] `physical_pluck`が撥弦系の実用的な基準音色として成立する
- [ ] `modal_mallet`が木質・棒状のBody方向として成立する
- [ ] `imaginary_metal_body`が既存Processorと混ぜても破綻しない架空音色として成立する
- [ ] Block SizeやSample Rateを変えてClick・Timing差・大きな音色破綻がない

### 人間の回答

- 判定：
- 修正指示：
- 確認者：
- 確認日：
"""
    write_utf8(REVIEW_ROOT / "review-summary.md", summary)


if __name__ == "__main__":
    main()
