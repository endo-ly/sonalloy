use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use clap::{Args, Subcommand};
use sonalloy_core::{
    DEFAULT_TEMPO_BPM, Diagnostic, DiagnosticCode, ParameterHandle, TimeSignature,
};

use crate::command::pattern::load_pattern;
use crate::output::{CliFailure, finish_failure, print_warnings};
use crate::realtime::{PlayOptions, ScheduledAuditionOptions};
#[derive(Debug, Subcommand)]
pub(super) enum AuditionCommand {
    /// Play a pattern through an audio output.
    Pattern(AuditionPatternArgs),
    /// Convert and play one MIDI channel through an audio output.
    Midi(AuditionMidiArgs),
}

#[derive(Debug, Subcommand)]
pub(super) enum DeviceCommand {
    /// List available audio outputs and MIDI inputs.
    List(DeviceListArgs),
}

#[derive(Debug, Args)]
pub(super) struct DeviceListArgs {
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct PlayArgs {
    /// Definition JSON path.
    pub(crate) definition: PathBuf,
    /// CPAL output device ID. The OS default is used when omitted.
    #[arg(long)]
    pub(crate) audio_device: Option<String>,
    /// CPAL input device ID. The OS default is used when external audio is required.
    #[arg(long)]
    pub(crate) audio_input_device: Option<String>,
    /// Midir input port ID. A single available port is selected automatically.
    #[arg(long)]
    pub(crate) midi_device: Option<String>,
    /// Requested output sample rate. The device default is used when omitted.
    #[arg(long)]
    pub(crate) sample_rate: Option<u32>,
    /// Requested callback buffer size in frames.
    #[arg(long, default_value_t = crate::realtime::DEFAULT_BUFFER_SIZE)]
    pub(crate) buffer_size: usize,
    /// Constant tempo supplied to the Core process context.
    #[arg(long, default_value_t = DEFAULT_TEMPO_BPM)]
    pub(crate) tempo: f64,
    /// Time signature supplied to the Core process context, for example 4/4.
    #[arg(long, default_value = "4/4")]
    pub(crate) time_signature: String,
    /// Map a macro identifier to a MIDI CC number; may be repeated.
    #[arg(long = "macro-cc")]
    pub(crate) macro_cc: Vec<String>,
}

#[derive(Debug, Args)]
pub(super) struct AuditionPatternArgs {
    /// Definition JSON path.
    pub(crate) definition: PathBuf,
    /// Musical-time pattern JSON path.
    pub(crate) pattern: PathBuf,
    /// CPAL output device ID. The OS default is used when omitted.
    #[arg(long)]
    pub(crate) audio_device: Option<String>,
    /// CPAL input device ID. The OS default is used when external audio is required.
    #[arg(long)]
    pub(crate) audio_input_device: Option<String>,
    /// Requested output sample rate. The device default is used when omitted.
    #[arg(long)]
    pub(crate) sample_rate: Option<u32>,
    /// Requested callback buffer size in frames.
    #[arg(long, default_value_t = crate::realtime::DEFAULT_BUFFER_SIZE)]
    pub(crate) buffer_size: usize,
    /// Additional tail in seconds for one-shot playback.
    #[arg(long, default_value_t = 1.0)]
    pub(crate) tail: f64,
    /// Repeat the pattern until Enter is pressed.
    #[arg(long)]
    pub(crate) r#loop: bool,
}

#[derive(Debug, Args)]
pub(super) struct AuditionMidiArgs {
    /// Definition JSON path.
    pub(crate) definition: PathBuf,
    /// Standard MIDI File path.
    pub(crate) midi: PathBuf,
    /// MIDI channel number from 1 to 16.
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=16))]
    pub(crate) channel: Option<u8>,
    /// CPAL output device ID. The OS default is used when omitted.
    #[arg(long)]
    pub(crate) audio_device: Option<String>,
    /// CPAL input device ID. The OS default is used when external audio is required.
    #[arg(long)]
    pub(crate) audio_input_device: Option<String>,
    /// Requested output sample rate. The device default is used when omitted.
    #[arg(long)]
    pub(crate) sample_rate: Option<u32>,
    /// Requested callback buffer size in frames.
    #[arg(long, default_value_t = crate::realtime::DEFAULT_BUFFER_SIZE)]
    pub(crate) buffer_size: usize,
    /// Additional tail in seconds for one-shot playback.
    #[arg(long, default_value_t = 1.0)]
    pub(crate) tail: f64,
}

fn run_device_list(json: bool) -> ExitCode {
    match crate::realtime::inventory() {
        Ok(report) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("device report is serializable")
                );
            } else {
                print_device_inventory(&report);
            }
            ExitCode::SUCCESS
        }
        Err(error) => finish_failure(
            json,
            CliFailure {
                code: 2,
                diagnostics: vec![error.diagnostic],
            },
        ),
    }
}

fn validate_play_args(args: &PlayArgs) -> Result<(), CliFailure> {
    let mut diagnostics = Vec::new();
    if !args.tempo.is_finite() || args.tempo <= 0.0 {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "tempo must be finite and greater than zero",
        ));
    }
    if args.buffer_size == 0 {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "buffer size must be greater than zero",
        ));
    }
    if args.sample_rate == Some(0) {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "sample rate must be greater than zero",
        ));
    }
    if parse_time_signature(&args.time_signature).is_err() {
        diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "time signature must use numerator/denominator notation",
            )
            .with_path("time_signature"),
        );
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(CliFailure {
            code: 2,
            diagnostics,
        })
    }
}

fn parse_time_signature(value: &str) -> Result<TimeSignature, &'static str> {
    let (numerator, denominator) = value
        .split_once('/')
        .ok_or("time signature must contain /")?;
    let numerator = numerator.parse::<u16>().map_err(|_| "invalid numerator")?;
    let denominator = denominator
        .parse::<u16>()
        .map_err(|_| "invalid denominator")?;
    let signature = TimeSignature {
        numerator,
        denominator,
    };
    signature
        .is_valid()
        .then_some(signature)
        .ok_or("invalid time signature")
}

fn parse_macro_cc(
    values: &[String],
    compiled: &sonalloy_core::CompiledInstrument,
) -> Result<[Option<ParameterHandle>; 128], CliFailure> {
    let mut mapping = [None; 128];
    let mut diagnostics = Vec::new();
    for value in values {
        let Some((macro_id, cc)) = value.split_once('=') else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "macro CC must use id=number",
                )
                .with_path("macro_cc"),
            );
            continue;
        };
        let Ok(cc) = cc.parse::<u8>() else {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::ValueOutOfRange, "macro CC must be 0..127")
                    .with_path("macro_cc"),
            );
            continue;
        };
        if cc > 127 {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::ValueOutOfRange, "macro CC must be 0..127")
                    .with_path("macro_cc"),
            );
            continue;
        }
        if cc == crate::midi::MOD_WHEEL_CONTROLLER || cc == crate::midi::SUSTAIN_PEDAL_CONTROLLER {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    "CC1 and CC64 are reserved for their standard MIDI meanings",
                )
                .with_path("macro_cc"),
            );
            continue;
        }
        let parameter_id = format!("macro.{macro_id}");
        let Some(parameter) = compiled.parameter_handle(&parameter_id) else {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::ParameterIdInvalid, "macro is not defined")
                    .with_path("macro_cc")
                    .with_detail(parameter_id),
            );
            continue;
        };
        if mapping[usize::from(cc)].is_some() {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::ValueOutOfRange, "a MIDI CC is mapped twice")
                    .with_path("macro_cc"),
            );
            continue;
        }
        if mapping.iter().flatten().any(|mapped| *mapped == parameter) {
            diagnostics.push(
                Diagnostic::error(DiagnosticCode::ValueOutOfRange, "a macro is mapped twice")
                    .with_path("macro_cc"),
            );
            continue;
        }
        mapping[usize::from(cc)] = Some(parameter);
    }
    if diagnostics.is_empty() {
        Ok(mapping)
    } else {
        Err(CliFailure {
            code: 2,
            diagnostics,
        })
    }
}

fn run_audition_pattern(args: &AuditionPatternArgs) -> ExitCode {
    if let Err(failure) = validate_audition_args(args.sample_rate, args.buffer_size, args.tail) {
        return finish_failure(false, failure);
    }
    let pattern = match load_pattern(&args.pattern) {
        Ok(pattern) => pattern,
        Err(failure) => return finish_failure(false, failure),
    };
    let pattern_diagnostics = crate::pattern::validate(&pattern);
    if !pattern_diagnostics.is_empty() {
        return finish_failure(
            false,
            CliFailure {
                code: 2,
                diagnostics: pattern_diagnostics,
            },
        );
    }
    let session = match crate::realtime::start_scheduled_audition(
        &pattern,
        Vec::new(),
        &ScheduledAuditionOptions {
            definition_path: &args.definition,
            audio_device: args.audio_device.as_deref(),
            audio_input_device: args.audio_input_device.as_deref(),
            requested_sample_rate: args.sample_rate,
            buffer_size: args.buffer_size,
            tail: args.tail,
            looping: args.r#loop,
        },
        |path, sample_rate, block_size| {
            super::load_and_compile(path, sample_rate, block_size)
                .map_err(|failure| (failure.code, failure.diagnostics))
        },
    ) {
        Ok(session) => session,
        Err((code, diagnostics)) => return finish_failure(false, CliFailure { code, diagnostics }),
    };
    println!("Audition: {}", session.instrument_name);
    println!("Audio: {} [{}]", session.audio_name, session.audio_id);
    if let (Some(name), Some(id)) = (&session.input_name, &session.input_id) {
        println!("Audio input: {name} [{id}]");
    }
    println!("Sample rate: {} Hz", session.sample_rate);
    println!("Device channels: {}", session.device_channels);
    println!("Requested buffer: {} frames", session.buffer_size);
    println!("Engine latency: {} frames", session.reported_latency_frames);
    if args.r#loop {
        println!("press Enter to stop");
    } else {
        println!("playing one-shot pattern");
    }
    print_warnings(&session.diagnostics);
    finish_scheduled_session(session, args.r#loop)
}

fn run_audition_midi(args: &AuditionMidiArgs) -> ExitCode {
    if let Err(failure) = validate_audition_args(args.sample_rate, args.buffer_size, args.tail) {
        return finish_failure(false, failure);
    }
    let parsed = match crate::midi::parse_midi(&args.midi) {
        Ok(parsed) => parsed,
        Err(diagnostics) => {
            return finish_failure(
                false,
                CliFailure {
                    code: 2,
                    diagnostics,
                },
            );
        }
    };
    let (pattern, diagnostics) =
        match crate::midi::import_pattern(parsed, args.channel.map(|channel| channel - 1)) {
            Ok(result) => result,
            Err(diagnostics) => {
                return finish_failure(
                    false,
                    CliFailure {
                        code: 2,
                        diagnostics,
                    },
                );
            }
        };
    let session = match crate::realtime::start_scheduled_audition(
        &pattern,
        diagnostics,
        &ScheduledAuditionOptions {
            definition_path: &args.definition,
            audio_device: args.audio_device.as_deref(),
            audio_input_device: args.audio_input_device.as_deref(),
            requested_sample_rate: args.sample_rate,
            buffer_size: args.buffer_size,
            tail: args.tail,
            looping: false,
        },
        |path, sample_rate, block_size| {
            super::load_and_compile(path, sample_rate, block_size)
                .map_err(|failure| (failure.code, failure.diagnostics))
        },
    ) {
        Ok(session) => session,
        Err((code, diagnostics)) => return finish_failure(false, CliFailure { code, diagnostics }),
    };
    println!("Audition: {}", session.instrument_name);
    println!("Audio: {} [{}]", session.audio_name, session.audio_id);
    if let (Some(name), Some(id)) = (&session.input_name, &session.input_id) {
        println!("Audio input: {name} [{id}]");
    }
    println!("Sample rate: {} Hz", session.sample_rate);
    println!("Device channels: {}", session.device_channels);
    println!("Requested buffer: {} frames", session.buffer_size);
    println!("Engine latency: {} frames", session.reported_latency_frames);
    println!("playing one-shot pattern");
    print_warnings(&session.diagnostics);
    finish_scheduled_session(session, false)
}

fn validate_audition_args(
    sample_rate: Option<u32>,
    buffer_size: usize,
    tail: f64,
) -> Result<(), CliFailure> {
    let mut diagnostics = Vec::new();
    if sample_rate == Some(0) {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "sample rate must be greater than zero",
        ));
    }
    if buffer_size == 0 {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "buffer size must be greater than zero",
        ));
    }
    if !tail.is_finite() || tail < 0.0 {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::ValueOutOfRange,
            "tail must be finite and non-negative",
        ));
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(CliFailure {
            code: 2,
            diagnostics,
        })
    }
}

fn print_device_inventory(report: &crate::realtime::DeviceInventoryReport) {
    println!("Audio inputs");
    if report.audio_inputs.is_empty() {
        println!("  none");
    } else {
        for audio in &report.audio_inputs {
            let marker = if audio.is_default { " (default)" } else { "" };
            println!("  {}{marker}", audio.name);
            println!("    id: {}", audio.id);
            if let Some(config) = &audio.default_config {
                println!(
                    "    default: {} Hz, {} channels, {}",
                    config.sample_rate, config.channels, config.sample_format
                );
            }
        }
    }
    println!("Audio outputs");
    if report.audio_outputs.is_empty() {
        println!("  none");
    } else {
        for audio in &report.audio_outputs {
            let marker = if audio.is_default { " (default)" } else { "" };
            println!("  {}{marker}", audio.name);
            println!("    id: {}", audio.id);
            if let Some(config) = &audio.default_config {
                println!(
                    "    default: {} Hz, {} channels, {}",
                    config.sample_rate, config.channels, config.sample_format
                );
                if let Some(buffer) = &config.buffer_size {
                    println!("    buffer: {}..={} frames", buffer.min, buffer.max);
                }
            }
        }
    }
    println!("MIDI inputs");
    if report.midi_inputs.is_empty() {
        println!("  none");
    } else {
        for midi in &report.midi_inputs {
            println!("  {}", midi.name);
            println!("    id: {}", midi.id);
        }
    }
}

fn finish_scheduled_session(session: crate::realtime::ScheduledSession, looping: bool) -> ExitCode {
    let status = session.status.clone();
    if looping {
        wait_for_enter(&status);
    } else {
        while !status.finished() && status.fatal() == crate::realtime::FatalStatus::None {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let fatal = status.fatal();
    drop(session);
    print_realtime_status(&status);
    match fatal {
        crate::realtime::FatalStatus::None => ExitCode::SUCCESS,
        _ => finish_failure(
            false,
            CliFailure {
                code: 3,
                diagnostics: vec![
                    fatal
                        .diagnostic()
                        .expect("a non-none fatal status has a diagnostic"),
                ],
            },
        ),
    }
}

fn wait_for_enter(status: &crate::realtime::RealtimeStatus) {
    let (stop_sender, stop_receiver) = mpsc::channel();
    let stop_thread = std::thread::spawn(move || {
        let mut input = String::new();
        let result = std::io::stdin().read_line(&mut input);
        let _ = stop_sender.send(result.is_ok());
    });
    while status.fatal() == crate::realtime::FatalStatus::None {
        match stop_receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    drop(stop_thread);
}

fn print_realtime_status(status: &crate::realtime::RealtimeStatus) {
    let xruns = status.xrun_count();
    let input_underflows = status.input_underflow_count();
    let input_overflows = status.input_overflow_count();
    if status.realtime_denied() {
        eprintln!("warning: realtime scheduling was denied by the audio backend");
    }
    if xruns > 0 {
        eprintln!("warning: audio xruns: {xruns}");
    }
    if input_underflows > 0 {
        eprintln!("warning: audio input underruns: {input_underflows}");
    }
    if input_overflows > 0 {
        eprintln!("warning: audio input overflows: {input_overflows}");
    }
    let callback_frames = status.callback_frame_stats();
    println!("XRuns: {xruns}");
    println!("Audio input underruns: {input_underflows}");
    println!("Audio input overflows: {input_overflows}");
    println!(
        "Realtime priority warning: {}",
        if status.realtime_denied() {
            "denied"
        } else {
            "none"
        }
    );
    match (callback_frames.min, callback_frames.max) {
        (Some(min), Some(max)) => println!(
            "Callback frames: {min}..={max} ({} callbacks)",
            callback_frames.count
        ),
        _ => println!("Callback frames: none"),
    }
}

pub(super) fn run_audition(command: AuditionCommand) -> ExitCode {
    match command {
        AuditionCommand::Pattern(args) => run_audition_pattern(&args),
        AuditionCommand::Midi(args) => run_audition_midi(&args),
    }
}

pub(super) fn run_device(command: DeviceCommand) -> ExitCode {
    match command {
        DeviceCommand::List(args) => run_device_list(args.json),
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn run_play(args: &PlayArgs) -> ExitCode {
    if let Err(failure) = validate_play_args(args) {
        return finish_failure(false, failure);
    }
    let time_signature =
        parse_time_signature(&args.time_signature).expect("validated time signature");
    let session = match crate::realtime::start_play(
        &PlayOptions {
            definition_path: &args.definition,
            audio_device: args.audio_device.as_deref(),
            audio_input_device: args.audio_input_device.as_deref(),
            midi_device: args.midi_device.as_deref(),
            sample_rate: args.sample_rate,
            buffer_size: args.buffer_size,
            tempo: args.tempo,
            time_signature,
            time_signature_display: &args.time_signature,
            macro_cc: &args.macro_cc,
        },
        |path, sample_rate, block_size| {
            super::load_and_compile(path, sample_rate, block_size)
                .map_err(|failure| (failure.code, failure.diagnostics))
        },
        |values, compiled| {
            parse_macro_cc(values, compiled).map_err(|failure| (failure.code, failure.diagnostics))
        },
    ) {
        Ok(session) => session,
        Err((code, diagnostics)) => return finish_failure(false, CliFailure { code, diagnostics }),
    };
    println!("Instrument: {}", session.instrument_name);
    println!("Audio: {} [{}]", session.audio_name, session.audio_id);
    if let (Some(name), Some(id)) = (&session.input_name, &session.input_id) {
        println!("Audio input: {name} [{id}]");
    }
    println!("Sample rate: {} Hz", session.sample_rate);
    println!("Device channels: {}", session.device_channels);
    println!("Sample format: {}", session.sample_format);
    println!("Requested buffer: {} frames", session.buffer_size);
    println!("Callback frames: measured at shutdown");
    let latency_frames_for_display =
        u32::try_from(session.reported_latency_frames).unwrap_or(u32::MAX);
    let latency_ms =
        f64::from(latency_frames_for_display) * 1_000.0 / f64::from(session.sample_rate);
    println!(
        "Engine latency: {} frames ({latency_ms:.3} ms)",
        session.reported_latency_frames
    );
    println!("MIDI: {} [{}]", session.midi_name, session.midi_id);
    println!("Tempo: {} BPM", session.tempo);
    println!("Time signature: {}", session.time_signature);
    print_warnings(&session.diagnostics);
    println!("playing {} through live MIDI", args.definition.display());
    println!("press Enter to stop");
    let status = session.status.clone();
    let (stop_sender, stop_receiver) = mpsc::channel();
    let stop_thread = std::thread::spawn(move || {
        let mut input = String::new();
        let result = std::io::stdin().read_line(&mut input);
        let _ = stop_sender.send(result.is_ok());
    });
    let mut input_error = false;
    while status.fatal() == crate::realtime::FatalStatus::None {
        match stop_receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(success) => {
                input_error = !success;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let fatal = status.fatal();
    if input_error {
        eprintln!("warning: could not read the stop command; stopping");
    }
    drop(session);
    if status.realtime_denied() {
        eprintln!("warning: realtime scheduling was denied by the audio backend");
    }
    let xruns = status.xrun_count();
    let input_underflows = status.input_underflow_count();
    let input_overflows = status.input_overflow_count();
    if xruns > 0 {
        eprintln!("warning: audio xruns: {xruns}");
    }
    if input_underflows > 0 {
        eprintln!("warning: audio input underruns: {input_underflows}");
    }
    if input_overflows > 0 {
        eprintln!("warning: audio input overflows: {input_overflows}");
    }
    let callback_frames = status.callback_frame_stats();
    println!("Stopped.");
    println!("XRuns: {xruns}");
    println!("Audio input underruns: {input_underflows}");
    println!("Audio input overflows: {input_overflows}");
    println!(
        "Realtime priority warning: {}",
        if status.realtime_denied() {
            "denied"
        } else {
            "none"
        }
    );
    match (callback_frames.min, callback_frames.max) {
        (Some(min), Some(max)) => println!(
            "Callback frames: {min}..={max} ({count} callbacks)",
            count = callback_frames.count
        ),
        _ => println!("Callback frames: none"),
    }
    if fatal == crate::realtime::FatalStatus::None {
        let _ = stop_thread.join();
        ExitCode::SUCCESS
    } else {
        drop(stop_thread);
        match fatal.diagnostic() {
            Some(diagnostic) => finish_failure(
                false,
                CliFailure {
                    code: 3,
                    diagnostics: vec![diagnostic],
                },
            ),
            None => ExitCode::SUCCESS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PlayArgs, parse_macro_cc, parse_time_signature, validate_play_args};
    use crate::realtime::DEFAULT_BUFFER_SIZE;
    use sonalloy_core::{CompileContext, ProcessSpec, TimeSignature, compile_instrument};
    use std::sync::Arc;

    fn args(tempo: f64, buffer_size: usize, sample_rate: Option<u32>) -> PlayArgs {
        PlayArgs {
            definition: "instrument.json".into(),
            audio_device: None,
            audio_input_device: None,
            midi_device: None,
            sample_rate,
            buffer_size,
            tempo,
            time_signature: "4/4".to_owned(),
            macro_cc: Vec::new(),
        }
    }

    fn compiled_with_macro() -> Arc<sonalloy_core::CompiledInstrument> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/instruments/basic-poly-synth.json");
        let text = std::fs::read_to_string(&path).expect("fixture reads");
        let mut definition: sonalloy_core::InstrumentDefinition =
            serde_json::from_str(&text).expect("fixture parses");
        definition.macros.push(sonalloy_core::MacroDefinition {
            id: "motion".to_owned(),
            name: "Motion".to_owned(),
            default: 0.0,
        });
        let result = compile_instrument(
            &definition,
            &CompileContext {
                definition_base_dir: path.parent().expect("fixture directory").to_path_buf(),
                process_spec: ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid spec"),
            },
        );
        result.instrument.expect("fixture compiles")
    }

    #[test]
    fn play_arguments_require_finite_tempo_and_positive_sizes() {
        assert!(validate_play_args(&args(120.0, DEFAULT_BUFFER_SIZE, None)).is_ok());
        assert!(validate_play_args(&args(f64::NAN, DEFAULT_BUFFER_SIZE, None)).is_err());
        assert!(validate_play_args(&args(120.0, 0, None)).is_err());
        assert!(validate_play_args(&args(120.0, DEFAULT_BUFFER_SIZE, Some(0))).is_err());
    }

    #[test]
    fn time_signature_parser_accepts_only_valid_meters() {
        assert_eq!(
            parse_time_signature("7/8").expect("valid meter"),
            TimeSignature {
                numerator: 7,
                denominator: 8,
            }
        );
        assert!(parse_time_signature("4/3").is_err());
        assert!(parse_time_signature("0/4").is_err());
        assert!(parse_time_signature("4/4/4").is_err());
    }

    #[test]
    fn macro_cc_parser_resolves_mappings_and_rejects_reserved_or_duplicate_controls() {
        let compiled = compiled_with_macro();
        let parameter = compiled
            .parameter_handle("macro.motion")
            .expect("macro parameter");
        let mapping =
            parse_macro_cc(&["motion=20".to_owned()], &compiled).expect("macro mapping parses");
        assert_eq!(mapping[20], Some(parameter));
        assert!(parse_macro_cc(&["motion=127".to_owned()], &compiled).is_ok());
        assert!(parse_macro_cc(&["motion=128".to_owned()], &compiled).is_err());
        assert!(parse_macro_cc(&["motion=255".to_owned()], &compiled).is_err());
        assert!(parse_macro_cc(&["motion=1".to_owned()], &compiled).is_err());
        assert!(
            parse_macro_cc(&["motion=20".to_owned(), "motion=21".to_owned()], &compiled).is_err()
        );
        assert!(parse_macro_cc(&["missing=22".to_owned()], &compiled).is_err());
    }
}
