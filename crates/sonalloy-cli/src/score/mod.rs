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

#[derive(Debug, Default)]
struct ErrorContext {
    token_location: Option<(usize, String)>,
    score_position: Option<time::Quarter>,
    section: Option<String>,
    occurrence: Option<usize>,
    bar: Option<usize>,
    step: Option<String>,
    source_bar: Option<usize>,
}

#[derive(Debug, thiserror::Error)]
#[error("{path}: {message}")]
struct Error {
    path: String,
    message: String,
    line: usize,
    column: usize,
    actual: Option<Box<Value>>,
    context: Option<Box<ErrorContext>>,
}

impl Error {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
            line: 0,
            column: 0,
            actual: None,
            context: None,
        }
    }
    fn at(mut self, path: &str) -> Self {
        if self.path.is_empty() {
            self.path = path.into();
        }
        self
    }
    fn with_token_location(mut self, path: &str, offset: usize, token: &str) -> Self {
        self = self.at(path);
        self.context.get_or_insert_with(Box::default).token_location =
            Some((offset, token.to_owned()));
        self
    }
    fn with_score_position(mut self, position: time::Quarter, source_bar: Option<usize>) -> Self {
        let context = self.context.get_or_insert_with(Box::default);
        context.score_position = Some(position);
        context.source_bar = source_bar;
        self
    }
    fn with_score_context(
        mut self,
        section: &str,
        occurrence: usize,
        bar: usize,
        step: String,
    ) -> Self {
        let context = self.context.get_or_insert_with(Box::default);
        context.section = Some(section.to_owned());
        context.occurrence = Some(occurrence);
        context.bar = Some(bar);
        context.step = Some(step);
        context.score_position = None;
        self
    }
    fn take_score_position(&mut self) -> Option<time::Quarter> {
        self.context
            .as_mut()
            .and_then(|context| context.score_position.take())
    }
    fn failure(mut self, source: &Source) -> CliFailure {
        if let Some((offset, token)) = self
            .context
            .as_deref()
            .and_then(|context| context.token_location.as_ref())
            && let Some((line, column)) = source.token_location(&self.path, *offset, token)
        {
            self.line = line;
            self.column = column;
        }
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
        let token = self
            .context
            .as_deref()
            .and_then(|context| context.token_location.as_ref())
            .map(|(_, token)| Box::new(Value::String(token.clone())));
        self.actual = self
            .actual
            .or(token)
            .or_else(|| source.values.get(&self.path).cloned().map(Box::new));
        let mut path_context = self.path.as_str();
        let mut bar = self
            .context
            .as_deref()
            .and_then(|context| context.bar)
            .map(Value::from);
        let mut step = self
            .context
            .as_deref()
            .and_then(|context| context.step.clone())
            .map(Value::from);
        loop {
            if bar.is_some() {
                break;
            }
            if let Some(value) = source.values.get(&format!("{path_context}.at.bar")) {
                bar = Some(value.clone());
                step = source
                    .values
                    .get(&format!("{path_context}.at.step"))
                    .cloned();
                break;
            }
            let Some(index) = path_context.rfind(['.', '[']) else {
                break;
            };
            path_context = &path_context[..index];
        }
        let section = self
            .context
            .as_deref()
            .and_then(|context| context.section.clone())
            .or_else(|| {
                self.path
                    .strip_prefix("sections.")
                    .and_then(|s| s.split('.').next())
                    .map(str::to_owned)
            });
        let source_bar = self
            .context
            .as_deref()
            .and_then(|context| context.source_bar);
        let occurrence = self
            .context
            .as_deref()
            .and_then(|context| context.occurrence);
        CliFailure { code: 1, diagnostics: vec![sonalloy_core::Diagnostic::error(sonalloy_core::DiagnosticCode::DefinitionError, &self.message).with_path(&self.path).with_detail(serde_json::json!({"source_type":"score","source_bar":source_bar,"bar":bar,"step":step,"line":self.line,"column":self.column,"actual":self.actual,"expected":self.message,"section":section,"occurrence":occurrence}).to_string())] }
    }
}

#[derive(Debug, Clone, Default)]
struct Source {
    text: String,
    positions: BTreeMap<String, (usize, usize)>,
    indices: BTreeMap<String, usize>,
    values: BTreeMap<String, Value>,
}

impl Source {
    fn token_location(&self, path: &str, offset: usize, token: &str) -> Option<(usize, usize)> {
        let value = self.values.get(path)?.as_str()?;
        let start = *self.indices.get(path)?;
        let end = self
            .indices
            .values()
            .copied()
            .filter(|index| *index > start)
            .min()
            .unwrap_or(self.text.len());
        let prefix = value.get(..offset)?;
        let occurrence = prefix.match_indices(token).count();
        let raw = self.text.get(start..end)?;
        let raw_offset = raw.match_indices(token).nth(occurrence)?.0;
        let source_prefix = self.text.get(..start + raw_offset)?;
        let line = source_prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = source_prefix
            .rsplit('\n')
            .next()?
            .trim_end_matches('\r')
            .chars()
            .count()
            + 1;
        Some((line, column))
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Occurrence {
    pub(crate) section: String,
    pub(crate) bars: usize,
    pub(crate) tonic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) scale: Option<String>,
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
    pub(crate) bpm: f64,
    pub(crate) meter: String,
    pub(crate) chords: Vec<ChordChange>,
    pub(crate) bar_start_ticks: Vec<u64>,
    pub(crate) bar_end_ticks: Vec<u64>,
    pub(crate) part_gains: BTreeMap<String, f64>,
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
    #[serde(skip_serializing)]
    pub(crate) token_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_bar: Option<usize>,
    pub(crate) tick: u64,
    pub(crate) at_quarters: String,
    pub(crate) duration_quarters: String,
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) section: String,
    pub(crate) bar: usize,
    pub(crate) step: String,
    pub(crate) occurrence: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ScorePlan {
    source: Source,
    pub(crate) title: String,
    pub(crate) instruments: serde_json::Map<String, Value>,
    pub(crate) lanes: Vec<ScoreLane>,
    pub(crate) sections: Vec<Occurrence>,
    pub(crate) length_ticks: u64,
    pub(crate) ticks_per_beat: u16,
    pub(crate) master: Option<Value>,
    pub(crate) fade_out_seconds: f64,
    pub(crate) timeline: PerformanceEvents,
    pub(crate) part_windows: Vec<PartWindow>,
}

#[derive(Debug, Clone)]
pub(crate) struct PartWindow {
    pub(crate) instrument: String,
    pub(crate) section_start_tick: u64,
    pub(crate) section_end_tick: u64,
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
}

impl ScorePlan {
    pub(crate) fn annotate_diagnostics(&self, diagnostics: &mut [sonalloy_core::Diagnostic]) {
        for diagnostic in diagnostics {
            if diagnostic.detail.is_none()
                && let Some(path) = diagnostic.path.as_deref()
                && (path.starts_with("instruments.") || path.starts_with("meta."))
            {
                diagnostic.detail = Error::new(path, &diagnostic.message)
                    .failure(&self.source)
                    .diagnostics
                    .remove(0)
                    .detail;
            }
        }
    }
}

#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct Bindings {
    /// Preset root; takes precedence over `SONALLOY_PRESET_ROOT` and Score-relative presets/.
    #[arg(long, value_name = "DIRECTORY")]
    pub(crate) preset_root: Option<PathBuf>,
    /// Bind an `external: true` instrument as ID=DEFINITION; repeat for each. Fixed instruments,
    /// unknown IDs, and duplicate bindings are rejected.
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
        .map_err(|error| error.failure(&source))
        .map(|mut plan| {
            for lane in &mut plan.lanes {
                for origin in &mut lane.sources {
                    if let Some((section_index, section)) = plan
                        .sections
                        .iter()
                        .enumerate()
                        .find(|(_, section)| {
                            origin.tick >= section.start_tick && origin.tick < section.end_tick
                        })
                        .or_else(|| {
                            plan.sections
                                .iter()
                                .enumerate()
                                .next_back()
                                .filter(|(_, section)| origin.tick == section.end_tick)
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
                        origin.occurrence = section_index + 1;
                        origin.bar = index + 1;
                        origin.step = ((elapsed
                            - bar_length * q(i64::try_from(index).expect("resolved bar"), 1))
                            * q(4, 1)
                            + q(1, 1))
                        .to_string();
                    }
                    let token_location = origin
                        .token_offset
                        .zip(origin.spelling.as_deref())
                        .and_then(|(offset, token)| {
                            source.token_location(&origin.path, offset, token)
                        });
                    if let Some((line, column)) = token_location {
                        origin.line = line;
                        origin.column = column;
                        continue;
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
            plan.source = source;
            plan
        })
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

pub(crate) fn to_yaml(
    document: &Value,
) -> std::result::Result<String, yaml_rust2::emitter::EmitError> {
    fn node(value: &Value) -> yaml_rust2::Yaml {
        use yaml_rust2::Yaml;
        match value {
            Value::Null => Yaml::Null,
            Value::Bool(value) => Yaml::Boolean(*value),
            Value::String(value) => Yaml::String(value.clone()),
            Value::Number(value) => value
                .as_i64()
                .map_or_else(|| Yaml::Real(value.to_string()), Yaml::Integer),
            Value::Array(values) => Yaml::Array(values.iter().map(node).collect()),
            Value::Object(values) => Yaml::Hash(
                values
                    .iter()
                    .map(|(key, value)| (Yaml::String(key.clone()), node(value)))
                    .collect(),
            ),
        }
    }
    let mut output = String::new();
    yaml_rust2::YamlEmitter::new(&mut output).dump(&node(document))?;
    output.push('\n');
    Ok(output)
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
    fn melody_diagnostics_follow_from_replace_and_use_source_tokens() {
        let diagnostic = |doc: &serde_json::Value, melody: &str, source_path: &str| {
            let mut doc = doc.clone();
            doc["meta"]["title"] = serde_json::json!("青い夜");
            doc["sections"]["phrase"]["lead"] = serde_json::json!({
                "instrument": "part",
                "melody": melody
            });
            let yaml = super::to_yaml(&doc).expect("Score YAML");
            let text = format!("# 日本語のコメント\n{yaml}");
            let expected_token = text.find(". - - -").expect("quoted tie token") + 2;
            let error = super::compile(&text).expect_err("unmatched tie fails");
            let diagnostic = &error.diagnostics[0];
            assert_eq!(diagnostic.path.as_deref(), Some(source_path));
            let detail: serde_json::Value =
                serde_json::from_str(diagnostic.detail.as_deref().expect("source detail"))
                    .expect("structured source detail");
            let before = &text[..expected_token];
            assert_eq!(
                detail["line"],
                before.bytes().filter(|byte| *byte == b'\n').count() + 1
            );
            assert_eq!(
                detail["column"],
                before
                    .rsplit('\n')
                    .next()
                    .expect("line prefix")
                    .chars()
                    .count()
                    + 1
            );
            assert_eq!(detail["actual"], "-");
            assert_eq!(detail["section"], "phrase");
            assert_eq!(detail["occurrence"], 1);
            assert_eq!(detail["bar"], 1);
            assert_eq!(detail["step"], "2");
            assert_eq!(detail["source_bar"], 1);
        };

        let mut inherited = document();
        inherited["melodies"]["source"] =
            serde_json::json!([". - - - | - - - - | - - - - | - - - -"]);
        inherited["melodies"]["from_copy"] = serde_json::json!({"from":"source"});
        diagnostic(&inherited, "from_copy", "melodies.source[0]");

        let mut replaced = document();
        replaced["melodies"]["base"] =
            serde_json::json!(["1 - - - | 3 - - - | 5 - - - | 1' - - -"]);
        replaced["melodies"]["replace_copy"] = serde_json::json!({
            "from":"base",
            "replace":{"1":". - - - | - - - - | - - - - | - - - -"}
        });
        diagnostic(&replaced, "replace_copy", "melodies.replace_copy.replace.1");

        let mut used = document();
        used["melodies"]["source"] = serde_json::json!([". - - - | - - - - | - - - - | - - - -"]);
        used["melodies"]["use_copy"] = serde_json::json!([{"use":"source","bars":[1,1]}]);
        diagnostic(&used, "use_copy", "melodies.source[0]");
    }

    #[test]
    fn triggers_require_an_explicit_note_and_rolls_replace_only_pattern_drums() {
        let mut missing_note = document();
        missing_note["sections"]["phrase"] = serde_json::json!({"events":[{"kind":"hit","instrument":"part","at":{"bar":1,"step":1}}]});
        let error = super::compile(&missing_note.to_string()).expect_err("trigger note required");
        let message = &error.diagnostics[0].message;
        assert!(message.contains("instruments.part.trigger_note_midi"));
        assert!(message.contains("score quarter 0"));

        let mut doc = document();
        doc["instruments"]["part"]["trigger_note_midi"] = serde_json::json!(36);
        doc["drums"] = serde_json::json!({"beat":{"parts":{"part":"x...|....|....|...."}}});
        doc["sections"]["phrase"] = serde_json::json!({
            "drums":"beat",
            "placements":[{"instrument":"part","degree":"1","at":{"bar":1,"step":1},"duration_quarters":"1/4"}],
            "events":[{"kind":"step_roll","instrument":"part","bar":1,"from_step":1,"through_step":1,"velocity_from":120,"velocity_to":120}]
        });
        let plan = super::compile(&doc.to_string()).expect("explicit trigger note");
        let notes = plan.lanes[0]
            .pattern
            .events
            .iter()
            .filter_map(|event| match event {
                super::MusicalEvent::Note { note, velocity, .. } => Some((*note, *velocity)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(notes.len(), 2);
        assert!(
            notes.contains(&(36, 120)),
            "roll replaces the ordinary drum hit"
        );
        assert!(
            notes.contains(&(60, 100)),
            "explicit placement remains layered"
        );

        doc["sections"]["phrase"]["events"] = serde_json::json!([
            {"kind":"step_roll","instrument":"part","bar":1,"from_step":1,"through_step":1,"velocity_from":120,"velocity_to":120},
            {"kind":"step_roll","instrument":"part","bar":1,"from_step":1,"through_step":1,"velocity_from":100,"velocity_to":100}
        ]);
        let error = super::compile(&doc.to_string()).expect_err("overlapping rolls rejected");
        assert!(
            error.diagnostics[0]
                .message
                .contains("overlapping drum rolls")
        );
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
        assert_eq!(snapshots.len(), 139);
        assert_eq!(
            snapshots
                .iter()
                .filter(|snapshot| snapshot["score"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("/showcase.score.yaml")))
                .count(),
            10
        );
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
    fn instrument_pan_is_validated_and_defaults_to_center() {
        let mut doc = document();
        doc["instruments"]["part"]["pan"] = serde_json::json!(2);
        let error = super::compile(&doc.to_string()).expect_err("pan out of range");
        assert_eq!(
            error.diagnostics[0].path.as_deref(),
            Some("instruments.part.pan")
        );
        doc["instruments"]["part"]["pan"] = serde_json::json!("left");
        assert!(super::compile(&doc.to_string()).is_err());
        doc["instruments"]["part"]["pan"] = serde_json::json!(-1);
        assert!(super::compile(&doc.to_string()).is_ok());
    }

    #[test]
    fn add_placements_append_across_inheritance_levels_and_placements_replaces() {
        let mut doc = document();
        doc["sections"] = serde_json::json!({
            "phrase": {"placements":[{"instrument":"part","degree":"1","at":{"bar":1,"step":1}}]},
            "bridge": {"from":"phrase","add_placements":[{"instrument":"part","degree":"3","at":{"bar":1,"step":2}}]},
            "final": {"from":"bridge","add_placements":[{"instrument":"part","degree":"5","at":{"bar":1,"step":3}}]},
            "solo": {"add_placements":[{"instrument":"part","degree":"3","at":{"bar":1,"step":4}}]},
            "replaced": {"from":"phrase","placements":[{"instrument":"part","degree":"5","at":{"bar":1,"step":4}}]}
        });
        let notes = |name: &str| {
            let mut doc = doc.clone();
            doc["form"][0]["section"] = serde_json::json!(name);
            let plan = super::compile(&doc.to_string()).expect(name);
            plan.lanes[0]
                .sources
                .iter()
                .map(|source| {
                    (
                        source.spelling.clone().expect("degree spelling"),
                        source.path.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            notes("final"),
            [
                ("1".to_owned(), "sections.phrase.placements[0]".to_owned()),
                (
                    "3".to_owned(),
                    "sections.bridge.add_placements[0]".to_owned()
                ),
                (
                    "5".to_owned(),
                    "sections.final.add_placements[0]".to_owned()
                ),
            ]
        );
        assert_eq!(
            notes("solo"),
            [("3".to_owned(), "sections.solo.add_placements[0]".to_owned())]
        );
        assert_eq!(
            notes("replaced"),
            [("5".to_owned(), "sections.replaced.placements[0]".to_owned())]
        );
        let mut invalid = doc.clone();
        invalid["sections"]["solo"]["add_placements"] =
            serde_json::json!([{"instrument":"part","degree":"9","at":{"bar":1,"step":1}}]);
        let error = super::compile(&invalid.to_string()).expect_err("invalid placement");
        assert_eq!(
            error.diagnostics[0].path.as_deref(),
            Some("sections.solo.add_placements[0]")
        );
        let mut invalid = doc.clone();
        invalid["sections"]["phrase"]["placements"] =
            serde_json::json!([{"instrument":"part","degree":"9","at":{"bar":1,"step":1}}]);
        let error = super::compile(&invalid.to_string()).expect_err("inherited placement");
        assert_eq!(
            error.diagnostics[0].path.as_deref(),
            Some("sections.phrase.placements[0]")
        );
    }

    #[test]
    fn form_scale_is_optional_and_does_not_change_notes() {
        let mut doc = document();
        doc["form"][0]
            .as_object_mut()
            .expect("form entry")
            .remove("scale");
        let plan = super::compile(&doc.to_string()).expect("scale-less score compiles");
        assert!(plan.sections[0].scale.is_none());
        let occurrence = serde_json::to_value(&plan.sections[0]).expect("occurrence serializes");
        assert!(occurrence.get("scale").is_none());
        let events = serde_json::to_value(&plan.lanes[0].pattern.events).expect("events");
        doc["form"][0]["scale"] = serde_json::json!("aeolian");
        let plan = super::compile(&doc.to_string()).expect("scaled score compiles");
        assert_eq!(plan.sections[0].scale.as_deref(), Some("aeolian"));
        assert_eq!(
            serde_json::to_value(&plan.lanes[0].pattern.events).expect("events"),
            events
        );
        doc["form"][0]["scale"] = serde_json::json!("mystery");
        let error = super::compile(&doc.to_string()).expect_err("unknown scale");
        assert_eq!(error.diagnostics[0].path.as_deref(), Some("form[0].scale"));
    }

    #[test]
    fn arps_honor_rests_ties_and_repeat_merging() {
        let mut doc = document();
        doc["meta"]["meter"] = serde_json::json!("2/4");
        doc["sections"]["phrase"] = serde_json::json!({
            "chords":["1:m"],
            "arp":{"instrument":"part","pattern":"up"}
        });
        doc["arps"] = serde_json::json!({"up":{"order":"0 . 2 - | - - . 1"}});
        let notes = |text: &str| {
            let plan = super::compile(text).expect("arp score compiles");
            plan.lanes[0]
                .pattern
                .events
                .iter()
                .filter_map(|event| match event {
                    super::MusicalEvent::Note {
                        tick,
                        duration_ticks,
                        ..
                    } => Some((*tick, *duration_ticks)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(notes(&doc.to_string()), [(0, 120), (240, 480), (840, 120)]);
        doc["arps"]["up"]["order"] = serde_json::json!("1 1 - 2 | . . . .");
        assert_eq!(notes(&doc.to_string()), [(0, 120), (120, 240), (360, 120)]);
        doc["arps"]["up"]["retrigger_each_step"] = serde_json::json!(false);
        assert_eq!(notes(&doc.to_string()), [(0, 360), (360, 120)]);
        doc["arps"]["up"]["retrigger_each_step"] = serde_json::json!(true);
        doc["arps"]["up"]["order"] = serde_json::json!("- 1 . . | . . . .");
        let error = super::compile(&doc.to_string()).expect_err("tie without a note");
        assert_eq!(
            error.diagnostics[0].path.as_deref(),
            Some("sections.phrase.arp")
        );
    }

    #[test]
    fn reference_composition_expands_all_sections() {
        let text = include_str!("../../../../testdata/scores/dopamine_overload.score.yaml");
        let plan = super::compile(text).expect("reference score compiles");
        assert_eq!(plan.sections.len(), 14);
        assert_eq!(plan.sections.iter().map(|s| s.bars).sum::<usize>(), 70);
        assert_eq!(plan.instruments.len(), 41);
        assert_eq!(plan.length_ticks, u64::from(plan.ticks_per_beat) * 280);
        assert!(
            plan.lanes
                .iter()
                .all(|lane| !lane.pattern.events.is_empty())
        );
    }
}

#[cfg(test)]
mod expression_tests;
