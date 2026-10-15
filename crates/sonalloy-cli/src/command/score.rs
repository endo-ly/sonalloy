use crate::output::{CliFailure, finish_failure, print_warnings};
use crate::performance::events::{MusicalEvent, PerformanceEvents};
use crate::performance::{self, PerformancePlan};
use crate::score::Bindings;
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Subcommand)]
pub(super) enum ScoreCommand {
    /// Create a valid one-bar Score YAML file.
    Init { path: PathBuf },
    /// Validate score syntax, musical semantics, references, and instruments.
    Validate(ScoreArgs),
    /// Inspect the resolved timeline and instrument events.
    Inspect(InspectArgs),
    /// Export a complete Type 1 Standard MIDI File.
    ExportMidi {
        #[command(flatten)]
        score: ScoreArgs,
        #[arg(long)]
        output: PathBuf,
    },
    /// Import one MIDI note channel into Score YAML without quantization.
    ImportMidi {
        midi: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        instrument: PathBuf,
        #[arg(long,value_parser=clap::value_parser!(u8).range(1..=16))]
        channel: Option<u8>,
        #[arg(long, default_value = "C")]
        tonic: String,
        #[arg(long)]
        json: bool,
    },
}
#[derive(Debug, Args)]
pub(super) struct ScoreArgs {
    pub(super) score: PathBuf,
    #[command(flatten)]
    pub(super) bindings: Bindings,
    #[arg(long)]
    pub(super) json: bool,
}
#[derive(Debug, Args)]
pub(super) struct InspectArgs {
    #[command(flatten)]
    score: ScoreArgs,
    /// Emit normalized browser preview metadata.
    #[arg(long, conflicts_with = "json")]
    preview_json: bool,
}

fn failure(path: impl AsRef<Path>, message: impl Into<String>) -> CliFailure {
    CliFailure {
        code: 1,
        diagnostics: vec![
            sonalloy_core::Diagnostic::error(
                sonalloy_core::DiagnosticCode::DefinitionError,
                message,
            )
            .with_path(path.as_ref().to_string_lossy()),
        ],
    }
}

pub(super) fn run(command: ScoreCommand) -> ExitCode {
    let json = match &command {
        ScoreCommand::Init { .. } => false,
        ScoreCommand::Validate(a) | ScoreCommand::ExportMidi { score: a, .. } => a.json,
        ScoreCommand::Inspect(a) => a.score.json || a.preview_json,
        ScoreCommand::ImportMidi { json, .. } => *json,
    };
    match execute(command) {
        Ok(report) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("report serializes")
                );
            } else {
                display_report(&report);
            }
            ExitCode::SUCCESS
        }
        Err(error) => finish_failure(json, error),
    }
}

fn execute(command: ScoreCommand) -> Result<Value, CliFailure> {
    match command {
        ScoreCommand::Init { path } => {
            let root=std::env::var_os("SONALLOY_PRESET_ROOT").map(PathBuf::from).or_else(||std::env::current_dir().ok()?.ancestors().map(|p|p.join("presets")).find(|p|p.is_dir())).ok_or_else(||failure(&path,"set SONALLOY_PRESET_ROOT to initialize a score with an available instrument"))?;
            let instrument =
                std::fs::canonicalize(root.join("LEAD/001-analog-saw-lead/definition.json"))
                    .map_err(|e| failure(&path, e.to_string()))?;
            let doc = json!({"meta":{"title":"New score","bpm":120,"meter":"4/4","tuning":"12edo","reference_octave":4,"gate":1},"form":[{"section":"phrase","bars":1,"tonic":"C","scale":"ionian"}],"sections":{"phrase":{"lead":{"instrument":"lead","melody":"phrase"}}},"melodies":{"phrase":["1 - - - | 3 - - - | 5 - - - | 1 - - -"]},"instruments":{"lead":{"definition":instrument}}});
            write_score(&path, &doc)?;
            Ok(json!({"status":"ok","output":path}))
        }
        ScoreCommand::Validate(args) => {
            let plan = load(&args)?;
            if !args.json {
                print_warnings(&plan.diagnostics);
            }
            Ok(
                json!({"status":"ok","sections":plan.score.sections.len(),"instruments":plan.score.instruments.len(),"diagnostics":plan.diagnostics}),
            )
        }
        ScoreCommand::Inspect(args) => {
            let plan = load(&args.score)?;
            if args.preview_json {
                return preview(&plan);
            }
            Ok(
                json!({"status":"ok","title":plan.score.title,"sections":plan.score.sections,"ticks_per_beat":plan.score.ticks_per_beat,"length_ticks":plan.length_ticks,"musical_duration_seconds":plan.musical_duration_seconds,"instruments":plan.score.instruments,"lanes":plan.score.lanes.iter().map(|lane|json!({"id":lane.id,"gain_db":lane.gain_db,"events":lane.pattern.events,"sources":lane.sources})).collect::<Vec<_>>(),"parts":plan.parts.iter().map(|part|json!({"id":part.definition.id,"lane_gains_db":part.render_lanes.iter().map(|lane|lane.gain_db).collect::<Vec<_>>(),"audio_input":part.definition.audio_input,"events":part.pattern.events,"summary":crate::performance::events::inspect(&part.pattern).expect("prepared events are valid"),"tempo_changes":part.pattern.tempo_changes,"time_signature_changes":part.pattern.time_signature_changes})).collect::<Vec<_>>(),"diagnostics":plan.diagnostics}),
            )
        }
        ScoreCommand::ExportMidi { score, output } => {
            let plan = load(&score)?;
            ensure_new(&output)?;
            let pending = tempfile::NamedTempFile::new_in(
                output
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )
            .map_err(|e| failure(&output, e.to_string()))?;
            crate::midi::export_score(pending.path(), &plan).map_err(|diagnostics| CliFailure {
                code: 1,
                diagnostics,
            })?;
            pending
                .persist_noclobber(&output)
                .map_err(|e| failure(&output, e.to_string()))?;
            Ok(json!({"status":"ok","output":output}))
        }
        ScoreCommand::ImportMidi {
            midi,
            output,
            instrument,
            channel,
            tonic,
            json: _,
        } => {
            ensure_new(&output)?;
            let parsed = crate::midi::parse_midi(&midi).map_err(|diagnostics| CliFailure {
                code: 1,
                diagnostics,
            })?;
            let (events, diagnostics) =
                crate::midi::import_performance(parsed, channel.map(|c| c - 1)).map_err(
                    |diagnostics| CliFailure {
                        code: 1,
                        diagnostics,
                    },
                )?;
            let instrument =
                std::fs::canonicalize(instrument).map_err(|e| failure(&output, e.to_string()))?;
            let doc = import_document(&events, &instrument, &tonic)?;
            write_score(&output, &doc)?;
            Ok(json!({"status":"ok","output":output,"diagnostics":diagnostics}))
        }
    }
}
fn load(args: &ScoreArgs) -> Result<PerformancePlan, CliFailure> {
    performance::load(
        &args.score,
        &args.bindings,
        super::DEFAULT_SAMPLE_RATE,
        super::DEFAULT_BLOCK_SIZE,
    )
}
fn ensure_new(path: &Path) -> Result<(), CliFailure> {
    if path.exists() {
        Err(failure(path, "destination already exists"))
    } else {
        Ok(())
    }
}
fn write_score(path: &Path, doc: &Value) -> Result<(), CliFailure> {
    let text = crate::score::to_yaml(doc).map_err(|e| failure(path, e.to_string()))?;
    crate::score::compile(&text)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| failure(path, e.to_string()))?;
    file.write_all(text.as_bytes())
        .map_err(|e| failure(path, e.to_string()))
}
fn preview(plan: &PerformancePlan) -> Result<Value, CliFailure> {
    if plan.parts.len() != 1 {
        return Err(failure("instruments", "preview requires exactly one lane"));
    }
    let events = &plan.parts[0].pattern;
    if events.events.is_empty()
        || events.events.len() > 32
        || plan.musical_duration_seconds > 10.0
        || !(30.0..=300.0).contains(&events.tempo_changes[0].bpm)
        || events.time_signature_changes[0].numerator > 32
    {
        return Err(failure(
            "sections",
            "browser preview requires 1..32 notes, at most 10 seconds, 30..300 BPM, and a meter numerator of at most 32",
        ));
    }
    if events.tempo_changes.len() != 1 || events.time_signature_changes.len() != 1 {
        return Err(failure("form", "preview requires one tempo and meter"));
    }
    let mut notes = Vec::new();
    for event in &events.events {
        if let MusicalEvent::Note {
            tick,
            duration_ticks,
            note,
            velocity,
        } = event
        {
            notes.push(
                json!({"tick":tick,"durationTicks":duration_ticks,"note":note,"velocity":velocity}),
            );
        } else {
            return Err(failure("events", "browser preview supports notes only"));
        }
    }
    Ok(
        json!({"tempoBpm":events.tempo_changes[0].bpm,"ticksPerBeat":events.ticks_per_beat,"timeSignature":{"numerator":events.time_signature_changes[0].numerator,"denominator":events.time_signature_changes[0].denominator},"lengthTicks":events.length_ticks,"notes":notes}),
    )
}

pub(crate) fn import_document(
    events: &PerformanceEvents,
    instrument: &Path,
    tonic: &str,
) -> Result<Value, CliFailure> {
    let tonic_pc = crate::score::tonic_pitch(tonic)?;
    let ppq = u64::from(events.ticks_per_beat);
    let mut form = Vec::new();
    let mut sections = serde_json::Map::new();
    let mut time_changes = Vec::new();
    for (i, meter) in events.time_signature_changes.iter().enumerate() {
        let start = meter.tick;
        let end = events
            .time_signature_changes
            .get(i + 1)
            .map_or(events.length_ticks, |m| m.tick);
        let length = num_rational::Ratio::new(end - start, ppq);
        let bar =
            num_rational::Ratio::new(u64::from(meter.numerator) * 4, u64::from(meter.denominator));
        let full = (length / bar).to_integer();
        let remainder = length - bar * num_rational::Ratio::from_integer(full);
        let count = full + u64::from(*remainder.numer() != 0);
        let name = format!("phrase{}", i + 1);
        let bpm = events
            .tempo_changes
            .iter()
            .rev()
            .find(|t| t.tick <= start)
            .expect("initial tempo")
            .bpm;
        let mut entry = json!({"section":name,"bars":count,"tonic":tonic,"scale":"ionian","meter":format!("{}/{}",meter.numerator,meter.denominator),"bpm":bpm});
        if i > 0
            && !events
                .tempo_changes
                .iter()
                .any(|change| change.tick == start)
        {
            entry.as_object_mut().expect("form entry").remove("bpm");
        }
        if *remainder.numer() != 0 {
            entry["last_bar_quarters"] = Value::String(remainder.to_string());
        }
        form.push(entry);
        let mut placements = Vec::new();
        let mut controls = Vec::new();
        for event in &events.events {
            let tick = event_tick(event);
            if tick < start || tick > end || (tick == end && end != events.length_ticks) {
                continue;
            }
            let offset = num_rational::Ratio::new(tick - start, ppq);
            let index = (offset / bar).to_integer().min(count - 1);
            let within = offset - bar * num_rational::Ratio::from_integer(index);
            let at = json!({"bar":index+1,"offset_quarters":within.to_string()});
            let mut control = serde_json::to_value(event).expect("event serializes");
            control["kind"] = control["type"].clone();
            control.as_object_mut().expect("map").remove("type");
            control.as_object_mut().expect("map").remove("tick");
            control["instrument"] = json!("part");
            control["at"] = at.clone();
            if let MusicalEvent::Note {
                duration_ticks,
                note,
                velocity,
                ..
            } = event
            {
                let relative = i32::from(*note) - 60 - tonic_pc;
                let octave = relative.div_euclid(12);
                let degree = [
                    "1", "#1", "2", "b3", "3", "4", "#4", "5", "b6", "6", "b7", "7",
                ][relative.rem_euclid(12) as usize];
                let suffix = if octave < 0 {
                    ",".repeat(octave.unsigned_abs() as usize)
                } else {
                    "'".repeat(usize::try_from(octave).expect("nonnegative octave"))
                };
                placements.push(json!({"instrument":"part","at":at,"degree":format!("{degree}{suffix}"),"duration_quarters":num_rational::Ratio::new(*duration_ticks,ppq).to_string(),"velocity":velocity}));
            } else {
                controls.push(control);
            }
        }
        sections.insert(name, json!({"placements":placements,"events":controls}));
        for tempo in &events.tempo_changes {
            if tempo.tick > start && tempo.tick < end {
                time_changes.push(json!({"at_quarters":num_rational::Ratio::new(tempo.tick,ppq).to_string(),"bpm":tempo.bpm}));
            }
        }
    }
    Ok(
        json!({"meta":{"title":events.name.as_deref().unwrap_or("Imported MIDI"),"bpm":events.tempo_changes[0].bpm,"meter":format!("{}/{}",events.time_signature_changes[0].numerator,events.time_signature_changes[0].denominator),"tuning":"12edo","reference_octave":4,"gate":1},"form":form,"time_changes":time_changes,"sections":sections,"instruments":{"part":{"definition":instrument}}}),
    )
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

fn display_report(report: &Value) {
    if let Some(title) = report["title"].as_str() {
        println!("{title}");
        println!(
            "duration: {:.3} seconds; {} ticks at {} PPQ",
            report["musical_duration_seconds"]
                .as_f64()
                .expect("duration"),
            report["length_ticks"],
            report["ticks_per_beat"]
        );
        if let Some(sections) = report["sections"].as_array() {
            for s in sections {
                println!(
                    "  {}: {} bars, {} {}, {} BPM, {} (ticks {}..{})",
                    s["section"].as_str().expect("name"),
                    s["bars"],
                    s["tonic"].as_str().expect("tonic"),
                    s["scale"].as_str().expect("scale"),
                    s["bpm"],
                    s["meter"].as_str().expect("meter"),
                    s["start_tick"],
                    s["end_tick"]
                );
            }
        }
        if let Some(parts) = report["parts"].as_array() {
            for part in parts {
                println!(
                    "  {}: {} notes",
                    part["id"].as_str().expect("id"),
                    part["summary"]["note_count"]
                );
            }
        }
    } else if let Some(output) = report["output"].as_str() {
        println!("wrote {output}");
    } else {
        println!(
            "valid: {} sections, {} instruments",
            report["sections"], report["instruments"]
        );
    }
}
