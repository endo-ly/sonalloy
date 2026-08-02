use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::asset::{AssetError, PreparedSample, SampleMetadata, prepare_asset};
use crate::definition::{
    AdsrDefinition, GeneratorDefinition, InstrumentDefinition, LfoDefinition, LfoWaveform,
    ModulationCurve, ModulationSourceDefinition, OscillatorDefinition, OscillatorWaveform,
    SampleInterpolation, SamplePlaybackMode, VoiceStealingDefinition,
};
use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSeverity};
use crate::parameter::{BUILTIN_SOURCE_IDS, ParameterCatalog, ParameterHandle};
use crate::process::ProcessSpec;
use crate::runtime::InstrumentRuntime;

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
    /// Metadata copied from the Definition.
    pub metadata: CompiledMetadata,
    /// Compiled performance settings.
    pub performance: CompiledPerformance,
    /// Enabled layers in Definition order.
    pub layers: Box<[CompiledLayer]>,
    /// Optional voice filter.
    pub voice_filter: Option<CompiledFilter>,
    /// Dense continuous parameter catalog.
    pub parameter_catalog: ParameterCatalog,
    /// Voice-scoped source table.
    pub sources: Box<[CompiledSource]>,
    /// Compiled routes grouped by target handle.
    pub routes: Box<[CompiledRoute]>,
    /// Route range for each parameter handle.
    pub route_ranges: Box<[RouteRange]>,
    /// Warnings retained for inspection and review output.
    pub diagnostics: Box<[Diagnostic]>,
}

impl CompiledInstrument {
    /// Create a fresh runtime instance that owns no active audio state yet.
    #[must_use]
    pub fn instantiate(self: &Arc<Self>) -> InstrumentRuntime {
        InstrumentRuntime::new(Arc::clone(self))
    }

    /// Return parameter descriptors in stable Definition order.
    #[must_use]
    pub fn parameters(&self) -> &[crate::parameter::ParameterDescriptor] {
        self.parameter_catalog.parameters()
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

    /// Return the route slice for one target handle.
    #[must_use]
    pub fn routes_for(&self, handle: ParameterHandle) -> &[CompiledRoute] {
        let Some(range) = self.route_ranges.get(handle.index()) else {
            return &[];
        };
        &self.routes[range.start..range.start + range.len]
    }
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
    /// Maximum number of voices.
    pub polyphony: usize,
    /// Voice allocation policy.
    pub voice_stealing: CompiledVoiceStealing,
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
}

/// Compiled trigger conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledLayerTrigger {
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

/// Compiled generator variants.
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledGenerator {
    /// Oscillator generator.
    Oscillator(CompiledOscillator),
    /// Prepared sample generator.
    Sample(CompiledSample),
}

/// Compiled oscillator settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledOscillator {
    /// Waveform selected by the Definition.
    pub waveform: OscillatorWaveform,
    /// Whether Note On resets the phase.
    pub phase_reset: bool,
}

/// Compiled sample configuration and prepared source.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSample {
    /// Prepared mono source, absent when the asset could not be loaded.
    pub source: Option<Arc<PreparedSample>>,
    /// MIDI note represented by the source recording.
    pub root_note: u8,
    /// Sample playback mode.
    pub playback_mode: SamplePlaybackMode,
    /// Sample interpolation mode.
    pub interpolation: SampleInterpolation,
    /// Path as written in the Definition.
    pub asset_path: String,
    /// Expected SHA-256 digest, when present.
    pub asset_sha256: Option<String>,
    /// Whether the sample can be triggered.
    pub enabled: bool,
    /// Source metadata when the asset was loaded.
    pub source_metadata: Option<SampleMetadata>,
}

/// Sample-rate-specific ADSR settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledAdsr {
    /// Attack duration in frames.
    pub attack_samples: usize,
    /// Decay duration in frames.
    pub decay_samples: usize,
    /// Sustain amplitude.
    pub sustain_level: f32,
    /// Release duration in frames.
    pub release_samples: usize,
}

/// Parameter handles used by the voice filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledFilterParameters {
    /// Cutoff handle.
    pub cutoff: ParameterHandle,
    /// Resonance handle.
    pub resonance: ParameterHandle,
}

/// Compiled voice filter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledFilter {
    /// Runtime parameter bindings.
    pub parameters: CompiledFilterParameters,
    /// Safe DSP cutoff upper bound for this process sample rate.
    pub effective_max_cutoff_hz: f32,
}

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
}

/// Compiled LFO settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledLfo {
    /// LFO waveform.
    pub waveform: LfoWaveform,
    /// Rate in hertz.
    pub rate_hz: f32,
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

/// A source reference in a compiled route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledSourceRef {
    /// Voice-scoped source table entry.
    Voice(SourceHandle),
    /// Shared pitch bend control.
    PitchBend,
    /// Shared modulation wheel control.
    ModWheel,
    /// Shared channel aftertouch control.
    Aftertouch,
}

/// Compiled route with a fixed target and source evaluation order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledRoute {
    /// Compiled source reference.
    pub source: CompiledSourceRef,
    /// Target parameter handle.
    pub target: ParameterHandle,
    /// Signed target-range amount.
    pub amount: f32,
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
    if has_errors(&diagnostics) {
        return CompileResult {
            instrument: None,
            diagnostics,
        };
    }

    let parameter_catalog = ParameterCatalog::from_definition(definition);
    let effective_max_cutoff_hz = effective_max_cutoff(context.process_spec.sample_rate);
    let voice_filter = definition.voice_filter.map(|filter| {
        if filter.cutoff_hz > effective_max_cutoff_hz {
            diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::FilterCutoffClamped,
                    format!(
                        "cutoff clamped to {effective_max_cutoff_hz:.3} Hz for the process sample rate"
                    ),
                )
                .with_path("voice_filter.cutoff_hz"),
            );
        }
        let cutoff = parameter_catalog
            .parameter_handle("voice.filter.cutoff")
            .expect("filter catalog entry exists");
        let resonance = parameter_catalog
            .parameter_handle("voice.filter.resonance")
            .expect("filter catalog entry exists");
        CompiledFilter {
            parameters: CompiledFilterParameters { cutoff, resonance },
            effective_max_cutoff_hz,
        }
    });

    let performance = CompiledPerformance {
        polyphony: usize::from(definition.performance.polyphony),
        voice_stealing: match definition.performance.voice_stealing {
            VoiceStealingDefinition::QuietestReleasingThenOldest => {
                CompiledVoiceStealing::QuietestReleasingThenOldest
            }
        },
    };
    let layers = definition
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| layer.enabled)
        .map(|(definition_index, layer)| {
            let generator = compile_generator(
                &layer.generator,
                definition_index,
                &context.definition_base_dir,
                context.process_spec.sample_rate,
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
                    .parameter_handle(&format!("layer.{}.gain", layer.id))
                    .expect("layer gain catalog entry exists"),
                pan: parameter_catalog
                    .parameter_handle(&format!("layer.{}.pan", layer.id))
                    .expect("layer pan catalog entry exists"),
                tuning: parameter_catalog
                    .parameter_handle(&format!("layer.{}.tuning", layer.id))
                    .expect("layer tuning catalog entry exists"),
            };
            CompiledLayer {
                id: layer.id.clone(),
                trigger: compile_trigger(layer.trigger),
                parameters,
                envelope,
                generator,
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();

    let (sources, routes, route_ranges) = compile_modulation(
        definition,
        &parameter_catalog,
        context.process_spec.sample_rate,
        &mut diagnostics,
    );
    if has_errors(&diagnostics) {
        return CompileResult {
            instrument: None,
            diagnostics,
        };
    }

    let compiled = CompiledInstrument {
        process_sample_rate: context.process_spec.sample_rate,
        metadata: CompiledMetadata {
            name: definition.metadata.name.clone(),
            author: definition.metadata.author.clone(),
            description: definition.metadata.description.clone(),
        },
        performance,
        layers,
        voice_filter,
        parameter_catalog,
        sources,
        routes,
        route_ranges,
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

type CompiledModulation = (
    Box<[CompiledSource]>,
    Box<[CompiledRoute]>,
    Box<[RouteRange]>,
);

#[allow(clippy::too_many_lines)]
fn compile_modulation(
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
        source_lookup.insert(source.id.clone(), SourceHandle(index));
    }
    if let Some(modulation) = &definition.modulation {
        for (source_index, source) in modulation.sources.iter().enumerate() {
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
            };
            let handle = SourceHandle(sources.len());
            source_lookup.insert(source_id(source).to_owned(), handle);
            sources.push(CompiledSource {
                id: source_id(source).to_owned(),
                source: compiled,
            });
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
            let source = match route.source.as_str() {
                "pitch_bend" => Some(CompiledSourceRef::PitchBend),
                "mod_wheel" => Some(CompiledSourceRef::ModWheel),
                "aftertouch" => Some(CompiledSourceRef::Aftertouch),
                _ => source_lookup
                    .get(&route.source)
                    .copied()
                    .map(CompiledSourceRef::Voice),
            };
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
            unresolved_routes.push((
                target.index(),
                CompiledRoute {
                    source,
                    target,
                    amount: route.amount,
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
        routes.into_boxed_slice(),
        route_ranges.into_boxed_slice(),
    )
}

fn source_id(source: &ModulationSourceDefinition) -> &str {
    match source {
        ModulationSourceDefinition::Lfo(value) => &value.id,
        ModulationSourceDefinition::Envelope(value) => &value.id,
        ModulationSourceDefinition::Random(value) => &value.id,
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
        rate_hz: value.rate_hz,
        phase: value.phase,
    }
}

fn compile_generator(
    generator: &GeneratorDefinition,
    layer_index: usize,
    definition_base_dir: &Path,
    sample_rate: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledGenerator {
    match generator {
        GeneratorDefinition::Oscillator(OscillatorDefinition {
            waveform,
            phase_reset,
        }) => CompiledGenerator::Oscillator(CompiledOscillator {
            waveform: *waveform,
            phase_reset: *phase_reset,
        }),
        GeneratorDefinition::Sample(sample) => {
            let path = format!("layers[{layer_index}].generator.sample.asset.path");
            let mut compiled = CompiledSample {
                source: None,
                root_note: sample.root_note,
                playback_mode: sample.playback_mode,
                interpolation: sample.interpolation,
                asset_path: sample.asset.path.clone(),
                asset_sha256: sample.asset.sha256.clone(),
                enabled: false,
                source_metadata: None,
            };
            if Path::new(&sample.asset.path).is_absolute() {
                diagnostics.push(
                    Diagnostic::warning(
                        DiagnosticCode::AssetAbsolutePath,
                        "absolute asset paths reduce Definition portability",
                    )
                    .with_path(path.clone()),
                );
            }
            match prepare_asset(&sample.asset, definition_base_dir, sample_rate) {
                Ok(prepared) => {
                    if sample.asset.sha256.is_none() {
                        diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::AssetHashMissing,
                                "asset sha256 is not specified",
                            )
                            .with_path(format!(
                                "layers[{layer_index}].generator.sample.asset.sha256"
                            )),
                        );
                    }
                    if prepared.downmixed {
                        diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::AssetDownmixed,
                                "stereo asset was downmixed to mono",
                            )
                            .with_path(path.clone()),
                        );
                    }
                    if (f64::from(prepared.sample.source_metadata.source_sample_rate) - sample_rate)
                        .abs()
                        > f64::EPSILON
                    {
                        diagnostics.push(
                            Diagnostic::warning(
                                DiagnosticCode::AssetResampled,
                                "asset was resampled to the process sample rate",
                            )
                            .with_path(path.clone()),
                        );
                    }
                    compiled.source_metadata = Some(prepared.sample.source_metadata.clone());
                    compiled.source = Some(Arc::new(prepared.sample));
                    compiled.enabled = true;
                }
                Err(error) => {
                    let (code, message) = asset_diagnostic(&error);
                    diagnostics.push(
                        Diagnostic::warning(code, message)
                            .with_path(path)
                            .with_detail(error.to_string()),
                    );
                }
            }
            CompiledGenerator::Sample(compiled)
        }
    }
}

fn asset_diagnostic(error: &AssetError) -> (DiagnosticCode, &'static str) {
    match error {
        AssetError::NotFound(_) => (DiagnosticCode::AssetNotFound, "sample asset is unavailable"),
        AssetError::HashMismatch { .. } => (
            DiagnosticCode::AssetHashMismatch,
            "sample asset sha256 does not match",
        ),
        AssetError::Decode(_) => (
            DiagnosticCode::AssetDecodeFailed,
            "sample asset could not be decoded",
        ),
        AssetError::Resample(_) => (
            DiagnosticCode::AssetDecodeFailed,
            "sample asset could not be prepared",
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
mod tests {
    use super::*;
    use crate::definition::tests::definition;

    fn context() -> CompileContext {
        CompileContext {
            definition_base_dir: PathBuf::from("."),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid spec"),
        }
    }

    #[test]
    fn conversion_helpers_match_audio_units() {
        assert!((db_to_linear(-6.0206) - 0.5).abs() < 0.001);
        assert!((cents_to_ratio(1200.0) - 2.0).abs() < 1.0e-6);
        assert!((midi_note_frequency(69, 1.0) - 440.0).abs() < 1.0e-6);
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
    fn disabled_layers_stay_in_the_catalog_but_not_the_runtime_layers() {
        let mut source = definition();
        let mut disabled = source.layers[0].clone();
        disabled.id = "disabled".to_owned();
        disabled.enabled = false;
        source.layers.insert(0, disabled);
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("enabled layer compiles");
        assert_eq!(compiled.layers.len(), 1);
        assert_eq!(compiled.parameters()[0].id, "layer.disabled.gain");
        assert_eq!(compiled.layers[0].id, "body");
    }

    #[test]
    fn cutoff_is_clamped_with_a_warning_but_catalog_range_is_stable() {
        let mut source = definition();
        source.voice_filter = Some(crate::definition::FilterDefinition {
            cutoff_hz: 20_000.0,
            resonance: 0.1,
        });
        let low_rate = CompileContext {
            process_spec: ProcessSpec::new(22_050.0, 257, 2).expect("valid spec"),
            ..context()
        };
        let result = compile_instrument(&source, &low_rate);
        let compiled = result.instrument.expect("compiled");
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Warning
                && diagnostic.code == DiagnosticCode::FilterCutoffClamped
        }));
        assert!(
            (compiled
                .voice_filter
                .expect("filter")
                .effective_max_cutoff_hz
                - 9_922.5)
                .abs()
                < 0.1
        );
        assert!(
            (compiled
                .parameters()
                .iter()
                .find(|parameter| parameter.id == "voice.filter.cutoff")
                .expect("cutoff parameter")
                .default
                - 20_000.0)
                .abs()
                < 0.1
        );
        assert!((compiled.parameters().last().expect("resonance").max - 1.0).abs() < f32::EPSILON);
    }

    #[test]
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
    fn routes_resolve_to_dense_targets_and_preserve_same_target_order() {
        let mut source = definition();
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![],
            routes: vec![
                crate::definition::ModulationRouteDefinition {
                    source: "velocity".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    amount: 0.1,
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "key_tracking".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    amount: -0.2,
                    curve: ModulationCurve::SmoothStep,
                },
            ],
        });
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("routes compile");
        let routes = compiled.routes_for(compiled.layers[0].parameters.gain);
        assert_eq!(routes.len(), 2);
        assert!((routes[0].amount - 0.1).abs() < f32::EPSILON);
        assert!((routes[1].amount + 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn all_voice_source_kinds_compile_with_routes() {
        let mut source = definition();
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![
                crate::definition::ModulationSourceDefinition::Lfo(
                    crate::definition::LfoDefinition {
                        id: "slow_lfo".to_owned(),
                        waveform: crate::definition::LfoWaveform::Sine,
                        rate_hz: 2.0,
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
            ],
            routes: vec![
                crate::definition::ModulationRouteDefinition {
                    source: "slow_lfo".to_owned(),
                    target: "layer.body.gain".to_owned(),
                    amount: 0.1,
                    curve: ModulationCurve::Linear,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "mod_env".to_owned(),
                    target: "layer.body.pan".to_owned(),
                    amount: 0.2,
                    curve: ModulationCurve::SmoothStep,
                },
                crate::definition::ModulationRouteDefinition {
                    source: "random".to_owned(),
                    target: "layer.body.tuning".to_owned(),
                    amount: 0.05,
                    curve: ModulationCurve::Linear,
                },
            ],
        });
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("all source kinds compile");
        assert_eq!(compiled.sources.len(), 5);
        assert_eq!(compiled.routes.len(), 3);
        assert!(
            compiled
                .routes
                .iter()
                .all(|route| route.target.index() < compiled.parameters().len())
        );
    }

    #[test]
    fn unresolved_routes_are_reported_without_panicking() {
        let mut source = definition();
        source.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![],
            routes: vec![crate::definition::ModulationRouteDefinition {
                source: "missing".to_owned(),
                target: "layer.missing.gain".to_owned(),
                amount: 0.1,
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
