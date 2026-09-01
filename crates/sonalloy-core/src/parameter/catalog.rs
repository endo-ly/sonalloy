use sha2::{Digest, Sha256};
use std::collections::HashMap;

use super::generator::{
    ADDITIVE_INHARMONICITY, ADDITIVE_MORPH, ADDITIVE_SPECTRUM_TILT, FORMANT_SHIFT,
    FORMANT_SPECTRAL_TILT, FORMANT_THROAT, FORMANT_VOWEL_POSITION, GRAIN_DENSITY, GRAIN_PAN_SPREAD,
    GRAIN_PITCH, GRAIN_RANDOMNESS, GRAIN_SIZE, GRANULAR_POSITION, GeneratorParameterSpec,
    MODAL_BRIGHTNESS, MODAL_DECAY, MODAL_STRUCTURE, NOISE_CORRELATION,
    OPERATOR_PARAMETER_SMOOTHING_SECONDS, OSCILLATOR_FEEDBACK, PHASE_DISTORTION,
    PHYSICAL_STRING_BRIGHTNESS, PHYSICAL_STRING_DECAY_SECONDS, PHYSICAL_STRING_STIFFNESS,
    PULSE_WIDTH, SPECTRAL_BLUR, SPECTRAL_FREEZE, SPECTRAL_MORPH, SPECTRAL_POSITION, SPECTRAL_SHIFT,
    SYNC_RATIO, UNISON_DETUNE, UNISON_SPREAD, WAVEFOLD, WAVESHAPE, WAVETABLE_POSITION,
};
use crate::definition::{
    GeneratorDefinition, InstrumentDefinition, OPERATOR_AM_RING_AMOUNT_MAX,
    OPERATOR_AM_RING_AMOUNT_MIN, OPERATOR_DETUNE_MAX, OPERATOR_DETUNE_MIN, OPERATOR_FEEDBACK_MAX,
    OPERATOR_FEEDBACK_MIN, OPERATOR_LEVEL_MAX, OPERATOR_LEVEL_MIN,
    OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX, OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN, OPERATOR_RATIO_MAX,
    OPERATOR_RATIO_MIN, OperatorModulationMode, OscillatorWaveform, ProcessorDefinition,
};
use crate::parameter::{
    ParameterDescriptor, ParameterHandle, ParameterOwner, ParameterScale, ParameterUnit,
    VectorAxis, layer_parameter_id,
};

/// Compiled catalog used by control code and runtime bindings.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterCatalog {
    descriptors: Box<[ParameterDescriptor]>,
    lookup: HashMap<String, ParameterHandle>,
    revision: super::ParameterCatalogRevision,
}

impl ParameterCatalog {
    pub(crate) fn from_definition(definition: &InstrumentDefinition) -> Self {
        let mut descriptors = Vec::with_capacity(definition.layers.len() * 3);
        for (definition_index, layer) in definition.layers.iter().enumerate() {
            push_layer_descriptors(&mut descriptors, definition_index, layer);
        }
        for (processor_index, processor) in definition.voice_processors.iter().enumerate() {
            push_processor_descriptors(
                &mut descriptors,
                processor,
                ParameterOwner::VoiceProcessor { processor_index },
                "voice.processor",
            );
        }
        for (processor_index, processor) in definition.global_processors.iter().enumerate() {
            push_processor_descriptors(
                &mut descriptors,
                processor,
                ParameterOwner::GlobalProcessor { processor_index },
                "global.processor",
            );
        }
        push_macro_descriptors(&mut descriptors, definition);
        push_vector_descriptors(&mut descriptors, definition);
        let descriptors = descriptors.into_boxed_slice();
        let lookup = descriptors
            .iter()
            .enumerate()
            .map(|(index, descriptor)| (descriptor.id.clone(), ParameterHandle::new(index)))
            .collect();
        Self {
            revision: catalog_revision(&descriptors),
            descriptors,
            lookup,
        }
    }

    /// Return descriptors in their stable catalog order.
    #[must_use]
    pub fn parameters(&self) -> &[ParameterDescriptor] {
        &self.descriptors
    }

    /// Resolve a canonical identifier before entering the audio path.
    #[must_use]
    pub fn parameter_handle(&self, id: &str) -> Option<ParameterHandle> {
        self.lookup.get(id).copied()
    }

    /// Return a descriptor by compiled handle.
    #[must_use]
    pub fn descriptor(&self, handle: ParameterHandle) -> Option<&ParameterDescriptor> {
        self.descriptors.get(handle.index())
    }

    /// Return the deterministic revision for this catalog's descriptors.
    #[must_use]
    pub fn revision(&self) -> super::ParameterCatalogRevision {
        self.revision
    }

    pub(crate) fn len(&self) -> usize {
        self.descriptors.len()
    }
}

fn catalog_revision(descriptors: &[ParameterDescriptor]) -> super::ParameterCatalogRevision {
    let mut hasher = Sha256::new();
    for descriptor in descriptors {
        update_text(&mut hasher, &descriptor.id);
        update_owner(&mut hasher, descriptor.owner);
        update_unit(&mut hasher, descriptor.unit);
        update_scale(&mut hasher, descriptor.scale);
        update_u32(&mut hasher, descriptor.min.to_bits());
        update_u32(&mut hasher, descriptor.max.to_bits());
        update_u32(&mut hasher, descriptor.default.to_bits());
        update_u32(&mut hasher, descriptor.smoothing_seconds.to_bits());
    }
    let digest = hasher.finalize();
    u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("SHA-256 has eight-byte prefix"),
    )
}

fn update_text(hasher: &mut Sha256, value: &str) {
    update_u64(hasher, usize_field(value.len()));
    hasher.update(value.as_bytes());
}

fn update_unit(hasher: &mut Sha256, unit: ParameterUnit) {
    update_u32(
        hasher,
        match unit {
            ParameterUnit::Decibels => 1,
            ParameterUnit::Pan => 2,
            ParameterUnit::Cents => 3,
            ParameterUnit::Hertz => 4,
            ParameterUnit::Ratio => 5,
            ParameterUnit::Seconds => 6,
            ParameterUnit::PerSecond => 7,
            ParameterUnit::Index => 8,
            ParameterUnit::DecibelsPerOctave => 9,
            ParameterUnit::Normalized => 10,
        },
    );
}

fn update_scale(hasher: &mut Sha256, scale: ParameterScale) {
    update_u32(
        hasher,
        match scale {
            ParameterScale::Linear => 0,
            ParameterScale::Log2 => 1,
        },
    );
}

fn update_owner(hasher: &mut Sha256, owner: ParameterOwner) {
    match owner {
        ParameterOwner::Layer { definition_index } => {
            update_u32(hasher, 1);
            update_u64(hasher, usize_field(definition_index));
        }
        ParameterOwner::LayerGenerator { definition_index } => {
            update_u32(hasher, 2);
            update_u64(hasher, usize_field(definition_index));
        }
        ParameterOwner::LayerProcessor {
            definition_index,
            processor_index,
        } => {
            update_u32(hasher, 3);
            update_u64(hasher, usize_field(definition_index));
            update_u64(hasher, usize_field(processor_index));
        }
        ParameterOwner::VoiceProcessor { processor_index } => {
            update_u32(hasher, 4);
            update_u64(hasher, usize_field(processor_index));
        }
        ParameterOwner::GlobalProcessor { processor_index } => {
            update_u32(hasher, 5);
            update_u64(hasher, usize_field(processor_index));
        }
        ParameterOwner::Macro { macro_index } => {
            update_u32(hasher, 6);
            update_u64(hasher, usize_field(macro_index));
        }
        ParameterOwner::VectorAxis { vector_index, axis } => {
            update_u32(hasher, 7);
            update_u64(hasher, usize_field(vector_index));
            update_u32(
                hasher,
                match axis {
                    VectorAxis::Position => 1,
                    VectorAxis::X => 2,
                    VectorAxis::Y => 3,
                },
            );
        }
    }
}

fn update_u32(hasher: &mut Sha256, value: u32) {
    hasher.update(value.to_be_bytes());
}

fn update_u64(hasher: &mut Sha256, value: u64) {
    hasher.update(value.to_be_bytes());
}

fn usize_field(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn push_layer_descriptors(
    descriptors: &mut Vec<ParameterDescriptor>,
    definition_index: usize,
    layer: &crate::definition::LayerDefinition,
) {
    for (suffix, unit, min, max, default) in [
        ("gain", ParameterUnit::Decibels, -60.0, 12.0, layer.gain_db),
        ("pan", ParameterUnit::Pan, -1.0, 1.0, layer.pan),
        (
            "tuning",
            ParameterUnit::Cents,
            -1200.0,
            1200.0,
            layer.tuning_cents,
        ),
    ] {
        descriptors.push(ParameterDescriptor {
            id: layer_parameter_id(&layer.id, suffix),
            owner: ParameterOwner::Layer { definition_index },
            unit,
            scale: ParameterScale::Linear,
            min,
            max,
            default,
            smoothing_seconds: 0.005,
        });
    }
    push_generator_descriptors(
        descriptors,
        &layer.generator,
        ParameterOwner::LayerGenerator { definition_index },
        &format!("layer.{}.generator", layer.id),
    );
    for (processor_index, processor) in layer.processors.iter().enumerate() {
        push_processor_descriptors(
            descriptors,
            processor,
            ParameterOwner::LayerProcessor {
                definition_index,
                processor_index,
            },
            &format!("layer.{}.processor", layer.id),
        );
    }
}

fn push_macro_descriptors(
    descriptors: &mut Vec<ParameterDescriptor>,
    definition: &InstrumentDefinition,
) {
    for (macro_index, value) in definition.macros.iter().enumerate() {
        descriptors.push(ParameterDescriptor {
            id: format!("macro.{}", value.id),
            owner: ParameterOwner::Macro { macro_index },
            unit: ParameterUnit::Normalized,
            scale: ParameterScale::Linear,
            min: 0.0,
            max: 1.0,
            default: value.default,
            smoothing_seconds: 0.005,
        });
    }
}

fn push_vector_descriptors(
    descriptors: &mut Vec<ParameterDescriptor>,
    definition: &InstrumentDefinition,
) {
    for (vector_index, vector) in definition.vectors.iter().enumerate() {
        let (id, axes) = match vector {
            crate::definition::VectorDefinition::TwoWay { id, .. } => (
                id.as_str(),
                [Some(("position", VectorAxis::Position)), None],
            ),
            crate::definition::VectorDefinition::FourWay { id, .. } => (
                id.as_str(),
                [Some(("x", VectorAxis::X)), Some(("y", VectorAxis::Y))],
            ),
        };
        for (suffix, axis) in axes.into_iter().flatten() {
            descriptors.push(ParameterDescriptor {
                id: format!("vector.{id}.{suffix}"),
                owner: ParameterOwner::VectorAxis { vector_index, axis },
                unit: ParameterUnit::Normalized,
                scale: ParameterScale::Linear,
                min: 0.0,
                max: 1.0,
                default: vector_axis_default(vector, axis),
                smoothing_seconds: 0.005,
            });
        }
    }
}

fn vector_axis_default(vector: &crate::definition::VectorDefinition, axis: VectorAxis) -> f32 {
    match (vector, axis) {
        (crate::definition::VectorDefinition::TwoWay { position, .. }, VectorAxis::Position) => {
            *position
        }
        (crate::definition::VectorDefinition::FourWay { x, .. }, VectorAxis::X) => *x,
        (crate::definition::VectorDefinition::FourWay { y, .. }, VectorAxis::Y) => *y,
        _ => 0.0,
    }
}

#[allow(clippy::too_many_lines)]
fn push_generator_descriptors(
    descriptors: &mut Vec<ParameterDescriptor>,
    generator: &GeneratorDefinition,
    owner: ParameterOwner,
    prefix: &str,
) {
    match generator {
        GeneratorDefinition::Oscillator(oscillator) => {
            if let OscillatorWaveform::Pulse { pulse_width } = oscillator.waveform {
                push_generator_descriptor(descriptors, prefix, owner, PULSE_WIDTH, pulse_width);
            }
            if let Some(hard_sync) = oscillator.hard_sync {
                push_generator_descriptor(descriptors, prefix, owner, SYNC_RATIO, hard_sync.ratio);
            }
            if let Some(waveshaping) = oscillator.waveshaping {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    WAVESHAPE,
                    waveshaping.amount,
                );
            }
            if let Some(phase_distortion) = oscillator.phase_distortion {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    PHASE_DISTORTION,
                    phase_distortion.amount,
                );
            }
            if let Some(wavefold) = oscillator.wavefold {
                push_generator_descriptor(descriptors, prefix, owner, WAVEFOLD, wavefold.amount);
            }
            if let Some(feedback) = oscillator.feedback {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    OSCILLATOR_FEEDBACK,
                    feedback.amount,
                );
            }
            if let Some(unison) = oscillator.unison {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_DETUNE,
                    unison.detune_cents,
                );
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_SPREAD,
                    unison.stereo_spread,
                );
            }
        }
        GeneratorDefinition::Noise(noise) => {
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                NOISE_CORRELATION,
                noise.stereo_correlation,
            );
        }
        GeneratorDefinition::PhysicalString(physical_string) => {
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                PHYSICAL_STRING_DECAY_SECONDS,
                physical_string.decay_seconds,
            );
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                PHYSICAL_STRING_BRIGHTNESS,
                physical_string.brightness,
            );
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                PHYSICAL_STRING_STIFFNESS,
                physical_string.stiffness,
            );
        }
        GeneratorDefinition::Modal(modal) => {
            push_generator_descriptor(descriptors, prefix, owner, MODAL_STRUCTURE, modal.structure);
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                MODAL_BRIGHTNESS,
                modal.brightness,
            );
            push_generator_descriptor(descriptors, prefix, owner, MODAL_DECAY, modal.decay);
        }
        GeneratorDefinition::Additive(additive) => {
            push_generator_descriptor(descriptors, prefix, owner, ADDITIVE_MORPH, additive.morph);
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                ADDITIVE_SPECTRUM_TILT,
                additive.spectrum_tilt_db_per_octave,
            );
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                ADDITIVE_INHARMONICITY,
                additive.inharmonicity,
            );
        }
        GeneratorDefinition::Formant(formant) => {
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                FORMANT_VOWEL_POSITION,
                formant.vowel_position,
            );
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                FORMANT_SHIFT,
                formant.formant_shift_cents,
            );
            push_generator_descriptor(descriptors, prefix, owner, FORMANT_THROAT, formant.throat);
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                FORMANT_SPECTRAL_TILT,
                formant.spectral_tilt_db_per_octave,
            );
        }
        GeneratorDefinition::Sample(_) | GeneratorDefinition::WaveSequence(_) => {}
        GeneratorDefinition::Spectral(spectral) => {
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                SPECTRAL_POSITION,
                spectral.position,
            );
            push_generator_descriptor(descriptors, prefix, owner, SPECTRAL_FREEZE, spectral.freeze);
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                SPECTRAL_BLUR,
                spectral.blur_seconds,
            );
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                SPECTRAL_SHIFT,
                spectral.shift_hz,
            );
            if spectral.asset_b.is_some() {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    SPECTRAL_MORPH,
                    spectral.morph,
                );
            }
        }
        GeneratorDefinition::Granular(granular) => {
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                GRANULAR_POSITION,
                granular.position,
            );
            push_generator_descriptor(descriptors, prefix, owner, GRAIN_SIZE, granular.grain_size);
            push_generator_descriptor(descriptors, prefix, owner, GRAIN_DENSITY, granular.density);
            push_generator_descriptor(descriptors, prefix, owner, GRAIN_PITCH, granular.pitch);
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                GRAIN_RANDOMNESS,
                granular.randomness,
            );
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                GRAIN_PAN_SPREAD,
                granular.pan_spread,
            );
        }
        GeneratorDefinition::Wavetable(wavetable) => {
            push_generator_descriptor(
                descriptors,
                prefix,
                owner,
                WAVETABLE_POSITION,
                wavetable.position,
            );
            if let Some(unison) = wavetable.unison {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_DETUNE,
                    unison.detune_cents,
                );
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_SPREAD,
                    unison.stereo_spread,
                );
            }
        }
        GeneratorDefinition::OperatorModulation(operator_modulation) => {
            let topology = operator_modulation.algorithm.topology();
            for (index, operator) in operator_modulation.operators.iter().take(4).enumerate() {
                push_operator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    index,
                    "ratio",
                    ParameterUnit::Ratio,
                    ParameterScale::Log2,
                    OPERATOR_RATIO_MIN,
                    OPERATOR_RATIO_MAX,
                    operator.ratio,
                );
                push_operator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    index,
                    "detune",
                    ParameterUnit::Cents,
                    ParameterScale::Linear,
                    OPERATOR_DETUNE_MIN,
                    OPERATOR_DETUNE_MAX,
                    operator.detune_cents,
                );
                if topology.carrier_mask & (1_u8 << index) != 0 {
                    push_operator_descriptor(
                        descriptors,
                        prefix,
                        owner,
                        index,
                        "level",
                        ParameterUnit::Normalized,
                        ParameterScale::Linear,
                        OPERATOR_LEVEL_MIN,
                        OPERATOR_LEVEL_MAX,
                        operator.level,
                    );
                }
                let has_output = topology
                    .incoming_masks
                    .iter()
                    .any(|mask| mask & (1_u8 << index) != 0);
                if has_output {
                    let (unit, min, max) = match operator_modulation.mode {
                        OperatorModulationMode::Phase | OperatorModulationMode::Frequency => (
                            ParameterUnit::Index,
                            OPERATOR_PHASE_FREQUENCY_AMOUNT_MIN,
                            OPERATOR_PHASE_FREQUENCY_AMOUNT_MAX,
                        ),
                        OperatorModulationMode::Amplitude | OperatorModulationMode::Ring => (
                            ParameterUnit::Normalized,
                            OPERATOR_AM_RING_AMOUNT_MIN,
                            OPERATOR_AM_RING_AMOUNT_MAX,
                        ),
                    };
                    push_operator_descriptor(
                        descriptors,
                        prefix,
                        owner,
                        index,
                        "modulation_amount",
                        unit,
                        ParameterScale::Linear,
                        min,
                        max,
                        operator.modulation_amount,
                    );
                }
                if matches!(
                    operator_modulation.mode,
                    OperatorModulationMode::Phase | OperatorModulationMode::Frequency
                ) {
                    push_operator_descriptor(
                        descriptors,
                        prefix,
                        owner,
                        index,
                        "feedback",
                        ParameterUnit::Normalized,
                        ParameterScale::Linear,
                        OPERATOR_FEEDBACK_MIN,
                        OPERATOR_FEEDBACK_MAX,
                        operator.feedback,
                    );
                }
            }
            if let Some(unison) = operator_modulation.unison {
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_DETUNE,
                    unison.detune_cents,
                );
                push_generator_descriptor(
                    descriptors,
                    prefix,
                    owner,
                    UNISON_SPREAD,
                    unison.stereo_spread,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_operator_descriptor(
    descriptors: &mut Vec<ParameterDescriptor>,
    prefix: &str,
    owner: ParameterOwner,
    index: usize,
    suffix: &str,
    unit: ParameterUnit,
    scale: ParameterScale,
    min: f32,
    max: f32,
    default: f32,
) {
    descriptors.push(ParameterDescriptor {
        id: format!("{prefix}.operator.{}.{}", index + 1, suffix),
        owner,
        unit,
        scale,
        min,
        max,
        default,
        smoothing_seconds: OPERATOR_PARAMETER_SMOOTHING_SECONDS,
    });
}

fn push_generator_descriptor(
    descriptors: &mut Vec<ParameterDescriptor>,
    prefix: &str,
    owner: ParameterOwner,
    spec: GeneratorParameterSpec,
    default: f32,
) {
    descriptors.push(ParameterDescriptor {
        id: format!("{prefix}.{}", spec.suffix),
        owner,
        unit: spec.unit,
        scale: spec.scale,
        min: spec.min,
        max: spec.max,
        default,
        smoothing_seconds: spec.smoothing_seconds,
    });
}

#[allow(clippy::too_many_lines)]
fn push_processor_descriptors(
    descriptors: &mut Vec<ParameterDescriptor>,
    processor: &ProcessorDefinition,
    owner: ParameterOwner,
    prefix: &str,
) {
    let processor_id = processor.id();
    let base = format!("{prefix}.{processor_id}");
    match processor {
        ProcessorDefinition::Filter(value) => {
            descriptors.push(ParameterDescriptor {
                id: format!("{base}.cutoff"),
                owner,
                unit: ParameterUnit::Hertz,
                scale: ParameterScale::Log2,
                min: 20.0,
                max: 20_000.0,
                default: value.cutoff_hz,
                smoothing_seconds: 0.010,
            });
            descriptors.push(ParameterDescriptor {
                id: format!("{base}.resonance"),
                owner,
                unit: ParameterUnit::Normalized,
                scale: ParameterScale::Linear,
                min: 0.0,
                max: 1.0,
                default: value.resonance,
                smoothing_seconds: 0.010,
            });
        }
        ProcessorDefinition::LadderFilter(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.cutoff"),
                owner,
                ParameterUnit::Hertz,
                ParameterScale::Log2,
                20.0,
                20_000.0,
                value.cutoff_hz,
                0.010,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.resonance"),
                owner,
                value.resonance,
                0.010,
                1.0,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.drive"),
                owner,
                value.drive,
                0.005,
                1.0,
            );
        }
        ProcessorDefinition::Drive(value) => {
            push_normalized_descriptor(
                descriptors,
                format!("{base}.amount"),
                owner,
                value.amount,
                0.005,
                1.0,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.005,
                1.0,
            );
        }
        ProcessorDefinition::Eq(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.low_gain_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -24.0,
                24.0,
                value.low_gain_db,
                0.005,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.mid_gain_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -24.0,
                24.0,
                value.mid_gain_db,
                0.005,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.high_gain_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -24.0,
                24.0,
                value.high_gain_db,
                0.005,
            );
        }
        ProcessorDefinition::Formant(value) => {
            push_normalized_descriptor(
                descriptors,
                format!("{base}.vowel_position"),
                owner,
                value.vowel_position,
                0.010,
                1.0,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.formant_shift"),
                owner,
                ParameterUnit::Cents,
                ParameterScale::Linear,
                -2400.0,
                2400.0,
                value.formant_shift_cents,
                0.010,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.throat"),
                owner,
                value.throat,
                0.010,
                1.0,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.010,
                1.0,
            );
        }
        ProcessorDefinition::Resonator(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.frequency_hz"),
                owner,
                ParameterUnit::Hertz,
                ParameterScale::Log2,
                40.0,
                12_000.0,
                value.frequency_hz,
                0.010,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.decay_seconds"),
                owner,
                ParameterUnit::Seconds,
                ParameterScale::Linear,
                0.02,
                10.0,
                value.decay_seconds,
                0.020,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.damping"),
                owner,
                value.damping,
                0.010,
                1.0,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.010,
                1.0,
            );
        }
        ProcessorDefinition::Bitcrusher(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.bit_depth"),
                owner,
                ParameterUnit::Index,
                ParameterScale::Linear,
                2.0,
                16.0,
                value.bit_depth,
                0.005,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.sample_rate_ratio"),
                owner,
                ParameterUnit::Ratio,
                ParameterScale::Log2,
                0.01,
                1.0,
                value.sample_rate_ratio,
                0.005,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.005,
                1.0,
            );
        }
        ProcessorDefinition::Chorus(value) => {
            push_processor_modulation_descriptors(
                descriptors,
                &base,
                owner,
                value.rate_hz,
                0.01,
                8.0,
                value.depth,
                0.0,
                1.0,
                value.feedback,
                0.0,
                0.85,
                value.width,
                value.mix,
            );
        }
        ProcessorDefinition::Flanger(value) => {
            push_processor_modulation_descriptors(
                descriptors,
                &base,
                owner,
                value.rate_hz,
                0.01,
                10.0,
                value.depth,
                0.0,
                1.0,
                value.feedback,
                -0.95,
                0.95,
                value.width,
                value.mix,
            );
        }
        ProcessorDefinition::Phaser(value) => {
            push_processor_modulation_descriptors(
                descriptors,
                &base,
                owner,
                value.rate_hz,
                0.01,
                8.0,
                value.depth,
                0.0,
                1.0,
                value.feedback,
                -0.9,
                0.9,
                value.width,
                value.mix,
            );
        }
        ProcessorDefinition::FrequencyShifter(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.shift_hz"),
                owner,
                ParameterUnit::Hertz,
                ParameterScale::Linear,
                -5000.0,
                5000.0,
                value.shift_hz,
                0.010,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.010,
                1.0,
            );
        }
        ProcessorDefinition::Delay(value) => {
            push_normalized_descriptor(
                descriptors,
                format!("{base}.feedback"),
                owner,
                value.feedback,
                0.010,
                0.95,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.010,
                1.0,
            );
        }
        ProcessorDefinition::Reverb(value) => {
            push_normalized_descriptor(
                descriptors,
                format!("{base}.decay"),
                owner,
                value.decay,
                0.020,
                0.98,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.damping"),
                owner,
                value.damping,
                0.020,
                1.0,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.width"),
                owner,
                value.width,
                0.020,
                1.0,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.020,
                1.0,
            );
        }
        ProcessorDefinition::Convolution(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.gain_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -24.0,
                24.0,
                value.gain_db,
                0.010,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.020,
                1.0,
            );
        }
        ProcessorDefinition::Vocoder(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.modulator_gain_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -24.0,
                24.0,
                value.modulator_gain_db,
                0.010,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.output_gain_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -24.0,
                24.0,
                value.output_gain_db,
                0.010,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.010,
                1.0,
            );
        }
        ProcessorDefinition::EnvelopeTransfer(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.input_gain_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -24.0,
                24.0,
                value.input_gain_db,
                0.010,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.floor_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -96.0,
                0.0,
                value.floor_db,
                0.010,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.010,
                1.0,
            );
        }
        ProcessorDefinition::SpectralMorph(value) => {
            push_normalized_descriptor(
                descriptors,
                format!("{base}.morph"),
                owner,
                value.morph,
                0.020,
                1.0,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.output_gain_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -24.0,
                24.0,
                value.output_gain_db,
                0.010,
            );
        }
        ProcessorDefinition::Gate(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.threshold_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -80.0,
                0.0,
                value.threshold_db,
                0.010,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.range_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -96.0,
                0.0,
                value.range_db,
                0.010,
            );
        }
        ProcessorDefinition::TransientShaper(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.attack"),
                owner,
                ParameterUnit::Index,
                ParameterScale::Linear,
                -1.0,
                1.0,
                value.attack,
                0.010,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.sustain"),
                owner,
                ParameterUnit::Index,
                ParameterScale::Linear,
                -1.0,
                1.0,
                value.sustain,
                0.010,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.010,
                1.0,
            );
        }
        ProcessorDefinition::Compressor(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.threshold_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -60.0,
                0.0,
                value.threshold_db,
                0.010,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.ratio"),
                owner,
                ParameterUnit::Ratio,
                ParameterScale::Log2,
                1.0,
                20.0,
                value.ratio,
                0.010,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.makeup_gain_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -12.0,
                24.0,
                value.makeup_gain_db,
                0.010,
            );
            push_normalized_descriptor(
                descriptors,
                format!("{base}.mix"),
                owner,
                value.mix,
                0.010,
                1.0,
            );
        }
        ProcessorDefinition::Limiter(value) => {
            push_processor_descriptor(
                descriptors,
                format!("{base}.ceiling_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -12.0,
                0.0,
                value.ceiling_db,
                0.010,
            );
            push_processor_descriptor(
                descriptors,
                format!("{base}.input_gain_db"),
                owner,
                ParameterUnit::Decibels,
                ParameterScale::Linear,
                -24.0,
                24.0,
                value.input_gain_db,
                0.010,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_processor_modulation_descriptors(
    descriptors: &mut Vec<ParameterDescriptor>,
    base: &str,
    owner: ParameterOwner,
    rate_hz: f32,
    rate_min: f32,
    rate_max: f32,
    depth: f32,
    depth_min: f32,
    depth_max: f32,
    feedback: f32,
    feedback_min: f32,
    feedback_max: f32,
    width: f32,
    mix: f32,
) {
    push_processor_descriptor(
        descriptors,
        format!("{base}.rate_hz"),
        owner,
        ParameterUnit::PerSecond,
        ParameterScale::Linear,
        rate_min,
        rate_max,
        rate_hz,
        0.010,
    );
    push_processor_descriptor(
        descriptors,
        format!("{base}.depth"),
        owner,
        ParameterUnit::Normalized,
        ParameterScale::Linear,
        depth_min,
        depth_max,
        depth,
        0.010,
    );
    push_processor_descriptor(
        descriptors,
        format!("{base}.feedback"),
        owner,
        ParameterUnit::Normalized,
        ParameterScale::Linear,
        feedback_min,
        feedback_max,
        feedback,
        0.010,
    );
    push_normalized_descriptor(
        descriptors,
        format!("{base}.width"),
        owner,
        width,
        0.010,
        1.0,
    );
    push_normalized_descriptor(descriptors, format!("{base}.mix"), owner, mix, 0.010, 1.0);
}

#[allow(clippy::too_many_arguments)]
fn push_processor_descriptor(
    descriptors: &mut Vec<ParameterDescriptor>,
    id: String,
    owner: ParameterOwner,
    unit: ParameterUnit,
    scale: ParameterScale,
    min: f32,
    max: f32,
    default: f32,
    smoothing_seconds: f32,
) {
    descriptors.push(ParameterDescriptor {
        id,
        owner,
        unit,
        scale,
        min,
        max,
        default,
        smoothing_seconds,
    });
}

fn push_normalized_descriptor(
    descriptors: &mut Vec<ParameterDescriptor>,
    id: String,
    owner: ParameterOwner,
    default: f32,
    smoothing_seconds: f32,
    max: f32,
) {
    descriptors.push(ParameterDescriptor {
        id,
        owner,
        unit: ParameterUnit::Normalized,
        scale: ParameterScale::Linear,
        min: 0.0,
        max,
        default,
        smoothing_seconds,
    });
}

#[cfg(test)]
mod tests {
    use super::{
        GeneratorDefinition, OscillatorWaveform, ParameterCatalog, ParameterOwner, ParameterScale,
        ParameterUnit,
    };
    use crate::definition::{ProcessorDefinition, tests::definition};
    use crate::parameter::{is_parameter_id, layer_generator_parameter_id};

    #[test]
    fn catalog_order_is_definition_order_then_processor_scope() {
        let mut source = definition();
        source.layers[0].processors.push(ProcessorDefinition::Drive(
            crate::definition::DriveProcessorDefinition {
                id: "drive".to_owned(),
                amount: 0.5,
                mix: 0.5,
            },
        ));
        source.voice_processors.push(ProcessorDefinition::Filter(
            crate::definition::FilterProcessorDefinition {
                id: "tone".to_owned(),
                mode: crate::definition::FilterModeDefinition::LowPass,
                cutoff_hz: 1_000.0,
                resonance: 0.2,
            },
        ));
        let catalog = ParameterCatalog::from_definition(&source);
        let ids: Vec<_> = catalog
            .parameters()
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "layer.body.gain",
                "layer.body.pan",
                "layer.body.tuning",
                "layer.body.processor.drive.amount",
                "layer.body.processor.drive.mix",
                "voice.processor.tone.cutoff",
                "voice.processor.tone.resonance"
            ]
        );
    }

    #[test]
    fn generator_parameters_follow_layer_parameters_and_preserve_metadata() {
        let mut source = definition();
        source.layers[0].generator =
            GeneratorDefinition::Oscillator(crate::definition::OscillatorDefinition {
                waveform: OscillatorWaveform::Pulse { pulse_width: 0.25 },
                phase_reset: true,
                phase: 0.0,
                hard_sync: None,
                waveshaping: None,
                phase_distortion: None,
                wavefold: None,
                feedback: None,
                unison: None,
            });
        let mut noise_layer = source.layers[0].clone();
        noise_layer.id = "texture".to_owned();
        noise_layer.generator = GeneratorDefinition::Noise(crate::definition::NoiseDefinition {
            color: crate::definition::NoiseColor::White,
            seed: 7,
            stereo_correlation: 0.6,
        });
        source.layers.push(noise_layer);

        let catalog = ParameterCatalog::from_definition(&source);
        let ids: Vec<_> = catalog
            .parameters()
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "layer.body.gain",
                "layer.body.pan",
                "layer.body.tuning",
                "layer.body.generator.pulse_width",
                "layer.texture.gain",
                "layer.texture.pan",
                "layer.texture.tuning",
                "layer.texture.generator.noise_correlation",
            ]
        );

        let pulse = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "layer.body.generator.pulse_width")
            .expect("pulse width descriptor");
        assert_eq!(
            pulse.owner,
            ParameterOwner::LayerGenerator {
                definition_index: 0
            }
        );
        assert_eq!(pulse.unit, ParameterUnit::Normalized);
        assert_eq!(pulse.scale, ParameterScale::Linear);
        assert!((pulse.min - 0.05).abs() < f32::EPSILON);
        assert!((pulse.max - 0.95).abs() < f32::EPSILON);
        assert!((pulse.default - 0.25).abs() < f32::EPSILON);
        assert!((pulse.smoothing_seconds - 0.005).abs() < f32::EPSILON);

        let correlation = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "layer.texture.generator.noise_correlation")
            .expect("noise correlation descriptor");
        assert_eq!(
            correlation.owner,
            ParameterOwner::LayerGenerator {
                definition_index: 1
            }
        );
        assert!((correlation.min - 0.0).abs() < f32::EPSILON);
        assert!((correlation.max - 1.0).abs() < f32::EPSILON);
        assert!((correlation.default - 0.6).abs() < f32::EPSILON);
        assert!((correlation.smoothing_seconds - 0.010).abs() < f32::EPSILON);
    }

    #[test]
    fn generator_parameter_ids_follow_the_canonical_grammar() {
        assert_eq!(
            layer_generator_parameter_id("body", "pulse_width"),
            "layer.body.generator.pulse_width"
        );
        assert!(is_parameter_id("layer.body.generator.pulse_width"));
        assert!(is_parameter_id("layer.body.generator.sync_ratio"));
        assert!(is_parameter_id("layer.body.generator.waveshape"));
        assert!(is_parameter_id("layer.body.generator.unison_detune"));
        assert!(is_parameter_id("layer.body.generator.unison_spread"));
        assert!(is_parameter_id("layer.body.generator.noise_correlation"));
        assert!(is_parameter_id("layer.body.generator.operator.1.ratio"));
        assert!(is_parameter_id(
            "layer.body.generator.operator.4.modulation_amount"
        ));
        assert!(!is_parameter_id("layer.Body.generator.pulse_width"));
        assert!(!is_parameter_id("layer.body.generator.operator.5.ratio"));
    }

    #[test]
    fn complex_generator_parameters_preserve_catalog_metadata() {
        let mut source = definition();
        source.layers[0].generator =
            GeneratorDefinition::Oscillator(crate::definition::OscillatorDefinition {
                waveform: OscillatorWaveform::Saw,
                phase_reset: true,
                phase: 0.0,
                hard_sync: Some(crate::definition::HardSyncDefinition { ratio: 3.0 }),
                waveshaping: Some(crate::definition::WaveshapingDefinition { amount: 0.25 }),
                phase_distortion: None,
                wavefold: None,
                feedback: None,
                unison: Some(crate::definition::UnisonDefinition {
                    voices: 5,
                    detune_cents: 18.0,
                    stereo_spread: 0.8,
                    phase_spread: 0.2,
                }),
            });
        let catalog = ParameterCatalog::from_definition(&source);
        let parameters = catalog.parameters();
        let ids: Vec<_> = parameters
            .iter()
            .map(|parameter| parameter.id.as_str())
            .collect();
        assert_eq!(
            &ids[3..],
            [
                "layer.body.generator.sync_ratio",
                "layer.body.generator.waveshape",
                "layer.body.generator.unison_detune",
                "layer.body.generator.unison_spread",
            ]
        );

        let sync = &parameters[3];
        assert_eq!(sync.unit, ParameterUnit::Ratio);
        assert_eq!(sync.scale, ParameterScale::Log2);
        assert!((sync.min - 1.0).abs() < f32::EPSILON);
        assert!((sync.max - 16.0).abs() < f32::EPSILON);
        assert!((sync.default - 3.0).abs() < f32::EPSILON);
        assert!((sync.smoothing_seconds - 0.005).abs() < f32::EPSILON);

        let detune = &parameters[5];
        assert_eq!(detune.unit, ParameterUnit::Cents);
        assert!(detune.min.abs() < f32::EPSILON);
        assert!((detune.max - 100.0).abs() < f32::EPSILON);
        assert!((detune.default - 18.0).abs() < f32::EPSILON);
        assert!((detune.smoothing_seconds - 0.010).abs() < f32::EPSILON);
    }

    #[test]
    fn operator_parameters_follow_topology_and_mode_contract() {
        let mut source = definition();
        let operator = crate::definition::OperatorDefinition {
            ratio: 1.0,
            detune_cents: 0.0,
            level: 0.0,
            modulation_amount: 0.0,
            feedback: 0.0,
            phase: 0.0,
            envelope: crate::definition::AdsrDefinition {
                attack_seconds: 0.0,
                decay_seconds: 0.1,
                sustain_level: 0.5,
                release_seconds: 0.1,
            },
        };
        let mut operators = vec![operator; 4];
        operators[0].level = 0.8;
        operators[1].modulation_amount = 2.0;
        operators[2].modulation_amount = 1.0;
        operators[3].modulation_amount = 0.5;
        source.layers[0].generator = GeneratorDefinition::OperatorModulation(
            crate::definition::OperatorModulationDefinition {
                mode: crate::definition::OperatorModulationMode::Phase,
                algorithm: crate::definition::OperatorAlgorithm::Stack4,
                operators,
                phase_reset: true,
                unison: Some(crate::definition::UnisonDefinition {
                    voices: 4,
                    detune_cents: 12.0,
                    stereo_spread: 0.7,
                    phase_spread: 0.4,
                }),
            },
        );

        let catalog = ParameterCatalog::from_definition(&source);
        let operator_one_level = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "layer.body.generator.operator.1.level")
            .expect("carrier level descriptor");
        assert_eq!(operator_one_level.unit, ParameterUnit::Normalized);
        assert!((operator_one_level.default - 0.8).abs() < f32::EPSILON);
        assert!(
            catalog
                .parameters()
                .iter()
                .all(|parameter| parameter.id != "layer.body.generator.operator.4.level")
        );

        let operator_two_amount = catalog
            .parameters()
            .iter()
            .find(|parameter| parameter.id == "layer.body.generator.operator.2.modulation_amount")
            .expect("modulation amount descriptor");
        assert_eq!(operator_two_amount.unit, ParameterUnit::Index);
        assert!((operator_two_amount.max - 8.0).abs() < f32::EPSILON);
        assert!((operator_two_amount.smoothing_seconds - 0.005).abs() < f32::EPSILON);
        assert!(
            catalog
                .parameters()
                .iter()
                .any(|parameter| parameter.id == "layer.body.generator.operator.4.feedback")
        );
        assert!(
            catalog
                .parameters()
                .iter()
                .any(|parameter| parameter.id == "layer.body.generator.unison_spread")
        );
    }
}
