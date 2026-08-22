use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sonalloy_core::{
    CompiledInstrument, Diagnostic, DiagnosticCode, ProcessEventKind, ScheduledEvent, TempoMap,
};

use crate::musical_time::{
    MusicalTimeError, TempoPoint, build_tempo_map, musical_duration_seconds, tick_to_frame,
};

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

#[derive(Debug, Clone)]
pub(crate) struct CompiledPattern {
    pub(crate) events: Vec<ScheduledEvent>,
    pub(crate) tempo_map: TempoMap,
    pub(crate) length_frames: u64,
    pub(crate) one_shot_duration_frames: u64,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PendingPatternEvent {
    pub(crate) absolute_frame: u64,
    pub(crate) original_tick: u64,
    pub(crate) source_index: usize,
    pub(crate) kind: ProcessEventKind,
}

struct PatternTimeContext<'a> {
    pattern: &'a PatternDefinition,
    tempo_changes: &'a [TempoPoint],
    sample_rate: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PatternMidiEventKind {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    SustainPedal { down: bool },
    PitchBend { value: f32 },
    ModWheel { value: f32 },
    Aftertouch { value: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PatternMidiEvent {
    pub(crate) tick: u64,
    pub(crate) source_index: usize,
    pub(crate) kind: PatternMidiEventKind,
}

impl PatternMidiEventKind {
    pub(crate) const fn priority(self) -> u8 {
        match self {
            Self::SustainPedal { down } => ProcessEventKind::SustainPedal { down }.priority(),
            Self::NoteOff { .. } => ProcessEventKind::NoteOff { note_id: 0 }.priority(),
            Self::PitchBend { value } => ProcessEventKind::PitchBend { value }.priority(),
            Self::ModWheel { value } => ProcessEventKind::ModWheel { value }.priority(),
            Self::Aftertouch { value } => ProcessEventKind::Aftertouch { value }.priority(),
            Self::NoteOn { note, velocity } => ProcessEventKind::NoteOn {
                note_id: 0,
                note_number: note,
                velocity,
            }
            .priority(),
        }
    }
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
    let tempo_changes = tempo_points(pattern);
    let musical_duration_seconds =
        musical_duration_seconds(pattern.length_ticks, pattern.ticks_per_beat, &tempo_changes)
            .map_err(|error| vec![time_error(error, "tempo_changes")])?;
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

#[allow(clippy::too_many_lines)]
pub(crate) fn compile(
    pattern: &PatternDefinition,
    instrument: &CompiledInstrument,
    sample_rate: f64,
) -> Result<CompiledPattern, Vec<Diagnostic>> {
    let diagnostics = validate(pattern);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let tempo_changes = tempo_points(pattern);
    let mut diagnostics = Vec::new();
    let length_frames = match tick_to_frame(
        pattern.length_ticks,
        pattern.ticks_per_beat,
        &tempo_changes,
        sample_rate,
    ) {
        Ok(frame) if frame > 0 => frame,
        Ok(_) => {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "pattern length must occupy at least one frame",
                )
                .with_path("length_ticks"),
            );
            0
        }
        Err(error) => {
            diagnostics.push(time_error(error, "length_ticks"));
            0
        }
    };
    let tempo_map = match build_tempo_map(pattern.ticks_per_beat, &tempo_changes, sample_rate) {
        Ok(map) => Some(map),
        Err(error) => {
            diagnostics.push(time_error(error, "tempo_changes"));
            None
        }
    };
    let time = PatternTimeContext {
        pattern,
        tempo_changes: &tempo_changes,
        sample_rate,
    };

    let mut pending = Vec::with_capacity(pattern.events.len().saturating_mul(2));
    let mut note_serial = 0_u32;
    for (source_index, event) in pattern.events.iter().enumerate() {
        match event {
            PatternEvent::Note {
                tick,
                duration_ticks,
                note,
                velocity,
            } => {
                let Some(note_serial_next) = note_serial.checked_add(1) else {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "pattern contains more notes than the note identity capacity",
                        )
                        .with_path(format!("events[{source_index}]")),
                    );
                    continue;
                };
                let note_id = loop_note_id(0, note_serial);
                note_serial = note_serial_next;
                let Some(end_tick) = tick.checked_add(*duration_ticks) else {
                    continue;
                };
                let start_frame =
                    match tick_to_frame(*tick, pattern.ticks_per_beat, &tempo_changes, sample_rate)
                    {
                        Ok(frame) => frame,
                        Err(error) => {
                            diagnostics
                                .push(time_error(error, format!("events[{source_index}].tick")));
                            continue;
                        }
                    };
                let end_frame = match tick_to_frame(
                    end_tick,
                    pattern.ticks_per_beat,
                    &tempo_changes,
                    sample_rate,
                ) {
                    Ok(frame) => frame,
                    Err(error) => {
                        diagnostics.push(time_error(
                            error,
                            format!("events[{source_index}].duration_ticks"),
                        ));
                        continue;
                    }
                };
                if start_frame == end_frame {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "note start and end must map to different frames",
                        )
                        .with_path(format!("events[{source_index}].duration_ticks")),
                    );
                    continue;
                }
                pending.push(PendingPatternEvent {
                    absolute_frame: start_frame,
                    original_tick: *tick,
                    source_index,
                    kind: ProcessEventKind::NoteOn {
                        note_id,
                        note_number: *note,
                        velocity: *velocity,
                    },
                });
                pending.push(PendingPatternEvent {
                    absolute_frame: end_frame,
                    original_tick: end_tick,
                    source_index,
                    kind: ProcessEventKind::NoteOff { note_id },
                });
            }
            PatternEvent::SustainPedal { tick, down } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::SustainPedal { down: *down },
                    &mut diagnostics,
                );
            }
            PatternEvent::PitchBend { tick, value } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::PitchBend { value: *value },
                    &mut diagnostics,
                );
            }
            PatternEvent::ModWheel { tick, value } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::ModWheel { value: *value },
                    &mut diagnostics,
                );
            }
            PatternEvent::Aftertouch { tick, value } => {
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::Aftertouch { value: *value },
                    &mut diagnostics,
                );
            }
            PatternEvent::ParameterChange {
                tick,
                parameter,
                native_value,
            } => {
                let Some(handle) = instrument.parameter_handle(parameter) else {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ParameterNotFound,
                            "parameter id is not present in the compiled catalog",
                        )
                        .with_path(format!("events[{source_index}].parameter")),
                    );
                    continue;
                };
                let Some(descriptor) = instrument.parameter_descriptor(handle) else {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ParameterNotFound,
                            "parameter descriptor is not present in the compiled catalog",
                        )
                        .with_path(format!("events[{source_index}].parameter")),
                    );
                    continue;
                };
                let normalized = match descriptor.normalize(*native_value) {
                    Ok(normalized) => normalized,
                    Err(error) => {
                        diagnostics.push(
                            Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string())
                                .with_path(format!("events[{source_index}].native_value")),
                        );
                        continue;
                    }
                };
                push_control_event(
                    &mut pending,
                    *tick,
                    source_index,
                    &time,
                    ProcessEventKind::ParameterChange {
                        parameter: handle,
                        normalized,
                    },
                    &mut diagnostics,
                );
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    pending.sort_by_key(|event| {
        (
            event.absolute_frame,
            event.original_tick,
            event.kind.priority(),
            event.source_index,
        )
    });
    let last_event_frame = pending
        .iter()
        .map(|event| event.absolute_frame)
        .max()
        .unwrap_or(0);
    let Some(one_shot_duration_frames) = last_event_frame
        .checked_add(1)
        .map(|frame| frame.max(length_frames))
    else {
        return Err(vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "pattern duration overflows the frame counter",
        )]);
    };
    let Some(tempo_map) = tempo_map else {
        return Err(vec![Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "pattern tempo map is invalid",
        )]);
    };
    Ok(CompiledPattern {
        events: pending
            .into_iter()
            .map(|event| ScheduledEvent {
                absolute_frame: event.absolute_frame,
                kind: event.kind,
            })
            .collect(),
        tempo_map,
        length_frames,
        one_shot_duration_frames,
    })
}

fn push_control_event(
    pending: &mut Vec<PendingPatternEvent>,
    tick: u64,
    source_index: usize,
    time: &PatternTimeContext<'_>,
    kind: ProcessEventKind,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let frame = match tick_to_frame(
        tick,
        time.pattern.ticks_per_beat,
        time.tempo_changes,
        time.sample_rate,
    ) {
        Ok(frame) => frame,
        Err(error) => {
            diagnostics.push(time_error(error, format!("events[{source_index}].tick")));
            return;
        }
    };
    pending.push(PendingPatternEvent {
        absolute_frame: frame,
        original_tick: tick,
        source_index,
        kind,
    });
}

pub(crate) fn midi_events(
    pattern: &PatternDefinition,
) -> Result<Vec<PatternMidiEvent>, Vec<Diagnostic>> {
    let diagnostics = validate(pattern);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let mut events = Vec::with_capacity(pattern.events.len().saturating_mul(2));
    let mut unsupported = Vec::new();
    for (source_index, event) in pattern.events.iter().enumerate() {
        match event {
            PatternEvent::Note {
                tick,
                duration_ticks,
                note,
                velocity,
            } => {
                let end_tick = tick
                    .checked_add(*duration_ticks)
                    .expect("validated note duration fits the tick counter");
                events.push(PatternMidiEvent {
                    tick: *tick,
                    source_index,
                    kind: PatternMidiEventKind::NoteOn {
                        note: *note,
                        velocity: *velocity,
                    },
                });
                events.push(PatternMidiEvent {
                    tick: end_tick,
                    source_index,
                    kind: PatternMidiEventKind::NoteOff { note: *note },
                });
            }
            PatternEvent::SustainPedal { tick, down } => events.push(PatternMidiEvent {
                tick: *tick,
                source_index,
                kind: PatternMidiEventKind::SustainPedal { down: *down },
            }),
            PatternEvent::PitchBend { tick, value } => events.push(PatternMidiEvent {
                tick: *tick,
                source_index,
                kind: PatternMidiEventKind::PitchBend { value: *value },
            }),
            PatternEvent::ModWheel { tick, value } => events.push(PatternMidiEvent {
                tick: *tick,
                source_index,
                kind: PatternMidiEventKind::ModWheel { value: *value },
            }),
            PatternEvent::Aftertouch { tick, value } => events.push(PatternMidiEvent {
                tick: *tick,
                source_index,
                kind: PatternMidiEventKind::Aftertouch { value: *value },
            }),
            PatternEvent::ParameterChange { .. } => unsupported.push(
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "Sonalloy parameter changes cannot be represented in Standard MIDI",
                )
                .with_path(format!("events[{source_index}]")),
            ),
        }
    }
    if !unsupported.is_empty() {
        return Err(unsupported);
    }
    events.sort_by_key(|event| (event.tick, event.kind.priority(), event.source_index));
    Ok(events)
}

pub(crate) fn time_signature_denominator_power(denominator: u8) -> u8 {
    u8::try_from(denominator.trailing_zeros()).expect("u8 denominator has a small exponent")
}

pub(crate) fn tempo_points(pattern: &PatternDefinition) -> Vec<TempoPoint> {
    pattern
        .tempo_changes
        .iter()
        .map(|change| TempoPoint {
            tick: change.tick,
            bpm: change.bpm,
        })
        .collect()
}

pub(crate) fn loop_note_id(iteration: u32, note_serial: u32) -> u64 {
    (u64::from(iteration) << 32) | u64::from(note_serial)
}

fn time_error(error: MusicalTimeError, path: impl Into<String>) -> Diagnostic {
    Diagnostic::error(DiagnosticCode::ValueOutOfRange, error.to_string()).with_path(path)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sonalloy_core::{CompileContext, ProcessSpec, compile_instrument};

    use super::*;

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

    #[test]
    fn same_tick_midi_events_use_core_priority() {
        let pattern = PatternDefinition {
            length_ticks: 480,
            events: vec![
                PatternEvent::Note {
                    tick: 0,
                    duration_ticks: 240,
                    note: 60,
                    velocity: 100,
                },
                PatternEvent::SustainPedal {
                    tick: 0,
                    down: true,
                },
            ],
            ..default_pattern()
        };

        let events = midi_events(&pattern).expect("valid MIDI events");
        assert!(matches!(
            events[0].kind,
            PatternMidiEventKind::SustainPedal { down: true }
        ));
        assert!(matches!(
            events[1].kind,
            PatternMidiEventKind::NoteOn { .. }
        ));
    }

    #[test]
    fn pattern_compile_preserves_musical_positions_across_sample_rates() {
        let definition = crate::default_definition();
        let pattern = default_pattern();
        for (sample_rate_hz, sample_rate) in [
            (44_100_u32, 44_100.0),
            (48_000, 48_000.0),
            (96_000, 96_000.0),
        ] {
            let process_spec = ProcessSpec::new(sample_rate, 257, 2).expect("process spec");
            let compiled_instrument = compile_instrument(
                &definition,
                &CompileContext {
                    definition_base_dir: PathBuf::from("."),
                    process_spec,
                },
            )
            .instrument
            .expect("instrument compiles");

            let compiled =
                compile(&pattern, &compiled_instrument, sample_rate).expect("pattern compiles");

            assert_eq!(compiled.length_frames, u64::from(sample_rate_hz) * 2);
            assert!(matches!(
                compiled.events.as_slice(),
                [
                    ScheduledEvent {
                        absolute_frame: 0,
                        kind: ProcessEventKind::NoteOn { .. }
                    },
                    ScheduledEvent {
                        absolute_frame,
                        kind: ProcessEventKind::NoteOff { .. }
                    }
                ] if *absolute_frame == u64::from(sample_rate_hz) / 2
            ));
        }
    }
}
