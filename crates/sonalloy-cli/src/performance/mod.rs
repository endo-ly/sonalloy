use crate::output::CliFailure;
use crate::score::{Bindings, EventSource, PartWindow, ScoreLane, ScorePlan};
use serde::Serialize;
use sonalloy_core::{Diagnostic, DiagnosticCode, DiagnosticSeverity};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) mod events;
mod master;
use events::{CompiledEvents, MusicalEvent, PerformanceEvents};
pub(crate) use master::{
    FfmpegError, LoudnessAnalysis, LoudnessMeasurement, MasterReport, analyze_loudness, encode_mp3,
    master, measure_loudness,
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AudioInput {
    pub(crate) part: String,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Part {
    pub(crate) id: String,
    pub(crate) instrument: PathBuf,
    pub(crate) audio_input: Option<AudioInput>,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Mix {
    pub(crate) fade_out_seconds: f64,
    pub(crate) master: Option<Master>,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Master {
    pub(crate) integrated_lufs: f64,
    pub(crate) true_peak_db: f64,
    pub(crate) target_lra_lu: Option<f64>,
}
#[derive(Debug, Clone)]
pub(crate) struct PerformanceDefinition {
    pub(crate) name: Option<String>,
    pub(crate) parts: Vec<Part>,
    pub(crate) mix: Mix,
}
#[derive(Debug)]
pub(crate) struct PerformancePlan {
    pub(crate) sample_rate: u32,
    pub(crate) definition: PerformanceDefinition,
    pub(crate) parts: Vec<PartLane>,
    pub(crate) audio_dependencies: Vec<Option<usize>>,
    pub(crate) length_ticks: u64,
    pub(crate) musical_duration_seconds: f64,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) score: ScorePlan,
}
#[derive(Debug)]
pub(crate) struct PartLane {
    pub(crate) definition: Part,
    pub(crate) instrument: Arc<sonalloy_core::CompiledInstrument>,
    pub(crate) pattern: PerformanceEvents,
    pub(crate) render_lanes: Vec<RenderLane>,
    pub(crate) midi_channel: Option<u8>,
}
#[derive(Debug)]
pub(crate) struct RenderLane {
    pub(crate) compiled: CompiledEvents,
    pub(crate) gain_db: f64,
}

fn failure(path: impl Into<String>, message: impl Into<String>) -> CliFailure {
    CliFailure {
        code: 1,
        diagnostics: vec![
            Diagnostic::error(DiagnosticCode::DefinitionError, message).with_path(path.into()),
        ],
    }
}

fn lane_failure(lane: &crate::score::ScoreLane, mut diagnostics: Vec<Diagnostic>) -> CliFailure {
    for diagnostic in &mut diagnostics {
        if let Some((index, suffix)) = diagnostic
            .path
            .as_deref()
            .and_then(|path| path.strip_prefix("events["))
            .and_then(|path| path.split_once(']'))
            .and_then(|(index, suffix)| index.parse::<usize>().ok().map(|i| (i, suffix.to_owned())))
        {
            if let Some(source) = lane.sources.get(index) {
                diagnostic.path = Some(format!("{}{suffix}", source.path));
                diagnostic.detail=Some(serde_json::json!({"source_type":"score","line":source.line,"column":source.column,"section":source.section,"bar":source.bar,"step":source.step,"actual":lane.pattern.events[index],"expected":diagnostic.message}).to_string());
            }
        }
    }
    CliFailure {
        code: 1,
        diagnostics,
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn definition_paths(
    score: &ScorePlan,
    path: &Path,
    bindings: &Bindings,
) -> Result<HashMap<String, PathBuf>, CliFailure> {
    let mut bound = HashMap::new();
    for binding in &bindings.bind {
        let (id, value) = binding
            .split_once('=')
            .filter(|(id, v)| !id.is_empty() && !v.is_empty())
            .ok_or_else(|| failure("bind", "expected ID=DEFINITION"))?;
        let spec = score
            .instruments
            .get(id)
            .ok_or_else(|| failure("bind", format!("unknown instrument {id}")))?;
        if spec.get("external") != Some(&serde_json::Value::Bool(true)) {
            return Err(failure(
                "bind",
                format!("instrument {id} is fixed and cannot be bound"),
            ));
        }
        if bound.insert(id.to_owned(), PathBuf::from(value)).is_some() {
            return Err(failure("bind", format!("duplicate binding {id}")));
        }
    }
    let base = path.parent().unwrap_or(Path::new("."));
    let preset_root = bindings
        .preset_root
        .clone()
        .or_else(|| std::env::var_os("SONALLOY_PRESET_ROOT").map(PathBuf::from))
        .or_else(|| {
            std::fs::canonicalize(base)
                .ok()?
                .ancestors()
                .map(|p| p.join("presets"))
                .find(|p| p.is_dir())
        });
    let mut result = HashMap::new();
    let mut ids = HashSet::new();
    for (id, spec) in &score.instruments {
        let mut diagnostics = Vec::new();
        validate_part_id(id, &format!("instruments.{id}"), &mut diagnostics);
        if !diagnostics.is_empty() {
            return Err(CliFailure {
                code: 1,
                diagnostics,
            });
        }
        if !ids.insert(id.to_ascii_lowercase()) {
            return Err(failure(
                format!("instruments.{id}"),
                "instrument ids must be unique ignoring ASCII case",
            ));
        }
        let definition = if let Some(preset) = spec.get("preset") {
            let preset = preset.as_str().ok_or_else(|| {
                failure(
                    format!("instruments.{id}.preset"),
                    "expected a preset id string",
                )
            })?;
            let components: Vec<_> = preset.split('/').collect();
            if components.len() != 2
                || components
                    .iter()
                    .any(|p| p.is_empty() || *p == "." || *p == ".." || p.contains(['\\', ':']))
            {
                return Err(failure(
                    format!("instruments.{id}.preset"),
                    "expected CATEGORY/NNN-name within the preset root",
                ));
            }
            let root = preset_root.as_ref().ok_or_else(|| {
                failure(
                    "preset_root",
                    "preset root not found; specify --preset-root",
                )
            })?;
            let root =
                std::fs::canonicalize(root).map_err(|e| failure("preset_root", e.to_string()))?;
            let resolved = std::fs::canonicalize(root.join(preset).join("definition.json"))
                .map_err(|e| failure(format!("instruments.{id}.preset"), e.to_string()))?;
            if !resolved.starts_with(&root) {
                return Err(failure(
                    format!("instruments.{id}.preset"),
                    "preset escapes the preset root",
                ));
            }
            resolved
        } else if let Some(definition) = spec.get("definition") {
            base.join(definition.as_str().ok_or_else(|| {
                failure(
                    format!("instruments.{id}.definition"),
                    "expected a path string",
                )
            })?)
        } else {
            bound.remove(id).ok_or_else(|| {
                failure(
                    format!("instruments.{id}"),
                    format!("external instrument requires --bind {id}=DEFINITION"),
                )
            })?
        };
        let definition = std::fs::canonicalize(&definition)
            .map_err(|e| failure(format!("instruments.{id}"), e.to_string()))?;
        result.insert(id.clone(), definition);
    }
    Ok(result)
}

pub(crate) fn load(
    path: &Path,
    bindings: &Bindings,
    sample_rate: u32,
    block_size: usize,
) -> Result<PerformancePlan, CliFailure> {
    let score = crate::score::read(path)?;
    let paths = definition_paths(&score, path, bindings).map_err(|mut failure| {
        score.annotate_diagnostics(&mut failure.diagnostics);
        failure
    })?;
    prepare(score, &paths, sample_rate, block_size)
}

#[allow(clippy::too_many_lines)]
fn prepare(
    score: ScorePlan,
    paths: &HashMap<String, PathBuf>,
    sample_rate: u32,
    block_size: usize,
) -> Result<PerformancePlan, CliFailure> {
    let InstrumentBank {
        instruments: cache,
        ranges,
        mut diagnostics,
    } = load_instruments(&score, paths, sample_rate, block_size)?;
    let mut lanes = score.lanes.clone();
    let timeline = score.timeline.clone();
    for (id, spec) in &score.instruments {
        if !lanes.iter().any(|lane| &lane.id == id) {
            let mut pattern = timeline.clone();
            pattern.events.clear();
            lanes.push(crate::score::ScoreLane {
                id: id.clone(),
                gain_db: spec["gain_db"].as_f64().unwrap_or(0.0),
                pattern,
                sources: Vec::new(),
            });
        }
    }
    let compiled_timeline =
        events::Timeline::new(&timeline, f64::from(sample_rate)).map_err(|diagnostics| {
            CliFailure {
                code: 1,
                diagnostics,
            }
        })?;
    let mut parts = Vec::new();
    let mut channels = HashMap::new();
    for lane in lanes {
        let instrument = Arc::clone(&cache[&paths[&lane.id]]);
        let spec = &score.instruments[&lane.id];
        let mut lane = lane;
        insert_part_window_resets(&mut lane, &score.part_windows, &instrument);
        let definition = Part {
            id: lane.id.clone(),
            instrument: paths[&lane.id].clone(),
            audio_input: spec
                .get("audio_input")
                .map(|v| {
                    v.as_str()
                        .map(|s| AudioInput { part: s.into() })
                        .ok_or_else(|| {
                            failure(
                                format!("instruments.{}.audio_input", lane.id),
                                "expected an instrument id string",
                            )
                        })
                })
                .transpose()?,
        };
        validate_lane(
            &lane,
            &definition,
            &instrument,
            ranges[&paths[&lane.id]],
            &mut diagnostics,
        )?;
        let compiled_pattern =
            events::compile_on_timeline(&lane.pattern, &instrument, &compiled_timeline)
                .map_err(|diagnostics| lane_failure(&lane, diagnostics))?;
        let next = channels.len() + 1;
        let channel = *channels.entry(lane.id.clone()).or_insert(next);
        if let Some(part) = parts
            .iter_mut()
            .find(|p: &&mut PartLane| p.definition.id == lane.id)
        {
            part.pattern.events.extend(lane.pattern.events);
            part.render_lanes.push(RenderLane {
                compiled: compiled_pattern,
                gain_db: lane.gain_db,
            });
        } else {
            parts.push(PartLane {
                definition,
                instrument,
                pattern: lane.pattern,
                render_lanes: vec![RenderLane {
                    compiled: compiled_pattern,
                    gain_db: lane.gain_db,
                }],
                midi_channel: u8::try_from(channel).ok().filter(|c| *c <= 16),
            });
        }
    }
    let mix = prepare_mix(&score)?;
    for part in &parts {
        if part.render_lanes.len() > 1 && part.pattern.events.iter().any(|event| !event.is_note()) {
            diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::DefinitionError,
                    "additional gain lanes have independent controller state",
                )
                .with_path(format!("instruments.{}", part.definition.id)),
            );
        }
    }
    let definition = PerformanceDefinition {
        name: Some(score.title.clone()),
        parts: parts.iter().map(|p| p.definition.clone()).collect(),
        mix,
    };
    let audio_dependencies = resolve_audio_dependencies(&definition, &mut diagnostics);
    score.annotate_diagnostics(&mut diagnostics);
    if diagnostics
        .iter()
        .any(|d| d.severity == DiagnosticSeverity::Error)
    {
        return Err(CliFailure {
            code: 1,
            diagnostics,
        });
    }
    let musical_duration_seconds = crate::musical_time::musical_duration_seconds(
        score.length_ticks,
        score.ticks_per_beat,
        &events::tempo_points(&timeline),
    )
    .map_err(|e| failure("form", e.to_string()))?;
    Ok(PerformancePlan {
        sample_rate,
        length_ticks: score.length_ticks,
        musical_duration_seconds,
        definition,
        parts,
        audio_dependencies,
        diagnostics,
        score,
    })
}

fn insert_part_window_resets(
    lane: &mut ScoreLane,
    windows: &[PartWindow],
    instrument: &sonalloy_core::CompiledInstrument,
) {
    let mut ranges: BTreeMap<(u64, u64), (u64, u64)> = BTreeMap::new();
    for window in windows.iter().filter(|window| window.instrument == lane.id) {
        ranges
            .entry((window.section_start_tick, window.section_end_tick))
            .and_modify(|(start, end)| {
                *start = (*start).max(window.start_tick);
                *end = (*end).min(window.end_tick);
            })
            .or_insert((window.start_tick, window.end_tick));
    }
    if ranges.is_empty() {
        return;
    }

    let mut reset_sustain = false;
    let mut reset_pitch_bend = false;
    let mut reset_mod_wheel = false;
    let mut reset_aftertouch = false;
    let mut parameters = BTreeSet::new();
    for event in &lane.pattern.events {
        match event {
            MusicalEvent::SustainPedal { .. } => reset_sustain = true,
            MusicalEvent::PitchBend { .. } => reset_pitch_bend = true,
            MusicalEvent::ModWheel { .. } => reset_mod_wheel = true,
            MusicalEvent::Aftertouch { .. } => reset_aftertouch = true,
            MusicalEvent::ParameterChange { parameter, .. }
            | MusicalEvent::ParameterRamp { parameter, .. } => {
                parameters.insert(parameter.clone());
            }
            MusicalEvent::Note { .. } => {}
        }
    }
    let parameter_defaults = parameters
        .into_iter()
        .filter_map(|parameter| {
            instrument
                .parameters()
                .iter()
                .find(|descriptor| descriptor.id == parameter)
                .map(|descriptor| (parameter, descriptor.default))
        })
        .collect::<Vec<_>>();

    let mut reset_ticks = BTreeSet::new();
    for ((section_start, section_end), (window_start, window_end)) in &ranges {
        if window_start >= window_end {
            reset_ticks.insert(*section_start);
            reset_ticks.insert(*section_end);
        } else {
            reset_ticks.insert(*window_start);
            reset_ticks.insert(*window_end);
        }
    }
    let mut events = Vec::new();
    let mut reset_sources = Vec::new();
    for tick in reset_ticks {
        let mut push_reset = |event| {
            events.push(event);
            reset_sources.push(part_window_source(tick));
        };
        if reset_sustain {
            push_reset(MusicalEvent::SustainPedal { tick, down: false });
        }
        for (parameter, native_value) in &parameter_defaults {
            push_reset(MusicalEvent::ParameterChange {
                tick,
                parameter: parameter.clone(),
                native_value: *native_value,
            });
        }
        if reset_pitch_bend {
            push_reset(MusicalEvent::PitchBend { tick, value: 0.0 });
        }
        if reset_mod_wheel {
            push_reset(MusicalEvent::ModWheel { tick, value: 0.0 });
        }
        if reset_aftertouch {
            push_reset(MusicalEvent::Aftertouch { tick, value: 0.0 });
        }
    }
    events.append(&mut lane.pattern.events);
    reset_sources.append(&mut lane.sources);
    lane.pattern.events = events;
    lane.sources = reset_sources;
}

fn part_window_source(tick: u64) -> EventSource {
    EventSource {
        path: "part_window".to_owned(),
        spelling: None,
        tick,
        at_quarters: String::new(),
        duration_quarters: "0".to_owned(),
        line: 0,
        column: 0,
        section: String::new(),
        bar: 0,
        step: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn part_window_clips_controls_and_resets_state_at_both_boundaries() {
        let definition_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/instruments/basic-poly-synth.json")
            .canonicalize()
            .expect("instrument fixture");
        let (instrument, _) = crate::command::load_and_compile(&definition_path, 48_000, 64)
            .expect("instrument compiles");
        let parameter = "voice.processor.tone.cutoff";
        let score = part_window_score();
        let mut plan = crate::score::compile(&score.to_string()).expect("Score window compiles");
        let windows = plan.part_windows.clone();
        assert_eq!(windows.len(), 1);
        assert_eq!(
            (
                windows[0].section_start_tick,
                windows[0].section_end_tick,
                windows[0].start_tick,
                windows[0].end_tick,
            ),
            (1_920, 9_600, 3_840, 7_680)
        );
        let mut lane = plan.lanes.pop().expect("control lane");

        assert!(!lane.pattern.events.iter().any(|event| matches!(
            event,
            MusicalEvent::SustainPedal {
                tick: 2_160,
                down: true
            } | MusicalEvent::ModWheel { value: 1.0, .. }
                | MusicalEvent::ParameterChange {
                    native_value: 1_500.0,
                    ..
                }
        )));
        assert!(lane.pattern.events.iter().any(|event| matches!(
            event,
            MusicalEvent::SustainPedal {
                tick: 240,
                down: true
            }
        )));
        assert!(lane.pattern.events.iter().any(|event| matches!(
            event,
            MusicalEvent::ParameterRamp {
                tick: 3_840,
                duration_ticks: 3_840,
                from_value,
                to_value,
                ..
            } if (*from_value - 3_000.0).abs() < 0.001 && (*to_value - 7_000.0).abs() < 0.001
        )));
        let pitch_bends = lane
            .pattern
            .events
            .iter()
            .filter_map(|event| match event {
                MusicalEvent::PitchBend { tick, value } => Some((*tick, *value)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(pitch_bends, [(4_320, 0.75)]);
        insert_part_window_resets(&mut lane, &windows, &instrument);
        for tick in [3_840, 7_680] {
            assert!(
                lane.pattern.events.iter().any(|event| matches!(
                    event,
                    MusicalEvent::SustainPedal { tick: event_tick, down: false }
                        if *event_tick == tick
                )),
                "events: {:?}",
                lane.pattern.events
            );
            assert!(lane.pattern.events.iter().any(|event| matches!(
                event,
                MusicalEvent::ModWheel { tick: event_tick, value: 0.0 }
                    if *event_tick == tick
            )));
            assert!(lane.pattern.events.iter().any(|event| matches!(
                event,
                MusicalEvent::ParameterChange {
                    tick: event_tick,
                    parameter: event_parameter,
                    native_value,
                } if *event_tick == tick
                    && event_parameter == parameter
                    && (*native_value - instrument.parameters().iter().find(|d| d.id == parameter).unwrap().default).abs() < 0.001
            )));
        }
        assert_eq!(lane.pattern.events.len(), lane.sources.len());
    }

    fn part_window_score() -> serde_json::Value {
        serde_json::json!({
            "meta": {"title":"Window", "bpm":120, "meter":"4/4", "tuning":"12edo", "gate":1},
            "form": [
                {"section":"intro", "bars":1, "tonic":"C", "scale":"ionian"},
                {"section":"phrase", "bars":4, "tonic":"C", "scale":"ionian"}
            ],
            "sections": {
                "intro":{"events":[
                    {"kind":"sustain_pedal", "instrument":"part", "down":true, "at":{"bar":1,"step":3}},
                    {"kind":"mod_wheel", "instrument":"part", "value":0.4, "at":{"bar":1,"step":1}}
                ]},
                "phrase":{"events":[
                    {"kind":"part_window", "instrument":"part", "bars":[2,3]},
                    {"kind":"sustain_pedal", "instrument":"part", "down":true, "at":{"bar":1,"step":3}},
                    {"kind":"pitch_bend", "instrument":"part", "value":0.75, "at":{"bar":2,"step":5}},
                    {"kind":"mod_wheel", "instrument":"part", "value":1, "at":{"bar":4,"step":1}},
                    {"kind":"automation", "instrument":"part", "parameter":"voice.processor.tone.cutoff", "from_value":1000, "to_value":9000, "at":{"bar":1,"step":1}, "duration_quarters":16},
                    {"kind":"parameter_change", "instrument":"part", "parameter":"voice.processor.tone.cutoff", "native_value":1500, "at":{"bar":4,"step":1}}
                ]}},
            "instruments": {"part":{"external":true}}
        })
    }
}

fn resolve_audio_dependencies(
    definition: &PerformanceDefinition,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Option<usize>> {
    let part_indices = definition
        .parts
        .iter()
        .enumerate()
        .map(|(index, part)| (part.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut dependencies = vec![None; definition.parts.len()];

    for (index, part) in definition.parts.iter().enumerate() {
        let Some(audio_input) = &part.audio_input else {
            continue;
        };
        let path = format!("instruments.{}.audio_input", part.id);
        let Some(&source_index) = part_indices.get(audio_input.part.as_str()) else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "audio_input must reference an existing instrument id",
                )
                .with_path(path),
            );
            continue;
        };
        if source_index == index {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "audio_input cannot reference its own instrument",
                )
                .with_path(path),
            );
            continue;
        }
        dependencies[index] = Some(source_index);
    }

    let mut states = vec![0; dependencies.len()];
    for index in 0..dependencies.len() {
        visit_audio_dependency(
            index,
            &definition.parts,
            &dependencies,
            &mut states,
            diagnostics,
        );
    }
    dependencies
}

fn visit_audio_dependency(
    index: usize,
    parts: &[Part],
    dependencies: &[Option<usize>],
    states: &mut [u8],
    diagnostics: &mut Vec<Diagnostic>,
) {
    if states[index] == 2 {
        return;
    }
    states[index] = 1;
    if let Some(source_index) = dependencies[index] {
        if states[source_index] == 1 {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "audio_input routing cannot contain a cycle",
                )
                .with_path(format!("instruments.{}.audio_input", parts[index].id)),
            );
        } else if states[source_index] == 0 {
            visit_audio_dependency(source_index, parts, dependencies, states, diagnostics);
        }
    }
    states[index] = 2;
}

pub(crate) fn render_order(demo: &PerformancePlan) -> Vec<usize> {
    fn visit(
        index: usize,
        dependencies: &[Option<usize>],
        visited: &mut [bool],
        order: &mut Vec<usize>,
    ) {
        if visited[index] {
            return;
        }
        visited[index] = true;
        if let Some(source_index) = dependencies[index] {
            visit(source_index, dependencies, visited, order);
        }
        order.push(index);
    }

    let mut order = Vec::with_capacity(demo.parts.len());
    let mut visited = vec![false; demo.parts.len()];
    for index in 0..demo.parts.len() {
        visit(index, &demo.audio_dependencies, &mut visited, &mut order);
    }
    order
}

pub(crate) fn prerender_order(demo: &PerformancePlan) -> Vec<usize> {
    fn mark_required(
        index: usize,
        dependencies: &[Option<usize>],
        rendered: &[bool],
        required: &mut [bool],
    ) {
        if required[index] || rendered[index] {
            return;
        }
        required[index] = true;
        if let Some(source_index) = dependencies[index]
            && !rendered[source_index]
        {
            mark_required(source_index, dependencies, rendered, required);
        }
    }

    let mut required = vec![false; demo.parts.len()];
    let mut rendered = vec![false; demo.parts.len()];
    for (index, source_index) in demo.audio_dependencies.iter().enumerate() {
        if let Some(source_index) = source_index
            && !rendered[*source_index]
        {
            mark_required(
                *source_index,
                &demo.audio_dependencies,
                &rendered,
                &mut required,
            );
        }
        rendered[index] = true;
    }
    render_order(demo)
        .into_iter()
        .filter(|index| required[*index])
        .collect()
}

fn validate_part_id(id: &str, part_path: &str, diagnostics: &mut Vec<Diagnostic>) {
    let valid_length = (1..=64).contains(&id.len());
    let valid_characters = id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    let valid_first = id
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric());
    let windows_reserved = is_windows_reserved_device_name(id);
    if !valid_length || !valid_characters || !valid_first || windows_reserved || id.ends_with('.') {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "id must be 1 to 64 ASCII characters, start with a letter or digit, contain only letters, digits, '.', '_' or '-', and not be a Windows reserved device name",
            )
            .with_path(part_path),
        );
    }
}

fn is_windows_reserved_device_name(id: &str) -> bool {
    let basename = id.split('.').next().unwrap_or(id).to_ascii_lowercase();
    matches!(basename.as_str(), "con" | "prn" | "aux" | "nul")
        || matches!(
            basename.as_bytes(),
            [b'c', b'o', b'm', b'1'..=b'9'] | [b'l', b'p', b't', b'1'..=b'9']
        )
}

pub(crate) fn gain_linear(gain_db: f64) -> Option<f64> {
    if !gain_db.is_finite() {
        return None;
    }
    let gain = 10.0_f64.powf(gain_db / 20.0);
    gain.is_finite().then_some(gain)
}

#[derive(Debug)]
pub(crate) struct StereoMix {
    pub(crate) sample_rate: u32,
    pub(crate) left: Vec<f32>,
    pub(crate) right: Vec<f32>,
}

impl StereoMix {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    pub(crate) fn add_part(
        &mut self,
        audio: &sonalloy_core::RenderedAudio,
        gain_db: f64,
    ) -> Result<(), String> {
        if audio.sample_rate != self.sample_rate {
            return Err("part sample rate does not match the Score sample rate".to_owned());
        }
        let [left, right] = audio.channels.as_slice() else {
            return Err("part render must contain exactly two channels".to_owned());
        };
        if left.len() != right.len() {
            return Err("part render channels have different lengths".to_owned());
        }
        let gain = gain_linear(gain_db).ok_or_else(|| "part gain is not finite".to_owned())?;
        #[allow(clippy::cast_possible_truncation)]
        let gain = gain as f32;
        if !gain.is_finite() {
            return Err("part gain cannot be represented as a finite audio gain".to_owned());
        }
        if self.left.len() < left.len() {
            self.left.resize(left.len(), 0.0);
            self.right.resize(right.len(), 0.0);
        }
        for (mixed, sample) in self.left.iter_mut().zip(left) {
            *mixed += *sample * gain;
        }
        for (mixed, sample) in self.right.iter_mut().zip(right) {
            *mixed += *sample * gain;
        }
        Ok(())
    }

    pub(crate) fn apply_fade(&mut self, fade_out_seconds: f64) -> Result<(), String> {
        if !fade_out_seconds.is_finite() || fade_out_seconds < 0.0 {
            return Err("fade_out_seconds must be finite and non-negative".to_owned());
        }
        #[allow(clippy::cast_precision_loss)]
        let duration_seconds = self.left.len() as f64 / f64::from(self.sample_rate);
        if fade_out_seconds > duration_seconds {
            return Err("fade_out_seconds must not exceed the final mix duration".to_owned());
        }
        if fade_out_seconds == 0.0 || self.left.is_empty() {
            return Ok(());
        }
        let fade_frames =
            sonalloy_core::seconds_to_frames(fade_out_seconds, f64::from(self.sample_rate))
                .map_err(|error| error.to_string())?;
        let fade_frames = usize::try_from(fade_frames)
            .map_err(|_| "fade frame count does not fit in memory".to_owned())?;
        if fade_frames == 0 {
            return Ok(());
        }
        let start = self.left.len().saturating_sub(fade_frames);
        for frame in start..self.left.len() {
            let gain = if fade_frames == 1 {
                0.0
            } else {
                #[allow(clippy::cast_precision_loss)]
                let denominator = (fade_frames - 1) as f32;
                #[allow(clippy::cast_precision_loss)]
                let progress = (frame - start) as f32 / denominator;
                (1.0 - progress).clamp(0.0, 1.0)
            };
            self.left[frame] *= gain;
            self.right[frame] *= gain;
        }
        Ok(())
    }

    pub(crate) fn into_audio(self) -> sonalloy_core::RenderedAudio {
        sonalloy_core::RenderedAudio {
            sample_rate: self.sample_rate,
            channels: vec![self.left, self.right],
        }
    }
}

fn prepare_mix(score: &ScorePlan) -> Result<Mix, CliFailure> {
    Ok(Mix {
        fade_out_seconds: score.fade_out_seconds,
        master: score
            .master
            .as_ref()
            .map(|v| {
                Ok(Master {
                    integrated_lufs: v["target_lufs"].as_f64().ok_or_else(|| {
                        failure("meta.master.target_lufs", "expected LUFS target")
                    })?,
                    true_peak_db: v["max_true_peak_db"].as_f64().ok_or_else(|| {
                        failure("meta.master.max_true_peak_db", "expected true peak ceiling")
                    })?,
                    target_lra_lu: v
                        .get("target_lra_lu")
                        .map(|v| {
                            v.as_f64()
                                .filter(|n| n.is_finite() && *n >= 0.0)
                                .ok_or_else(|| {
                                    failure(
                                        "meta.master.target_lra_lu",
                                        "expected nonnegative LRA target",
                                    )
                                })
                        })
                        .transpose()?,
                })
            })
            .transpose()?,
    })
}

fn validate_lane(
    lane: &crate::score::ScoreLane,
    definition: &Part,
    instrument: &sonalloy_core::CompiledInstrument,
    range: Option<sonalloy_core::InstrumentRecommendedRange>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), CliFailure> {
    if (instrument.required_input_channels() > 0) != definition.audio_input.is_some() {
        return Err(failure(
            format!("instruments.{}.audio_input", lane.id),
            if instrument.required_input_channels() > 0 {
                "instrument requires audio_input"
            } else {
                "instrument does not use audio_input"
            },
        ));
    }
    if definition.audio_input.is_some() && instrument.required_input_channels() != 2 {
        return Err(failure(
            format!("instruments.{}.audio_input", lane.id),
            "score audio routes require a stereo input",
        ));
    }
    for (index, event) in lane.pattern.events.iter().enumerate() {
        if let MusicalEvent::Note { note, velocity, .. } = event {
            if !instrument.layers.iter().any(|l| {
                (l.trigger.key_min..=l.trigger.key_max).contains(note)
                    && (l.trigger.velocity_min..=l.trigger.velocity_max).contains(velocity)
            }) {
                return Err(lane_failure(
                    lane,
                    vec![
                        Diagnostic::error(
                            DiagnosticCode::DefinitionError,
                            format!("note {note} velocity {velocity} cannot trigger any layer"),
                        )
                        .with_path(format!("events[{index}]")),
                    ],
                ));
            }
        }
    }
    if let Some(range) = range {
        let outside=lane.pattern.events.iter().filter(|e|matches!(e,MusicalEvent::Note{note,..} if *note<range.min_midi||*note>range.max_midi)).count();
        if outside > 0 {
            diagnostics.push(
                Diagnostic::warning(
                    DiagnosticCode::ValueOutOfRange,
                    format!(
                        "{outside} notes are outside the recommended range {}..{}",
                        range.min_midi, range.max_midi
                    ),
                )
                .with_path(format!("instruments.{}", lane.id)),
            );
        }
    }
    Ok(())
}

struct InstrumentBank {
    instruments: HashMap<PathBuf, Arc<sonalloy_core::CompiledInstrument>>,
    ranges: HashMap<PathBuf, Option<sonalloy_core::InstrumentRecommendedRange>>,
    diagnostics: Vec<Diagnostic>,
}
fn load_instruments(
    score: &ScorePlan,
    paths: &HashMap<String, PathBuf>,
    sample_rate: u32,
    block_size: usize,
) -> Result<InstrumentBank, CliFailure> {
    let mut cache = HashMap::new();
    let mut ranges = HashMap::new();
    let mut diagnostics = Vec::new();
    for (id, spec) in &score.instruments {
        let path = &paths[id];
        if !cache.contains_key(path) {
            let (instrument, warnings, metadata) =
                crate::command::load_with_metadata(path, sample_rate, block_size)?;
            diagnostics.extend(warnings);
            cache.insert(path.clone(), instrument);
            ranges.insert(path.clone(), metadata.recommended_range);
        }
        if let Some(value) = spec.get("gain_db") {
            let gain = value
                .as_f64()
                .filter(|v| gain_linear(*v).is_some())
                .ok_or_else(|| {
                    failure(format!("instruments.{id}.gain_db"), "gain must be finite")
                })?;
            let _ = gain;
        }
    }
    Ok(InstrumentBank {
        instruments: cache,
        ranges,
        diagnostics,
    })
}
