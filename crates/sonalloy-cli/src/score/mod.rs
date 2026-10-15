mod lower;
mod parse;
mod resolve;
mod syntax;
mod time;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::output::CliFailure;
use crate::performance::events::{MeterChange, MusicalEvent, PerformanceEvents, TempoChange};
use time::q;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
#[error("{path}: {message}")]
struct Error {
    path: String,
    message: String,
    line: usize,
    column: usize,
    actual: Option<Value>,
}

impl Error {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
            line: 0,
            column: 0,
            actual: None,
        }
    }
    fn at(mut self, path: &str) -> Self {
        if self.path.is_empty() {
            self.path = path.into();
        }
        self
    }
    fn failure(mut self, source: &Source) -> CliFailure {
        if self.line == 0 {
            let mut path = self.path.as_str();
            loop {
                if let Some(&(line, column)) = source.positions.get(path) {
                    self.line = line;
                    self.column = column;
                    break;
                }
                let Some(index) = path.rfind(['.', '[']) else {
                    break;
                };
                path = &path[..index];
            }
        }
        self.actual = self
            .actual
            .or_else(|| source.values.get(&self.path).cloned());
        CliFailure { code: 1, diagnostics: vec![sonalloy_core::Diagnostic::error(sonalloy_core::DiagnosticCode::DefinitionError, &self.message).with_path(&self.path).with_detail(serde_json::json!({"line":self.line,"column":self.column,"actual":self.actual,"expected":self.message}).to_string())] }
    }
}

#[derive(Default)]
struct Source {
    positions: BTreeMap<String, (usize, usize)>,
    values: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Occurrence {
    pub(crate) section: String,
    pub(crate) bars: usize,
    pub(crate) tonic: String,
    pub(crate) scale: String,
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
    pub(crate) bpm: f64,
    pub(crate) meter: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ScoreLane {
    pub(crate) id: String,
    pub(crate) gain_db: f64,
    pub(crate) pattern: PerformanceEvents,
}

#[derive(Debug, Clone)]
pub(crate) struct ScorePlan {
    pub(crate) title: String,
    pub(crate) instruments: serde_json::Map<String, Value>,
    pub(crate) lanes: Vec<ScoreLane>,
    pub(crate) sections: Vec<Occurrence>,
    pub(crate) length_ticks: u64,
    pub(crate) ticks_per_beat: u16,
    pub(crate) master: Option<Value>,
    pub(crate) fade_out_seconds: f64,
}

#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct Bindings {
    /// Root containing the preset category directories.
    #[arg(long, value_name = "DIRECTORY")]
    pub(crate) preset_root: Option<PathBuf>,
    /// Bind an external instrument to its definition; repeat for each instrument.
    #[arg(long = "bind", value_name = "ID=DEFINITION")]
    pub(crate) bind: Vec<String>,
}

pub(crate) fn read(path: &Path) -> std::result::Result<ScorePlan, CliFailure> {
    let text = std::fs::read_to_string(path).map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![
            sonalloy_core::Diagnostic::error(
                sonalloy_core::DiagnosticCode::DefinitionError,
                "could not read score",
            )
            .with_path(path.to_string_lossy())
            .with_detail(error.to_string()),
        ],
    })?;
    compile(&text)
}

pub(crate) fn compile(text: &str) -> std::result::Result<ScorePlan, CliFailure> {
    let mut source = Source::default();
    let result = parse::parse(text, &mut source).and_then(|document| {
        resolve::validate_structure(&document)?;
        lower::lower(&document)
    });
    result.map_err(|error| error.failure(&source))
}

fn object<'a>(v: &'a Value, path: &str) -> Result<&'a serde_json::Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| Error::new(path, "expected a mapping"))
}
fn array<'a>(v: &'a Value, path: &str) -> Result<&'a Vec<Value>> {
    v.as_array()
        .ok_or_else(|| Error::new(path, "expected a sequence"))
}
fn string<'a>(v: &'a Value, path: &str) -> Result<&'a str> {
    v.as_str()
        .ok_or_else(|| Error::new(path, "expected a string"))
}
fn integer(v: &Value, path: &str) -> Result<i64> {
    v.as_i64()
        .ok_or_else(|| Error::new(path, "expected an integer"))
}
fn number(v: &Value, path: &str) -> Result<f64> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| Error::new(path, "expected a finite number"))
}
fn required<'a>(v: &'a Value, key: &str, path: &str) -> Result<&'a Value> {
    v.get(key).ok_or_else(|| {
        Error::new(
            format!("{path}.{key}").trim_start_matches('.'),
            "required field is missing",
        )
    })
}
fn text_field<'a>(v: &'a Value, key: &str, path: &str) -> Result<&'a str> {
    string(required(v, key, path)?, &format!("{path}.{key}"))
}
fn optional_number(v: &Value, key: &str, default: f64, path: &str) -> Result<f64> {
    v.get(key)
        .map_or(Ok(default), |v| number(v, &format!("{path}.{key}")))
}
fn optional_int(v: &Value, key: &str, default: i64, path: &str) -> Result<i64> {
    v.get(key)
        .map_or(Ok(default), |v| integer(v, &format!("{path}.{key}")))
}
fn fields(v: &Value, allowed: &[&str], path: &str) -> Result<()> {
    for key in object(v, path)?.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(Error::new(
                format!("{path}.{key}").trim_start_matches('.'),
                format!("unknown field; expected {}", allowed.join(", ")),
            ));
        }
    }
    Ok(())
}

pub(crate) fn tonic_pitch(text: &str) -> std::result::Result<i32, CliFailure> {
    syntax::tonic(text).map_err(|e| e.failure(&Source::default()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn audition_scores_preserve_the_complete_event_sequences() {
        use crate::performance::events::MusicalEvent;
        use sha2::{Digest, Sha256};
        let snapshots: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../../testdata/scores/audition-events.sha256.json"
        ))
        .expect("event fingerprints");
        assert_eq!(snapshots.len(), 160);
        for snapshot in snapshots {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(snapshot["score"].as_str().expect("score path"));
            let plan = super::read(&path).expect("audition score compiles");
            assert_eq!(plan.lanes.len(), 1, "{}", path.display());
            let events = &plan.lanes[0].pattern;
            let mut ordered = events.events.clone();
            let position = |e: &MusicalEvent| match e {
                MusicalEvent::Note { tick, .. } => (*tick, 2),
                MusicalEvent::SustainPedal { tick, .. }
                | MusicalEvent::PitchBend { tick, .. }
                | MusicalEvent::ModWheel { tick, .. }
                | MusicalEvent::Aftertouch { tick, .. }
                | MusicalEvent::ParameterChange { tick, .. }
                | MusicalEvent::ParameterRamp { tick, .. } => (*tick, 1),
            };
            ordered.sort_by_key(position);
            let mut lines = vec![format!(
                "time|{}|{}|{}|{}|{}",
                events.length_ticks,
                events.ticks_per_beat,
                events.tempo_changes[0].bpm.to_bits(),
                events.time_signature_changes[0].numerator,
                events.time_signature_changes[0].denominator
            )];
            for e in ordered {
                lines.push(match e {
                    MusicalEvent::Note {
                        tick,
                        duration_ticks,
                        note,
                        velocity,
                    } => format!("n|{tick}|{duration_ticks}|{note}|{velocity}"),
                    MusicalEvent::SustainPedal { tick, down } => {
                        format!("s|{tick}|{}", u8::from(down))
                    }
                    MusicalEvent::PitchBend { tick, value } => {
                        format!("b|{tick}|{}", value.to_bits())
                    }
                    MusicalEvent::ModWheel { tick, value } => {
                        format!("w|{tick}|{}", value.to_bits())
                    }
                    MusicalEvent::Aftertouch { tick, value } => {
                        format!("a|{tick}|{}", value.to_bits())
                    }
                    MusicalEvent::ParameterChange {
                        tick,
                        parameter,
                        native_value,
                    } => format!("p|{tick}|{parameter}|{}", native_value.to_bits()),
                    MusicalEvent::ParameterRamp { .. } => panic!("unexpected audition ramp"),
                });
            }
            assert_eq!(
                Sha256::digest(lines.join("\n").as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                snapshot["sha256"].as_str().expect("digest"),
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn reference_composition_expands_all_sections() {
        let text = include_str!("../../../../testdata/scores/dopamine_overload.score.yaml");
        let plan = super::compile(text).expect("reference score compiles");
        assert_eq!(plan.sections.len(), 14);
        assert_eq!(plan.sections.iter().map(|s| s.bars).sum::<usize>(), 70);
        assert_eq!(plan.instruments.len(), 42);
        assert_eq!(plan.length_ticks, u64::from(plan.ticks_per_beat) * 280);
        assert!(
            plan.lanes
                .iter()
                .all(|lane| !lane.pattern.events.is_empty())
        );
    }
}
