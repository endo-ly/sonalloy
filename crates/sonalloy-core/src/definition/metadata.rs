use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode};

/// Human-readable instrument information and optional library metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentMetadata {
    /// Instrument name.
    pub name: String,
    /// Optional author name.
    #[serde(default)]
    pub author: Option<String>,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional primary library category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Optional library search tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Optional practical MIDI range for the instrument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_range: Option<InstrumentRecommendedRange>,
    /// Optional short browser preview definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<InstrumentPreviewDefinition>,
}

/// Practical MIDI range recommended for one instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentRecommendedRange {
    /// Lowest recommended MIDI note.
    pub min_midi: u8,
    /// Highest recommended MIDI note.
    pub max_midi: u8,
}

/// Note-only performance used for a browser preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentPreviewDefinition {
    /// Preview tempo in beats per minute.
    pub tempo_bpm: f64,
    /// Number of ticks in one quarter-note beat.
    pub ticks_per_beat: u16,
    /// Preview time signature.
    pub time_signature: InstrumentPreviewTimeSignature,
    /// Preview length in ticks.
    pub length_ticks: u64,
    /// Note events in non-decreasing tick order.
    pub notes: Vec<InstrumentPreviewNote>,
}

/// Time signature used by a browser preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentPreviewTimeSignature {
    /// Beats in one bar.
    pub numerator: u8,
    /// Note value that represents one beat.
    pub denominator: u8,
}

/// One note event in a browser preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentPreviewNote {
    /// Start position in ticks.
    pub tick: u64,
    /// Note duration in ticks.
    pub duration_ticks: u64,
    /// MIDI note number.
    pub note: u8,
    /// MIDI velocity.
    pub velocity: u8,
}

pub(crate) fn validate_metadata(diagnostics: &mut Vec<Diagnostic>, metadata: &InstrumentMetadata) {
    if let Some(category) = &metadata.category {
        validate_text(diagnostics, "metadata.category", category, 64, "category");
    }

    if metadata.tags.len() > 12 {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "tags must contain between 0 and 12 items",
            )
            .with_path("metadata.tags"),
        );
    }
    let mut normalized_tags = HashSet::new();
    for (index, tag) in metadata.tags.iter().enumerate() {
        let path = format!("metadata.tags[{index}]");
        validate_text(diagnostics, &path, tag, 32, "tag");
        if !normalized_tags.insert(tag.to_ascii_lowercase()) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "tags must not contain ASCII case-insensitive duplicates",
                )
                .with_path(path),
            );
        }
    }

    if let Some(range) = metadata.recommended_range {
        if range.min_midi > 127 || range.max_midi > 127 || range.min_midi > range.max_midi {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "recommended_range must contain MIDI notes from 0 to 127 with min_midi at most max_midi",
                )
                .with_path("metadata.recommended_range"),
            );
        }
    }

    if let Some(preview) = &metadata.preview {
        validate_preview(diagnostics, preview, metadata.recommended_range);
    }
}

fn validate_text(
    diagnostics: &mut Vec<Diagnostic>,
    path: &str,
    value: &str,
    max_chars: usize,
    field: &str,
) {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max_chars {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                format!("{field} must contain between 1 and {max_chars} characters after trimming"),
            )
            .with_path(path),
        );
    }
    if trimmed != value {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                format!("{field} must not have leading or trailing whitespace"),
            )
            .with_path(path),
        );
    }
    if value.chars().any(char::is_control) {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                format!("{field} must not contain control characters"),
            )
            .with_path(path),
        );
    }
}

fn validate_preview(
    diagnostics: &mut Vec<Diagnostic>,
    preview: &InstrumentPreviewDefinition,
    recommended_range: Option<InstrumentRecommendedRange>,
) {
    validate_preview_header(diagnostics, preview);
    validate_preview_notes(diagnostics, preview, recommended_range);
    validate_preview_duration(diagnostics, preview);
}

fn validate_preview_header(
    diagnostics: &mut Vec<Diagnostic>,
    preview: &InstrumentPreviewDefinition,
) {
    if !preview.tempo_bpm.is_finite() || !(30.0..=300.0).contains(&preview.tempo_bpm) {
        push_value_error(
            diagnostics,
            "metadata.preview.tempo_bpm",
            "preview.tempo_bpm must be finite and between 30 and 300 BPM",
        );
    }
    if !(1..=32_767).contains(&preview.ticks_per_beat) {
        push_value_error(
            diagnostics,
            "metadata.preview.ticks_per_beat",
            "preview.ticks_per_beat must be between 1 and 32767",
        );
    }
    let signature = preview.time_signature;
    if !(1..=32).contains(&signature.numerator) {
        push_value_error(
            diagnostics,
            "metadata.preview.time_signature.numerator",
            "preview.time_signature.numerator must be between 1 and 32",
        );
    }
    if signature.denominator == 0
        || signature.denominator > 128
        || !signature.denominator.is_power_of_two()
    {
        push_value_error(
            diagnostics,
            "metadata.preview.time_signature.denominator",
            "preview.time_signature.denominator must be a power of two between 1 and 128",
        );
    }
    if preview.length_ticks == 0 {
        push_value_error(
            diagnostics,
            "metadata.preview.length_ticks",
            "preview.length_ticks must be greater than zero",
        );
    }
    if !(1..=32).contains(&preview.notes.len()) {
        push_value_error(
            diagnostics,
            "metadata.preview.notes",
            "preview.notes must contain between 1 and 32 notes",
        );
    }
}

fn validate_preview_notes(
    diagnostics: &mut Vec<Diagnostic>,
    preview: &InstrumentPreviewDefinition,
    recommended_range: Option<InstrumentRecommendedRange>,
) {
    let mut previous_tick = None;
    for (index, note) in preview.notes.iter().enumerate() {
        let path = format!("metadata.preview.notes[{index}]");
        if note.note > 127 {
            push_value_error(
                diagnostics,
                format!("{path}.note"),
                "preview note must be between MIDI 0 and 127",
            );
        }
        if !(1..=127).contains(&note.velocity) {
            push_value_error(
                diagnostics,
                format!("{path}.velocity"),
                "preview velocity must be between 1 and 127",
            );
        }
        if note.tick >= preview.length_ticks {
            push_value_error(
                diagnostics,
                format!("{path}.tick"),
                "preview note tick must be before length_ticks",
            );
        }
        if note.duration_ticks == 0 {
            push_value_error(
                diagnostics,
                format!("{path}.duration_ticks"),
                "preview note duration_ticks must be greater than zero",
            );
        }
        if note
            .tick
            .checked_add(note.duration_ticks)
            .is_none_or(|end| end > preview.length_ticks)
        {
            push_value_error(
                diagnostics,
                path.clone(),
                "preview note must end at or before length_ticks",
            );
        }
        if previous_tick.is_some_and(|previous| note.tick < previous) {
            push_value_error(
                diagnostics,
                format!("{path}.tick"),
                "preview notes must be ordered by non-decreasing tick",
            );
        }
        previous_tick = Some(note.tick);
        if let Some(range) = recommended_range
            && (note.note < range.min_midi || note.note > range.max_midi)
        {
            push_value_error(
                diagnostics,
                format!("{path}.note"),
                "preview note must be inside recommended_range",
            );
        }
    }
}

fn validate_preview_duration(
    diagnostics: &mut Vec<Diagnostic>,
    preview: &InstrumentPreviewDefinition,
) {
    if preview.ticks_per_beat > 0 && preview.tempo_bpm.is_finite() && preview.tempo_bpm > 0.0 {
        #[allow(clippy::cast_precision_loss)]
        let seconds = preview.length_ticks as f64 / f64::from(preview.ticks_per_beat) * 60.0
            / preview.tempo_bpm;
        if !seconds.is_finite() || seconds > 10.0 {
            push_value_error(
                diagnostics,
                "metadata.preview.length_ticks",
                "preview music time must be at most 10 seconds",
            );
        }
    }
}

fn push_value_error(
    diagnostics: &mut Vec<Diagnostic>,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    diagnostics.push(Diagnostic::error(DiagnosticCode::ValueOutOfRange, message).with_path(path));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> InstrumentMetadata {
        InstrumentMetadata {
            name: "Preview Test".to_owned(),
            author: Some("Sonalloy".to_owned()),
            description: Some("Metadata validation fixture".to_owned()),
            category: Some("Bass".to_owned()),
            tags: vec!["Sub".to_owned(), "Clean".to_owned(), "Warm".to_owned()],
            recommended_range: Some(InstrumentRecommendedRange {
                min_midi: 24,
                max_midi: 60,
            }),
            preview: Some(InstrumentPreviewDefinition {
                tempo_bpm: 120.0,
                ticks_per_beat: 480,
                time_signature: InstrumentPreviewTimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                length_ticks: 1920,
                notes: vec![
                    InstrumentPreviewNote {
                        tick: 0,
                        duration_ticks: 360,
                        note: 36,
                        velocity: 92,
                    },
                    InstrumentPreviewNote {
                        tick: 480,
                        duration_ticks: 360,
                        note: 43,
                        velocity: 108,
                    },
                    InstrumentPreviewNote {
                        tick: 960,
                        duration_ticks: 480,
                        note: 48,
                        velocity: 100,
                    },
                ],
            }),
        }
    }

    fn diagnostics(metadata: &InstrumentMetadata) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        validate_metadata(&mut diagnostics, metadata);
        diagnostics
    }

    #[test]
    fn valid_schema_v6_metadata_has_no_diagnostics() {
        assert!(diagnostics(&metadata()).is_empty());
    }

    #[test]
    fn category_rejects_empty_and_outer_whitespace() {
        let mut empty = metadata();
        empty.category = Some(String::new());
        assert!(!diagnostics(&empty).is_empty());

        let mut padded = metadata();
        padded.category = Some(" Bass".to_owned());
        assert!(!diagnostics(&padded).is_empty());
    }

    #[test]
    fn tags_reject_too_many_empty_and_case_insensitive_duplicates() {
        let mut too_many = metadata();
        too_many.tags = (0..13).map(|index| format!("tag{index}")).collect();
        assert!(!diagnostics(&too_many).is_empty());

        let mut empty = metadata();
        empty.tags = vec![String::new()];
        assert!(!diagnostics(&empty).is_empty());

        let mut duplicate = metadata();
        duplicate.tags = vec!["Warm".to_owned(), "warm".to_owned()];
        assert!(!diagnostics(&duplicate).is_empty());
    }

    #[test]
    fn recommended_range_rejects_reversed_and_out_of_midi_values() {
        let mut reversed = metadata();
        reversed.recommended_range = Some(InstrumentRecommendedRange {
            min_midi: 60,
            max_midi: 24,
        });
        assert!(!diagnostics(&reversed).is_empty());

        let mut out_of_range = metadata();
        out_of_range.recommended_range = Some(InstrumentRecommendedRange {
            min_midi: 128,
            max_midi: 127,
        });
        assert!(!diagnostics(&out_of_range).is_empty());
    }

    #[test]
    fn preview_rejects_tempo_and_time_signature_values() {
        for tempo in [29.9, 300.1, f64::NAN] {
            let mut value = metadata();
            value.preview.as_mut().expect("preview").tempo_bpm = tempo;
            assert!(!diagnostics(&value).is_empty());
        }

        for signature in [
            InstrumentPreviewTimeSignature {
                numerator: 0,
                denominator: 4,
            },
            InstrumentPreviewTimeSignature {
                numerator: 4,
                denominator: 3,
            },
        ] {
            let mut value = metadata();
            value.preview.as_mut().expect("preview").time_signature = signature;
            assert!(!diagnostics(&value).is_empty());
        }
    }

    #[test]
    fn preview_rejects_note_count_velocity_timing_and_range_values() {
        let mut no_notes = metadata();
        no_notes.preview.as_mut().expect("preview").notes.clear();
        assert!(!diagnostics(&no_notes).is_empty());

        let mut too_many = metadata();
        too_many.preview.as_mut().expect("preview").notes = (0..33)
            .map(|index| InstrumentPreviewNote {
                tick: index,
                duration_ticks: 1,
                note: 36,
                velocity: 100,
            })
            .collect();
        assert!(!diagnostics(&too_many).is_empty());

        for velocity in [0, 128] {
            let mut value = metadata();
            value.preview.as_mut().expect("preview").notes[0].velocity = velocity;
            assert!(!diagnostics(&value).is_empty());
        }

        let mut tick_outside = metadata();
        tick_outside.preview.as_mut().expect("preview").notes[0].tick = 1920;
        assert!(!diagnostics(&tick_outside).is_empty());

        let mut zero_duration = metadata();
        zero_duration.preview.as_mut().expect("preview").notes[0].duration_ticks = 0;
        assert!(!diagnostics(&zero_duration).is_empty());

        let mut end_outside = metadata();
        end_outside.preview.as_mut().expect("preview").notes[0].duration_ticks = 1921;
        assert!(!diagnostics(&end_outside).is_empty());

        let mut note_outside = metadata();
        note_outside.preview.as_mut().expect("preview").notes[0].note = 61;
        assert!(!diagnostics(&note_outside).is_empty());
    }

    #[test]
    fn preview_rejects_music_longer_than_ten_seconds() {
        let mut value = metadata();
        value.preview.as_mut().expect("preview").length_ticks = 9_601;
        assert!(!diagnostics(&value).is_empty());
    }
}
