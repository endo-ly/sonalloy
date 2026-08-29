#[allow(clippy::wildcard_imports)]
use super::*;

/// Modulation sources and routes stored in a Definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationDefinition {
    /// User-defined source definitions.
    pub sources: Vec<ModulationSourceDefinition>,
    /// Source-to-parameter connections.
    pub routes: Vec<ModulationRouteDefinition>,
}

/// A user-defined voice-scoped modulation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModulationSourceDefinition {
    /// Periodic bipolar source.
    Lfo(LfoDefinition),
    /// Note lifecycle envelope source.
    Envelope(ModEnvelopeDefinition),
    /// Deterministic note-scoped sample-and-hold source.
    Random(RandomDefinition),
    /// Multi-segment envelope source.
    Mseg(MsegDefinition),
    /// Held step sequence source.
    Step(StepModulatorDefinition),
    /// Deterministic stepped random source.
    SampleHold(SampleHoldDefinition),
    /// Deterministic interpolated random source.
    SmoothRandom(SmoothRandomDefinition),
    /// Instrument-scoped amplitude envelope of the external audio bus.
    EnvelopeFollower(EnvelopeFollowerDefinition),
}

/// External audio envelope follower settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeFollowerDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Envelope attack time in milliseconds.
    pub attack_ms: f32,
    /// Envelope release time in milliseconds.
    pub release_ms: f32,
    /// External input gain in decibels.
    pub input_gain_db: f32,
}

/// LFO source settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LfoDefinition {
    /// Stable source identifier.
    pub id: String,
    /// LFO waveform.
    pub waveform: LfoWaveform,
    /// Oscillation rate.
    pub rate: ModulationRateDefinition,
    /// Initial phase in the half-open zero-to-one range.
    pub phase: f32,
}

/// Rate unit used by periodic and stepped modulation sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationRateUnit {
    /// Cycles, steps, or transitions per second.
    PerSecond,
    /// Cycles, steps, or transitions per quarter-note beat.
    PerBeat,
}

/// Rate used by periodic and stepped modulation sources.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationRateDefinition {
    /// Rate value.
    pub value: f32,
    /// Rate unit.
    pub unit: ModulationRateUnit,
}

/// Duration unit used by MSEG segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationDurationUnit {
    /// Duration in seconds.
    Seconds,
    /// Duration in quarter-note beats.
    Beats,
}

/// Duration used by an MSEG segment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationDurationDefinition {
    /// Duration value.
    pub value: f32,
    /// Duration unit.
    pub unit: ModulationDurationUnit,
}

/// MSEG segment curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationSegmentCurve {
    /// Linear interpolation.
    Linear,
    /// Smooth-step interpolation.
    SmoothStep,
}

/// One MSEG segment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MsegSegmentDefinition {
    /// Segment duration.
    pub duration: ModulationDurationDefinition,
    /// Segment target in the bipolar source range.
    pub target: f32,
    /// Segment interpolation curve.
    pub curve: ModulationSegmentCurve,
}

/// Optional MSEG loop range. The end index is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MsegLoopDefinition {
    /// First segment in the loop.
    pub start_segment: usize,
    /// Exclusive end segment in the loop.
    pub end_segment: usize,
}

/// Multi-segment voice modulation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MsegDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Initial source value.
    pub initial_value: f32,
    /// Ordered segments.
    pub segments: Vec<MsegSegmentDefinition>,
    /// Optional loop range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_range: Option<MsegLoopDefinition>,
}

/// Held step sequence modulation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepModulatorDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Bipolar values held for each step.
    pub values: Vec<f32>,
    /// Step rate.
    pub rate: ModulationRateDefinition,
}

/// Deterministic sample-and-hold modulation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleHoldDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Explicit deterministic seed.
    pub seed: u64,
    /// Update rate.
    pub rate: ModulationRateDefinition,
}

/// Deterministic smooth-random modulation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmoothRandomDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Explicit deterministic seed.
    pub seed: u64,
    /// Transition rate.
    pub rate: ModulationRateDefinition,
}

/// A named normalized instrument control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacroDefinition {
    /// Stable macro identifier.
    pub id: String,
    /// Human-readable macro name.
    pub name: String,
    /// Initial normalized value.
    pub default: f32,
}

/// A constant-power layer vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum VectorDefinition {
    /// Two-layer constant-power crossfade.
    TwoWay {
        /// Stable vector identifier.
        id: String,
        /// Human-readable vector name.
        name: String,
        /// Layer at position zero.
        layer_a: LayerId,
        /// Layer at position one.
        layer_b: LayerId,
        /// Initial position.
        position: f32,
    },
    /// Four-layer constant-power XY mixer.
    FourWay {
        /// Stable vector identifier.
        id: String,
        /// Human-readable vector name.
        name: String,
        /// Top-left layer.
        top_left: LayerId,
        /// Top-right layer.
        top_right: LayerId,
        /// Bottom-left layer.
        bottom_left: LayerId,
        /// Bottom-right layer.
        bottom_right: LayerId,
        /// Initial horizontal position.
        x: f32,
        /// Initial vertical position.
        y: f32,
    },
}

/// LFO waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LfoWaveform {
    /// Sine waveform.
    Sine,
    /// Bipolar triangle waveform.
    Triangle,
}

/// Note lifecycle modulation envelope settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModEnvelopeDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Attack duration in seconds.
    pub attack_seconds: f32,
    /// Decay duration in seconds.
    pub decay_seconds: f32,
    /// Sustain level.
    pub sustain_level: f32,
    /// Release duration in seconds.
    pub release_seconds: f32,
}

/// Note-scoped deterministic random source settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomDefinition {
    /// Stable source identifier.
    pub id: String,
    /// Explicit deterministic seed.
    pub seed: u64,
}

/// Source-to-target modulation connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationRouteDefinition {
    /// Source identifier, including built-in source identifiers.
    pub source: String,
    /// Canonical parameter identifier.
    pub target: String,
    /// Signed modulation depth in the target's declared modulation unit.
    pub depth: ModulationDepthDefinition,
    /// Source shaping curve.
    pub curve: ModulationCurve,
}

/// Signed modulation depth written by an instrument author.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationDepthDefinition {
    /// Signed depth applied when the shaped source reaches one.
    pub value: f32,
    /// Unit required by the target parameter.
    pub unit: ModulationUnit,
}

/// Curve applied to a source before its route depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationCurve {
    /// No shaping.
    Linear,
    /// Smooth interpolation around zero.
    SmoothStep,
}

pub(super) fn validate_modulation(
    diagnostics: &mut Vec<Diagnostic>,
    modulation: &ModulationDefinition,
) {
    validate_modulation_sources(diagnostics, modulation);
    validate_modulation_routes(diagnostics, modulation);
}

#[allow(clippy::too_many_lines)]
fn validate_modulation_sources(
    diagnostics: &mut Vec<Diagnostic>,
    modulation: &ModulationDefinition,
) {
    let mut mseg_count = 0;
    let mut step_count = 0;
    let mut sample_hold_count = 0;
    let mut smooth_random_count = 0;
    if modulation.sources.len() > MAX_VOICE_MODULATION_SOURCES {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                format!(
                    "modulation sources must contain at most {MAX_VOICE_MODULATION_SOURCES} entries"
                ),
            )
            .with_path("modulation.sources"),
        );
    }
    let mut source_ids = HashSet::new();
    for (index, source) in modulation.sources.iter().enumerate() {
        validate_modulation_source_id(diagnostics, &mut source_ids, index, source_id(source));
        match source {
            ModulationSourceDefinition::Lfo(value) => {
                validate_rate(
                    diagnostics,
                    &format!("modulation.sources[{index}].rate"),
                    value.rate,
                );
                if !value.phase.is_finite() || !(0.0..1.0).contains(&value.phase) {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::SourceValueInvalid,
                            "lfo phase must be finite and between 0 and 1",
                        )
                        .with_path(format!("modulation.sources[{index}].phase")),
                    );
                }
            }
            ModulationSourceDefinition::Envelope(value) => {
                validate_modulation_envelope(diagnostics, index, value);
            }
            ModulationSourceDefinition::Random(_) => {}
            ModulationSourceDefinition::Mseg(value) => {
                mseg_count += 1;
                validate_mseg(diagnostics, index, value);
            }
            ModulationSourceDefinition::Step(value) => {
                step_count += 1;
                if value.values.is_empty() || value.values.len() > 64 {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "step values must contain between 1 and 64 entries",
                        )
                        .with_path(format!("modulation.sources[{index}].values")),
                    );
                }
                for (value_index, value) in value.values.iter().enumerate() {
                    validate_range(
                        diagnostics,
                        format!("modulation.sources[{index}].values[{value_index}]"),
                        *value,
                        -1.0..=1.0,
                        "step value must be finite and between -1 and 1",
                    );
                }
                validate_rate(
                    diagnostics,
                    &format!("modulation.sources[{index}].rate"),
                    value.rate,
                );
            }
            ModulationSourceDefinition::SampleHold(value) => {
                sample_hold_count += 1;
                validate_rate(
                    diagnostics,
                    &format!("modulation.sources[{index}].rate"),
                    value.rate,
                );
            }
            ModulationSourceDefinition::SmoothRandom(value) => {
                smooth_random_count += 1;
                validate_rate(
                    diagnostics,
                    &format!("modulation.sources[{index}].rate"),
                    value.rate,
                );
            }
            ModulationSourceDefinition::EnvelopeFollower(value) => {
                validate_range(
                    diagnostics,
                    format!("modulation.sources[{index}].attack_ms"),
                    value.attack_ms,
                    0.1..=200.0,
                    "envelope follower attack_ms must be finite and between 0.1 and 200 ms",
                );
                validate_range(
                    diagnostics,
                    format!("modulation.sources[{index}].release_ms"),
                    value.release_ms,
                    1.0..=2_000.0,
                    "envelope follower release_ms must be finite and between 1 and 2000 ms",
                );
                validate_range(
                    diagnostics,
                    format!("modulation.sources[{index}].input_gain_db"),
                    value.input_gain_db,
                    -24.0..=24.0,
                    "envelope follower input_gain_db must be finite and between -24 and 24 dB",
                );
            }
        }
    }
    validate_modulation_source_limits(
        diagnostics,
        mseg_count,
        step_count,
        sample_hold_count,
        smooth_random_count,
    );
}

fn validate_modulation_source_limits(
    diagnostics: &mut Vec<Diagnostic>,
    mseg_count: usize,
    step_count: usize,
    sample_hold_count: usize,
    smooth_random_count: usize,
) {
    for (count, limit, name) in [
        (mseg_count, MAX_MSEG_SOURCES, "mseg"),
        (step_count, MAX_STEP_SOURCES, "step"),
        (sample_hold_count, MAX_SAMPLE_HOLD_SOURCES, "sample_hold"),
        (
            smooth_random_count,
            MAX_SMOOTH_RANDOM_SOURCES,
            "smooth_random",
        ),
    ] {
        if count > limit {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    format!("{name} sources must contain at most {limit} entries"),
                )
                .with_path("modulation.sources"),
            );
        }
    }
}

fn validate_modulation_source_id<'a>(
    diagnostics: &mut Vec<Diagnostic>,
    source_ids: &mut HashSet<&'a str>,
    index: usize,
    id: &'a str,
) {
    let path = format!("modulation.sources[{index}].id");
    if !is_component_id(id) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::SourceIdInvalid,
                "source id has invalid format",
            )
            .with_path(path.clone()),
        );
    }
    if BUILTIN_SOURCE_IDS.contains(&id) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::SourceIdDuplicated,
                "user source id conflicts with a built-in source",
            )
            .with_path(path.clone()),
        );
    }
    if !source_ids.insert(id) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::SourceIdDuplicated,
                "source id must be unique",
            )
            .with_path(path),
        );
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

fn validate_modulation_routes(
    diagnostics: &mut Vec<Diagnostic>,
    modulation: &ModulationDefinition,
) {
    for (index, route) in modulation.routes.iter().enumerate() {
        if !is_source_id(&route.source) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::SourceIdInvalid,
                    "route source id is invalid",
                )
                .with_path(format!("modulation.routes[{index}].source")),
            );
        }
        if !is_parameter_id(&route.target) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::RouteTargetInvalid,
                    "route target has invalid format",
                )
                .with_path(format!("modulation.routes[{index}].target")),
            );
        }
        validate_finite(
            diagnostics,
            format!("modulation.routes[{index}].depth.value"),
            route.depth.value,
            "route depth value must be finite",
        );
    }
}

fn is_source_id(value: &str) -> bool {
    if is_component_id(value) {
        return true;
    }
    let parts: Vec<_> = value.split('.').collect();
    matches!(parts.as_slice(), ["macro", macro_id] if is_component_id(macro_id))
}

fn validate_rate(diagnostics: &mut Vec<Diagnostic>, path: &str, rate: ModulationRateDefinition) {
    let range = match rate.unit {
        ModulationRateUnit::PerSecond => 0.01..=40.0,
        ModulationRateUnit::PerBeat => (1.0 / 64.0)..=16.0,
    };
    validate_range(
        diagnostics,
        format!("{path}.value"),
        rate.value,
        range,
        "modulation rate must be finite and within its unit range",
    );
}

fn validate_mseg(diagnostics: &mut Vec<Diagnostic>, index: usize, mseg: &MsegDefinition) {
    let path = format!("modulation.sources[{index}]");
    validate_range(
        diagnostics,
        format!("{path}.initial_value"),
        mseg.initial_value,
        -1.0..=1.0,
        "mseg initial_value must be finite and between -1 and 1",
    );
    if mseg.segments.is_empty() || mseg.segments.len() > 64 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "mseg segments must contain between 1 and 64 entries",
            )
            .with_path(format!("{path}.segments")),
        );
    }
    for (segment_index, segment) in mseg.segments.iter().enumerate() {
        validate_range(
            diagnostics,
            format!("{path}.segments[{segment_index}].target"),
            segment.target,
            -1.0..=1.0,
            "mseg target must be finite and between -1 and 1",
        );
        let duration_range = match segment.duration.unit {
            ModulationDurationUnit::Seconds => f32::EPSILON..=100.0,
            ModulationDurationUnit::Beats => (1.0 / 128.0)..=64.0,
        };
        validate_range(
            diagnostics,
            format!("{path}.segments[{segment_index}].duration.value"),
            segment.duration.value,
            duration_range,
            "mseg duration must be finite and within its unit range",
        );
    }
    if let Some(loop_range) = mseg.loop_range
        && (loop_range.start_segment >= loop_range.end_segment
            || loop_range.end_segment > mseg.segments.len())
    {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "mseg loop range must be within the segment list and non-empty",
            )
            .with_path(format!("{path}.loop_range")),
        );
    }
}

pub(super) fn validate_macros(diagnostics: &mut Vec<Diagnostic>, macros: &[MacroDefinition]) {
    if macros.len() > 16 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "macros must contain at most 16 entries",
            )
            .with_path("macros"),
        );
    }
    let mut ids = HashSet::new();
    for (index, value) in macros.iter().enumerate() {
        let path = format!("macros[{index}]");
        if !is_component_id(&value.id) {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::ParameterIdInvalid, "macro id is invalid")
                    .with_path(format!("{path}.id")),
            );
        }
        if !ids.insert(&value.id) {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::IdDuplicated, "macro id must be unique")
                    .with_path(format!("{path}.id")),
            );
        }
        validate_range(
            diagnostics,
            format!("{path}.default"),
            value.default,
            0.0..=1.0,
            "macro default must be finite and between 0 and 1",
        );
    }
}

pub(super) fn validate_vectors(
    diagnostics: &mut Vec<Diagnostic>,
    vectors: &[VectorDefinition],
    layers: &[LayerDefinition],
) {
    if vectors.len() > 8 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "vectors must contain at most 8 entries",
            )
            .with_path("vectors"),
        );
    }
    let mut vector_ids = HashSet::new();
    let mut assigned_layers = HashSet::new();
    for (index, vector) in vectors.iter().enumerate() {
        let path = format!("vectors[{index}]");
        let (id, layer_ids_for_vector, axes) = match vector {
            VectorDefinition::TwoWay {
                id,
                layer_a,
                layer_b,
                position,
                ..
            } => (id, vec![layer_a, layer_b], vec![("position", *position)]),
            VectorDefinition::FourWay {
                id,
                top_left,
                top_right,
                bottom_left,
                bottom_right,
                x,
                y,
                ..
            } => (
                id,
                vec![top_left, top_right, bottom_left, bottom_right],
                vec![("x", *x), ("y", *y)],
            ),
        };
        if !is_component_id(id) {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::ParameterIdInvalid, "vector id is invalid")
                    .with_path(format!("{path}.id")),
            );
        }
        if !vector_ids.insert(id) {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::IdDuplicated, "vector id must be unique")
                    .with_path(format!("{path}.id")),
            );
        }
        let mut local_layers = HashSet::new();
        for (layer_index, layer_id) in layer_ids_for_vector.iter().enumerate() {
            let Some(layer) = layers.iter().find(|layer| layer.id == **layer_id) else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParameterNotFound,
                        "vector layer does not exist",
                    )
                    .with_path(format!("{path}.layer_{layer_index}")),
                );
                continue;
            };
            if !layer.enabled {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "vector layer must be enabled",
                    )
                    .with_path(format!("{path}.layer_{layer_index}")),
                );
            }
            if !local_layers.insert(*layer_id) {
                diagnostics.push(
                    Diagnostic::error(DiagnosticCode::IdDuplicated, "vector layers must be unique")
                        .with_path(format!("{path}.layer_{layer_index}")),
                );
            }
            if !assigned_layers.insert(*layer_id) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::IdDuplicated,
                        "a layer may belong to only one vector",
                    )
                    .with_path(format!("{path}.layer_{layer_index}")),
                );
            }
        }
        for (axis, value) in axes {
            validate_range(
                diagnostics,
                format!("{path}.{axis}"),
                value,
                0.0..=1.0,
                "vector axis must be finite and between 0 and 1",
            );
        }
    }
}

fn validate_modulation_envelope(
    diagnostics: &mut Vec<Diagnostic>,
    index: usize,
    envelope: &ModEnvelopeDefinition,
) {
    for (field, value) in [
        ("attack_seconds", envelope.attack_seconds),
        ("decay_seconds", envelope.decay_seconds),
        ("release_seconds", envelope.release_seconds),
    ] {
        validate_range(
            diagnostics,
            format!("modulation.sources[{index}].{field}"),
            value,
            0.0..=30.0,
            "modulation envelope time must be finite and between 0 and 30 seconds",
        );
    }
    validate_range(
        diagnostics,
        format!("modulation.sources[{index}].sustain_level"),
        envelope.sustain_level,
        0.0..=1.0,
        "modulation envelope sustain must be finite and between 0 and 1",
    );
}

#[cfg(test)]
mod tests {
    use super::super::tests::definition;
    use super::*;

    #[test]
    fn performance_modulation_and_vector_schema_round_trip() {
        let mut source = definition();
        let mut bright = source.layers[0].clone();
        bright.id = "bright".to_owned();
        source.layers.push(bright);
        source.performance = PerformanceDefinition::Monophonic {
            legato: true,
            portamento: Some(PortamentoDefinition { time_seconds: 0.1 }),
        };
        source.macros.push(MacroDefinition {
            id: "motion".to_owned(),
            name: "Motion".to_owned(),
            default: 0.25,
        });
        source.vectors.push(VectorDefinition::TwoWay {
            id: "tone".to_owned(),
            name: "Tone".to_owned(),
            layer_a: "body".to_owned(),
            layer_b: "bright".to_owned(),
            position: 0.5,
        });
        source.modulation = Some(ModulationDefinition {
            sources: vec![
                ModulationSourceDefinition::Lfo(LfoDefinition {
                    id: "tempo_lfo".to_owned(),
                    waveform: LfoWaveform::Triangle,
                    rate: ModulationRateDefinition {
                        value: 1.0,
                        unit: ModulationRateUnit::PerBeat,
                    },
                    phase: 0.25,
                }),
                ModulationSourceDefinition::Mseg(MsegDefinition {
                    id: "motion_env".to_owned(),
                    initial_value: -1.0,
                    segments: vec![
                        MsegSegmentDefinition {
                            duration: ModulationDurationDefinition {
                                value: 0.25,
                                unit: ModulationDurationUnit::Beats,
                            },
                            target: 1.0,
                            curve: ModulationSegmentCurve::SmoothStep,
                        },
                        MsegSegmentDefinition {
                            duration: ModulationDurationDefinition {
                                value: 0.1,
                                unit: ModulationDurationUnit::Seconds,
                            },
                            target: 0.0,
                            curve: ModulationSegmentCurve::Linear,
                        },
                    ],
                    loop_range: Some(MsegLoopDefinition {
                        start_segment: 0,
                        end_segment: 2,
                    }),
                }),
                ModulationSourceDefinition::Step(StepModulatorDefinition {
                    id: "steps".to_owned(),
                    values: vec![-1.0, 0.0, 1.0],
                    rate: ModulationRateDefinition {
                        value: 2.0,
                        unit: ModulationRateUnit::PerSecond,
                    },
                }),
                ModulationSourceDefinition::SampleHold(SampleHoldDefinition {
                    id: "sample_hold".to_owned(),
                    seed: 7,
                    rate: ModulationRateDefinition {
                        value: 2.0,
                        unit: ModulationRateUnit::PerSecond,
                    },
                }),
                ModulationSourceDefinition::SmoothRandom(SmoothRandomDefinition {
                    id: "smooth_random".to_owned(),
                    seed: 11,
                    rate: ModulationRateDefinition {
                        value: 1.0,
                        unit: ModulationRateUnit::PerBeat,
                    },
                }),
            ],
            routes: vec![ModulationRouteDefinition {
                source: "macro.motion".to_owned(),
                target: "vector.tone.position".to_owned(),
                depth: ModulationDepthDefinition {
                    value: 1.0,
                    unit: ModulationUnit::Normalized,
                },
                curve: ModulationCurve::Linear,
            }],
        });

        assert!(source.validate().is_empty());
        let json = serde_json::to_string(&source).expect("definition serializes");
        let restored: InstrumentDefinition =
            serde_json::from_str(&json).expect("definition parses");
        assert_eq!(source, restored);
    }

    #[test]
    fn performance_modulation_and_vector_ranges_report_field_paths() {
        let mut source = definition();
        source.performance = PerformanceDefinition::Monophonic {
            legato: false,
            portamento: Some(PortamentoDefinition { time_seconds: 0.0 }),
        };
        source.macros = vec![
            MacroDefinition {
                id: "motion".to_owned(),
                name: "Motion".to_owned(),
                default: 1.5,
            },
            MacroDefinition {
                id: "motion".to_owned(),
                name: "Duplicate".to_owned(),
                default: 0.0,
            },
        ];
        source.vectors.push(VectorDefinition::TwoWay {
            id: "tone".to_owned(),
            name: "Tone".to_owned(),
            layer_a: "missing".to_owned(),
            layer_b: "body".to_owned(),
            position: 0.5,
        });
        source.modulation = Some(ModulationDefinition {
            sources: vec![ModulationSourceDefinition::Step(StepModulatorDefinition {
                id: "steps".to_owned(),
                values: Vec::new(),
                rate: ModulationRateDefinition {
                    value: 0.0,
                    unit: ModulationRateUnit::PerSecond,
                },
            })],
            routes: Vec::new(),
        });

        let diagnostics = source.validate();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("performance.portamento.time_seconds")
        }));
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path.as_deref() == Some("macros[0].default"))
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("macros[1].id")
                && diagnostic.code == DiagnosticCode::IdDuplicated
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("vectors[0].layer_0")
                && diagnostic.code == DiagnosticCode::ParameterNotFound
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("modulation.sources[0].values")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("modulation.sources[0].rate.value")
        }));
    }

    #[test]
    fn modulation_source_resource_limits_are_validated() {
        let mut source = definition();
        source.modulation = Some(ModulationDefinition {
            sources: (0..65)
                .map(|index| {
                    ModulationSourceDefinition::Lfo(LfoDefinition {
                        id: format!("lfo_{index}"),
                        waveform: LfoWaveform::Sine,
                        rate: ModulationRateDefinition {
                            value: 1.0,
                            unit: ModulationRateUnit::PerSecond,
                        },
                        phase: 0.0,
                    })
                })
                .collect(),
            routes: Vec::new(),
        });

        let diagnostics = source.validate();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("modulation.sources")
                && diagnostic.code == DiagnosticCode::ValueOutOfRange
        }));
    }

    #[test]
    fn routes_require_depth_and_reject_legacy_amount_fields() {
        let mut legacy = serde_json::to_value(definition()).expect("definition serializes");
        legacy["modulation"] = serde_json::json!({
            "sources": [],
            "routes": [{
                "source": "velocity",
                "target": "layer.body.gain",
                "amount": 0.5,
                "curve": "linear"
            }]
        });
        assert!(serde_json::from_value::<InstrumentDefinition>(legacy).is_err());

        let mut unknown_depth = serde_json::to_value(definition()).expect("definition serializes");
        unknown_depth["modulation"] = serde_json::json!({
            "sources": [],
            "routes": [{
                "source": "velocity",
                "target": "layer.body.gain",
                "depth": {"value": 1.0, "unit": "decibels", "extra": 1},
                "curve": "linear"
            }]
        });
        assert!(serde_json::from_value::<InstrumentDefinition>(unknown_depth).is_err());
    }

    #[test]
    fn modulation_route_identifiers_are_validated_without_trimming() {
        let mut value = definition();
        value.modulation = Some(ModulationDefinition {
            sources: vec![],
            routes: vec![
                ModulationRouteDefinition {
                    source: "bad-id".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    depth: ModulationDepthDefinition {
                        value: 0.0,
                        unit: crate::parameter::ModulationUnit::Decibels,
                    },
                    curve: ModulationCurve::Linear,
                },
                ModulationRouteDefinition {
                    source: "velocity".to_owned(),
                    target: " layer.body.gain".to_owned(),
                    depth: ModulationDepthDefinition {
                        value: 0.0,
                        unit: crate::parameter::ModulationUnit::Decibels,
                    },
                    curve: ModulationCurve::Linear,
                },
            ],
        });
        let diagnostics = value.validate();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::SourceIdInvalid
                && diagnostic.path.as_deref() == Some("modulation.routes[0].source")
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::RouteTargetInvalid
                && diagnostic.path.as_deref() == Some("modulation.routes[1].target")
        }));
    }
}
