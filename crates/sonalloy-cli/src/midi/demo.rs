use std::path::Path;

use midly::{Format, Header, Smf, Timing, num::u15};
use sonalloy_core::{Diagnostic, DiagnosticCode};

use crate::demo::LoadedDemo;
use crate::midi::pattern::{build_track, midi_events, pattern_export_events};

pub(crate) fn export_demo(path: &Path, demo: &LoadedDemo) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    for (index, part) in demo.parts.iter().enumerate() {
        if part.midi_channel.is_none() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::MidiError,
                    "no MIDI channel is available for this Demo part",
                )
                .with_path(format!("parts[{index}].midi_channel")),
            );
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let conductor_events = match pattern_export_events(&demo.parts[0].pattern, &[]) {
        Ok(events) => events,
        Err(diagnostics) => return Err(prefix_pattern_diagnostics(diagnostics, 0)),
    };
    let mut part_events = Vec::with_capacity(demo.parts.len());
    for (index, part) in demo.parts.iter().enumerate() {
        let events = match midi_events(&part.pattern) {
            Ok(events) => events,
            Err(diagnostics) => return Err(prefix_pattern_diagnostics(diagnostics, index)),
        };
        let events = match pattern_export_events(&part.pattern, &events) {
            Ok(events) => events,
            Err(diagnostics) => return Err(prefix_pattern_diagnostics(diagnostics, index)),
        };
        part_events.push(events);
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
        Timing::Metrical(u15::new(demo.parts[0].pattern.ticks_per_beat)),
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
