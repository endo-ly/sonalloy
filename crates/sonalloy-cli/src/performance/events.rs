#[path = "compile.rs"]
mod compile;

pub(crate) use compile::{
    CompiledEvents, Timeline, compile, compile_on_timeline, loop_note_id, resolve_parameter_ramp,
    tempo_points,
};

use std::collections::BTreeSet;

use serde::Serialize;
use sonalloy_core::{Diagnostic, DiagnosticCode};

use crate::musical_time::musical_duration_seconds;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PerformanceEvents {
    pub(crate) name: Option<String>,
    pub(crate) ticks_per_beat: u16,
    pub(crate) length_ticks: u64,
    pub(crate) tempo_changes: Vec<TempoChange>,
    pub(crate) time_signature_changes: Vec<MeterChange>,
    pub(crate) events: Vec<MusicalEvent>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct TempoChange {
    pub(crate) tick: u64,
    pub(crate) bpm: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct MeterChange {
    pub(crate) tick: u64,
    pub(crate) numerator: u8,
    pub(crate) denominator: u8,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum MusicalEvent {
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
    ParameterRamp {
        tick: u64,
        duration_ticks: u64,
        parameter: String,
        from_value: f32,
        to_value: f32,
    },
}

impl MusicalEvent {
    pub(crate) const fn is_note(&self) -> bool {
        matches!(self, Self::Note { .. })
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EventInspection {
    pub(crate) name: Option<String>,
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
    pub(crate) parameter_ramp_count: usize,
    pub(crate) distinct_parameter_ids: Vec<String>,
    pub(crate) musical_duration_seconds: f64,
}
#[cfg(test)]
pub(crate) fn default_pattern() -> PerformanceEvents {
    PerformanceEvents {
        name: None,
        ticks_per_beat: 480,
        length_ticks: 1_920,
        tempo_changes: vec![TempoChange {
            tick: 0,
            bpm: 120.0,
        }],
        time_signature_changes: vec![MeterChange {
            tick: 0,
            numerator: 4,
            denominator: 4,
        }],
        events: vec![MusicalEvent::Note {
            tick: 0,
            duration_ticks: 480,
            note: 60,
            velocity: 100,
        }],
    }
}

pub(crate) fn validate(pattern: &PerformanceEvents) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
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

    for (index, event) in pattern.events.iter().enumerate() {
        validate_event(event, index, pattern.length_ticks, &mut diagnostics);
    }
    diagnostics
}

fn validate_tempo_changes(pattern: &PerformanceEvents, diagnostics: &mut Vec<Diagnostic>) {
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

fn validate_time_signature_changes(pattern: &PerformanceEvents, diagnostics: &mut Vec<Diagnostic>) {
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
    event: &MusicalEvent,
    index: usize,
    length_ticks: u64,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = format!("events[{index}]");
    match event {
        MusicalEvent::Note {
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
        MusicalEvent::SustainPedal { tick, .. } => {
            validate_control_tick(*tick, length_ticks, &path, diagnostics);
        }
        MusicalEvent::PitchBend { tick, value } => {
            validate_control_tick(*tick, length_ticks, &path, diagnostics);
            validate_normalized_value(*value, -1.0, 1.0, &path, "pitch bend", diagnostics);
        }
        MusicalEvent::ModWheel { tick, value } => {
            validate_control_tick(*tick, length_ticks, &path, diagnostics);
            validate_normalized_value(*value, 0.0, 1.0, &path, "mod wheel", diagnostics);
        }
        MusicalEvent::Aftertouch { tick, value } => {
            validate_control_tick(*tick, length_ticks, &path, diagnostics);
            validate_normalized_value(*value, 0.0, 1.0, &path, "aftertouch", diagnostics);
        }
        MusicalEvent::ParameterRamp {
            tick,
            duration_ticks,
            parameter,
            from_value,
            to_value,
        } => {
            validate_control_tick(*tick, length_ticks, &path, diagnostics);
            if *duration_ticks == 0
                || tick
                    .checked_add(*duration_ticks)
                    .is_none_or(|end| end > length_ticks)
            {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "ramp duration_ticks must be positive and its end tick must fit within length_ticks",
                    )
                    .with_path(format!("{path}.duration_ticks")),
                );
            }
            if parameter.is_empty() {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParameterNotFound,
                        "parameter must not be empty",
                    )
                    .with_path(format!("{path}.parameter")),
                );
            }
            for (field, value) in [("from_value", from_value), ("to_value", to_value)] {
                if !value.is_finite() {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            format!("{field} must be finite"),
                        )
                        .with_path(format!("{path}.{field}")),
                    );
                }
            }
        }
        MusicalEvent::ParameterChange {
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

pub(crate) fn inspect(pattern: &PerformanceEvents) -> Result<EventInspection, Vec<Diagnostic>> {
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
    let mut counts = [0_usize; 6];
    for event in &pattern.events {
        match event {
            MusicalEvent::Note { note, velocity, .. } => {
                notes.push(*note);
                velocities.push(*velocity);
            }
            MusicalEvent::SustainPedal { .. } => counts[0] += 1,
            MusicalEvent::PitchBend { .. } => counts[1] += 1,
            MusicalEvent::ModWheel { .. } => counts[2] += 1,
            MusicalEvent::Aftertouch { .. } => counts[3] += 1,
            MusicalEvent::ParameterChange { parameter, .. } => {
                counts[4] += 1;
                distinct_parameter_ids.insert(parameter.clone());
            }
            MusicalEvent::ParameterRamp { parameter, .. } => {
                counts[5] += 1;
                distinct_parameter_ids.insert(parameter.clone());
            }
        }
    }
    Ok(EventInspection {
        name: pattern.name.clone(),
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
        parameter_ramp_count: counts[5],
        distinct_parameter_ids: distinct_parameter_ids.into_iter().collect(),
        musical_duration_seconds,
    })
}

#[allow(clippy::too_many_lines)]
pub(crate) fn slice_section(
    pattern: &mut PerformanceEvents,
    sections: &[crate::score::Occurrence],
    name: &str,
    instrument: &sonalloy_core::CompiledInstrument,
) -> Result<(), String> {
    let found: Vec<_> = sections.iter().filter(|s| s.section == name).collect();
    if found.len() != 1 {
        return Err("section selection must identify exactly one occurrence".into());
    }
    let section = found[0];
    let start = section.start_tick;
    let end = section.end_tick;
    let tempos = tempo_points(pattern);
    let seconds = |tick| {
        crate::musical_time::musical_duration_seconds(tick, pattern.ticks_per_beat, &tempos)
            .map_err(|e| e.to_string())
    };
    let mut state = std::collections::BTreeMap::new();
    let mut events = Vec::new();
    for event in &pattern.events {
        let mut event = event.clone();
        let at = match &event {
            MusicalEvent::Note { tick, .. }
            | MusicalEvent::ParameterRamp { tick, .. }
            | MusicalEvent::SustainPedal { tick, .. }
            | MusicalEvent::PitchBend { tick, .. }
            | MusicalEvent::ModWheel { tick, .. }
            | MusicalEvent::Aftertouch { tick, .. }
            | MusicalEvent::ParameterChange { tick, .. } => *tick,
        };
        if at >= end {
            continue;
        }
        match &mut event {
            MusicalEvent::Note {
                tick,
                duration_ticks,
                ..
            } => {
                let note_end = (*tick + *duration_ticks).min(end);
                if note_end <= start {
                    if sustained_at_cut(pattern, note_end, start) {
                        *tick = 0;
                        *duration_ticks = 1;
                        events.push(event);
                    }
                    continue;
                }
                *tick = (*tick).max(start) - start;
                *duration_ticks = note_end - start - *tick;
                events.push(event);
            }
            MusicalEvent::ParameterRamp {
                tick,
                duration_ticks,
                parameter,
                from_value,
                to_value,
            } => {
                let ramp_end = *tick + *duration_ticks;
                let handle = instrument
                    .parameter_handle(parameter)
                    .ok_or_else(|| format!("unknown parameter {parameter}"))?;
                let descriptor = instrument
                    .parameter_descriptor(handle)
                    .expect("catalog handle");
                let from = descriptor
                    .normalize(*from_value)
                    .map_err(|e| e.to_string())?;
                let to = descriptor.normalize(*to_value).map_err(|e| e.to_string())?;
                let value_at = |position: u64| -> Result<f32, String> {
                    let fraction = ((seconds(position)? - seconds(at)?)
                        / (seconds(ramp_end)? - seconds(at)?))
                    .clamp(0.0, 1.0);
                    #[allow(clippy::cast_possible_truncation)]
                    let fraction = fraction as f32;
                    descriptor
                        .denormalize(from + (to - from) * fraction)
                        .map_err(|e| e.to_string())
                };
                if ramp_end <= start {
                    state.insert(
                        format!("parameter.{parameter}"),
                        MusicalEvent::ParameterChange {
                            tick: 0,
                            parameter: parameter.clone(),
                            native_value: *to_value,
                        },
                    );
                    continue;
                }
                *from_value = value_at(at.max(start))?;
                *to_value = value_at(ramp_end.min(end))?;
                *tick = at.max(start) - start;
                *duration_ticks = ramp_end.min(end) - at.max(start);
                events.push(event);
            }
            _ => {
                let key = match &event {
                    MusicalEvent::SustainPedal { .. } => "sustain".into(),
                    MusicalEvent::PitchBend { .. } => "bend".into(),
                    MusicalEvent::ModWheel { .. } => "wheel".into(),
                    MusicalEvent::Aftertouch { .. } => "aftertouch".into(),
                    MusicalEvent::ParameterChange { parameter, .. } => {
                        format!("parameter.{parameter}")
                    }
                    _ => unreachable!(),
                };
                match &mut event {
                    MusicalEvent::SustainPedal { tick, .. }
                    | MusicalEvent::PitchBend { tick, .. }
                    | MusicalEvent::ModWheel { tick, .. }
                    | MusicalEvent::Aftertouch { tick, .. }
                    | MusicalEvent::ParameterChange { tick, .. } => {
                        *tick = at.saturating_sub(start);
                    }
                    _ => unreachable!(),
                }
                if at < start {
                    state.insert(key, event);
                } else {
                    events.push(event);
                }
            }
        }
    }
    let mut all: Vec<_> = state.into_values().collect();
    all.extend(events);
    let at = |e: &MusicalEvent| match e {
        MusicalEvent::Note { tick, .. }
        | MusicalEvent::ParameterRamp { tick, .. }
        | MusicalEvent::SustainPedal { tick, .. }
        | MusicalEvent::PitchBend { tick, .. }
        | MusicalEvent::ModWheel { tick, .. }
        | MusicalEvent::Aftertouch { tick, .. }
        | MusicalEvent::ParameterChange { tick, .. } => *tick,
    };
    all.sort_by_key(at);
    pattern.events = all;
    let initial = pattern
        .tempo_changes
        .iter()
        .rev()
        .find(|t| t.tick <= start)
        .expect("initial tempo")
        .bpm;
    let mut changes = vec![TempoChange {
        tick: 0,
        bpm: initial,
    }];
    changes.extend(
        pattern
            .tempo_changes
            .iter()
            .filter(|t| t.tick > start && t.tick < end)
            .map(|t| TempoChange {
                tick: t.tick - start,
                bpm: t.bpm,
            }),
    );
    pattern.tempo_changes = changes;
    let initial = *pattern
        .time_signature_changes
        .iter()
        .rev()
        .find(|t| t.tick <= start)
        .expect("initial meter");
    let mut meters = vec![MeterChange { tick: 0, ..initial }];
    meters.extend(
        pattern
            .time_signature_changes
            .iter()
            .filter(|t| t.tick > start && t.tick < end)
            .map(|t| MeterChange {
                tick: t.tick - start,
                ..*t
            }),
    );
    pattern.time_signature_changes = meters;
    pattern.length_ticks = end - start;
    Ok(())
}

fn sustained_at_cut(pattern: &PerformanceEvents, note_end: u64, start: u64) -> bool {
    let down = pattern
        .events
        .iter()
        .filter_map(|event| match event {
            MusicalEvent::SustainPedal { tick, down } if *tick < note_end => Some(*down),
            _ => None,
        })
        .next_back()
        .unwrap_or(false);
    down&&!pattern.events.iter().any(|event|matches!(event,MusicalEvent::SustainPedal{tick,down:false} if *tick>=note_end&&*tick<=start))
}

#[cfg(test)]
mod tests {
    use super::default_pattern;

    #[test]
    fn section_selection_preserves_sustain_bend_and_normalized_ramp_state() {
        let doc = serde_json::json!({"meta":{"title":"Selection","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},"form":[{"section":"first","bars":1,"tonic":"C","scale":"ionian"},{"section":"second","bars":1,"tonic":"C","scale":"ionian","bpm":60}],"sections":{"first":{"placements":[{"instrument":"part","degree":"1","at":{"bar":1,"step":1},"duration_quarters":6},{"instrument":"part","degree":"3","at":{"bar":1,"step":1},"duration_quarters":2}],"events":[{"kind":"sustain_pedal","instrument":"part","at":{"bar":1,"step":5},"down":true},{"kind":"pitch_bend","instrument":"part","at":{"bar":1,"step":13},"value":0.5},{"kind":"automation","instrument":"part","at":{"bar":1,"step":9},"duration_quarters":4,"parameter":"layer.body.gain","from_value":-24,"to_value":0}]},"second":{"events":[{"kind":"sustain_pedal","instrument":"part","at":{"bar":1,"step":5},"down":false}]}},"instruments":{"part":{"external":true}}});
        let plan = crate::score::compile(&doc.to_string()).expect("score");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/instruments/basic-poly-synth.json");
        let (instrument, _) =
            crate::command::load_and_compile(&path, 48000, 257).expect("instrument");
        let mut events = plan.lanes[0].pattern.clone();

        super::slice_section(&mut events, &plan.sections, "second", &instrument)
            .expect("selected section");

        assert_eq!(events.length_ticks, 1920);
        assert!((events.tempo_changes[0].bpm - 60.0).abs() < f64::EPSILON);
        assert!(events.events.iter().any(|e| matches!(
            e,
            super::MusicalEvent::Note {
                tick: 0,
                duration_ticks: 960,
                note: 60,
                ..
            }
        )));
        assert!(events.events.iter().any(|e| matches!(
            e,
            super::MusicalEvent::Note {
                tick: 0,
                duration_ticks: 1,
                note: 64,
                ..
            }
        )));
        assert!(events.events.iter().any(|e|matches!(e,super::MusicalEvent::PitchBend{tick:0,value} if (*value-0.5).abs()<f32::EPSILON)));
        assert!(events.events.iter().any(|e| matches!(
            e,
            super::MusicalEvent::SustainPedal {
                tick: 0,
                down: true
            }
        )));
        assert!(events.events.iter().any(|e|matches!(e,super::MusicalEvent::ParameterRamp{tick:0,duration_ticks:960,from_value,..} if (*from_value+16.0).abs()<1e-4)));
    }

    #[test]
    fn ramp_validation_checks_duration_end_and_native_endpoints() {
        for (tick, duration_ticks, from_value, to_value, field) in [
            (0, 0, 0.0, 1.0, "duration_ticks"),
            (1900, 21, 0.0, 1.0, "duration_ticks"),
            (u64::MAX, 1, 0.0, 1.0, "duration_ticks"),
            (0, 480, f32::NAN, 1.0, "from_value"),
            (0, 480, 0.0, f32::INFINITY, "to_value"),
        ] {
            let mut pattern = default_pattern();
            let expected_path = format!("events[{}].{field}", pattern.events.len());
            pattern.events.push(super::MusicalEvent::ParameterRamp {
                tick,
                duration_ticks,
                parameter: "macro.motion".to_owned(),
                from_value,
                to_value,
            });
            let diagnostics = super::validate(&pattern);
            assert!(
                diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == sonalloy_core::DiagnosticCode::ValueOutOfRange
                        && diagnostic.path.as_deref() == Some(expected_path.as_str())
                }),
                "expected {expected_path}, received {diagnostics:?}"
            );
        }
        let mut pattern = default_pattern();
        pattern.events.push(super::MusicalEvent::ParameterRamp {
            tick: 0,
            duration_ticks: 1920,
            parameter: "macro.motion".to_owned(),
            from_value: 0.0,
            to_value: 1.0,
        });
        let inspection = super::inspect(&pattern).expect("ramp ends at pattern boundary");
        assert_eq!(inspection.parameter_ramp_count, 1);
        assert_eq!(inspection.distinct_parameter_ids, ["macro.motion"]);
    }
}
