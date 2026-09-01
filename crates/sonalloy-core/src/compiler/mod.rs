use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) mod convolution;
mod generator;
mod modulation;
mod processor;
pub(crate) mod spectral;
pub(crate) mod wavetable;

pub use generator::*;
pub use modulation::*;
pub use processor::*;

use crate::asset::{AssetError, PreparedAsset, prepare_asset, resolved_asset_path};
use crate::definition::{
    AdsrDefinition, AssetReference, ExternalAudioChannels, InstrumentDefinition, LayerTriggerEvent,
    VectorDefinition, VoiceStealingDefinition,
};
use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSeverity};
use crate::parameter::{
    ParameterCatalog, ParameterCatalogRevision, ParameterHandle, layer_parameter_id,
};
use crate::process::ProcessSpec;

pub(crate) const BASIC_FREQUENCY_LIMIT_RATIO: f64 = 0.45;
pub(crate) const PHYSICAL_FREQUENCY_LIMIT_RATIO: f64 = BASIC_FREQUENCY_LIMIT_RATIO;

pub(crate) fn effective_max_frequency(sample_rate: f64, ratio: f64) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    {
        (sample_rate * ratio) as f32
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AssetCacheKey {
    path: PathBuf,
    sha256: Option<String>,
    sample_rate_bits: u64,
}

pub(crate) fn prepare_cached_asset(
    reference: &AssetReference,
    definition_base_dir: &Path,
    sample_rate: f64,
    asset_cache: &mut HashMap<AssetCacheKey, Result<PreparedAsset, AssetError>>,
) -> Result<PreparedAsset, AssetError> {
    let resolved = resolved_asset_path(definition_base_dir, &reference.path);
    let path = std::fs::canonicalize(&resolved).unwrap_or(resolved);
    let key = AssetCacheKey {
        path,
        sha256: reference
            .sha256
            .as_ref()
            .map(|value| value.to_ascii_lowercase()),
        sample_rate_bits: sample_rate.to_bits(),
    };
    if let Some(result) = asset_cache.get(&key) {
        return result.clone();
    }
    let result = prepare_asset(reference, definition_base_dir, sample_rate);
    asset_cache.insert(key, result.clone());
    result
}

pub(crate) fn source_id_hash(source_id: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in source_id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
pub const VOCODER_BANDS: usize = 24;
pub const SPECTRAL_MORPH_FFT_SIZE: usize = 1024;
pub const SPECTRAL_MORPH_HOP_SIZE: usize = 256;
pub const SPECTRAL_MORPH_LATENCY_FRAMES: usize = 1024;

pub use spectral::PreparedSpectralAsset;

/// Input required to compile a Definition for one engine configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct CompileContext {
    /// Directory used to resolve referenced assets.
    pub definition_base_dir: PathBuf,
    /// Engine sample rate and block configuration.
    pub process_spec: ProcessSpec,
}

/// Result of compiling an instrument.
#[derive(Debug, Clone)]
pub struct CompileResult {
    /// Immutable compiled instrument when no error diagnostics were produced.
    pub instrument: Option<Arc<CompiledInstrument>>,
    /// Errors and warnings collected during compilation.
    pub diagnostics: Vec<Diagnostic>,
}

/// Runtime-independent instrument configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledInstrument {
    /// Sample rate used to compile sample and time-dependent values.
    pub process_sample_rate: f64,
    /// Maximum latency used to align layer paths before voice mixing.
    pub(crate) layer_alignment_latency_frames: usize,
    /// Fixed latency from instrument input events to final output.
    pub reported_latency_frames: usize,
    /// External audio bus requirement, when present.
    pub external_audio: Option<CompiledExternalAudioInput>,
    /// Metadata copied from the Definition.
    pub metadata: CompiledMetadata,
    /// Compiled performance settings.
    pub performance: CompiledPerformance,
    /// Enabled layers in Definition order.
    pub layers: Box<[CompiledLayer]>,
    /// Processors applied after the layer mix for each voice.
    pub voice_processors: Box<[CompiledProcessor]>,
    /// Processors applied after the voice sum for the instrument.
    pub global_processors: Box<[CompiledProcessor]>,
    /// Dense continuous parameter catalog.
    pub parameter_catalog: ParameterCatalog,
    /// Product-level maximum values after processor-specific runtime limits.
    pub(crate) effective_parameter_maxima: Box<[f32]>,
    /// Voice-scoped source table.
    pub sources: Box<[CompiledSource]>,
    /// Voice sources referenced by at least one compiled route.
    pub(crate) used_voice_sources: Box<[bool]>,
    /// Instrument-scoped source table.
    pub instrument_sources: Box<[CompiledInstrumentSource]>,
    /// Compiled vector bindings.
    pub vectors: Box<[CompiledVector]>,
    /// Macro definitions retained for inspection metadata.
    pub macro_definitions: Box<[crate::definition::MacroDefinition]>,
    /// Vector definitions retained for inspection metadata.
    pub vector_definitions: Box<[VectorDefinition]>,
    /// Compiled routes grouped by target handle.
    pub routes: Box<[CompiledRoute]>,
    /// Route range for each parameter handle.
    pub route_ranges: Box<[RouteRange]>,
    /// Warnings retained for inspection and review output.
    pub diagnostics: Box<[Diagnostic]>,
}

impl CompiledInstrument {
    /// Return parameter descriptors in stable Definition order.
    #[must_use]
    pub fn parameters(&self) -> &[crate::parameter::ParameterDescriptor] {
        self.parameter_catalog.parameters()
    }

    /// Return the latency used to align layer paths before voice mixing.
    #[must_use]
    pub fn layer_alignment_latency_frames(&self) -> usize {
        self.layer_alignment_latency_frames
    }

    /// Return the fixed latency from instrument input events to output.
    #[must_use]
    pub fn reported_latency_frames(&self) -> usize {
        self.reported_latency_frames
    }

    /// Return the deterministic revision of the parameter catalog.
    #[must_use]
    pub fn parameter_catalog_revision(&self) -> ParameterCatalogRevision {
        self.parameter_catalog.revision()
    }

    /// Resolve a canonical parameter identifier for control code.
    #[must_use]
    pub fn parameter_handle(&self, id: &str) -> Option<ParameterHandle> {
        self.parameter_catalog.parameter_handle(id)
    }

    /// Return one descriptor by handle.
    #[must_use]
    pub fn parameter_descriptor(
        &self,
        handle: ParameterHandle,
    ) -> Option<&crate::parameter::ParameterDescriptor> {
        self.parameter_catalog.descriptor(handle)
    }

    pub(crate) fn effective_parameter_maximum(&self, handle: ParameterHandle) -> Option<f32> {
        self.effective_parameter_maxima.get(handle.index()).copied()
    }

    /// Return the route slice for one target handle.
    #[must_use]
    pub fn routes_for(&self, handle: ParameterHandle) -> &[CompiledRoute] {
        self.routes_for_checked(handle).unwrap_or(&[])
    }

    pub(crate) fn routes_for_checked(&self, handle: ParameterHandle) -> Option<&[CompiledRoute]> {
        let range = self.route_ranges.get(handle.index())?;
        let end = range.start.checked_add(range.len)?;
        self.routes.get(range.start..end)
    }

    /// Return the number of external input channels required by this instrument.
    #[must_use]
    pub fn required_input_channels(&self) -> usize {
        self.external_audio
            .map_or(0, |input| input.channels.channel_count())
    }
}

/// Compiled external input channel requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledExternalAudioInput {
    /// Required input layout.
    pub channels: ExternalAudioChannels,
}

/// Compiled metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledMetadata {
    /// Instrument name.
    pub name: String,
    /// Optional author.
    pub author: Option<String>,
    /// Optional description.
    pub description: Option<String>,
}

/// Compiled performance settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledPerformance {
    /// Performance mode and transition policy.
    pub mode: CompiledPerformanceMode,
    /// Number of prepared voices.
    pub voice_count: usize,
}

/// Compiled performance mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledPerformanceMode {
    /// Polyphonic voice allocation.
    Polyphonic {
        /// Voice stealing policy.
        voice_stealing: CompiledVoiceStealing,
    },
    /// Last-note-priority monophonic performance.
    Monophonic {
        /// Whether connected notes retain voice state.
        legato: bool,
        /// Glide duration in frames.
        portamento_frames: Option<usize>,
    },
}

/// Compiled voice allocation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledVoiceStealing {
    /// Prefer quiet releasing voices, then oldest active voices.
    QuietestReleasingThenOldest,
}

/// Parameter handles used by one compiled layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledLayerParameters {
    /// Layer gain handle.
    pub gain: ParameterHandle,
    /// Layer pan handle.
    pub pan: ParameterHandle,
    /// Layer tuning handle.
    pub tuning: ParameterHandle,
}

/// Compiled layer configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledLayer {
    /// Original index in the Definition layer array.
    pub definition_index: usize,
    /// Stable layer identifier.
    pub id: String,
    /// Compiled trigger conditions.
    pub trigger: CompiledLayerTrigger,
    /// Runtime parameter bindings.
    pub parameters: CompiledLayerParameters,
    /// Sample-rate-specific envelope.
    pub envelope: CompiledAdsr,
    /// Compiled generator.
    pub generator: CompiledGenerator,
    /// Maximum latency along the layer's generator and processor path.
    pub max_path_latency_frames: usize,
    /// Processors applied after the generator.
    pub processors: Box<[CompiledProcessor]>,
}

/// Compiled trigger conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledLayerTrigger {
    /// Event at which the layer starts.
    pub event: LayerTriggerEvent,
    /// Lowest accepted MIDI note.
    pub key_min: u8,
    /// Highest accepted MIDI note.
    pub key_max: u8,
    /// Lowest accepted velocity.
    pub velocity_min: u8,
    /// Highest accepted velocity.
    pub velocity_max: u8,
}

impl CompiledLayerTrigger {
    pub(crate) fn matches(self, note_number: u8, velocity: u8) -> bool {
        (self.key_min..=self.key_max).contains(&note_number)
            && (self.velocity_min..=self.velocity_max).contains(&velocity)
    }
}

/// Compile a validated Definition for a process configuration.
///
/// # Panics
///
/// Panics only if the parameter catalog does not contain a parameter generated
/// from the same Definition. Validation and catalog construction keep those
/// entries synchronized.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn compile_instrument(
    definition: &InstrumentDefinition,
    context: &CompileContext,
) -> CompileResult {
    let mut diagnostics = definition.validate();
    if let Err(error) = context.process_spec.validate() {
        diagnostics.push(
            Diagnostic::error(DiagnosticCode::CompileError, error.to_string())
                .with_path("process_spec"),
        );
    }
    let required_input_channels = definition
        .external_audio
        .map_or(0, |external_audio| external_audio.channels.channel_count());
    if context.process_spec.input_channels != required_input_channels {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                format!(
                    "process spec input channel count must be {required_input_channels} for this instrument"
                ),
            )
            .with_path("process_spec.input_channels"),
        );
    }
    if has_errors(&diagnostics) {
        return CompileResult {
            instrument: None,
            diagnostics,
        };
    }

    let parameter_catalog = ParameterCatalog::from_definition(definition);
    let mut asset_cache = HashMap::new();
    let mut wavetable_asset_cache = HashMap::new();
    let mut spectral_asset_cache = HashMap::new();
    let mut spectral_plan_cache = HashMap::new();

    let performance =
        compile_performance(&definition.performance, context.process_spec.sample_rate);
    let layers = definition
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.enabled)
        .map(|(definition_index, layer)| {
            let generator = generator::compile_generator(
                &layer.generator,
                definition_index,
                &layer.id,
                &parameter_catalog,
                &context.definition_base_dir,
                context.process_spec.sample_rate,
                context.process_spec.max_block_size,
                &mut asset_cache,
                &mut wavetable_asset_cache,
                &mut spectral_asset_cache,
                &mut spectral_plan_cache,
                &mut diagnostics,
            );
            let envelope_path = format!("layers[{definition_index}].envelope");
            let envelope = compile_adsr(
                layer.envelope,
                context.process_spec.sample_rate,
                &envelope_path,
                &mut diagnostics,
            );
            let parameters = CompiledLayerParameters {
                gain: parameter_catalog
                    .parameter_handle(&layer_parameter_id(&layer.id, "gain"))
                    .expect("layer gain catalog entry exists"),
                pan: parameter_catalog
                    .parameter_handle(&layer_parameter_id(&layer.id, "pan"))
                    .expect("layer pan catalog entry exists"),
                tuning: parameter_catalog
                    .parameter_handle(&layer_parameter_id(&layer.id, "tuning"))
                    .expect("layer tuning catalog entry exists"),
            };
            let processors = processor::compile_processor_chain(
                &layer.processors,
                processor::ProcessorPlacement::Layer,
                Some(&layer.id),
                &format!("layers[{definition_index}].processors"),
                &parameter_catalog,
                context.process_spec.sample_rate,
                &context.definition_base_dir,
                &mut asset_cache,
                &mut diagnostics,
            );
            let max_path_latency_frames = generator
                .max_intrinsic_latency_frames()
                .saturating_add(processor_chain_latency(&processors));
            CompiledLayer {
                definition_index,
                id: layer.id.clone(),
                trigger: compile_trigger(layer.trigger),
                parameters,
                envelope,
                generator,
                max_path_latency_frames,
                processors,
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();

    let voice_processors = processor::compile_processor_chain(
        &definition.voice_processors,
        processor::ProcessorPlacement::Voice,
        None,
        "voice_processors",
        &parameter_catalog,
        context.process_spec.sample_rate,
        &context.definition_base_dir,
        &mut asset_cache,
        &mut diagnostics,
    );
    let mut global_processors = processor::compile_processor_chain(
        &definition.global_processors,
        processor::ProcessorPlacement::Global,
        None,
        "global_processors",
        &parameter_catalog,
        context.process_spec.sample_rate,
        &context.definition_base_dir,
        &mut asset_cache,
        &mut diagnostics,
    );

    let (sources, instrument_sources, routes, route_ranges) = modulation::compile_modulation(
        definition,
        &parameter_catalog,
        context.process_spec.sample_rate,
        &mut diagnostics,
    );
    let used_voice_sources = compile_used_voice_sources(&routes, sources.len());
    let vectors = compile_vectors(definition, &layers, &parameter_catalog, &mut diagnostics);
    if has_errors(&diagnostics) {
        return CompileResult {
            instrument: None,
            diagnostics,
        };
    }

    let layer_alignment_latency_frames = layers
        .iter()
        .map(|layer| layer.max_path_latency_frames)
        .max()
        .unwrap_or(0);
    let voice_processor_latency = processor_chain_latency(&voice_processors);
    assign_external_input_alignment(
        &mut global_processors,
        layer_alignment_latency_frames.saturating_add(voice_processor_latency),
    );
    let global_processor_latency = processor_chain_latency(&global_processors);
    let effective_parameter_maxima = effective_parameter_maxima(
        &parameter_catalog,
        &layers,
        &voice_processors,
        &global_processors,
    );

    let compiled = CompiledInstrument {
        process_sample_rate: context.process_spec.sample_rate,
        layer_alignment_latency_frames,
        reported_latency_frames: layer_alignment_latency_frames
            .saturating_add(voice_processor_latency)
            .saturating_add(global_processor_latency),
        external_audio: definition.external_audio.map(|external_audio| {
            CompiledExternalAudioInput {
                channels: external_audio.channels,
            }
        }),
        metadata: CompiledMetadata {
            name: definition.metadata.name.clone(),
            author: definition.metadata.author.clone(),
            description: definition.metadata.description.clone(),
        },
        performance,
        layers,
        voice_processors,
        global_processors,
        parameter_catalog,
        effective_parameter_maxima,
        sources,
        used_voice_sources,
        routes,
        route_ranges,
        instrument_sources,
        vectors,
        macro_definitions: definition.macros.clone().into_boxed_slice(),
        vector_definitions: definition.vectors.clone().into_boxed_slice(),
        diagnostics: diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
            .cloned()
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    };
    CompileResult {
        instrument: Some(Arc::new(compiled)),
        diagnostics,
    }
}

fn processor_chain_latency(processors: &[CompiledProcessor]) -> usize {
    processors.iter().fold(0, |total, processor| {
        total.saturating_add(processor.processor.intrinsic_latency_frames())
    })
}

fn compile_used_voice_sources(routes: &[CompiledRoute], source_count: usize) -> Box<[bool]> {
    let mut used = vec![false; source_count];
    for route in routes {
        if let CompiledSourceRef::Voice(handle) = route.source
            && let Some(value) = used.get_mut(handle.index())
        {
            *value = true;
        }
    }
    used.into_boxed_slice()
}

fn assign_external_input_alignment(
    processors: &mut [CompiledProcessor],
    mut preceding_latency_frames: usize,
) {
    for processor in processors {
        match &mut processor.processor {
            CompiledProcessorKind::Gate(value)
                if value.detector == CompiledDynamicsDetector::ExternalAudio =>
            {
                value.external_input_alignment_frames = preceding_latency_frames;
            }
            CompiledProcessorKind::Compressor(value)
                if value.detector == CompiledDynamicsDetector::ExternalAudio =>
            {
                value.external_input_alignment_frames = preceding_latency_frames;
            }
            CompiledProcessorKind::Vocoder(value) => {
                value.external_input_alignment_frames = preceding_latency_frames;
            }
            CompiledProcessorKind::EnvelopeTransfer(value) => {
                value.external_input_alignment_frames = preceding_latency_frames;
            }
            CompiledProcessorKind::SpectralMorph(value) => {
                value.external_input_alignment_frames = preceding_latency_frames;
            }
            _ => {}
        }
        preceding_latency_frames =
            preceding_latency_frames.saturating_add(processor.processor.intrinsic_latency_frames());
    }
}

fn compile_performance(
    performance: &crate::definition::PerformanceDefinition,
    sample_rate: f64,
) -> CompiledPerformance {
    match performance {
        crate::definition::PerformanceDefinition::Polyphonic {
            polyphony,
            voice_stealing,
        } => CompiledPerformance {
            mode: CompiledPerformanceMode::Polyphonic {
                voice_stealing: match voice_stealing {
                    VoiceStealingDefinition::QuietestReleasingThenOldest => {
                        CompiledVoiceStealing::QuietestReleasingThenOldest
                    }
                },
            },
            voice_count: usize::from(*polyphony),
        },
        crate::definition::PerformanceDefinition::Monophonic { legato, portamento } => {
            CompiledPerformance {
                mode: CompiledPerformanceMode::Monophonic {
                    legato: *legato,
                    portamento_frames: portamento.map(|value| {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        { (f64::from(value.time_seconds) * sample_rate).round() as usize }.max(1)
                    }),
                },
                voice_count: 1,
            }
        }
    }
}

fn compile_vectors(
    definition: &InstrumentDefinition,
    layers: &[CompiledLayer],
    catalog: &ParameterCatalog,
    diagnostics: &mut Vec<Diagnostic>,
) -> Box<[CompiledVector]> {
    let mut result = Vec::with_capacity(definition.vectors.len());
    for (vector_index, vector) in definition.vectors.iter().enumerate() {
        let layer_index = |id: &str| {
            layers
                .iter()
                .position(|layer| layer.id == id)
                .ok_or_else(|| {
                    Diagnostic::error(
                        DiagnosticCode::ParameterNotFound,
                        "vector layer is not present in the compiled layer table",
                    )
                    .with_path(format!("vectors[{vector_index}]"))
                })
        };
        let parameter = |axis: &str| {
            catalog
                .parameter_handle(&format!("vector.{}.{}", vector_id(vector), axis))
                .ok_or_else(|| {
                    Diagnostic::error(
                        DiagnosticCode::ParameterNotFound,
                        "vector axis is not present in the parameter catalog",
                    )
                    .with_path(format!("vectors[{vector_index}]"))
                })
        };
        let compiled = match vector {
            VectorDefinition::TwoWay {
                layer_a, layer_b, ..
            } => layer_index(layer_a)
                .and_then(|a| layer_index(layer_b).map(|b| (a, b)))
                .and_then(|(a, b)| {
                    parameter("position").map(|position| CompiledVector::TwoWay {
                        position,
                        layers: [a, b],
                    })
                }),
            VectorDefinition::FourWay {
                top_left,
                top_right,
                bottom_left,
                bottom_right,
                ..
            } => layer_index(top_left)
                .and_then(|top_left| layer_index(top_right).map(|top_right| (top_left, top_right)))
                .and_then(|(top_left, top_right)| {
                    layer_index(bottom_left).map(|bottom_left| (top_left, top_right, bottom_left))
                })
                .and_then(|(top_left, top_right, bottom_left)| {
                    layer_index(bottom_right)
                        .map(|bottom_right| (top_left, top_right, bottom_left, bottom_right))
                })
                .and_then(|(top_left, top_right, bottom_left, bottom_right)| {
                    parameter("x").and_then(|x| {
                        parameter("y").map(|y| CompiledVector::FourWay {
                            x,
                            y,
                            layers: [top_left, top_right, bottom_left, bottom_right],
                        })
                    })
                }),
        };
        match compiled {
            Ok(value) => result.push(value),
            Err(error) => diagnostics.push(error),
        }
    }
    result.into_boxed_slice()
}

fn vector_id(vector: &VectorDefinition) -> &str {
    match vector {
        VectorDefinition::TwoWay { id, .. } | VectorDefinition::FourWay { id, .. } => id,
    }
}

fn effective_parameter_maxima(
    parameter_catalog: &ParameterCatalog,
    layers: &[CompiledLayer],
    voice_processors: &[CompiledProcessor],
    global_processors: &[CompiledProcessor],
) -> Box<[f32]> {
    let mut maxima = parameter_catalog
        .parameters()
        .iter()
        .map(|parameter| parameter.max)
        .collect::<Vec<_>>();
    for processor in layers
        .iter()
        .flat_map(|layer| layer.processors.iter())
        .chain(voice_processors.iter())
        .chain(global_processors.iter())
    {
        match &processor.processor {
            CompiledProcessorKind::Filter(filter) => {
                if let Some(maximum) = maxima.get_mut(filter.parameters.cutoff.index()) {
                    *maximum = maximum.min(filter.effective_max_cutoff_hz);
                }
            }
            CompiledProcessorKind::LadderFilter(filter) => {
                if let Some(maximum) = maxima.get_mut(filter.parameters.cutoff.index()) {
                    *maximum = maximum.min(filter.effective_max_cutoff_hz);
                }
            }
            CompiledProcessorKind::FrequencyShifter(shifter) => {
                if let Some(maximum) = maxima.get_mut(shifter.parameters.shift_hz.index()) {
                    *maximum = maximum.min(shifter.effective_abs_shift_hz);
                }
            }
            _ => {}
        }
    }
    maxima.into_boxed_slice()
}

#[allow(clippy::too_many_lines)]
fn brown_noise_coefficient(sample_rate: f64) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let coefficient = (-std::f64::consts::TAU * 20.0 / sample_rate).exp() as f32;
    coefficient.clamp(0.0, 1.0)
}

fn asset_diagnostic(error: &AssetError) -> (DiagnosticCode, &'static str) {
    match error {
        AssetError::NotFound(_) => (DiagnosticCode::AssetNotFound, "asset is unavailable"),
        AssetError::HashMismatch { .. } => (
            DiagnosticCode::AssetHashMismatch,
            "asset sha256 does not match",
        ),
        AssetError::Decode(_) => (
            DiagnosticCode::AssetDecodeFailed,
            "asset could not be decoded",
        ),
        AssetError::Resample(_) => (
            DiagnosticCode::AssetDecodeFailed,
            "asset could not be prepared",
        ),
    }
}

fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
}

fn compile_trigger(trigger: crate::definition::LayerTriggerDefinition) -> CompiledLayerTrigger {
    CompiledLayerTrigger {
        event: trigger.event,
        key_min: trigger.key_min,
        key_max: trigger.key_max,
        velocity_min: trigger.velocity_min,
        velocity_max: trigger.velocity_max,
    }
}

fn compile_adsr(
    definition: AdsrDefinition,
    sample_rate: f64,
    base_path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledAdsr {
    CompiledAdsr {
        attack_samples: seconds_to_samples(
            definition.attack_seconds,
            sample_rate,
            base_path,
            "attack_seconds",
            diagnostics,
        ),
        decay_samples: seconds_to_samples(
            definition.decay_seconds,
            sample_rate,
            base_path,
            "decay_seconds",
            diagnostics,
        ),
        sustain_level: definition.sustain_level,
        release_samples: seconds_to_samples(
            definition.release_seconds,
            sample_rate,
            base_path,
            "release_seconds",
            diagnostics,
        ),
    }
}

fn seconds_to_samples(
    seconds: f32,
    sample_rate: f64,
    base_path: &str,
    field: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    let frames = (f64::from(seconds) * sample_rate).round();
    #[allow(clippy::cast_precision_loss)]
    let max_usize = usize::MAX as f64;
    if !frames.is_finite() || frames < 0.0 || frames > max_usize {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CompileError,
                "envelope duration does not fit in the process frame counter",
            )
            .with_path(format!("{base_path}.{field}")),
        );
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        frames as usize
    }
}

fn effective_max_cutoff(sample_rate: f64) -> f32 {
    #[allow(clippy::manual_clamp)]
    let max = (sample_rate * 0.45).min(20_000.0).max(1.0);
    #[allow(clippy::cast_possible_truncation)]
    {
        max as f32
    }
}

/// Convert decibels to linear amplitude.
#[must_use]
pub fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Convert cents to a frequency ratio.
#[must_use]
pub fn cents_to_ratio(cents: f32) -> f32 {
    2.0_f32.powf(cents / 1200.0)
}

/// Convert a MIDI note and tuning ratio to frequency in Hz.
#[must_use]
pub fn midi_note_frequency(note_number: u8, tuning_ratio: f32) -> f32 {
    440.0 * 2.0_f32.powf((f32::from(note_number) - 69.0) / 12.0) * tuning_ratio
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use super::{
        CompileContext, CompiledOscillatorBackend, CompiledProcessorKind, cents_to_ratio,
        compile_instrument, db_to_linear, midi_note_frequency,
    };
    use crate::ProcessSpec;
    pub(crate) use crate::definition::tests::definition;
    use crate::definition::{ModulationCurve, ProcessorDefinition};
    use crate::diagnostics::{DiagnosticCode, DiagnosticSeverity};
    use crate::parameter::ParameterHandle;

    pub(crate) fn context() -> CompileContext {
        CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid spec"),
        }
    }

    #[test]
    fn conversion_helpers_match_audio_units() {
        assert!((db_to_linear(-6.0206) - 0.5).abs() < 0.001);
        assert!((cents_to_ratio(1200.0) - 2.0).abs() < 1.0e-6);
        assert!((midi_note_frequency(69, 1.0) - 440.0).abs() < 1.0e-6);
    }

    #[test]
    fn oscillator_backend_frequency_limits_are_rate_derived() {
        assert!(
            (CompiledOscillatorBackend::Basic.effective_max_frequency(48_000.0) - 21_600.0).abs()
                < f32::EPSILON
        );
        assert!(
            (CompiledOscillatorBackend::VariableShapeSync {
                sync_ratio: ParameterHandle::new(0),
            }
            .effective_max_frequency(48_000.0)
                - 11_520.0)
                .abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn valid_definition_compiles_to_catalog_bindings() {
        let source = definition();
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("compiled instrument");
        assert!(result.diagnostics.is_empty());
        assert_eq!(compiled.layers.len(), 1);
        assert_eq!(compiled.parameters()[0].id, "layer.body.gain");
        assert_eq!(
            compiled.parameter_handle("layer.body.gain"),
            Some(compiled.layers[0].parameters.gain)
        );
    }

    #[test]
    fn cutoff_is_clamped_with_a_warning_but_catalog_range_is_stable() {
        let mut source = definition();
        source.voice_processors.push(ProcessorDefinition::Filter(
            crate::definition::FilterProcessorDefinition {
                id: "tone".to_owned(),
                mode: crate::definition::FilterModeDefinition::LowPass,
                cutoff_hz: 20_000.0,
                resonance: 0.1,
            },
        ));
        let low_rate = CompileContext {
            process_spec: ProcessSpec::new(22_050.0, 257, 0, 2).expect("valid spec"),
            ..context()
        };
        let result = compile_instrument(&source, &low_rate);
        let compiled = result.instrument.expect("compiled");
        let warning = result.diagnostics.iter().find(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Warning
                && diagnostic.code == DiagnosticCode::FilterCutoffClamped
        });
        assert_eq!(
            warning.expect("cutoff warning").message,
            "cutoff exceeds the process-safe maximum and will be clamped to 9922.500 Hz during DSP processing"
        );
        assert!(
            (match &compiled.voice_processors[0].processor {
                CompiledProcessorKind::Filter(filter) => filter.effective_max_cutoff_hz,
                _ => panic!("voice processor must be a filter"),
            } - 9_922.5)
                .abs()
                < 0.1
        );
        assert!(
            (compiled
                .parameters()
                .iter()
                .find(|parameter| parameter.id == "voice.processor.tone.cutoff")
                .expect("cutoff parameter")
                .default
                - 20_000.0)
                .abs()
                < 0.1
        );
        assert!((compiled.parameters().last().expect("resonance").max - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn invalid_definition_does_not_produce_compiled_state() {
        let mut source = definition();
        source.layers[0].gain_db = 99.0;
        let result = compile_instrument(&source, &context());
        assert!(result.instrument.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn all_voice_source_kinds_compile_with_routes() {
        let mut source = definition();
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![
                crate::definition::ModulationSourceDefinition::Lfo(
                    crate::definition::LfoDefinition {
                        id: "slow_lfo".to_owned(),
                        waveform: crate::definition::LfoWaveform::Sine,
                        rate: crate::definition::ModulationRateDefinition {
                            value: 2.0,
                            unit: crate::definition::ModulationRateUnit::PerSecond,
                        },
                        phase: 0.0,
                    },
                ),
                crate::definition::ModulationSourceDefinition::Envelope(
                    crate::definition::ModEnvelopeDefinition {
                        id: "mod_env".to_owned(),
                        attack_seconds: 0.01,
                        decay_seconds: 0.1,
                        sustain_level: 0.5,
                        release_seconds: 0.1,
                    },
                ),
                crate::definition::ModulationSourceDefinition::Random(
                    crate::definition::RandomDefinition {
                        id: "random".to_owned(),
                        seed: 7,
                    },
                ),
                crate::definition::ModulationSourceDefinition::Mseg(
                    crate::definition::MsegDefinition {
                        id: "motion_env".to_owned(),
                        initial_value: 0.0,
                        segments: vec![crate::definition::MsegSegmentDefinition {
                            duration: crate::definition::ModulationDurationDefinition {
                                value: 0.1,
                                unit: crate::definition::ModulationDurationUnit::Seconds,
                            },
                            target: 1.0,
                            curve: crate::definition::ModulationSegmentCurve::Linear,
                        }],
                        loop_range: None,
                    },
                ),
                crate::definition::ModulationSourceDefinition::Step(
                    crate::definition::StepModulatorDefinition {
                        id: "step".to_owned(),
                        values: vec![-1.0, 1.0],
                        rate: crate::definition::ModulationRateDefinition {
                            value: 2.0,
                            unit: crate::definition::ModulationRateUnit::PerSecond,
                        },
                    },
                ),
                crate::definition::ModulationSourceDefinition::SampleHold(
                    crate::definition::SampleHoldDefinition {
                        id: "sample_hold".to_owned(),
                        seed: 11,
                        rate: crate::definition::ModulationRateDefinition {
                            value: 2.0,
                            unit: crate::definition::ModulationRateUnit::PerSecond,
                        },
                    },
                ),
                crate::definition::ModulationSourceDefinition::SmoothRandom(
                    crate::definition::SmoothRandomDefinition {
                        id: "smooth_random".to_owned(),
                        seed: 13,
                        rate: crate::definition::ModulationRateDefinition {
                            value: 2.0,
                            unit: crate::definition::ModulationRateUnit::PerSecond,
                        },
                    },
                ),
            ],
            routes: vec![
                crate::definition::ModulationRouteDefinition {
                    source: "slow_lfo".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 7.2,
                        unit: crate::parameter::ModulationUnit::Decibels,
                    },
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "mod_env".to_owned(),
                    target: "layer.body.pan".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 0.4,
                        unit: crate::parameter::ModulationUnit::Pan,
                    },
                    curve: ModulationCurve::SmoothStep,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "random".to_owned(),
                    target: "layer.body.tuning".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 120.0,
                        unit: crate::parameter::ModulationUnit::Cents,
                    },
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "motion_env".to_owned(),
                    target: "layer.body.pan".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 0.4,
                        unit: crate::parameter::ModulationUnit::Pan,
                    },
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "step".to_owned(),
                    target: "layer.body.tuning".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 120.0,
                        unit: crate::parameter::ModulationUnit::Cents,
                    },
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "sample_hold".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 7.2,
                        unit: crate::parameter::ModulationUnit::Decibels,
                    },
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "smooth_random".to_owned(),
                    target: "layer.body.tuning".to_owned(),
                    depth: crate::definition::ModulationDepthDefinition {
                        value: 120.0,
                        unit: crate::parameter::ModulationUnit::Cents,
                    },
                    curve: ModulationCurve::Linear,
                },
            ],
        });
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("all source kinds compile");
        assert_eq!(compiled.sources.len(), 9);
        assert_eq!(compiled.routes.len(), 7);
        assert!(
            compiled
                .routes
                .iter()
                .all(|route| route.target.index() < compiled.parameters().len())
        );
    }
}
