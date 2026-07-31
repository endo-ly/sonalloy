use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::asset::{AssetError, PreparedSample, SampleMetadata, prepare_asset};
use crate::definition::{
    AdsrDefinition, GeneratorDefinition, InstrumentDefinition, LayerId, LayerTriggerDefinition,
    OscillatorDefinition, OscillatorWaveform, SampleInterpolation, SamplePlaybackMode,
    VoiceStealingDefinition,
};
use crate::diagnostics::{Diagnostic, DiagnosticCode, DiagnosticSeverity};
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
    /// Metadata copied from the Definition.
    pub metadata: CompiledMetadata,
    /// Compiled performance settings.
    pub performance: CompiledPerformance,
    /// Validated layers in Definition order.
    pub layers: Box<[CompiledLayer]>,
    /// Optional voice filter.
    pub voice_filter: Option<CompiledFilter>,
    /// Compiled velocity response.
    pub velocity_response: CompiledVelocityResponse,
    /// Warnings retained for inspection and review output.
    pub diagnostics: Box<[Diagnostic]>,
}

impl CompiledInstrument {
    /// Create a fresh runtime instance that owns no active audio state yet.
    #[must_use]
    pub fn instantiate(self: &Arc<Self>) -> InstrumentRuntime {
        InstrumentRuntime::new(Arc::clone(self))
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

/// Compiled layer configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledLayer {
    /// Stable layer identifier.
    pub id: LayerId,
    /// Compiled trigger conditions.
    pub trigger: CompiledLayerTrigger,
    /// Linear layer gain.
    pub gain_linear: f32,
    /// Constant-power pan position.
    pub pan: f32,
    /// Tuning ratio.
    pub tuning_ratio: f32,
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

/// Compiled voice filter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledFilter {
    /// Effective cutoff in Hz.
    pub cutoff_hz: f32,
    /// Normalized resonance.
    pub resonance: f32,
}

/// Compiled velocity response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledVelocityResponse {
    /// Amount of layer gain reduction at low velocity.
    pub layer_gain_amount: f32,
    /// Cutoff reduction in octaves at low velocity.
    pub filter_cutoff_octaves: f32,
}

/// Compile a validated Definition for a process configuration.
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

    let voice_filter = definition.voice_filter.map(|filter| {
        #[allow(clippy::manual_clamp)]
        let effective_max_f64 = (context.process_spec.sample_rate * 0.45)
            .min(20_000.0)
            .max(1.0);
        #[allow(clippy::cast_possible_truncation)]
        let effective_max = effective_max_f64 as f32;
        let cutoff_hz = if filter.cutoff_hz > effective_max {
            diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::FilterCutoffClamped,
                    format!("cutoff clamped to {effective_max:.3} Hz for the process sample rate"),
                )
                .with_path("voice_filter.cutoff_hz"),
            );
            effective_max
        } else {
            filter.cutoff_hz
        };
        CompiledFilter {
            cutoff_hz,
            resonance: filter.resonance,
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
        .map(|(layer_index, layer)| {
            let generator = compile_generator(
                &layer.generator,
                layer_index,
                &context.definition_base_dir,
                context.process_spec.sample_rate,
                &mut diagnostics,
            );
            let envelope = compile_adsr(
                layer.envelope,
                context.process_spec.sample_rate,
                layer_index,
                &mut diagnostics,
            );
            CompiledLayer {
                id: layer.id.clone(),
                trigger: compile_trigger(layer.trigger),
                gain_linear: db_to_linear(layer.gain_db),
                pan: layer.pan,
                tuning_ratio: cents_to_ratio(layer.tuning_cents),
                envelope,
                generator,
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let compiled = CompiledInstrument {
        metadata: CompiledMetadata {
            name: definition.metadata.name.clone(),
            author: definition.metadata.author.clone(),
            description: definition.metadata.description.clone(),
        },
        performance,
        layers,
        voice_filter,
        velocity_response: CompiledVelocityResponse {
            layer_gain_amount: definition.velocity_response.layer_gain_amount,
            filter_cutoff_octaves: definition.velocity_response.filter_cutoff_octaves,
        },
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

fn compile_trigger(trigger: LayerTriggerDefinition) -> CompiledLayerTrigger {
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
    layer_index: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledAdsr {
    CompiledAdsr {
        attack_samples: seconds_to_samples(
            definition.attack_seconds,
            sample_rate,
            layer_index,
            "attack_seconds",
            diagnostics,
        ),
        decay_samples: seconds_to_samples(
            definition.decay_seconds,
            sample_rate,
            layer_index,
            "decay_seconds",
            diagnostics,
        ),
        sustain_level: definition.sustain_level,
        release_samples: seconds_to_samples(
            definition.release_seconds,
            sample_rate,
            layer_index,
            "release_seconds",
            diagnostics,
        ),
    }
}

fn seconds_to_samples(
    seconds: f32,
    sample_rate: f64,
    layer_index: usize,
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
            .with_path(format!("layers[{layer_index}].envelope.{field}")),
        );
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        frames as usize
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
    fn valid_definition_compiles_to_immutable_values() {
        let source = definition();
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("compiled instrument");
        assert!(result.diagnostics.is_empty());
        assert_eq!(compiled.performance.polyphony, 4);
        assert_eq!(compiled.layers.len(), 1);
        assert_eq!(compiled.layers[0].envelope.attack_samples, 480);
    }

    #[test]
    fn compiler_selects_the_enabled_layer_instead_of_the_first_layer() {
        let mut source = definition();
        let mut disabled = source.layers[0].clone();
        disabled.id = "disabled".to_owned();
        disabled.enabled = false;
        source.layers.insert(0, disabled);

        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("enabled layer compiles");
        assert_eq!(compiled.layers.len(), 1);
        assert_eq!(compiled.layers[0].id, "body");
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn cutoff_is_clamped_with_a_warning() {
        let mut source = definition();
        source.voice_filter = Some(crate::definition::FilterDefinition {
            cutoff_hz: 20_000.0,
            resonance: 0.1,
        });
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("compiled with warning");
        assert!((compiled.voice_filter.expect("filter").cutoff_hz - 20_000.0).abs() < 1.0e-6);

        let low_rate = CompileContext {
            process_spec: ProcessSpec::new(22_050.0, 257, 2).expect("valid spec"),
            ..context()
        };
        let result = compile_instrument(&source, &low_rate);
        assert!(result.instrument.is_some());
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Warning
                && diagnostic.code == DiagnosticCode::FilterCutoffClamped
        }));
        assert!(
            (result
                .instrument
                .expect("compiled")
                .voice_filter
                .expect("filter")
                .cutoff_hz
                - 9_922.5)
                .abs()
                < 0.1
        );
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
    fn compile_is_not_affected_by_source_mutation() {
        let mut source = definition();
        let result = compile_instrument(&source, &context());
        let compiled = result.instrument.expect("compiled instrument");
        source.layers[0].gain_db = -60.0;
        assert!((compiled.layers[0].gain_linear - db_to_linear(-12.0)).abs() < 1.0e-6);
    }
}
