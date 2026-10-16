use std::collections::BTreeSet;
use std::path::Path;

use midly::{Format, Header, MetaMessage, Smf, Timing, TrackEvent, TrackEventKind, num::u15};
use sonalloy_core::{Diagnostic, DiagnosticCode};

use crate::midi::pattern::{build_track, conductor_events, midi_events, midi_export_events};
use crate::performance::events::{MusicalEvent, PerformanceEvents};
use crate::performance::{PartLane, PerformancePlan};

pub(crate) fn export_score(
    path: &Path,
    plan: &PerformancePlan,
) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    if plan.parts.len() > 2048 {
        return Err(vec![Diagnostic::error(
            DiagnosticCode::MidiError,
            "MIDI supports at most 128 ports of 16 channels",
        )]);
    }
    let mut diagnostics = Vec::new();
    let conductor_pattern = &plan.parts[0].pattern;
    let conductor = build_track(
        conductor_events(conductor_pattern)?,
        0,
        plan.length_ticks,
        plan.definition.name.as_deref().map(str::as_bytes),
    )?;
    let mut tracks = vec![conductor];
    for (index, part) in plan.parts.iter().enumerate() {
        let pattern = merge_lanes(part, &mut diagnostics);
        let events = midi_events(&pattern).map_err(|mut errors| {
            for error in &mut errors {
                error.path = Some(format!("instruments.{}", part.definition.id));
            }
            errors
        })?;
        let mut track = build_track(
            midi_export_events(&events),
            u8::try_from(index % 16).expect("channel"),
            plan.length_ticks,
            Some(part.definition.id.as_bytes()),
        )?;
        if index >= 16 {
            track.insert(
                0,
                TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Meta(MetaMessage::MidiPort(
                        u8::try_from(index / 16).expect("validated port").into(),
                    )),
                },
            );
        }
        tracks.push(track);
    }
    let mut smf = Smf::new(Header::new(
        Format::Parallel,
        Timing::Metrical(u15::new(conductor_pattern.ticks_per_beat)),
    ));
    smf.tracks = tracks;
    smf.save(path).map_err(|error| {
        vec![
            Diagnostic::error(DiagnosticCode::MidiError, "could not write MIDI output")
                .with_path(path.to_string_lossy())
                .with_detail(error.to_string()),
        ]
    })?;
    Ok(diagnostics)
}

fn warning(part: &PartLane, message: impl Into<String>) -> Diagnostic {
    Diagnostic::warning(DiagnosticCode::MidiError, message)
        .with_path(format!("instruments.{}", part.definition.id))
}

fn control_kind(event: &MusicalEvent) -> Option<usize> {
    match event {
        MusicalEvent::SustainPedal { .. } => Some(0),
        MusicalEvent::PitchBend { .. } => Some(1),
        MusicalEvent::ModWheel { .. } => Some(2),
        MusicalEvent::Aftertouch { .. } => Some(3),
        _ => None,
    }
}

fn event_tick(event: &MusicalEvent) -> u64 {
    match event {
        MusicalEvent::Note { tick, .. }
        | MusicalEvent::SustainPedal { tick, .. }
        | MusicalEvent::PitchBend { tick, .. }
        | MusicalEvent::ModWheel { tick, .. }
        | MusicalEvent::Aftertouch { tick, .. }
        | MusicalEvent::ParameterChange { tick, .. }
        | MusicalEvent::ParameterRamp { tick, .. } => *tick,
    }
}

fn merge_lanes(part: &PartLane, diagnostics: &mut Vec<Diagnostic>) -> PerformanceEvents {
    let mut pattern = part.pattern.clone();
    pattern.events.retain(MusicalEvent::is_note);
    for (kind, label) in ["sustain pedal", "pitch bend", "mod wheel", "aftertouch"]
        .iter()
        .enumerate()
    {
        let sequences: Vec<Vec<_>> = part
            .render_lanes
            .iter()
            .map(|lane| {
                let mut events: Vec<_> = lane
                    .pattern
                    .events
                    .iter()
                    .filter(|e| control_kind(e) == Some(kind))
                    .cloned()
                    .collect();
                events.sort_by_key(event_tick);
                events
            })
            .collect();
        if sequences.windows(2).all(|pair| pair[0] == pair[1]) {
            if let Some(events) = sequences.into_iter().next() {
                pattern.events.extend(events);
            }
        } else {
            diagnostics.push(warning(
                part,
                format!("{label} omitted because gain lanes have different controller sequences"),
            ));
        }
    }
    let mut changes = 0;
    let mut ramps = 0;
    let mut parameters = BTreeSet::new();
    for event in &part.pattern.events {
        match event {
            MusicalEvent::ParameterChange { parameter, .. } => {
                changes += 1;
                parameters.insert(parameter.as_str());
            }
            MusicalEvent::ParameterRamp { parameter, .. } => {
                ramps += 1;
                parameters.insert(parameter.as_str());
            }
            _ => {}
        }
    }
    if changes + ramps > 0 {
        diagnostics.push(warning(part, format!("Sonalloy parameters omitted: {changes} ParameterChange, {ramps} ParameterRamp; IDs: {}", parameters.into_iter().collect::<Vec<_>>().join(", "))));
    }
    let mut notes: Vec<_> = pattern
        .events
        .iter()
        .filter_map(|event| {
            if let MusicalEvent::Note {
                tick,
                duration_ticks,
                note,
                ..
            } = event
            {
                Some((*note, *tick, tick + duration_ticks))
            } else {
                None
            }
        })
        .collect();
    notes.sort_unstable();
    let mut ends = [0; 128];
    let mut overlaps = 0;
    for (note, start, end) in notes {
        let latest = &mut ends[usize::from(note)];
        if start < *latest {
            overlaps += 1;
        }
        *latest = (*latest).max(end);
    }
    if overlaps > 0 {
        diagnostics.push(warning(part, format!("{overlaps} overlapping same-pitch notes exported; MIDI 1.0 note-off pairing depends on the receiver")));
    }
    pattern.events.sort_by_key(event_tick);
    pattern
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::Bindings;
    use serde_json::json;

    #[test]
    fn identical_lane_controls_merge_once_and_conflicts_are_isolated_by_kind() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("score.yaml");
        let instrument = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/instruments/basic-poly-synth.json");
        let doc = json!({"meta":{"title":"Controls","bpm":120,"meter":"4/4","tuning":"12edo"},
            "form":[{"section":"a","bars":1,"tonic":"C"}],
            "instruments":{"part":{"definition":instrument}},
            "sections":{"a":{"placements":[
                {"instrument":"part","degree":"1","at":{"bar":1,"step":1},"duration_quarters":1},
                {"instrument":"part","degree":"3","at":{"bar":1,"step":3},"duration_quarters":1,"gain_db":-6}],
                "events":[{"kind":"sustain_pedal","instrument":"part","at":{"bar":1,"step":1},"down":true},
                    {"kind":"mod_wheel","instrument":"part","at":{"bar":1,"step":2},"value":0.5},
                    {"kind":"part_window","instrument":"part","bars":[1,1]}]}}});
        std::fs::write(&path, doc.to_string()).unwrap();
        let mut plan = crate::performance::load(&path, &Bindings::default(), 48000, 257).unwrap();
        let part = &mut plan.parts[0];
        let controls: Vec<_> = part.render_lanes[0]
            .pattern
            .events
            .iter()
            .filter(|e| control_kind(e).is_some())
            .cloned()
            .collect();
        part.render_lanes[1].pattern.events.extend(controls.clone());
        let mut diagnostics = Vec::new();
        let merged = merge_lanes(part, &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            merged
                .events
                .iter()
                .filter(|e| control_kind(e).is_some())
                .count(),
            controls.len()
        );
        assert_eq!(merged.events.iter().filter(|e| e.is_note()).count(), 2);
        assert!(controls.iter().any(|e| matches!(
            e,
            MusicalEvent::SustainPedal {
                tick: 1920,
                down: false
            }
        )));

        part.pattern.events.extend([
            MusicalEvent::ParameterChange {
                tick: 0,
                parameter: "layer.body.gain".into(),
                native_value: -12.0,
            },
            MusicalEvent::ParameterRamp {
                tick: 240,
                duration_ticks: 120,
                parameter: "layer.body.gain".into(),
                from_value: -12.0,
                to_value: -14.0,
            },
        ]);
        let merged = merge_lanes(part, &mut diagnostics);
        assert_eq!(midi_events(&merged).unwrap().len(), 4 + controls.len());
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0]
                .message
                .contains("1 ParameterChange, 1 ParameterRamp; IDs: layer.body.gain")
        );
        part.pattern.events.retain(|e| {
            matches!(
                e,
                MusicalEvent::Note { .. }
                    | MusicalEvent::SustainPedal { .. }
                    | MusicalEvent::ModWheel { .. }
            )
        });
        diagnostics.clear();

        part.render_lanes[1]
            .pattern
            .events
            .retain(|e| control_kind(e) != Some(1));
        part.render_lanes[1]
            .pattern
            .events
            .push(MusicalEvent::PitchBend {
                tick: 120,
                value: 0.5,
            });
        let merged = merge_lanes(part, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("pitch bend omitted"));
        assert_eq!(
            merged
                .events
                .iter()
                .filter(|e| control_kind(e).is_some())
                .count(),
            controls.len()
        );
    }
}
