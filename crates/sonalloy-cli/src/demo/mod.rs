use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sonalloy_core::{Diagnostic, DiagnosticCode, DiagnosticSeverity};

use crate::command::load_and_compile;
use crate::command::pattern::load_pattern;
use crate::musical_time::musical_duration_seconds;
use crate::output::CliFailure;
use crate::pattern::{CompiledPattern, PatternDefinition, compile as compile_pattern};

mod master;

pub(crate) use master::{FfmpegError, MasterReport, encode_mp3, master};

pub(crate) const DEMO_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DemoDefinition {
    pub(crate) schema_version: u32,
    #[serde(default)]
    pub(crate) name: Option<String>,
    pub(crate) parts: Vec<DemoPart>,
    #[serde(default)]
    pub(crate) mix: DemoMix,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DemoPart {
    pub(crate) id: String,
    pub(crate) instrument: PathBuf,
    pub(crate) pattern: PathBuf,
    #[serde(default)]
    pub(crate) gain_db: f64,
    #[serde(default)]
    pub(crate) midi_channel: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DemoMix {
    #[serde(default)]
    pub(crate) fade_out_seconds: f64,
    #[serde(default)]
    pub(crate) master: Option<DemoMaster>,
}

impl Default for DemoMix {
    fn default() -> Self {
        Self {
            fade_out_seconds: 0.0,
            master: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DemoMaster {
    pub(crate) integrated_lufs: f64,
    pub(crate) true_peak_db: f64,
    pub(crate) loudness_range_lu: f64,
}

#[derive(Debug)]
pub(crate) struct LoadedDemo {
    pub(crate) definition: DemoDefinition,
    pub(crate) parts: Vec<LoadedDemoPart>,
    pub(crate) length_ticks: u64,
    pub(crate) musical_duration_seconds: f64,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub(crate) struct LoadedDemoPart {
    pub(crate) definition: DemoPart,
    pub(crate) instrument: Arc<sonalloy_core::CompiledInstrument>,
    pub(crate) pattern: PatternDefinition,
    pub(crate) compiled_pattern: CompiledPattern,
    pub(crate) midi_channel: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DemoInspection {
    pub(crate) schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    pub(crate) part_count: usize,
    pub(crate) ticks_per_beat: u16,
    pub(crate) length_ticks: u64,
    pub(crate) musical_duration_seconds: f64,
    pub(crate) tempo_change_count: usize,
    pub(crate) time_signature_change_count: usize,
    pub(crate) parts: Vec<DemoPartInspection>,
    pub(crate) mix: DemoMix,
    pub(crate) ffmpeg_required: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DemoPartInspection {
    pub(crate) id: String,
    pub(crate) instrument: PathBuf,
    pub(crate) pattern: PathBuf,
    pub(crate) gain_db: f64,
    pub(crate) midi_channel: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TimeAxisChange {
    tick: u64,
    bpm: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct TimeAxis {
    ticks_per_beat: u16,
    tempo_changes: Vec<TimeAxisChange>,
    time_signature_changes: Vec<(u64, u8, u8)>,
}

struct DemoLoadContext<'a> {
    base_dir: &'a Path,
    sample_rate: u32,
    block_size: usize,
    diagnostics: &'a mut Vec<Diagnostic>,
    exit_code: &'a mut u8,
    reference_axis: &'a mut Option<TimeAxis>,
}

pub(crate) fn load(
    path: &Path,
    sample_rate: u32,
    block_size: usize,
) -> Result<LoadedDemo, CliFailure> {
    let (definition, base_dir) = load_definition(path)?;
    let mut diagnostics = validate_definition(&definition);
    let mut exit_code = 1;
    let midi_channels = resolve_midi_channels(&definition.parts);
    let mut reference_axis = None;
    let mut loaded_parts = Vec::with_capacity(definition.parts.len());
    let mut context = DemoLoadContext {
        base_dir: &base_dir,
        sample_rate,
        block_size,
        diagnostics: &mut diagnostics,
        exit_code: &mut exit_code,
        reference_axis: &mut reference_axis,
    };

    for (index, (part, midi_channel)) in definition.parts.iter().zip(midi_channels).enumerate() {
        if let Some(loaded_part) = load_part(index, part, midi_channel, &mut context) {
            loaded_parts.push(loaded_part);
        }
    }

    if has_errors(&diagnostics) {
        return Err(CliFailure {
            code: exit_code,
            diagnostics,
        });
    }
    if loaded_parts.len() != definition.parts.len() {
        return Err(CliFailure {
            code: 1,
            diagnostics: vec![Diagnostic::error(
                DiagnosticCode::DefinitionError,
                "not every Demo part could be prepared",
            )],
        });
    }

    let first_pattern = &loaded_parts[0].pattern;
    let length_ticks = loaded_parts
        .iter()
        .map(|part| part.pattern.length_ticks)
        .max()
        .expect("Demo validation requires at least one part");
    let tempo_changes = crate::pattern::tempo_points(first_pattern);
    let musical_duration_seconds =
        musical_duration_seconds(length_ticks, first_pattern.ticks_per_beat, &tempo_changes)
            .map_err(|error| CliFailure {
                code: 1,
                diagnostics: vec![
                    Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string())
                        .with_path("parts[0].pattern.tempo_changes"),
                ],
            })?;
    Ok(LoadedDemo {
        definition,
        parts: loaded_parts,
        length_ticks,
        musical_duration_seconds,
        diagnostics,
    })
}

fn load_part(
    index: usize,
    part: &DemoPart,
    midi_channel: Option<u8>,
    context: &mut DemoLoadContext<'_>,
) -> Option<LoadedDemoPart> {
    let instrument = load_instrument_reference(index, part, context);
    let pattern = load_pattern_reference(index, part, context)?;
    let pattern_diagnostics = crate::pattern::validate(&pattern);
    if has_errors(&pattern_diagnostics) {
        append_part_diagnostics(context.diagnostics, pattern_diagnostics, index, "pattern");
        return None;
    }
    let axis = time_axis(&pattern);
    if let Some(expected) = context.reference_axis.as_ref() {
        append_time_axis_diagnostics(context.diagnostics, expected, &axis, index);
    } else {
        *context.reference_axis = Some(axis);
    }
    let instrument = instrument?;
    let compiled_pattern =
        match compile_pattern(&pattern, &instrument, f64::from(context.sample_rate)) {
            Ok(compiled_pattern) => compiled_pattern,
            Err(pattern_diagnostics) => {
                append_part_diagnostics(context.diagnostics, pattern_diagnostics, index, "pattern");
                return None;
            }
        };
    Some(LoadedDemoPart {
        definition: part.clone(),
        instrument,
        pattern,
        compiled_pattern,
        midi_channel,
    })
}

fn load_instrument_reference(
    index: usize,
    part: &DemoPart,
    context: &mut DemoLoadContext<'_>,
) -> Option<Arc<sonalloy_core::CompiledInstrument>> {
    let path = resolve_reference_path(context.base_dir, &part.instrument);
    match load_and_compile(&path, context.sample_rate, context.block_size) {
        Ok((instrument, instrument_diagnostics)) => {
            append_reference_diagnostics(
                context.diagnostics,
                instrument_diagnostics,
                index,
                "instrument",
                &path,
            );
            Some(instrument)
        }
        Err(failure) => {
            *context.exit_code = (*context.exit_code).max(failure.code);
            append_reference_diagnostics(
                context.diagnostics,
                failure.diagnostics,
                index,
                "instrument",
                &path,
            );
            None
        }
    }
}

fn load_pattern_reference(
    index: usize,
    part: &DemoPart,
    context: &mut DemoLoadContext<'_>,
) -> Option<PatternDefinition> {
    let path = resolve_reference_path(context.base_dir, &part.pattern);
    match load_pattern(&path) {
        Ok(pattern) => Some(pattern),
        Err(failure) => {
            *context.exit_code = (*context.exit_code).max(failure.code);
            append_reference_diagnostics(
                context.diagnostics,
                failure.diagnostics,
                index,
                "pattern",
                &path,
            );
            None
        }
    }
}

pub(crate) fn inspect(demo: &LoadedDemo) -> DemoInspection {
    let first_pattern = &demo.parts[0].pattern;
    DemoInspection {
        schema_version: demo.definition.schema_version,
        name: demo.definition.name.clone(),
        part_count: demo.parts.len(),
        ticks_per_beat: first_pattern.ticks_per_beat,
        length_ticks: demo.length_ticks,
        musical_duration_seconds: demo.musical_duration_seconds,
        tempo_change_count: first_pattern.tempo_changes.len(),
        time_signature_change_count: first_pattern.time_signature_changes.len(),
        parts: demo
            .parts
            .iter()
            .map(|part| DemoPartInspection {
                id: part.definition.id.clone(),
                instrument: part.definition.instrument.clone(),
                pattern: part.definition.pattern.clone(),
                gain_db: part.definition.gain_db,
                midi_channel: part.midi_channel,
            })
            .collect(),
        mix: demo.definition.mix.clone(),
        ffmpeg_required: demo.definition.mix.master.is_some(),
        diagnostics: demo.diagnostics.clone(),
    }
}

pub(crate) fn validate_definition(definition: &DemoDefinition) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if definition.schema_version != DEMO_SCHEMA_VERSION {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::SchemaUnsupported,
                format!("Demo schema_version must be {DEMO_SCHEMA_VERSION}"),
            )
            .with_path("schema_version"),
        );
    }
    if definition.parts.is_empty() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::RequiredFieldMissing,
                "parts must contain at least one part",
            )
            .with_path("parts"),
        );
    }

    let mut part_ids = HashSet::with_capacity(definition.parts.len());
    let mut explicit_channels = [false; 16];
    for (index, part) in definition.parts.iter().enumerate() {
        let path = format!("parts[{index}]");
        validate_part_id(&part.id, &path, &mut diagnostics);
        if !part_ids.insert(&part.id) {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::IdDuplicated, "part id must be unique")
                    .with_path(format!("{path}.id")),
            );
        }
        if !part.gain_db.is_finite() || gain_linear(part.gain_db).is_none() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "gain_db must be finite and produce a finite linear gain",
                )
                .with_path(format!("{path}.gain_db")),
            );
        }
        if let Some(channel) = part.midi_channel {
            if (1..=16).contains(&channel) {
                let channel_index = usize::from(channel - 1);
                if explicit_channels[channel_index] {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::MidiError,
                            "explicit midi_channel values must be unique",
                        )
                        .with_path(format!("{path}.midi_channel")),
                    );
                }
                explicit_channels[channel_index] = true;
            } else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "midi_channel must be between 1 and 16",
                    )
                    .with_path(format!("{path}.midi_channel")),
                );
            }
        }
    }
    if !definition.mix.fade_out_seconds.is_finite() || definition.mix.fade_out_seconds < 0.0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "fade_out_seconds must be finite and non-negative",
            )
            .with_path("mix.fade_out_seconds"),
        );
    }
    if let Some(master) = &definition.mix.master {
        validate_master_value(
            master.integrated_lufs,
            -70.0..=-5.0,
            "integrated_lufs",
            "mix.master.integrated_lufs",
            &mut diagnostics,
        );
        validate_master_value(
            master.true_peak_db,
            -9.0..=0.0,
            "true_peak_db",
            "mix.master.true_peak_db",
            &mut diagnostics,
        );
        validate_master_value(
            master.loudness_range_lu,
            1.0..=50.0,
            "loudness_range_lu",
            "mix.master.loudness_range_lu",
            &mut diagnostics,
        );
    }
    diagnostics
}

fn validate_part_id(id: &str, part_path: &str, diagnostics: &mut Vec<Diagnostic>) {
    let valid_length = (1..=64).contains(&id.len());
    let valid_characters = id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    let valid_first = id
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric());
    if !valid_length || !valid_characters || !valid_first {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "id must be 1 to 64 ASCII characters, start with a letter or digit, and contain only letters, digits, '.', '_' or '-'",
            )
            .with_path(format!("{part_path}.id")),
        );
    }
}

fn validate_master_value(
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    name: &str,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !value.is_finite() || !range.contains(&value) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                format!("{name} must be finite and within the FFmpeg loudnorm range"),
            )
            .with_path(path),
        );
    }
}

fn resolve_midi_channels(parts: &[DemoPart]) -> Vec<Option<u8>> {
    let mut used = [false; 16];
    for part in parts {
        if let Some(channel) = part
            .midi_channel
            .filter(|channel| (1..=16).contains(channel))
        {
            used[usize::from(channel - 1)] = true;
        }
    }
    parts
        .iter()
        .map(|part| {
            if let Some(channel) = part.midi_channel {
                return (1..=16).contains(&channel).then_some(channel);
            }
            let (index, channel) = used
                .iter_mut()
                .enumerate()
                .find_map(|(index, used)| (!*used).then_some((index, used)))?;
            *channel = true;
            u8::try_from(index + 1).ok()
        })
        .collect()
}

fn time_axis(pattern: &PatternDefinition) -> TimeAxis {
    TimeAxis {
        ticks_per_beat: pattern.ticks_per_beat,
        tempo_changes: pattern
            .tempo_changes
            .iter()
            .map(|change| TimeAxisChange {
                tick: change.tick,
                bpm: change.bpm,
            })
            .collect(),
        time_signature_changes: pattern
            .time_signature_changes
            .iter()
            .map(|change| (change.tick, change.numerator, change.denominator))
            .collect(),
    }
}

fn append_time_axis_diagnostics(
    diagnostics: &mut Vec<Diagnostic>,
    expected: &TimeAxis,
    actual: &TimeAxis,
    part_index: usize,
) {
    let prefix = format!("parts[{part_index}].pattern");
    if expected.ticks_per_beat != actual.ticks_per_beat {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "all Demo patterns must use the same ticks_per_beat",
            )
            .with_path(format!("{prefix}.ticks_per_beat")),
        );
    }
    if expected.tempo_changes != actual.tempo_changes {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "all Demo patterns must use the same tempo_changes",
            )
            .with_path(format!("{prefix}.tempo_changes")),
        );
    }
    if expected.time_signature_changes != actual.time_signature_changes {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "all Demo patterns must use the same time_signature_changes",
            )
            .with_path(format!("{prefix}.time_signature_changes")),
        );
    }
}

fn load_definition(path: &Path) -> Result<(DemoDefinition, PathBuf), CliFailure> {
    let text = std::fs::read_to_string(path).map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(DiagnosticCode::DefinitionError, "could not read Demo input")
                .with_path(path.to_string_lossy())
                .with_detail(error.to_string()),
        ],
    })?;
    let definition = serde_json::from_str(&text).map_err(|error| CliFailure {
        code: 1,
        diagnostics: vec![
            Diagnostic::error(DiagnosticCode::JsonInvalid, "could not parse Demo JSON")
                .with_path(path.to_string_lossy())
                .with_detail(format!(
                    "line {}, column {}: {error}",
                    error.line(),
                    error.column()
                )),
        ],
    })?;
    let base_dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    Ok((definition, base_dir))
}

pub(crate) fn resolve_reference_path(base_dir: &Path, reference: &Path) -> PathBuf {
    if reference.is_absolute() {
        reference.to_path_buf()
    } else {
        base_dir.join(reference)
    }
}

pub(crate) fn gain_linear(gain_db: f64) -> Option<f64> {
    if !gain_db.is_finite() {
        return None;
    }
    let gain = 10.0_f64.powf(gain_db / 20.0);
    gain.is_finite().then_some(gain)
}

#[derive(Debug)]
pub(crate) struct StereoMix {
    pub(crate) sample_rate: u32,
    pub(crate) left: Vec<f32>,
    pub(crate) right: Vec<f32>,
}

impl StereoMix {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    pub(crate) fn add_part(
        &mut self,
        audio: &sonalloy_core::RenderedAudio,
        gain_db: f64,
    ) -> Result<(), String> {
        if audio.sample_rate != self.sample_rate {
            return Err("part sample rate does not match the Demo sample rate".to_owned());
        }
        let [left, right] = audio.channels.as_slice() else {
            return Err("part render must contain exactly two channels".to_owned());
        };
        if left.len() != right.len() {
            return Err("part render channels have different lengths".to_owned());
        }
        let gain = gain_linear(gain_db).ok_or_else(|| "part gain is not finite".to_owned())?;
        #[allow(clippy::cast_possible_truncation)]
        let gain = gain as f32;
        if !gain.is_finite() {
            return Err("part gain cannot be represented as a finite audio gain".to_owned());
        }
        if self.left.len() < left.len() {
            self.left.resize(left.len(), 0.0);
            self.right.resize(right.len(), 0.0);
        }
        for (mixed, sample) in self.left.iter_mut().zip(left) {
            *mixed += *sample * gain;
        }
        for (mixed, sample) in self.right.iter_mut().zip(right) {
            *mixed += *sample * gain;
        }
        Ok(())
    }

    pub(crate) fn apply_fade(&mut self, fade_out_seconds: f64) -> Result<(), String> {
        if !fade_out_seconds.is_finite() || fade_out_seconds < 0.0 {
            return Err("fade_out_seconds must be finite and non-negative".to_owned());
        }
        #[allow(clippy::cast_precision_loss)]
        let duration_seconds = self.left.len() as f64 / f64::from(self.sample_rate);
        if fade_out_seconds > duration_seconds {
            return Err("fade_out_seconds must not exceed the final mix duration".to_owned());
        }
        if fade_out_seconds == 0.0 || self.left.is_empty() {
            return Ok(());
        }
        let fade_frames =
            sonalloy_core::seconds_to_frames(fade_out_seconds, f64::from(self.sample_rate))
                .map_err(|error| error.to_string())?;
        let fade_frames = usize::try_from(fade_frames)
            .map_err(|_| "fade frame count does not fit in memory".to_owned())?;
        if fade_frames == 0 {
            return Ok(());
        }
        let start = self.left.len().saturating_sub(fade_frames);
        for frame in start..self.left.len() {
            let gain = if fade_frames == 1 {
                0.0
            } else {
                #[allow(clippy::cast_precision_loss)]
                let denominator = (fade_frames - 1) as f32;
                #[allow(clippy::cast_precision_loss)]
                let progress = (frame - start) as f32 / denominator;
                (1.0 - progress).clamp(0.0, 1.0)
            };
            self.left[frame] *= gain;
            self.right[frame] *= gain;
        }
        Ok(())
    }

    pub(crate) fn into_audio(self) -> sonalloy_core::RenderedAudio {
        sonalloy_core::RenderedAudio {
            sample_rate: self.sample_rate,
            channels: vec![self.left, self.right],
        }
    }
}

fn append_reference_diagnostics(
    target: &mut Vec<Diagnostic>,
    diagnostics: Vec<Diagnostic>,
    part_index: usize,
    field: &str,
    reference_path: &Path,
) {
    let reference_path = reference_path.to_string_lossy();
    let prefix = format!("parts[{part_index}].{field}");
    for mut diagnostic in diagnostics {
        diagnostic.path = Some(match diagnostic.path.take() {
            None => prefix.clone(),
            Some(path) if path == reference_path => prefix.clone(),
            Some(path) => format!("{prefix}.{path}"),
        });
        target.push(diagnostic);
    }
}

fn append_part_diagnostics(
    target: &mut Vec<Diagnostic>,
    diagnostics: Vec<Diagnostic>,
    part_index: usize,
    field: &str,
) {
    let prefix = format!("parts[{part_index}].{field}");
    for mut diagnostic in diagnostics {
        diagnostic.path = Some(match diagnostic.path.take() {
            Some(path) => format!("{prefix}.{path}"),
            None => prefix.clone(),
        });
        target.push(diagnostic);
    }
}

fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        DemoDefinition, DemoMix, DemoPart, append_time_axis_diagnostics, gain_linear,
        resolve_midi_channels, resolve_reference_path, time_axis, validate_definition,
    };

    fn part(id: &str) -> DemoPart {
        DemoPart {
            id: id.to_owned(),
            instrument: "instrument.json".into(),
            pattern: "pattern.json".into(),
            gain_db: 0.0,
            midi_channel: None,
        }
    }

    fn definition(parts: Vec<DemoPart>) -> DemoDefinition {
        DemoDefinition {
            schema_version: 1,
            name: None,
            parts,
            mix: DemoMix::default(),
        }
    }

    #[test]
    fn defaults_and_schema_validation_are_explicit() {
        let parsed: DemoDefinition = serde_json::from_str(
            r#"{"schema_version":1,"parts":[{"id":"lead","instrument":"i.json","pattern":"p.json"}]}"#,
        )
        .expect("Demo parses");

        assert!(parsed.mix.fade_out_seconds.abs() < f64::EPSILON);
        assert!(parsed.mix.master.is_none());
        assert!(validate_definition(&parsed).is_empty());
        let mut unsupported = parsed;
        unsupported.schema_version = 2;
        assert_eq!(
            validate_definition(&unsupported)[0].path.as_deref(),
            Some("schema_version")
        );

        let empty = definition(Vec::new());
        assert!(
            validate_definition(&empty)
                .iter()
                .any(|diagnostic| { diagnostic.path.as_deref() == Some("parts") })
        );
        let unknown_field = serde_json::from_str::<DemoDefinition>(
            r#"{"schema_version":1,"parts":[],"unknown":true}"#,
        );
        assert!(unknown_field.is_err());
    }

    #[test]
    fn validation_covers_ids_channels_and_master_values() {
        let mut first = part("bad/id");
        first.midi_channel = Some(2);
        first.gain_db = f64::NAN;
        let mut second = part("bad/id");
        second.midi_channel = Some(2);
        let mut definition = definition(vec![first, second]);
        definition.mix.master = Some(super::DemoMaster {
            integrated_lufs: -100.0,
            true_peak_db: 1.0,
            loudness_range_lu: 0.0,
        });

        let diagnostics = validate_definition(&definition);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.path.as_deref() == Some("parts[0].id") })
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.path.as_deref() == Some("parts[1].id") })
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.path.as_deref() == Some("parts[0].gain_db") })
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.path.as_deref() == Some("parts[1].midi_channel") })
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("mix.master.integrated_lufs")
        }));
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.path.as_deref() == Some("mix.master.true_peak_db")
            })
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.path.as_deref() == Some("mix.master.loudness_range_lu")
        }));
    }

    #[test]
    fn omitted_channels_are_assigned_in_definition_order() {
        let mut a = part("a");
        a.midi_channel = Some(2);
        let mut c = part("c");
        c.midi_channel = Some(5);
        let channels = resolve_midi_channels(&[a, part("b"), c, part("d")]);

        assert_eq!(channels, [Some(2), Some(1), Some(5), Some(3)]);
    }

    #[test]
    fn omitted_channels_beyond_midi_capacity_remain_unassigned() {
        let parts = (0..17)
            .map(|index| part(&format!("part-{index}")))
            .collect::<Vec<_>>();
        let channels = resolve_midi_channels(&parts);

        assert_eq!(
            channels[..16],
            [
                Some(1),
                Some(2),
                Some(3),
                Some(4),
                Some(5),
                Some(6),
                Some(7),
                Some(8),
                Some(9),
                Some(10),
                Some(11),
                Some(12),
                Some(13),
                Some(14),
                Some(15),
                Some(16),
            ]
        );
        assert_eq!(channels[16], None);
    }

    #[test]
    fn patterns_share_time_axis_but_may_have_different_lengths() {
        let expected = crate::pattern::default_pattern();
        let mut shorter = expected.clone();
        shorter.length_ticks = 960;
        let expected_axis = time_axis(&expected);
        let mut diagnostics = Vec::new();
        append_time_axis_diagnostics(&mut diagnostics, &expected_axis, &time_axis(&shorter), 1);
        assert!(diagnostics.is_empty());

        shorter.ticks_per_beat = 960;
        shorter.tempo_changes[0].bpm = 100.0;
        shorter.time_signature_changes[0].numerator = 3;
        append_time_axis_diagnostics(&mut diagnostics, &expected_axis, &time_axis(&shorter), 2);
        let paths = diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.path.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "parts[2].pattern.ticks_per_beat",
                "parts[2].pattern.tempo_changes",
                "parts[2].pattern.time_signature_changes",
            ]
        );
    }

    #[test]
    fn relative_references_use_the_demo_directory() {
        assert_eq!(
            resolve_reference_path(Path::new("song"), Path::new("instruments/bass.json")),
            Path::new("song/instruments/bass.json")
        );
        assert_eq!(
            resolve_reference_path(Path::new("song"), Path::new("/tmp/bass.json")),
            Path::new("/tmp/bass.json")
        );
    }

    #[test]
    fn gain_conversion_uses_decibels() {
        let gain = gain_linear(-6.0).expect("finite gain");
        assert!((gain - 0.501_187_233_627_272_2).abs() < 1.0e-12);
        assert!(gain_linear(f64::INFINITY).is_none());
    }

    #[test]
    fn stereo_mix_extends_and_applies_each_part_gain() {
        let mut mix = super::StereoMix::new(48_000);
        let first = sonalloy_core::RenderedAudio {
            sample_rate: 48_000,
            channels: vec![vec![1.0, 2.0], vec![3.0, 4.0]],
        };
        let second = sonalloy_core::RenderedAudio {
            sample_rate: 48_000,
            channels: vec![vec![5.0], vec![6.0]],
        };

        mix.add_part(&first, 0.0).expect("first part mixes");
        mix.add_part(&second, -6.0).expect("second part mixes");

        assert_eq!(mix.left.len(), 2);
        assert!((mix.left[0] - 1.0 - 5.0 * 0.501_187_2).abs() < 1.0e-6);
        assert!((mix.left[1] - 2.0).abs() < f32::EPSILON);
        assert!((mix.right[0] - 3.0 - 6.0 * 0.501_187_2).abs() < 1.0e-6);
        assert!((mix.right[1] - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn stereo_mix_fade_reaches_zero_at_the_final_frame() {
        let mut mix = super::StereoMix {
            sample_rate: 4,
            left: vec![1.0; 4],
            right: vec![1.0; 4],
        };

        mix.apply_fade(0.5).expect("fade applies");

        assert_eq!(mix.left, vec![1.0, 1.0, 1.0, 0.0]);
        assert_eq!(mix.right, vec![1.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn stereo_mix_one_frame_fade_silences_the_final_frame() {
        let mut mix = super::StereoMix {
            sample_rate: 4,
            left: vec![1.0; 4],
            right: vec![1.0; 4],
        };

        mix.apply_fade(0.25).expect("one-frame fade applies");

        assert_eq!(mix.left, vec![1.0, 1.0, 1.0, 0.0]);
        assert_eq!(mix.right, vec![1.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn stereo_mix_rejects_a_fade_longer_than_the_mix() {
        let mut mix = super::StereoMix {
            sample_rate: 48_000,
            left: vec![0.0; 48_000],
            right: vec![0.0; 48_000],
        };

        assert!(mix.apply_fade(1.1).is_err());
    }
}
