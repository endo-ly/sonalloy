use crate::output::CliFailure;
use crate::score::{Bindings, ScorePlan};
use serde::Serialize;
use sonalloy_core::{Diagnostic, DiagnosticCode, DiagnosticSeverity};
use std::collections::{HashMap, HashSet};
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
    pub(crate) gain_db: f64,
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
    let paths = definition_paths(&score, path, bindings)?;
    prepare(score, paths, sample_rate, block_size)
}

fn prepare(
    score: ScorePlan,
    paths: HashMap<String, PathBuf>,
    sample_rate: u32,
    block_size: usize,
) -> Result<PerformancePlan, CliFailure> {
    let mut cache = HashMap::new();
    let mut diagnostics = Vec::new();
    for (id, path) in &paths {
        if !cache.contains_key(path) {
            let (instrument, warnings) =
                crate::command::load_and_compile(path, sample_rate, block_size)?;
            diagnostics.extend(warnings);
            cache.insert(path.clone(), instrument);
        }
        let spec = &score.instruments[id];
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
    let mut lanes = score.lanes.clone();
    let timeline = lanes
        .first()
        .map(|l| l.pattern.clone())
        .ok_or_else(|| failure("sections", "score contains no performance events"))?;
    for (id, spec) in &score.instruments {
        if !lanes.iter().any(|lane| &lane.id == id) {
            let mut pattern = timeline.clone();
            pattern.events.clear();
            lanes.push(crate::score::ScoreLane {
                id: id.clone(),
                gain_db: spec["gain_db"].as_f64().unwrap_or(0.0),
                pattern,
            });
        }
    }
    let mut parts = Vec::new();
    let mut channels = HashMap::new();
    for lane in lanes {
        let instrument = Arc::clone(&cache[&paths[&lane.id]]);
        let spec = &score.instruments[&lane.id];
        let definition = Part {
            id: lane.id.clone(),
            instrument: paths[&lane.id].clone(),
            gain_db: lane.gain_db,
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
        for event in &lane.pattern.events {
            if let MusicalEvent::Note { note, velocity, .. } = event {
                if !instrument.layers.iter().any(|l| {
                    (l.trigger.key_min..=l.trigger.key_max).contains(note)
                        && (l.trigger.velocity_min..=l.trigger.velocity_max).contains(velocity)
                }) {
                    return Err(failure(
                        format!("instruments.{}", lane.id),
                        format!("note {note} velocity {velocity} cannot trigger any layer"),
                    ));
                }
            }
        }
        let compiled_pattern = events::compile(&lane.pattern, &instrument, f64::from(sample_rate))
            .map_err(|diagnostics| CliFailure {
                code: 1,
                diagnostics,
            })?;
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
            let mut definition = definition;
            definition.gain_db = 0.0;
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
    let mix = Mix {
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
    };
    let definition = PerformanceDefinition {
        name: Some(score.title.clone()),
        parts: parts.iter().map(|p| p.definition.clone()).collect(),
        mix,
    };
    let audio_dependencies = resolve_audio_dependencies(&definition, &mut diagnostics);
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
        length_ticks: score.length_ticks,
        musical_duration_seconds,
        definition,
        parts,
        audio_dependencies,
        diagnostics,
        score,
    })
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
        let path = format!("parts[{index}].audio_input.part");
        let Some(&source_index) = part_indices.get(audio_input.part.as_str()) else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "audio_input.part must reference an existing part id",
                )
                .with_path(path),
            );
            continue;
        };
        if source_index == index {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::DefinitionError,
                    "audio_input.part cannot reference its own part",
                )
                .with_path(path),
            );
            continue;
        }
        dependencies[index] = Some(source_index);
    }

    let mut states = vec![0; dependencies.len()];
    for index in 0..dependencies.len() {
        visit_audio_dependency(index, &dependencies, &mut states, diagnostics);
    }
    dependencies
}

fn visit_audio_dependency(
    index: usize,
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
                .with_path(format!("parts[{index}].audio_input.part")),
            );
        } else if states[source_index] == 0 {
            visit_audio_dependency(source_index, dependencies, states, diagnostics);
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
    if !valid_length || !valid_characters || !valid_first || windows_reserved {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "id must be 1 to 64 ASCII characters, start with a letter or digit, contain only letters, digits, '.', '_' or '-', and not be a Windows reserved device name",
            )
            .with_path(format!("{part_path}.id")),
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
            return Err("part sample rate does not match the Demo sample rate".to_owned());
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
