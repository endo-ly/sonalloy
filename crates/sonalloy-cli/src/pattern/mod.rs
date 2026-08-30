mod compile;

pub(crate) use compile::{CompiledPattern, compile, loop_note_id};

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sonalloy_core::{Diagnostic, DiagnosticCode};

use crate::musical_time::musical_duration_seconds;

pub(crate) const PATTERN_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PatternDefinition {
    pub(crate) schema_version: u32,
    #[serde(default)]
    pub(crate) name: Option<String>,
    pub(crate) ticks_per_beat: u16,
    pub(crate) length_ticks: u64,
    pub(crate) tempo_changes: Vec<PatternTempoChange>,
    pub(crate) time_signature_changes: Vec<PatternTimeSignatureChange>,
    pub(crate) events: Vec<PatternEvent>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PatternTempoChange {
    pub(crate) tick: u64,
    pub(crate) bpm: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PatternTimeSignatureChange {
    pub(crate) tick: u64,
    pub(crate) numerator: u8,
    pub(crate) denominator: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PatternEvent {
    Note {
        tick: u64,
        duration_ticks: u64,
        note: u8,
        velocity: u8,
    },
    SustainPedal {
        tick: u64,
        down: bool,
    },
    PitchBend {
        tick: u64,
        value: f32,
    },
    ModWheel {
        tick: u64,
        value: f32,
    },
    Aftertouch {
        tick: u64,
        value: f32,
    },
    ParameterChange {
        tick: u64,
        parameter: String,
        native_value: f32,
    },
}

impl PatternEvent {
    pub(crate) const fn is_note(&self) -> bool {
        matches!(self, Self::Note { .. })
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PatternInspection {
    pub(crate) name: Option<String>,
    pub(crate) schema_version: u32,
    pub(crate) ticks_per_beat: u16,
    pub(crate) length_ticks: u64,
    pub(crate) tempo_change_count: usize,
    pub(crate) time_signature_change_count: usize,
    pub(crate) note_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note_min: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note_max: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) velocity_min: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) velocity_max: Option<u8>,
    pub(crate) sustain_event_count: usize,
    pub(crate) pitch_bend_event_count: usize,
    pub(crate) mod_wheel_event_count: usize,
    pub(crate) aftertouch_event_count: usize,
    pub(crate) parameter_change_count: usize,
    pub(crate) distinct_parameter_ids: Vec<String>,
    pub(crate) musical_duration_seconds: f64,
}
pub(crate) fn default_pattern() -> PatternDefinition {
    PatternDefinition {
        schema_version: PATTERN_SCHEMA_VERSION,
        name: None,
        ticks_per_beat: 480,
        length_ticks: 1_920,
        tempo_changes: vec![PatternTempoChange {
            tick: 0,
            bpm: 120.0,
        }],
        time_signature_changes: vec![PatternTimeSignatureChange {
            tick: 0,
            numerator: 4,
            denominator: 4,
        }],
        events: vec![PatternEvent::Note {
            tick: 0,
            duration_ticks: 480,
            note: 60,
            velocity: 100,
        }],
    }
}

pub(crate) fn validate(pattern: &PatternDefinition) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if pattern.schema_version != PATTERN_SCHEMA_VERSION {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::SchemaUnsupported,
                format!("pattern schema_version must be {PATTERN_SCHEMA_VERSION}"),
            )
            .with_path("schema_version"),
        );
    }
    if pattern.ticks_per_beat == 0 || pattern.ticks_per_beat > 32_767 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "ticks_per_beat must be between 1 and 32767",
            )
            .with_path("ticks_per_beat"),
        );
    }
    if pattern.length_ticks == 0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "length_ticks must be greater than zero",
            )
            .with_path("length_ticks"),
        );
    }
    validate_tempo_changes(pattern, &mut diagnostics);
    validate_time_signature_changes(pattern, &mut diagnostics);

    if !pattern.events.iter().any(PatternEvent::is_note) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "events must contain at least one note",
            )
            .with_path("events"),
        );
    }
    for (index, event) in pattern.events.iter().enumerate() {
        validate_event(event, index, pattern.length_ticks, &mut diagnostics);
    }
    diagnostics
}

fn validate_tempo_changes(pattern: &PatternDefinition, diagnostics: &mut Vec<Diagnostic>) {
    if pattern.tempo_changes.is_empty() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::RequiredFieldMissing,
                "tempo_changes must contain at least one change",
            )
            .with_path("tempo_changes"),
        );
        return;
    }
    if pattern.tempo_changes[0].tick != 0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "the first tempo change must be at tick zero",
            )
            .with_path("tempo_changes[0].tick"),
        );
    }
    for (index, change) in pattern.tempo_changes.iter().enumerate() {
        let path = format!("tempo_changes[{index}]");
        if change.tick >= pattern.length_ticks && pattern.length_ticks > 0 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "tempo change tick must be less than length_ticks",
                )
                .with_path(format!("{path}.tick")),
            );
        }
        if !change.bpm.is_finite() || change.bpm <= 0.0 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "bpm must be finite and greater than zero",
                )
                .with_path(format!("{path}.bpm")),
            );
        }
        if index > 0 && pattern.tempo_changes[index - 1].tick >= change.tick {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::EventOrderInvalid,
                    "tempo change ticks must be strictly ascending",
                )
                .with_path(format!("{path}.tick")),
            );
        }
    }
}

fn validate_time_signature_changes(pattern: &PatternDefinition, diagnostics: &mut Vec<Diagnostic>) {
    if pattern.time_signature_changes.is_empty() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::RequiredFieldMissing,
                "time_signature_changes must contain at least one change",
            )
            .with_path("time_signature_changes"),
        );
        return;
    }
    if pattern.time_signature_changes[0].tick != 0 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "the first time signature change must be at tick zero",
            )
            .with_path("time_signature_changes[0].tick"),
        );
    }
    for (index, change) in pattern.time_signature_changes.iter().enumerate() {
        let path = format!("time_signature_changes[{index}]");
        if change.tick >= pattern.length_ticks && pattern.length_ticks > 0 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "time signature tick must be less than length_ticks",
                )
                .with_path(format!("{path}.tick")),
            );
        }
        if change.numerator == 0 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "time signature numerator must be greater than zero",
                )
                .with_path(format!("{path}.numerator")),
            );
        }
        if change.denominator == 0
            || change.denominator > 128
            || !change.denominator.is_power_of_two()
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "time signature denominator must be a power of two between 1 and 128",
                )
                .with_path(format!("{path}.denominator")),
            );
        }
        if index > 0 && pattern.time_signature_changes[index - 1].tick >= change.tick {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::EventOrderInvalid,
                    "time signature ticks must be strictly ascending",
                )
                .with_path(format!("{path}.tick")),
            );
        }
    }
}

#[allow(clippy::too_many_lines)]
fn validate_event(
    event: &PatternEvent,
    index: usize,
    length_ticks: u64,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = format!("events[{index}]");
    match event {
        PatternEvent::Note {
            tick,
            duration_ticks,
            note,
            velocity,
        } => {
            if *tick >= length_ticks {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "note tick must be less than length_ticks",
                    )
                    .with_path(format!("{path}.tick")),
                );
            }
            if *duration_ticks == 0 {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "duration_ticks must be greater than zero",
                    )
                    .with_path(format!("{path}.duration_ticks")),
                );
            }
            let Some(end_tick) = tick.checked_add(*duration_ticks) else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "note end tick overflows the tick counter",
                    )
                    .with_path(format!("{path}.duration_ticks")),
                );
                return;
            };
            if end_tick > length_ticks {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "note end tick must not exceed length_ticks",
                    )
                    .with_path(format!("{path}.duration_ticks")),
                );
            }
            if *note > 127 {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "note must be between 0 and 127",
                    )
                    .with_path(format!("{path}.note")),
                );
            }
            if *velocity == 0 || *velocity > 127 {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "velocity must be between 1 and 127",
                    )
                    .with_path(format!("{path}.velocity")),
                );
            }
        }
        PatternEvent::SustainPedal { tick, .. } => {
            validate_control_tick(*tick, length_ticks, &path, diagnostics);
        }
        PatternEvent::PitchBend { tick, value } => {
            validate_control_tick(*tick, length_ticks, &path, diagnostics);
            validate_normalized_value(*value, -1.0, 1.0, &path, "pitch bend", diagnostics);
        }
        PatternEvent::ModWheel { tick, value } => {
            validate_control_tick(*tick, length_ticks, &path, diagnostics);
            validate_normalized_value(*value, 0.0, 1.0, &path, "mod wheel", diagnostics);
        }
        PatternEvent::Aftertouch { tick, value } => {
            validate_control_tick(*tick, length_ticks, &path, diagnostics);
            validate_normalized_value(*value, 0.0, 1.0, &path, "aftertouch", diagnostics);
        }
        PatternEvent::ParameterChange {
            tick,
            parameter,
            native_value,
        } => {
            validate_control_tick(*tick, length_ticks, &path, diagnostics);
            if parameter.is_empty() {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParameterNotFound,
                        "parameter must not be empty",
                    )
                    .with_path(format!("{path}.parameter")),
                );
            }
            if !native_value.is_finite() {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "native_value must be finite",
                    )
                    .with_path(format!("{path}.native_value")),
                );
            }
        }
    }
}

fn validate_control_tick(
    tick: u64,
    length_ticks: u64,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if tick > length_ticks {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "control event tick must not exceed length_ticks",
            )
            .with_path(format!("{path}.tick")),
        );
    }
}

fn validate_normalized_value(
    value: f32,
    min: f32,
    max: f32,
    path: &str,
    label: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !value.is_finite() || !(min..=max).contains(&value) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                format!("{label} value must be finite and between {min} and {max}"),
            )
            .with_path(format!("{path}.value")),
        );
    }
}

pub(crate) fn inspect(pattern: &PatternDefinition) -> Result<PatternInspection, Vec<Diagnostic>> {
    let diagnostics = validate(pattern);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let tempo_changes = compile::tempo_points(pattern);
    let musical_duration_seconds =
        musical_duration_seconds(pattern.length_ticks, pattern.ticks_per_beat, &tempo_changes)
            .map_err(|error| vec![compile::time_error(error, "tempo_changes")])?;
    let mut notes = Vec::new();
    let mut velocities = Vec::new();
    let mut distinct_parameter_ids = BTreeSet::new();
    let mut counts = [0_usize; 5];
    for event in &pattern.events {
        match event {
            PatternEvent::Note { note, velocity, .. } => {
                notes.push(*note);
                velocities.push(*velocity);
            }
            PatternEvent::SustainPedal { .. } => counts[0] += 1,
            PatternEvent::PitchBend { .. } => counts[1] += 1,
            PatternEvent::ModWheel { .. } => counts[2] += 1,
            PatternEvent::Aftertouch { .. } => counts[3] += 1,
            PatternEvent::ParameterChange { parameter, .. } => {
                counts[4] += 1;
                distinct_parameter_ids.insert(parameter.clone());
            }
        }
    }
    Ok(PatternInspection {
        name: pattern.name.clone(),
        schema_version: pattern.schema_version,
        ticks_per_beat: pattern.ticks_per_beat,
        length_ticks: pattern.length_ticks,
        tempo_change_count: pattern.tempo_changes.len(),
        time_signature_change_count: pattern.time_signature_changes.len(),
        note_count: notes.len(),
        note_min: notes.iter().copied().min(),
        note_max: notes.iter().copied().max(),
        velocity_min: velocities.iter().copied().min(),
        velocity_max: velocities.iter().copied().max(),
        sustain_event_count: counts[0],
        pitch_bend_event_count: counts[1],
        mod_wheel_event_count: counts[2],
        aftertouch_event_count: counts[3],
        parameter_change_count: counts[4],
        distinct_parameter_ids: distinct_parameter_ids.into_iter().collect(),
        musical_duration_seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::{PatternDefinition, default_pattern, validate};

    #[test]
    fn default_pattern_is_valid_and_one_bar_long() {
        let pattern = default_pattern();

        assert!(validate(&pattern).is_empty());
        assert_eq!(pattern.length_ticks, 1_920);
    }

    #[test]
    fn unknown_event_fields_are_rejected_by_serde() {
        let error = serde_json::from_str::<PatternDefinition>(
            r#"{
                "schema_version": 1,
                "name": null,
                "ticks_per_beat": 480,
                "length_ticks": 480,
                "tempo_changes": [{"tick": 0, "bpm": 120.0}],
                "time_signature_changes": [{"tick": 0, "numerator": 4, "denominator": 4}],
                "events": [{"type": "note", "tick": 0, "duration_ticks": 240, "note": 60, "velocity": 100, "extra": true}]
            }"#,
        )
        .expect_err("unknown event field must fail");

        assert!(error.to_string().contains("unknown field"));
    }
}
