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
        }
    }

    fn diagnostics(metadata: &InstrumentMetadata) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        validate_metadata(&mut diagnostics, metadata);
        diagnostics
    }

    #[test]
    fn category_and_tags_reject_empty_outer_whitespace_and_duplicates() {
        let mut empty_category = metadata();
        empty_category.category = Some(String::new());
        assert_ne!(diagnostics(&empty_category), Vec::new());

        let mut padded_category = metadata();
        padded_category.category = Some(" Bass".to_owned());
        assert_ne!(diagnostics(&padded_category), Vec::new());

        let mut too_many_tags = metadata();
        too_many_tags.tags = (0..13).map(|index| format!("tag{index}")).collect();
        assert_ne!(diagnostics(&too_many_tags), Vec::new());

        let mut empty_tag = metadata();
        empty_tag.tags = vec![String::new()];
        assert_ne!(diagnostics(&empty_tag), Vec::new());

        let mut case_insensitive_duplicate = metadata();
        case_insensitive_duplicate.tags = vec!["Warm".to_owned(), "warm".to_owned()];
        assert_ne!(diagnostics(&case_insensitive_duplicate), Vec::new());
    }

    #[test]
    fn recommended_range_rejects_reversed_and_out_of_midi_values() {
        let mut reversed = metadata();
        reversed.recommended_range = Some(InstrumentRecommendedRange {
            min_midi: 60,
            max_midi: 24,
        });
        assert_ne!(diagnostics(&reversed), Vec::new());

        let mut out_of_range = metadata();
        out_of_range.recommended_range = Some(InstrumentRecommendedRange {
            min_midi: 128,
            max_midi: 127,
        });
        assert_ne!(diagnostics(&out_of_range), Vec::new());
    }
}
