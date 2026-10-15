use std::path::Path;

use midly::{Format, Header, Smf, Timing, num::u15};
use sonalloy_core::{Diagnostic, DiagnosticCode};

use crate::midi::pattern::{build_track, conductor_events, midi_events, midi_export_events};
use crate::performance::PerformancePlan;

pub(crate) fn export_score(path: &Path, demo: &PerformancePlan) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    for (index, part) in demo.parts.iter().enumerate() {
        if part.render_lanes.len() > 1 && part.pattern.events.iter().any(|event| !event.is_note()) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "independent controller states on gain lanes cannot share one MIDI channel",
                )
                .with_path(format!("instruments.{}", part.definition.id)),
            );
        }
        if part.midi_channel.is_none() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "no MIDI channel is available for this Score part",
                )
                .with_path(format!("parts[{index}].midi_channel")),
            );
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let conductor_index = 0;
    let conductor_part = &demo.parts[0];
    let conductor_pattern = &conductor_part.pattern;
    let conductor_events = match conductor_events(conductor_pattern) {
        Ok(events) => events,
        Err(diagnostics) => return Err(prefix_pattern_diagnostics(diagnostics, conductor_index)),
    };
    let mut part_events = Vec::with_capacity(demo.parts.len());
    for (index, part) in demo.parts.iter().enumerate() {
        match midi_events(&part.pattern) {
            Ok(events) => part_events.push(midi_export_events(&events)),
            Err(errors) => {
                diagnostics.extend(prefix_pattern_diagnostics(errors, index).into_iter().map(
                    |mut diagnostic| {
                        let context = format!("part {index} ({})", part.definition.id);
                        diagnostic.detail = Some(match diagnostic.detail.take() {
                            Some(detail) => format!("{context}: {detail}"),
                            None => context,
                        });
                        diagnostic
                    },
                ));
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let conductor_name = demo.definition.name.as_deref().map(str::as_bytes);
    let conductor = build_track(conductor_events, 0, demo.length_ticks, conductor_name)?;
    let mut tracks = vec![conductor];
    for (part, events) in demo.parts.iter().zip(part_events) {
        let channel = part
            .midi_channel
            .expect("validated MIDI channel")
            .saturating_sub(1);
        let track = build_track(
            events,
            channel,
            demo.length_ticks,
            Some(part.definition.id.as_bytes()),
        )?;
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
    })
}

fn prefix_pattern_diagnostics(diagnostics: Vec<Diagnostic>, part_index: usize) -> Vec<Diagnostic> {
    let prefix = format!("parts[{part_index}].pattern");
    diagnostics
        .into_iter()
        .map(|mut diagnostic| {
            diagnostic.path = Some(match diagnostic.path.take() {
                Some(path) => format!("{prefix}.{path}"),
                None => prefix.clone(),
            });
            diagnostic
        })
        .collect()
}
