mod lower;
mod parse;
mod resolve;
mod syntax;
mod time;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use num_traits::ToPrimitive;
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
    actual: Option<Box<Value>>,
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
            .or_else(|| source.values.get(&self.path).cloned().map(Box::new));
        let mut context = self.path.as_str();
        let mut bar = None;
        let mut step = None;
        loop {
            if let Some(value) = source.values.get(&format!("{context}.at.bar")) {
                bar = Some(value.clone());
                step = source.values.get(&format!("{context}.at.step")).cloned();
                break;
            }
            let Some(index) = context.rfind(['.', '[']) else {
                break;
            };
            context = &context[..index];
        }
        CliFailure { code: 1, diagnostics: vec![sonalloy_core::Diagnostic::error(sonalloy_core::DiagnosticCode::DefinitionError, &self.message).with_path(&self.path).with_detail(serde_json::json!({"source_type":"score","bar":bar,"step":step,"line":self.line,"column":self.column,"actual":self.actual,"expected":self.message,"section":self.path.strip_prefix("sections.").and_then(|s|s.split('.').next())}).to_string())] }
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
    pub(crate) chords: Vec<ChordChange>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChordChange {
    pub(crate) tick: u64,
    pub(crate) chord: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ScoreLane {
    pub(crate) id: String,
    pub(crate) gain_db: f64,
    pub(crate) pattern: PerformanceEvents,
    pub(crate) sources: Vec<EventSource>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EventSource {
    pub(crate) path: String,
    pub(crate) spelling: Option<String>,
    pub(crate) tick: u64,
    pub(crate) at_quarters: String,
    pub(crate) duration_quarters: String,
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) section: String,
    pub(crate) bar: usize,
    pub(crate) step: String,
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
    pub(crate) timeline: PerformanceEvents,
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
    result
        .map(|mut plan| {
            for lane in &mut plan.lanes {
                for origin in &mut lane.sources {
                    if let Some(section) = plan
                        .sections
                        .iter()
                        .find(|section| {
                            origin.tick >= section.start_tick && origin.tick < section.end_tick
                        })
                        .or_else(|| {
                            plan.sections
                                .last()
                                .filter(|section| origin.tick == section.end_tick)
                        })
                    {
                        let (numerator, denominator) =
                            syntax::meter(&section.meter).expect("resolved meter");
                        let elapsed = q(
                            i64::try_from(origin.tick - section.start_tick)
                                .expect("resolved musical time"),
                            i64::from(plan.ticks_per_beat),
                        );
                        let bar_length = q(i64::from(numerator) * 4, i64::from(denominator));
                        let index = (&elapsed / &bar_length)
                            .to_integer()
                            .to_usize()
                            .expect("resolved bar")
                            .min(section.bars - 1);
                        origin.section = section.section.clone();
                        origin.bar = index + 1;
                        origin.step = ((elapsed
                            - bar_length * q(i64::try_from(index).expect("resolved bar"), 1))
                            * q(4, 1)
                            + q(1, 1))
                        .to_string();
                    }
                    let mut path = origin.path.as_str();
                    loop {
                        if let Some(&(line, column)) = source.positions.get(path) {
                            origin.line = line;
                            origin.column = column;
                            break;
                        }
                        let Some(i) = path.rfind(['.', '[']) else {
                            break;
                        };
                        path = &path[..i];
                    }
                }
            }
            plan
        })
        .map_err(|error| error.failure(&source))
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
    fn document() -> serde_json::Value {
        serde_json::json!({"meta":{"title":"Timing","bpm":120,"meter":"4/4","tuning":"12edo","reference_octave":4,"gate":1},"form":[{"section":"phrase","bars":1,"tonic":"C","scale":"ionian"}],"sections":{"phrase":{"lead":{"instrument":"part","melody":"phrase"}}},"melodies":{"phrase":["1 - - - | 3 - - - | 5 - - - | 1' - - -"]},"instruments":{"part":{"external":true}}})
    }

    #[test]
    fn exact_placements_preserve_enharmonic_spelling_and_ignore_gate() {
        let mut doc = document();
        doc["meta"]["gate"] = serde_json::json!(0.5);
        doc["form"][0]["tonic"] = serde_json::json!("A");
        doc["form"][0]["scale"] = serde_json::json!("aeolian");
        doc["sections"]["phrase"] = serde_json::json!({"placements":[{"instrument":"part","degree":"#1","at":{"bar":1,"offset_quarters":"1/7"},"duration_quarters":"1/3"},{"instrument":"part","degree":"b2","at":{"bar":1,"offset_quarters":"1/3"},"duration_quarters":"1/3"}]});
        let plan = super::compile(&doc.to_string()).expect("exact placements");
        assert_eq!(plan.ticks_per_beat, 3360);
        let events = &plan.lanes[0].pattern.events;
        assert!(matches!(
            events[0],
            super::MusicalEvent::Note {
                tick: 480,
                duration_ticks: 1120,
                note: 70,
                ..
            }
        ));
        assert!(matches!(
            events[1],
            super::MusicalEvent::Note {
                tick: 1120,
                duration_ticks: 1120,
                note: 70,
                ..
            }
        ));
        assert_eq!(
            plan.lanes[0]
                .sources
                .iter()
                .map(|s| s.spelling.as_deref())
                .collect::<Vec<_>>(),
            [Some("#1"), Some("b2")]
        );
        doc["sections"]["phrase"]["placements"][0]["at"]["offset_quarters"] =
            serde_json::json!("1/32767");
        assert!(
            super::compile(&doc.to_string()).is_err(),
            "unrepresentable PPQ must not quantize"
        );
    }

    #[test]
    fn meter_tuplets_ties_and_partial_bars_share_one_exact_timeline() {
        let mut doc = document();
        doc["meta"]["meter"] = serde_json::json!("6/8");
        doc["meta"]["gate"] = serde_json::json!(0.5);
        doc["melodies"]["phrase"] = serde_json::json!(["[1 2 b3] | - - | . . | 4 - | 5 - | 1' -"]);
        doc["time_changes"] = serde_json::json!([{"at_quarters":"1/3","bpm":90}]);
        let plan = super::compile(&doc.to_string()).expect("6/8 tuplets");
        let events = &plan.lanes[0].pattern.events;
        assert_eq!(plan.length_ticks, 1440);
        assert_eq!(events.len(), 6);
        assert!(matches!(
            events[2],
            super::MusicalEvent::Note {
                tick: 160,
                duration_ticks: 160,
                note: 63,
                ..
            }
        ));
        assert_eq!(plan.timeline.tempo_changes[1].tick, 160);
        doc["meta"]["meter"] = serde_json::json!("3/4");
        doc["melodies"]["phrase"] = serde_json::json!(["1 - - - | - - - - | . . . ."]);
        let plan = super::compile(&doc.to_string()).expect("3/4 tie");
        assert!(matches!(
            plan.lanes[0].pattern.events[0],
            super::MusicalEvent::Note {
                duration_ticks: 480,
                ..
            }
        ));
        doc["form"][0]["last_bar_quarters"] = serde_json::json!("2");
        doc["melodies"]["phrase"] = serde_json::json!(["1 - - - | - - - -"]);
        assert_eq!(
            super::compile(&doc.to_string())
                .expect("partial bar")
                .length_ticks,
            960
        );
    }

    #[test]
    fn invalid_scores_identify_the_source_instead_of_guessing() {
        let good = document();
        let mut unknown = good.clone();
        unknown["meta"]["key"] = serde_json::json!("C");
        let mut cycle = good.clone();
        cycle["sections"]["unused"] = serde_json::json!({"from":"unused"});
        let mut conflict = good.clone();
        conflict["sections"]["phrase"]["events"] = serde_json::json!([{"kind":"mod_wheel","instrument":"part","value":0,"at":{"bar":1,"step":1}},{"kind":"mod_wheel","instrument":"part","value":1,"at":{"bar":1,"step":1}}]);
        let mut malformed = good.clone();
        malformed["sections"]["phrase"]["events"] = serde_json::json!([{"kind":"hit","instrument":"part","down":true,"at":{"bar":1,"step":1}}]);
        let cases = [
            (unknown.to_string(), "meta.key"),
            (cycle.to_string(), "sections.unused"),
            (conflict.to_string(), "sections.phrase.events[1]"),
            (malformed.to_string(), "sections.phrase.events[0].down"),
            ("meta: {title: A, title: B}".into(), "meta.title"),
            ("meta: &a {title: A}\nform: *a".into(), "meta"),
        ];
        for (text, path) in cases {
            let error = super::compile(&text).expect_err("invalid score");
            assert_eq!(error.diagnostics[0].path.as_deref(), Some(path));
            let detail: serde_json::Value = serde_json::from_str(
                error.diagnostics[0]
                    .detail
                    .as_deref()
                    .expect("source detail"),
            )
            .expect("structured source");
            assert!(detail["line"].as_u64().expect("line") > 0);
            assert!(detail["column"].as_u64().expect("column") > 0);
        }
    }

    #[test]
    fn audition_scores_preserve_the_complete_event_sequences() {
        use crate::performance::events::MusicalEvent;
        use sha2::{Digest, Sha256};
        use std::fmt::Write;
        let snapshots: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../../testdata/expected/score-auditions.sha256.json"
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
                Sha256::digest(lines.join("\n").as_bytes()).iter().fold(
                    String::new(),
                    |mut text, byte| {
                        write!(&mut text, "{byte:02x}").expect("string write");
                        text
                    }
                ),
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
