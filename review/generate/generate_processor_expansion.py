#!/usr/bin/env python3
"""Generate the deterministic Processor Expansion review package."""

from __future__ import annotations

import copy
import json
import shutil
import tempfile
from pathlib import Path

from common import (
    BLOCK_SIZES,
    SAMPLE_RATE,
    measure_stereo,
    render_events,
    render_note,
    run_cli,
    sha256_file,
    timed_render,
    write_definition,
    write_utf8,
)
from measure_wav import compare_wav, measure

ROOT = Path(__file__).resolve().parents[2]
EVENT_DURATION_FRAMES = 48_000
BLOCK_SIZE_LIMIT = 1.0e-3


def processor(processor_type: str, processor_id: str, **fields: object) -> dict[str, object]:
    value: dict[str, object] = {"type": processor_type, "id": processor_id}
    value.update(fields)
    return value


def load_json(path: Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def noise_layer(
    template: dict[str, object],
    *,
    layer_id: str,
    gain_db: float,
    pan: float,
    seed: int,
    stereo_correlation: float,
) -> dict[str, object]:
    value = copy.deepcopy(template)
    value["id"] = layer_id
    value["gain_db"] = gain_db
    value["pan"] = pan
    value["generator"] = {
        "noise": {
            "color": "white",
            "seed": seed,
            "stereo_correlation": stereo_correlation,
        }
    }
    value["processors"] = []
    return value


def base_instrument() -> dict[str, object]:
    source = load_json(ROOT / "testdata" / "instruments" / "basic-poly-synth.json")
    source["metadata"] = {
        "name": "Processor Expansion Source",
        "author": "Sonalloy",
        "description": "Harmonic source for processor review",
    }
    saw = copy.deepcopy(source["layers"][0])
    saw["id"] = "body"
    saw["gain_db"] = -12.0
    saw["pan"] = -0.12
    saw["processors"] = []
    source["layers"] = [saw]
    source["voice_processors"] = []
    source["global_processors"] = []
    source["modulation"] = None
    return source


def filter_variants() -> dict[str, dict[str, object]]:
    variants: dict[str, dict[str, object]] = {}
    for mode in ("low_pass", "high_pass", "band_pass", "notch"):
        value = base_instrument()
        value["voice_processors"] = [
            processor(
                "filter",
                f"filter_{mode}",
                mode=mode,
                cutoff_hz=1_800.0,
                resonance=0.38,
            )
        ]
        variants[f"filter_{mode}"] = value
    return variants


def eq_variants() -> dict[str, dict[str, object]]:
    settings = {
        "flat": (0.0, 0.0, 0.0),
        "low_boost": (9.0, 0.0, 0.0),
        "mid_cut": (0.0, -10.0, 0.0),
        "high_boost": (0.0, 0.0, 9.0),
        "combined": (5.0, -5.0, 5.0),
    }
    variants: dict[str, dict[str, object]] = {}
    for name, (low, mid, high) in settings.items():
        value = base_instrument()
        value["voice_processors"] = [
            processor(
                "eq",
                f"eq_{name}",
                low_frequency_hz=180.0,
                low_gain_db=low,
                mid_frequency_hz=1_200.0,
                mid_gain_db=mid,
                mid_q=1.1,
                high_frequency_hz=7_000.0,
                high_gain_db=high,
            )
        ]
        variants[f"eq_{name}"] = value
    return variants


def resonator_variants() -> dict[str, dict[str, object]]:
    settings = {
        "220hz": (220.0, 0.55, 0.45),
        "440hz": (440.0, 0.55, 0.45),
        "short_decay": (330.0, 0.12, 0.45),
        "long_decay": (330.0, 2.0, 0.45),
        "dark_damping": (330.0, 0.8, 0.9),
    }
    variants: dict[str, dict[str, object]] = {}
    for name, (frequency, decay, damping) in settings.items():
        value = base_instrument()
        value["voice_processors"] = [
            processor(
                "resonator",
                f"resonator_{name}",
                frequency_hz=frequency,
                decay_seconds=decay,
                damping=damping,
                mix=0.65,
            )
        ]
        variants[f"resonator_{name}"] = value
    return variants


def bitcrusher_variants() -> dict[str, dict[str, object]]:
    settings = {
        "16bit_fullrate": (16.0, 1.0),
        "8bit": (8.0, 1.0),
        "4bit": (4.0, 1.0),
        "quarter_rate": (16.0, 0.25),
        "combined": (6.0, 0.25),
    }
    variants: dict[str, dict[str, object]] = {}
    for name, (bit_depth, ratio) in settings.items():
        value = base_instrument()
        value["layers"][0]["processors"] = [
            processor(
                "bitcrusher",
                f"crusher_{name}",
                bit_depth=bit_depth,
                sample_rate_ratio=ratio,
                mix=0.8,
            )
        ]
        variants[f"bitcrusher_{name}"] = value
    return variants


def modulation_fx_variants() -> dict[str, dict[str, object]]:
    variants: dict[str, dict[str, object]] = {}
    variants["chorus_narrow"] = base_instrument()
    variants["chorus_narrow"]["global_processors"] = [
        processor(
            "chorus",
            "chorus_narrow",
            delay_ms=12.0,
            rate_hz=0.35,
            depth=0.45,
            feedback=0.08,
            width=0.25,
            mix=0.45,
        )
    ]
    variants["chorus_wide"] = base_instrument()
    variants["chorus_wide"]["global_processors"] = [
        processor(
            "chorus",
            "chorus_wide",
            delay_ms=24.0,
            rate_hz=0.2,
            depth=0.8,
            feedback=0.18,
            width=0.9,
            mix=0.5,
        )
    ]
    variants["flanger_feedback"] = base_instrument()
    variants["flanger_feedback"]["global_processors"] = [
        processor(
            "flanger",
            "flanger_feedback",
            delay_ms=2.5,
            rate_hz=0.4,
            depth=0.75,
            feedback=0.72,
            width=0.8,
            mix=0.48,
        )
    ]
    variants["flanger_negative_feedback"] = base_instrument()
    variants["flanger_negative_feedback"]["global_processors"] = [
        processor(
            "flanger",
            "flanger_negative_feedback",
            delay_ms=2.5,
            rate_hz=0.4,
            depth=0.75,
            feedback=-0.72,
            width=0.8,
            mix=0.48,
        )
    ]
    variants["phaser_4stage"] = base_instrument()
    variants["phaser_4stage"]["global_processors"] = [
        processor(
            "phaser",
            "phaser_4stage",
            stages=4,
            center_hz=650.0,
            sweep_octaves=3.0,
            rate_hz=0.28,
            depth=0.85,
            feedback=0.35,
            width=0.85,
            mix=0.55,
        )
    ]
    variants["phaser_8stage"] = base_instrument()
    variants["phaser_8stage"]["global_processors"] = [
        processor(
            "phaser",
            "phaser_8stage",
            stages=8,
            center_hz=900.0,
            sweep_octaves=4.5,
            rate_hz=0.18,
            depth=0.85,
            feedback=-0.35,
            width=0.95,
            mix=0.55,
        )
    ]
    return variants


def dynamics_variants() -> dict[str, dict[str, object]]:
    variants: dict[str, dict[str, object]] = {}
    variants["dynamics_dry"] = base_instrument()
    variants["compressor_gentle"] = base_instrument()
    variants["compressor_gentle"]["global_processors"] = [
        processor(
            "compressor",
            "compressor_gentle",
            threshold_db=-18.0,
            ratio=2.0,
            attack_ms=18.0,
            release_ms=180.0,
            knee_db=8.0,
            makeup_gain_db=2.0,
            mix=1.0,
        )
    ]
    variants["compressor_strong"] = base_instrument()
    variants["compressor_strong"]["global_processors"] = [
        processor(
            "compressor",
            "compressor_strong",
            threshold_db=-30.0,
            ratio=8.0,
            attack_ms=3.0,
            release_ms=70.0,
            knee_db=3.0,
            makeup_gain_db=5.0,
            mix=1.0,
        )
    ]
    variants["compressor_parallel"] = base_instrument()
    variants["compressor_parallel"]["global_processors"] = [
        processor(
            "compressor",
            "compressor_parallel",
            threshold_db=-24.0,
            ratio=6.0,
            attack_ms=5.0,
            release_ms=100.0,
            knee_db=6.0,
            makeup_gain_db=4.0,
            mix=0.35,
        )
    ]
    variants["limiter"] = base_instrument()
    variants["limiter"]["global_processors"] = [
        processor(
            "limiter",
            "limiter",
            ceiling_db=-6.0,
            release_ms=80.0,
            input_gain_db=12.0,
        )
    ]
    return variants


def asset_reference(name: str, asset_dir: Path) -> dict[str, object]:
    asset = asset_dir / name
    return {"path": f"../assets/{name}", "sha256": sha256_file(asset)}


def full_chain_variants(asset_dir: Path) -> dict[str, dict[str, object]]:
    digital = base_instrument()
    digital["metadata"] = {
        "name": "Digital Pad",
        "author": "Sonalloy",
        "description": "Wavetable and additive pad through tone, modulation, space, and dynamics",
    }
    digital["layers"] = [
        {
            **copy.deepcopy(digital["layers"][0]),
            "id": "table",
            "gain_db": -14.0,
            "generator": {
                "wavetable": {
                    "asset": asset_reference("digital-motion.wav", asset_dir),
                    "frame_length": 256,
                    "position": 0.35,
                    "phase_reset": True,
                    "phase": 0.0,
                }
            },
        },
        {
            **copy.deepcopy(digital["layers"][0]),
            "id": "harmonics",
            "gain_db": -22.0,
            "generator": {
                "additive": {
                    "phase_reset": True,
                    "morph": 0.25,
                    "spectrum_tilt_db_per_octave": -4.0,
                    "inharmonicity": 0.02,
                    "partials": [
                        {
                            "id": "fundamental",
                            "ratio": 1.0,
                            "amplitude_a": 1.0,
                            "amplitude_b": 0.9,
                            "phase": 0.0,
                        },
                        {
                            "id": "third",
                            "ratio": 3.0,
                            "amplitude_a": 0.28,
                            "amplitude_b": 0.4,
                            "phase": 0.0,
                        },
                        {
                            "id": "fifth",
                            "ratio": 5.0,
                            "amplitude_a": 0.14,
                            "amplitude_b": 0.24,
                            "phase": 0.0,
                        },
                    ],
                }
            },
        },
    ]
    digital["voice_processors"] = [
        processor(
            "eq",
            "pad_eq",
            low_frequency_hz=180.0,
            low_gain_db=3.0,
            mid_frequency_hz=1_100.0,
            mid_gain_db=-2.0,
            mid_q=1.0,
            high_frequency_hz=7_500.0,
            high_gain_db=3.0,
        )
    ]
    digital["global_processors"] = [
        processor(
            "chorus",
            "pad_chorus",
            delay_ms=20.0,
            rate_hz=0.22,
            depth=0.7,
            feedback=0.12,
            width=0.85,
            mix=0.32,
        ),
        processor(
            "reverb",
            "pad_reverb",
            pre_delay_seconds=0.018,
            decay=0.62,
            damping=0.35,
            width=0.9,
            mix=0.18,
        ),
        processor(
            "compressor",
            "pad_compressor",
            threshold_db=-20.0,
            ratio=2.5,
            attack_ms=14.0,
            release_ms=160.0,
            knee_db=6.0,
            makeup_gain_db=2.0,
            mix=0.75,
        ),
    ]
    digital["modulation"] = None

    metallic_source = load_json(ROOT / "testdata" / "instruments" / "operator-modulation-reference.json")
    metallic = copy.deepcopy(metallic_source)
    metallic["metadata"] = {
        "name": "Metallic Pluck",
        "author": "Sonalloy",
        "description": "Operator modulation and sample attack with resonant and time-based processing",
    }
    metallic["layers"][0]["processors"] = [
        processor(
            "resonator",
            "pluck_ring",
            frequency_hz=440.0,
            decay_seconds=0.45,
            damping=0.4,
            mix=0.35,
        )
    ]
    sample_source = load_json(ROOT / "testdata" / "instruments" / "metallic-hybrid.json")
    sample_layer = copy.deepcopy(sample_source["layers"][0])
    sample_layer["id"] = "attack"
    sample_layer["gain_db"] = -13.0
    sample_layer["generator"]["sample"]["zones"][0]["asset"] = asset_reference(
        "metal-hit.wav", asset_dir
    )
    sample_layer["processors"] = []
    metallic["layers"].append(sample_layer)
    metallic["global_processors"] = [
        processor(
            "phaser",
            "pluck_phaser",
            stages=6,
            center_hz=750.0,
            sweep_octaves=3.5,
            rate_hz=0.35,
            depth=0.7,
            feedback=0.25,
            width=0.8,
            mix=0.3,
        ),
        processor(
            "delay",
            "pluck_delay",
            time={"value": 0.16, "unit": "seconds"},
            feedback_mode="stereo",
            feedback=0.28,
            taps=[],
            mix=0.16,
        ),
        processor("limiter", "pluck_limiter", ceiling_db=-2.0, release_ms=70.0, input_gain_db=2.0),
    ]
    metallic["modulation"] = None

    texture = base_instrument()
    texture["metadata"] = {
        "name": "Lo-fi Texture",
        "author": "Sonalloy",
        "description": "Granular and noise texture through bit reduction, EQ, modulation, and space",
    }
    texture["layers"][0]["id"] = "granular"
    texture["layers"][0]["gain_db"] = -16.0
    texture["layers"][0]["generator"] = {
        "granular": {
            "asset": asset_reference("mono-texture.wav", asset_dir),
            "root_note": 60,
            "region": {"start_seconds": 0.1, "end_seconds": 1.2},
            "position": 0.48,
            "grain_size": 0.07,
            "density": 28.0,
            "pitch": 0.0,
            "randomness": 0.35,
            "pan_spread": 0.85,
            "seed": 9912,
        }
    }
    texture["layers"][0]["processors"] = [
        processor(
            "bitcrusher",
            "texture_crush",
            bit_depth=8.0,
            sample_rate_ratio=0.45,
            mix=0.65,
        )
    ]
    texture["layers"].append(
        noise_layer(
            texture["layers"][0],
            layer_id="noise",
            gain_db=-36.0,
            pan=0.12,
            seed=4812,
            stereo_correlation=0.35,
        )
    )
    texture["global_processors"] = [
        processor(
            "eq",
            "texture_eq",
            low_frequency_hz=160.0,
            low_gain_db=-2.0,
            mid_frequency_hz=1_400.0,
            mid_gain_db=3.0,
            mid_q=1.3,
            high_frequency_hz=6_500.0,
            high_gain_db=-4.0,
        ),
        processor(
            "flanger",
            "texture_flanger",
            delay_ms=3.0,
            rate_hz=0.3,
            depth=0.65,
            feedback=-0.35,
            width=0.9,
            mix=0.28,
        ),
        processor(
            "reverb",
            "texture_reverb",
            pre_delay_seconds=0.01,
            decay=0.68,
            damping=0.55,
            width=0.95,
            mix=0.22,
        ),
    ]
    texture["modulation"] = None
    return {
        "full_chain_digital_pad": digital,
        "full_chain_metallic_pluck": metallic,
        "full_chain_lofi_texture": texture,
    }


def render_event_fixture(path: Path) -> None:
    write_definition(
        path,
        {
            "events": [
                {"absolute_frame": 0, "type": "note_on", "note": 60, "velocity": 112, "note_id": 1},
                {
                    "absolute_frame": 12_000,
                    "type": "parameter_change",
                    "parameter": "global.processor.pad_chorus.mix",
                    "native_value": 0.72,
                },
                {
                    "absolute_frame": 24_000,
                    "type": "parameter_change",
                    "parameter": "global.processor.pad_compressor.threshold_db",
                    "native_value": -22.799999999999997,
                },
                {"absolute_frame": 36_000, "type": "note_off", "note_id": 1},
            ]
        },
    )


def main() -> None:
    review_root = ROOT / "review" / "processor-expansion"
    definition_dir = review_root / "definitions"
    event_dir = review_root / "events"
    audio_dir = review_root / "audio" / "technical"
    inspect_dir = review_root / "inspect"
    asset_dir = review_root / "assets"
    for directory in (definition_dir, event_dir, audio_dir, inspect_dir, asset_dir):
        directory.mkdir(parents=True, exist_ok=True)

    for asset_name in ("digital-motion.wav", "metal-hit.wav", "mono-texture.wav"):
        shutil.copy2(ROOT / "testdata" / "assets" / asset_name, asset_dir / asset_name)

    variants: dict[str, dict[str, object]] = {}
    variants.update(filter_variants())
    variants.update(eq_variants())
    variants.update(resonator_variants())
    variants.update(bitcrusher_variants())
    variants.update(modulation_fx_variants())
    variants.update(dynamics_variants())
    variants.update(full_chain_variants(asset_dir))

    definitions: dict[str, Path] = {}
    for name, value in variants.items():
        path = definition_dir / f"{name}.json"
        write_definition(path, value)
        definitions[name] = path

    event_path = event_dir / "full-chain-digital-events.json"
    render_event_fixture(event_path)

    validation: dict[str, object] = {}
    for name, definition in definitions.items():
        result = json.loads(
            run_cli(["instrument", "validate", str(definition), "--json"])
        )
        if name == "full_chain_digital_pad" and any(
            diagnostic.get("code") == "WAVETABLE_DC_OFFSET"
            for diagnostic in result.get("diagnostics", [])
        ):
            raise RuntimeError("Digital Pad wavetable contains a DC-offset frame")
        validation[name] = result
        write_utf8(
            inspect_dir / f"{name}.json",
            run_cli(["instrument", "inspect", str(definition), "--json"]),
        )

    jobs: dict[str, tuple[Path, str]] = {
        **{f"{name}.wav": (path, "note") for name, path in definitions.items() if not name.startswith("full_chain_")},
        "full_chain_digital_pad.wav": (definitions["full_chain_digital_pad"], "events"),
        "full_chain_metallic_pluck.wav": (definitions["full_chain_metallic_pluck"], "note"),
        "full_chain_lofi_texture.wav": (definitions["full_chain_lofi_texture"], "note"),
    }
    for name, (definition, render_kind) in jobs.items():
        output = audio_dir / name
        if render_kind == "events":
            render_events(
                definition,
                event_path,
                output,
                block_size=257,
                duration_frames=EVENT_DURATION_FRAMES,
                tail_seconds=0.4,
            )
        else:
            render_note(
                definition,
                note=60,
                output=output,
                block_size=257,
                sample_rate=SAMPLE_RATE,
                gate_seconds=0.7,
                tail_seconds=0.4,
            )

    block_outputs: dict[str, Path] = {}
    digital = definitions["full_chain_digital_pad"]
    for block_size in BLOCK_SIZES:
        output = audio_dir / f"full_chain_digital_block_{block_size}.wav"
        render_events(
            digital,
            event_path,
            output,
            block_size=block_size,
            duration_frames=EVENT_DURATION_FRAMES,
            tail_seconds=0.4,
        )
        block_outputs[str(block_size)] = output

    sample_rate_outputs: dict[str, Path] = {}
    for sample_rate in (44_100, 48_000, 96_000):
        output = audio_dir / f"full_chain_digital_sample_rate_{sample_rate}.wav"
        render_note(
            digital,
            note=60,
            output=output,
            block_size=257,
            sample_rate=sample_rate,
            gate_seconds=0.7,
            tail_seconds=0.4,
        )
        sample_rate_outputs[str(sample_rate)] = output

    repeat_path = audio_dir / "full_chain_digital_repeat.wav"
    render_events(
        digital,
        event_path,
        repeat_path,
        block_size=257,
        duration_frames=EVENT_DURATION_FRAMES,
        tail_seconds=0.4,
    )

    audio_metrics = {
        name: {
            **measure(audio_dir / name, list(BLOCK_SIZES)),
            "stereo": measure_stereo(audio_dir / name),
        }
        for name in jobs
    }
    digital_pad_dc = abs(audio_metrics["full_chain_digital_pad.wav"]["dc"])
    if digital_pad_dc > 1.0e-3:
        raise RuntimeError(f"Digital Pad render has excessive DC offset: {digital_pad_dc}")
    block_comparison = {
        block_size: compare_wav(block_outputs["257"], block_outputs[block_size])
        for block_size in map(str, BLOCK_SIZES)
    }
    for block_size, comparison in block_comparison.items():
        if (
            not comparison.get("compatible")
            or comparison.get("max_abs_difference", 1.0) > BLOCK_SIZE_LIMIT
        ):
            raise RuntimeError(f"block-size mismatch at {block_size}: {comparison}")

    reset_comparison = compare_wav(audio_dir / "full_chain_digital_pad.wav", repeat_path)
    if (
        not reset_comparison.get("compatible")
        or reset_comparison.get("max_abs_difference", 1.0) != 0.0
    ):
        raise RuntimeError(f"full-chain render is not reproducible: {reset_comparison}")

    filter_reference = audio_dir / "filter_low_pass.wav"
    filter_comparison = {
        name.removeprefix("filter_").removesuffix(".wav"): compare_wav(
            filter_reference, audio_dir / name
        )
        for name in jobs
        if name.startswith("filter_") and name != "filter_low_pass.wav"
    }

    with tempfile.TemporaryDirectory(prefix="sonalloy-processor-review-") as temporary:
        performance = timed_render(
            digital,
            event_path,
            Path(temporary) / "full-chain-performance.wav",
            duration_frames=EVENT_DURATION_FRAMES,
            block_size=257,
            sample_rate=SAMPLE_RATE,
            release=True,
        )

    metrics = {
        "sample_rate": SAMPLE_RATE,
        "event_duration_frames": EVENT_DURATION_FRAMES,
        "definition_count": len(definitions),
        "validation": validation,
        "audio": audio_metrics,
        "filter_mode_comparison_to_low_pass": filter_comparison,
        "block_size_comparison": block_comparison,
        "sample_rate_audio": {
            rate: measure(path, list(BLOCK_SIZES))
            for rate, path in sample_rate_outputs.items()
        },
        "fresh_runtime_reset_reproducibility": {
            "comparison": reset_comparison,
            "sha256": sha256_file(audio_dir / "full_chain_digital_pad.wav"),
            "repeat_sha256": sha256_file(repeat_path),
        },
        "release_render": performance,
    }
    write_utf8(review_root / "metrics.json", json.dumps(metrics, ensure_ascii=False, indent=2) + "\n")
    write_utf8(
        review_root / "README.md",
        (
            "# Processor Expansion Review\n\n"
            "Processor ExpansionのDefinition、Inspect、技術WAV、Metricsを同じ条件で再生成するPackageです。\n\n"
            "## 生成\n\n"
            "```bash\n"
            "python3 review/generate/generate_processor_expansion.py\n"
            "```\n\n"
            "`definitions/`にはProcessorごとの比較用Definition、`events/`にはParameter Changeを含むEvent Sequence、`audio/technical/`には未正規化の技術確認用WAV、`inspect/`にはCompile後のJSONを保存します。\n\n"
            "`metrics.json`はFinite性、Peak / RMS / DC、Stereo情報、Filter Mode差、Block Size、Sample Rate、Reset再現性、Release RenderのRealtime比を記録します。音質の判定は`review-summary.md`へ人間が記入します。\n"
        ),
    )
    write_utf8(
        review_root / "review-summary.md",
        (
            "# Processor Expansion Review\n\n"
            "## Automated checks\n\n"
            "- Filter 4 Mode、EQ、Resonator、Bitcrusher、Chorus、Flanger、Phaser、Compressor、Limiter、Full Chain 3のDefinitionを生成した。\n"
            "- 生成したDefinitionをValidateし、Compile後のInspect JSONを保存した。\n"
            "- Processor単体比較はHarmonic Sourceを使い、Noise LayerはLo-fi Textureへ明示的に追加した。\n"
            "- すべての技術WAVについてFinite性、Peak、RMS、DC、Stereo情報を測定した。\n"
            "- 44.1 / 48 / 96 kHz、Block Size 32 / 64 / 257 / 1024、Fresh RuntimeとReset後の出力を比較した。\n"
            "- Release BuildのRender時間とRealtime比を`metrics.json`へ記録した。\n\n"
            "## Human listening record\n\n"
            "次の確認を同じ再生環境・音量で行い、結果を追記する。Metrics合格だけでは音質合格としない。\n\n"
            "| 対象 | 確認内容 | 結果 |\n"
            "|---|---|---|\n"
            "| `filter_*.wav` | Low / High / Band / Notchの差、Resonanceの破綻 | 未確認 |\n"
            "| `eq_*.wav` | Boost / Cutが帯域変化として自然に聞こえるか | 未確認 |\n"
            "| `resonator_*.wav` | 220 / 440 HzのPitch、Decay、Dampingの使いやすさ | 未確認 |\n"
            "| `bitcrusher_*.wav` | Bit DepthとSample-rate ReductionのDigital Texture | 未確認 |\n"
            "| `chorus_*.wav` / `flanger_*.wav` | Stereo幅、揺れ、Sweep、Feedbackの濁り | 未確認 |\n"
            "| `phaser_*.wav` | Sweepの滑らかさ、4 / 8段の差、Jet感 | 未確認 |\n"
            "| `compressor_*.wav` / `limiter.wav` | Punch、Pumping、Peak抑制、歪み | 未確認 |\n"
            "| `full_chain_digital_pad.wav` | Padの広がり、EQ、Chorus、Reverb、Compressorの一体感 | 未確認 |\n"
            "| `full_chain_metallic_pluck.wav` | Operator + SampleのAttack、Resonator、Phaser、Delay、Limiter | 未確認 |\n"
            "| `full_chain_lofi_texture.wav` | Granular + Noise、Bitcrusher、Flanger、Reverbの実用性 | 未確認 |\n"
            "| `full_chain_digital_block_*.wav` | Block Size変更によるClickや時間軸の差 | 未確認 |\n"
            "| `full_chain_digital_sample_rate_*.wav` | Sample Rate変更による音色と安定性 | 未確認 |\n\n"
            "人間の試聴後、各行の結果と必要な修正内容を具体的に記録する。\n"
        ),
    )


if __name__ == "__main__":
    main()
