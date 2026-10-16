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
    /// Emit the structure summary instead of per-event detail.
    #[arg(long, conflicts_with = "preview_json")]
    summary: bool,
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
            let doc = json!({"meta":{"title":"New score","bpm":120,"meter":"4/4","tuning":"12edo","reference_octave":4,"gate":1},"form":[{"section":"phrase","bars":1,"tonic":"C","scale":"ionian"}],"sections":{"phrase":{"lead":{"instrument":"lead","melody":"phrase"}}},"melodies":{"phrase":["1 - - - | 3 - - - | 5 - - - | 1 - - -"]},"instruments":{"lead":{"preset":"LEAD/001-analog-saw-lead"}}});
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
            if args.summary {
                if !args.score.json {
                    print_warnings(&plan.diagnostics);
                }
                return Ok(summary(&plan));
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

/// Build the structure summary for `score inspect --summary`: whole-score
/// totals, one entry per section occurrence, and per-instrument aggregates.
/// Section note counts cover Note Ons starting inside each occurrence, so
/// repeated appearances of one section stay separate.
fn summary(plan: &PerformancePlan) -> Value {
    let mut section_notes = vec![0_usize; plan.score.sections.len()];
    let mut total_notes = 0_usize;
    for part in &plan.parts {
        for event in &part.pattern.events {
            if let MusicalEvent::Note { tick, .. } = event {
                total_notes += 1;
                if let Some(index) = plan
                    .score
                    .sections
                    .iter()
                    .position(|section| tick >= &section.start_tick && tick < &section.end_tick)
                {
                    section_notes[index] += 1;
                }
            }
        }
    }
    let sections = plan
        .score
        .sections
        .iter()
        .enumerate()
        .map(|(index, section)| {
            let mut entry = json!({
                "section": section.section,
                "occurrence": index + 1,
                "bars": section.bars,
                "start_tick": section.start_tick,
                "end_tick": section.end_tick,
                "tonic": section.tonic,
                "bpm": section.bpm,
                "meter": section.meter,
                "note_count": section_notes[index],
            });
            if let Some(scale) = &section.scale {
                entry["scale"] = json!(scale);
            }
            entry
        })
        .collect::<Vec<_>>();
    let instruments = plan
        .parts
        .iter()
        .map(|part| {
            let inspection = crate::performance::events::inspect(&part.pattern)
                .expect("prepared events are valid");
            let control_event_count = inspection.sustain_event_count
                + inspection.pitch_bend_event_count
                + inspection.mod_wheel_event_count
                + inspection.aftertouch_event_count
                + inspection.parameter_change_count
                + inspection.parameter_ramp_count;
            json!({
                "id": part.definition.id,
                "note_count": inspection.note_count,
                "note_min": inspection.note_min,
                "note_max": inspection.note_max,
                "velocity_min": inspection.velocity_min,
                "velocity_max": inspection.velocity_max,
                "control_event_count": control_event_count,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "status": "ok",
        "title": plan.score.title,
        "summary": {
            "section_count": plan.score.sections.len(),
            "instrument_count": plan.parts.len(),
            "note_count": total_notes,
            "musical_duration_seconds": plan.musical_duration_seconds,
            "ticks_per_beat": plan.score.ticks_per_beat,
            "length_ticks": plan.length_ticks,
            "sections": sections,
            "instruments": instruments,
        },
        "diagnostics": plan.diagnostics,
    })
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
        let mut entry = json!({"section":name,"bars":count,"tonic":tonic,"meter":format!("{}/{}",meter.numerator,meter.denominator),"bpm":bpm});
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
    if let Some(summary) = report["summary"].as_object() {
        println!("{}", report["title"].as_str().expect("title"));
        println!(
            "{} sections, {} instruments, {} notes",
            summary["section_count"], summary["instrument_count"], summary["note_count"]
        );
        println!(
            "duration: {:.3} seconds; {} ticks at {} PPQ",
            summary["musical_duration_seconds"]
                .as_f64()
                .expect("duration"),
            summary["length_ticks"],
            summary["ticks_per_beat"]
        );
        for s in summary["sections"].as_array().expect("sections") {
            let scale = s["scale"]
                .as_str()
                .map_or_else(String::new, |name| format!(" {name}"));
            println!(
                "  #{} {}: {} bars, {}{}, {} BPM, {}, {} notes (ticks {}..{})",
                s["occurrence"],
                s["section"].as_str().expect("name"),
                s["bars"],
                s["tonic"].as_str().expect("tonic"),
                scale,
                s["bpm"],
                s["meter"].as_str().expect("meter"),
                s["note_count"],
                s["start_tick"],
                s["end_tick"]
            );
        }
        for part in summary["instruments"].as_array().expect("instruments") {
            let id = part["id"].as_str().expect("id");
            match (
                part["note_min"].as_u64(),
                part["note_max"].as_u64(),
                part["velocity_min"].as_u64(),
                part["velocity_max"].as_u64(),
            ) {
                (Some(min), Some(max), Some(velocity_min), Some(velocity_max)) => println!(
                    "  {id}: {} notes, MIDI {min}..{max}, velocity {velocity_min}..{velocity_max}, {} control events",
                    part["note_count"], part["control_event_count"]
                ),
                _ => println!(
                    "  {id}: {} notes, {} control events",
                    part["note_count"], part["control_event_count"]
                ),
            }
        }
        return;
    }
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
                let scale = s["scale"]
                    .as_str()
                    .map_or_else(String::new, |name| format!(" {name}"));
                println!(
                    "  {}: {} bars, {}{}, {} BPM, {} (ticks {}..{})",
                    s["section"].as_str().expect("name"),
                    s["bars"],
                    s["tonic"].as_str().expect("tonic"),
                    scale,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_aggregates_repeated_sections_and_instruments() {
        let directory = tempfile::tempdir().expect("directory");
        let instrument = directory.path().join("instrument.json");
        std::fs::write(
            &instrument,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../presets/BASS/001-clean-sub-bass/definition.json"
            )),
        )
        .expect("instrument");
        let score_path = directory.path().join("score.yaml");
        std::fs::write(
            &score_path,
            "meta: {title: Summary, bpm: 120, meter: 4/4, tuning: 12edo, gate: 1}\n\
             form:\n\
             \x20 - {section: phrase, bars: 1, tonic: C, scale: ionian}\n\
             \x20 - {section: phrase, bars: 1, tonic: C}\n\
             \x20 - {section: answer, bars: 1, tonic: C}\n\
             sections:\n\
             \x20 phrase:\n\
             \x20   placements:\n\
             \x20     - {instrument: a, degree: \"1\", at: {bar: 1, step: 1}, duration_quarters: 1}\n\
             \x20     - {instrument: b, degree: \"3\", at: {bar: 1, step: 3}, duration_quarters: 1}\n\
             \x20 answer:\n\
             \x20   placements:\n\
             \x20     - {instrument: a, degree: \"5\", at: {bar: 1, step: 1}, duration_quarters: 1}\n\
             instruments:\n\
             \x20 a: {definition: instrument.json}\n\
             \x20 b: {definition: instrument.json}\n",
        )
        .expect("score");
        let report = execute(ScoreCommand::Inspect(InspectArgs {
            score: ScoreArgs {
                score: score_path.clone(),
                bindings: Bindings::default(),
                json: false,
            },
            preview_json: false,
            summary: true,
        }))
        .expect("summary report");
        let detailed = execute(ScoreCommand::Inspect(InspectArgs {
            score: ScoreArgs {
                score: score_path,
                bindings: Bindings::default(),
                json: false,
            },
            preview_json: false,
            summary: false,
        }))
        .expect("detailed report");
        let summary = report["summary"].as_object().expect("summary object");
        assert_eq!(summary["section_count"], 3);
        assert_eq!(summary["instrument_count"], 2);
        assert_eq!(summary["note_count"], 5);
        let sections = summary["sections"].as_array().expect("sections");
        let entries = sections
            .iter()
            .map(|s| {
                (
                    s["section"].as_str().expect("name"),
                    s["occurrence"].as_i64().expect("occurrence"),
                    s["note_count"].as_i64().expect("notes"),
                    s.get("scale").and_then(|scale| scale.as_str()),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            [
                ("phrase", 1, 2, Some("ionian")),
                ("phrase", 2, 2, None),
                ("answer", 3, 1, None),
            ]
        );
        let notes = summary["instruments"]
            .as_array()
            .expect("instruments")
            .iter()
            .map(|part| part["note_count"].as_i64().expect("notes"))
            .sum::<i64>();
        assert_eq!(notes, summary["note_count"]);
        assert_eq!(report["diagnostics"], detailed["diagnostics"]);
        assert!(
            report["diagnostics"]
                .as_array()
                .is_some_and(|diagnostics| !diagnostics.is_empty()),
            "the summary carries the resolution diagnostics: {report}"
        );
    }
}
