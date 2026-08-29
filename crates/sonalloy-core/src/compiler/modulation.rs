use std::collections::{HashMap, HashSet};

use super::{CompiledAdsr, compile_adsr, db_to_linear};
use crate::definition::{
    AdsrDefinition, InstrumentDefinition, LfoDefinition, LfoWaveform, ModulationCurve,
    ModulationDurationUnit, ModulationRateUnit, ModulationSourceDefinition, MsegDefinition,
};
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::parameter::{BUILTIN_SOURCE_IDS, ParameterCatalog, ParameterHandle, ParameterOwner};

/// Dense source handle for voice-scoped sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceHandle(usize);

impl SourceHandle {
    /// Return the dense source index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Dense handle for an instrument-scoped source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstrumentSourceHandle(usize);

impl InstrumentSourceHandle {
    /// Return the dense source index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Compiled voice source.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSource {
    /// Stable source identifier.
    pub id: String,
    /// Compiled source behavior.
    pub source: CompiledVoiceSource,
}

/// Compiled voice source behavior.
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledVoiceSource {
    /// Built-in note velocity source.
    Velocity,
    /// Built-in key tracking source.
    KeyTracking,
    /// Periodic low-frequency oscillator.
    Lfo(CompiledLfo),
    /// Note lifecycle envelope.
    Envelope(CompiledModEnvelope),
    /// Note-scoped deterministic random value.
    Random(CompiledRandom),
    /// Multi-segment envelope.
    Mseg(CompiledMseg),
    /// Held step sequence.
    Step(CompiledStep),
    /// Deterministic sample-and-hold source.
    SampleHold(CompiledSampleHold),
    /// Deterministic smooth-random source.
    SmoothRandom(CompiledSmoothRandom),
}

/// Compiled LFO settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledLfo {
    /// LFO waveform.
    pub waveform: LfoWaveform,
    /// Rate value.
    pub rate: f32,
    /// Rate unit.
    pub rate_unit: ModulationRateUnit,
    /// Initial phase.
    pub phase: f32,
}

/// Compiled modulation envelope settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledModEnvelope {
    /// Sample-rate-specific envelope.
    pub envelope: CompiledAdsr,
}

/// Compiled deterministic random settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledRandom {
    /// Explicit source seed.
    pub seed: u64,
    /// FNV-1a hash of the stable source identifier, precomputed off the audio path.
    pub source_hash: u64,
}

/// Compiled MSEG segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledMsegSegment {
    /// Duration value.
    pub duration: f32,
    /// Duration unit.
    pub duration_unit: ModulationDurationUnit,
    /// Segment target.
    pub target: f32,
    /// Segment interpolation curve.
    pub curve: crate::definition::ModulationSegmentCurve,
}

/// Compiled MSEG source.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledMseg {
    /// Initial value.
    pub initial_value: f32,
    /// Compiled segments.
    pub segments: Box<[CompiledMsegSegment]>,
    /// Optional loop range.
    pub loop_range: Option<(usize, usize)>,
}

/// Compiled step source.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledStep {
    /// Held values.
    pub values: Box<[f32]>,
    /// Step rate.
    pub rate: f32,
    /// Rate unit.
    pub rate_unit: ModulationRateUnit,
}

/// Compiled sample-and-hold source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledSampleHold {
    /// Explicit seed.
    pub seed: u64,
    /// Source identifier hash.
    pub source_hash: u64,
    /// Update rate.
    pub rate: f32,
    /// Rate unit.
    pub rate_unit: ModulationRateUnit,
}

/// Compiled smooth-random source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledSmoothRandom {
    /// Explicit seed.
    pub seed: u64,
    /// Source identifier hash.
    pub source_hash: u64,
    /// Transition rate.
    pub rate: f32,
    /// Rate unit.
    pub rate_unit: ModulationRateUnit,
}

/// Compiled instrument-scoped source.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledInstrumentSource {
    /// Stable source identifier.
    pub id: String,
    /// Compiled source behavior.
    pub source: CompiledInstrumentSourceKind,
}

/// Compiled instrument-scoped source behavior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompiledInstrumentSourceKind {
    /// Shared pitch bend.
    PitchBend,
    /// Shared modulation wheel.
    ModWheel,
    /// Shared channel aftertouch.
    Aftertouch,
    /// Macro parameter source.
    Macro { parameter: ParameterHandle },
    /// Transport beat phase.
    BeatPhase,
    /// Transport bar phase.
    BarPhase,
    /// Shared external amplitude envelope.
    EnvelopeFollower(CompiledEnvelopeFollower),
}

/// Sample-rate-specific envelope follower settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledEnvelopeFollower {
    /// Attack coefficient.
    pub attack_coeff: f32,
    /// Release coefficient.
    pub release_coeff: f32,
    /// Input gain in linear amplitude.
    pub input_gain_linear: f32,
}

/// A source reference in a compiled route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledSourceRef {
    /// Voice-scoped source table entry.
    Voice(SourceHandle),
    /// Instrument-scoped source table entry.
    Instrument(InstrumentSourceHandle),
}

/// Compiled layer-vector binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledVector {
    /// Two-way crossfade.
    TwoWay {
        /// Position parameter.
        position: ParameterHandle,
        /// Bound layer indices.
        layers: [usize; 2],
    },
    /// Four-way XY mixer.
    FourWay {
        /// Horizontal axis parameter.
        x: ParameterHandle,
        /// Vertical axis parameter.
        y: ParameterHandle,
        /// Bound layer indices in top-left, top-right, bottom-left, bottom-right order.
        layers: [usize; 4],
    },
}

/// Compiled route with a fixed target and source evaluation order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledRoute {
    /// Compiled source reference.
    pub source: CompiledSourceRef,
    /// Target parameter handle.
    pub target: ParameterHandle,
    /// Signed depth in the target descriptor's modulation domain.
    pub depth: f32,
    /// Source curve.
    pub curve: ModulationCurve,
}

/// Contiguous route range for one target handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteRange {
    /// First route index.
    pub start: usize,
    /// Number of routes.
    pub len: usize,
}

type CompiledModulation = (
    Box<[CompiledSource]>,
    Box<[CompiledInstrumentSource]>,
    Box<[CompiledRoute]>,
    Box<[RouteRange]>,
);

#[allow(clippy::too_many_lines)]
pub(super) fn compile_modulation(
    definition: &InstrumentDefinition,
    catalog: &ParameterCatalog,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledModulation {
    let mut sources = Vec::with_capacity(BUILTIN_SOURCE_IDS.len());
    sources.push(CompiledSource {
        id: "velocity".to_owned(),
        source: CompiledVoiceSource::Velocity,
    });
    sources.push(CompiledSource {
        id: "key_tracking".to_owned(),
        source: CompiledVoiceSource::KeyTracking,
    });
    let mut source_lookup = HashMap::new();
    for (index, source) in sources.iter().enumerate() {
        source_lookup.insert(
            source.id.clone(),
            CompiledSourceRef::Voice(SourceHandle(index)),
        );
    }
    if let Some(modulation) = &definition.modulation {
        for (source_index, source) in modulation.sources.iter().enumerate() {
            if matches!(source, ModulationSourceDefinition::EnvelopeFollower(_)) {
                continue;
            }
            let compiled = match source {
                ModulationSourceDefinition::Lfo(value) => {
                    CompiledVoiceSource::Lfo(compile_lfo(value))
                }
                ModulationSourceDefinition::Envelope(value) => {
                    let envelope_path = format!("modulation.sources[{source_index}]");
                    CompiledVoiceSource::Envelope(CompiledModEnvelope {
                        envelope: compile_adsr(
                            AdsrDefinition {
                                attack_seconds: value.attack_seconds,
                                decay_seconds: value.decay_seconds,
                                sustain_level: value.sustain_level,
                                release_seconds: value.release_seconds,
                            },
                            sample_rate,
                            &envelope_path,
                            diagnostics,
                        ),
                    })
                }
                ModulationSourceDefinition::Random(value) => {
                    CompiledVoiceSource::Random(CompiledRandom {
                        seed: value.seed,
                        source_hash: source_id_hash(&value.id),
                    })
                }
                ModulationSourceDefinition::Mseg(value) => {
                    CompiledVoiceSource::Mseg(compile_mseg(value))
                }
                ModulationSourceDefinition::Step(value) => {
                    CompiledVoiceSource::Step(CompiledStep {
                        values: value.values.clone().into_boxed_slice(),
                        rate: value.rate.value,
                        rate_unit: value.rate.unit,
                    })
                }
                ModulationSourceDefinition::SampleHold(value) => {
                    CompiledVoiceSource::SampleHold(CompiledSampleHold {
                        seed: value.seed,
                        source_hash: source_id_hash(&value.id),
                        rate: value.rate.value,
                        rate_unit: value.rate.unit,
                    })
                }
                ModulationSourceDefinition::SmoothRandom(value) => {
                    CompiledVoiceSource::SmoothRandom(CompiledSmoothRandom {
                        seed: value.seed,
                        source_hash: source_id_hash(&value.id),
                        rate: value.rate.value,
                        rate_unit: value.rate.unit,
                    })
                }
                ModulationSourceDefinition::EnvelopeFollower(_) => continue,
            };
            let handle = SourceHandle(sources.len());
            source_lookup.insert(
                source_id(source).to_owned(),
                CompiledSourceRef::Voice(handle),
            );
            sources.push(CompiledSource {
                id: source_id(source).to_owned(),
                source: compiled,
            });
        }
    }

    let required_source_ids = definition
        .modulation
        .as_ref()
        .map(|modulation| {
            modulation
                .routes
                .iter()
                .map(|route| route.source.as_str())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let mut instrument_sources = Vec::new();
    for (id, source) in [
        ("pitch_bend", CompiledInstrumentSourceKind::PitchBend),
        ("mod_wheel", CompiledInstrumentSourceKind::ModWheel),
        ("aftertouch", CompiledInstrumentSourceKind::Aftertouch),
        (
            "transport_beat_phase",
            CompiledInstrumentSourceKind::BeatPhase,
        ),
        (
            "transport_bar_phase",
            CompiledInstrumentSourceKind::BarPhase,
        ),
    ] {
        if !required_source_ids.contains(id) {
            continue;
        }
        insert_instrument_source(id, source, &mut instrument_sources, &mut source_lookup);
    }
    for macro_definition in &definition.macros {
        let id = format!("macro.{}", macro_definition.id);
        if !required_source_ids.contains(id.as_str()) {
            continue;
        }
        let parameter = catalog
            .parameter_handle(&id)
            .expect("macro parameter catalog entry exists");
        insert_instrument_source(
            &id,
            CompiledInstrumentSourceKind::Macro { parameter },
            &mut instrument_sources,
            &mut source_lookup,
        );
    }
    if let Some(modulation) = &definition.modulation {
        for source in &modulation.sources {
            let ModulationSourceDefinition::EnvelopeFollower(value) = source else {
                continue;
            };
            if !required_source_ids.contains(value.id.as_str()) {
                continue;
            }
            insert_instrument_source(
                &value.id,
                CompiledInstrumentSourceKind::EnvelopeFollower(CompiledEnvelopeFollower {
                    attack_coeff: super::processor::time_constant_coefficient(
                        value.attack_ms / 1_000.0,
                        sample_rate,
                    ),
                    release_coeff: super::processor::time_constant_coefficient(
                        value.release_ms / 1_000.0,
                        sample_rate,
                    ),
                    input_gain_linear: db_to_linear(value.input_gain_db),
                }),
                &mut instrument_sources,
                &mut source_lookup,
            );
        }
    }
    let mut unresolved_routes = Vec::new();
    if let Some(modulation) = &definition.modulation {
        for (index, route) in modulation.routes.iter().enumerate() {
            let Some(target) = catalog.parameter_handle(&route.target) else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParameterNotFound,
                        "route target does not name a continuous parameter",
                    )
                    .with_path(format!("modulation.routes[{index}].target")),
                );
                continue;
            };
            let source = source_lookup.get(&route.source).copied();
            let Some(source) = source else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::SourceNotFound,
                        "route source is not defined",
                    )
                    .with_path(format!("modulation.routes[{index}].source")),
                );
                continue;
            };
            let descriptor = catalog
                .descriptor(target)
                .expect("compiled route target handle must be valid");
            if route.depth.unit != descriptor.modulation_unit() {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::RouteDepthUnitInvalid,
                        "route depth unit does not match the target parameter",
                    )
                    .with_path(format!("modulation.routes[{index}].depth.unit"))
                    .with_detail(format!(
                        "expected {:?}, received {:?}",
                        descriptor.modulation_unit(),
                        route.depth.unit
                    )),
                );
                continue;
            }
            let maximum = descriptor.max_modulation_depth();
            if !route.depth.value.is_finite() || route.depth.value.abs() > maximum {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::RouteDepthInvalid,
                        "route depth exceeds the target modulation range",
                    )
                    .with_path(format!("modulation.routes[{index}].depth.value"))
                    .with_detail(format!(
                        "allowed absolute depth is at most {maximum} {:?}",
                        descriptor.modulation_unit()
                    )),
                );
                continue;
            }
            if !route_source_allowed(descriptor.owner, source) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::GlobalRouteScopeInvalid,
                        "global processor targets accept only shared external controls",
                    )
                    .with_path(format!("modulation.routes[{index}].source")),
                );
                continue;
            }
            unresolved_routes.push((
                target.index(),
                CompiledRoute {
                    source,
                    target,
                    depth: route.depth.value,
                    curve: route.curve,
                },
            ));
        }
    }
    unresolved_routes.sort_by_key(|(target, _)| *target);
    let mut routes = Vec::with_capacity(unresolved_routes.len());
    let mut route_ranges = vec![RouteRange { start: 0, len: 0 }; catalog.len()];
    let mut index = 0;
    while index < unresolved_routes.len() {
        let target = unresolved_routes[index].0;
        let start = routes.len();
        while index < unresolved_routes.len() && unresolved_routes[index].0 == target {
            routes.push(unresolved_routes[index].1);
            index += 1;
        }
        route_ranges[target] = RouteRange {
            start,
            len: routes.len() - start,
        };
    }
    (
        sources.into_boxed_slice(),
        instrument_sources.into_boxed_slice(),
        routes.into_boxed_slice(),
        route_ranges.into_boxed_slice(),
    )
}

fn route_source_allowed(owner: ParameterOwner, source: CompiledSourceRef) -> bool {
    match owner {
        ParameterOwner::GlobalProcessor { .. } => {
            matches!(source, CompiledSourceRef::Instrument(_))
        }
        ParameterOwner::Macro { .. } => false,
        ParameterOwner::Layer { .. }
        | ParameterOwner::LayerGenerator { .. }
        | ParameterOwner::LayerProcessor { .. }
        | ParameterOwner::VoiceProcessor { .. }
        | ParameterOwner::VectorAxis { .. } => true,
    }
}

fn source_id(source: &ModulationSourceDefinition) -> &str {
    match source {
        ModulationSourceDefinition::Lfo(value) => &value.id,
        ModulationSourceDefinition::Envelope(value) => &value.id,
        ModulationSourceDefinition::Random(value) => &value.id,
        ModulationSourceDefinition::Mseg(value) => &value.id,
        ModulationSourceDefinition::Step(value) => &value.id,
        ModulationSourceDefinition::SampleHold(value) => &value.id,
        ModulationSourceDefinition::SmoothRandom(value) => &value.id,
        ModulationSourceDefinition::EnvelopeFollower(value) => &value.id,
    }
}

pub(crate) fn source_id_hash(source_id: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in source_id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn compile_lfo(value: &LfoDefinition) -> CompiledLfo {
    CompiledLfo {
        waveform: value.waveform,
        rate: value.rate.value,
        rate_unit: value.rate.unit,
        phase: value.phase,
    }
}

fn compile_mseg(value: &MsegDefinition) -> CompiledMseg {
    CompiledMseg {
        initial_value: value.initial_value,
        segments: value
            .segments
            .iter()
            .map(|segment| CompiledMsegSegment {
                duration: segment.duration.value,
                duration_unit: segment.duration.unit,
                target: segment.target,
                curve: segment.curve,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        loop_range: value
            .loop_range
            .map(|loop_range| (loop_range.start_segment, loop_range.end_segment)),
    }
}

fn insert_instrument_source(
    id: &str,
    source: CompiledInstrumentSourceKind,
    sources: &mut Vec<CompiledInstrumentSource>,
    lookup: &mut HashMap<String, CompiledSourceRef>,
) {
    let handle = InstrumentSourceHandle(sources.len());
    let previous = lookup.insert(id.to_owned(), CompiledSourceRef::Instrument(handle));
    debug_assert!(previous.is_none(), "source identifier was already compiled");
    sources.push(CompiledInstrumentSource {
        id: id.to_owned(),
        source,
    });
}

#[cfg(test)]
mod tests {
    use super::super::tests::{context, definition};
    use super::{CompiledInstrumentSourceKind, ModulationCurve};
    use crate::compile_instrument;
    use crate::diagnostics::DiagnosticCode;

    #[test]
    fn routes_resolve_to_dense_targets_and_preserve_same_target_order() {
        let mut source = definition();
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![],
            routes: vec![
                crate::definition::ModulationRouteDefinition {
                    source: "velocity".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 7.2,
                        unit: crate::parameter::ModulationUnit::Decibels,
                    },
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "key_tracking".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: -14.4,
                        unit: crate::parameter::ModulationUnit::Decibels,
                    },
                    curve: ModulationCurve::SmoothStep,
                },
            ],
        });
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("routes compile");
        let routes = compiled.routes_for(compiled.layers[0].parameters.gain);
        assert_eq!(routes.len(), 2);
        assert!((routes[0].depth - 7.2).abs() < f32::EPSILON);
        assert!((routes[1].depth + 14.4).abs() < f32::EPSILON);
    }

    #[test]
    fn route_depth_units_and_limits_are_checked_against_the_target_descriptor() {
        let mut source = definition();
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![],
            routes: vec![
                crate::definition::ModulationRouteDefinition {
                    source: "velocity".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 1.0,
                        unit: crate::parameter::ModulationUnit::Pan,
                    },
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "velocity".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 72.1,
                        unit: crate::parameter::ModulationUnit::Decibels,
                    },
                    curve: ModulationCurve::Linear,
                },
            ],
        });

        let result = compile_instrument(&source, &context());

        assert!(result.instrument.is_none());
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::RouteDepthUnitInvalid
                && diagnostic.path.as_deref() == Some("modulation.routes[0].depth.unit")
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::RouteDepthInvalid
                && diagnostic.path.as_deref() == Some("modulation.routes[1].depth.value")
        }));
    }

    #[test]
    fn macros_vectors_and_transport_sources_compile_as_instrument_bindings() {
        let mut source = definition();
        let mut bright = source.layers[0].clone();
        bright.id = "bright".to_owned();
        source.layers.push(bright);
        source.macros.push(crate::definition::MacroDefinition {
            id: "motion".to_owned(),
            name: "Motion".to_owned(),
            default: 0.25,
        });
        source
            .vectors
            .push(crate::definition::VectorDefinition::TwoWay {
                id: "tone".to_owned(),
                name: "Tone".to_owned(),
                layer_a: "body".to_owned(),
                layer_b: "bright".to_owned(),
                position: 0.25,
            });
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: Vec::new(),
            routes: vec![
                crate::definition::ModulationRouteDefinition {
                    source: "macro.motion".to_owned(),
                    target: "vector.tone.position".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 1.0,
                        unit: crate::parameter::ModulationUnit::Normalized,
                    },
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "transport_beat_phase".to_owned(),
                    target: "layer.body.tuning".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 20.0,
                        unit: crate::parameter::ModulationUnit::Cents,
                    },
                    curve: ModulationCurve::Linear,
                },
            ],
        });

        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("instrument bindings compile");

        assert_eq!(compiled.macro_definitions.len(), 1);
        assert_eq!(compiled.vector_definitions.len(), 1);
        assert_eq!(compiled.vectors.len(), 1);
        assert!(compiled.parameter_handle("macro.motion").is_some());
        assert!(compiled.parameter_handle("vector.tone.position").is_some());
        assert!(
            compiled
                .instrument_sources
                .iter()
                .any(|source| matches!(&source.source, CompiledInstrumentSourceKind::Macro { .. }))
        );
        assert!(
            compiled
                .instrument_sources
                .iter()
                .any(|source| matches!(&source.source, CompiledInstrumentSourceKind::BeatPhase))
        );
        assert_eq!(compiled.routes.len(), 2);
    }

    #[test]
    fn macro_parameters_cannot_be_modulation_targets() {
        let mut source = definition();
        source.macros.push(crate::definition::MacroDefinition {
            id: "motion".to_owned(),
            name: "Motion".to_owned(),
            default: 0.0,
        });
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: Vec::new(),
            routes: vec![crate::definition::ModulationRouteDefinition {
                source: "velocity".to_owned(),
                target: "macro.motion".to_owned(),
                depth: crate::definition::ModulationDepthDefinition {
                    value: 1.0,
                    unit: crate::parameter::ModulationUnit::Normalized,
                },
                curve: ModulationCurve::Linear,
            }],
        });

        let result = compile_instrument(&source, &context());

        assert!(result.instrument.is_none());
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::GlobalRouteScopeInvalid
                && diagnostic.path.as_deref() == Some("modulation.routes[0].source")
        }));
    }

    #[test]
    fn unresolved_routes_are_reported_without_panicking() {
        let mut source = definition();
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![],
            routes: vec![crate::definition::ModulationRouteDefinition {
                source: "missing".to_owned(),
                target: "layer.missing.gain".to_owned(),
                depth: crate::definition::ModulationDepthDefinition {
                    value: 0.1,
                    unit: crate::parameter::ModulationUnit::Normalized,
                },
                curve: ModulationCurve::Linear,
            }],
        });
        let result = compile_instrument(&source, &context());
        assert!(result.instrument.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::ParameterNotFound)
        );
    }
}
