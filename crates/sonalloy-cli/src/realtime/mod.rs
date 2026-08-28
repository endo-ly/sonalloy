use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cpal::traits::StreamTrait;
use crossbeam_queue::ArrayQueue;
use midir::MidiInputConnection;
use sonalloy_core::{
    Diagnostic, DiagnosticCode, InstrumentProcessor, ParameterHandle, ProcessSpec, TimeSignature,
    seconds_to_frames,
};

use super::{
    AuditionMidiArgs, AuditionPatternArgs, CliFailure, PlayArgs, finish_failure, load_and_compile,
    load_pattern, print_warnings,
};

mod audio;
mod device;
mod midi;
mod scheduled;

pub(crate) const DEFAULT_BUFFER_SIZE: usize = 256;
const INPUT_STARTUP_DEADLINE: Duration = Duration::from_secs(1);

struct LiveSession {
    _stream: cpal::Stream,
    _input_stream: Option<cpal::Stream>,
    _midi: MidiInputConnection<midi::LiveMidiState>,
    status: Arc<audio::RealtimeStatus>,
}

pub(crate) fn run_device_list(json: bool) -> ExitCode {
    match device::inventory() {
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

pub(crate) fn run_play(args: &PlayArgs) -> ExitCode {
    if let Err(failure) = validate_play_args(args) {
        return finish_failure(false, failure);
    }
    let session = match start_play(args) {
        Ok(session) => session,
        Err(failure) => return finish_failure(false, failure),
    };
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
    while status.fatal() == audio::FatalStatus::None {
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
    if fatal == audio::FatalStatus::None {
        let _ = stop_thread.join();
        ExitCode::SUCCESS
    } else {
        drop(stop_thread);
        let Some(diagnostic) = fatal.diagnostic() else {
            return ExitCode::SUCCESS;
        };
        finish_failure(
            false,
            CliFailure {
                code: 3,
                diagnostics: vec![diagnostic],
            },
        )
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
        if cc == crate::midi_common::MOD_WHEEL_CONTROLLER
            || cc == crate::midi_common::SUSTAIN_PEDAL_CONTROLLER
        {
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

pub(crate) fn run_audition_pattern(args: &AuditionPatternArgs) -> ExitCode {
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
    run_scheduled_audition(
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
    )
}

pub(crate) fn run_audition_midi(args: &AuditionMidiArgs) -> ExitCode {
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
    run_scheduled_audition(
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
    )
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

fn select_audio_input(
    requested_id: Option<&str>,
    required_channels: usize,
    sample_rate: u32,
    buffer_size: usize,
) -> Result<Option<device::SelectedAudioInputDevice>, CliFailure> {
    if required_channels == 0 {
        if requested_id.is_some() {
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::AudioDeviceError,
                    "the instrument does not use external audio input",
                )],
            });
        }
        return Ok(None);
    }
    device::select_audio_input(requested_id, sample_rate, buffer_size, required_channels)
        .map(Some)
        .map_err(|error| CliFailure {
            code: 2,
            diagnostics: vec![error.diagnostic],
        })
}

struct ScheduledAuditionOptions<'a> {
    definition_path: &'a std::path::Path,
    audio_device: Option<&'a str>,
    audio_input_device: Option<&'a str>,
    requested_sample_rate: Option<u32>,
    buffer_size: usize,
    tail: f64,
    looping: bool,
}

#[allow(clippy::too_many_lines)]
fn run_scheduled_audition(
    pattern: &crate::pattern::PatternDefinition,
    mut diagnostics: Vec<Diagnostic>,
    options: &ScheduledAuditionOptions<'_>,
) -> ExitCode {
    let selected_audio = match device::select_audio(
        options.audio_device,
        options.requested_sample_rate,
        options.buffer_size,
    ) {
        Ok(device) => device,
        Err(error) => {
            return finish_failure(
                false,
                CliFailure {
                    code: 2,
                    diagnostics: vec![error.diagnostic],
                },
            );
        }
    };
    let sample_rate = selected_audio.config.sample_rate();
    let (compiled, compile_diagnostics) =
        match load_and_compile(options.definition_path, sample_rate, options.buffer_size) {
            Ok(result) => result,
            Err(failure) => return finish_failure(false, failure),
        };
    diagnostics.extend(compile_diagnostics);
    let input_channels = compiled.required_input_channels();
    let selected_input = match select_audio_input(
        options.audio_input_device,
        input_channels,
        sample_rate,
        options.buffer_size,
    ) {
        Ok(input) => input,
        Err(failure) => return finish_failure(false, failure),
    };
    let compiled_pattern = match crate::pattern::compile(pattern, &compiled, f64::from(sample_rate))
    {
        Ok(pattern) => pattern,
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
    let tail_frames = match seconds_to_frames(options.tail, f64::from(sample_rate)) {
        Ok(frames) => frames,
        Err(error) => {
            return finish_failure(
                false,
                CliFailure {
                    code: 2,
                    diagnostics: vec![Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        error.to_string(),
                    )],
                },
            );
        }
    };
    let reported_latency_frames = compiled.reported_latency_frames;
    let feed = match scheduled::ScheduledEventFeed::new(
        compiled_pattern,
        tail_frames,
        reported_latency_frames,
        options.looping,
        f64::from(sample_rate),
    ) {
        Ok(feed) => feed,
        Err(error) => {
            return finish_failure(
                false,
                CliFailure {
                    code: 2,
                    diagnostics: vec![
                        Diagnostic::error(
                            DiagnosticCode::ValueOutOfRange,
                            "could not prepare scheduled pattern playback",
                        )
                        .with_detail(error.to_string()),
                    ],
                },
            );
        }
    };
    let process_spec = match ProcessSpec::new(
        f64::from(sample_rate),
        options.buffer_size,
        input_channels,
        2,
    ) {
        Ok(spec) => spec,
        Err(error) => {
            return finish_failure(
                false,
                CliFailure {
                    code: 2,
                    diagnostics: vec![
                        Diagnostic::error(
                            DiagnosticCode::ProcessError,
                            "could not prepare the realtime processor",
                        )
                        .with_detail(error.to_string()),
                    ],
                },
            );
        }
    };
    let mut runtime = compiled.instantiate();
    if let Err(error) = runtime.prepare(process_spec) {
        return finish_failure(
            false,
            CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::ProcessError,
                        "could not prepare the realtime processor",
                    )
                    .with_detail(error.to_string()),
                ],
            },
        );
    }
    let status = Arc::new(audio::RealtimeStatus::new());
    let streams = match audio::build_scheduled_stream(
        &selected_audio,
        selected_input.as_ref(),
        input_channels,
        runtime,
        feed,
        &status,
        options.buffer_size,
    ) {
        Ok(stream) => stream,
        Err(error) => {
            return finish_failure(
                false,
                CliFailure {
                    code: 2,
                    diagnostics: vec![error.diagnostic],
                },
            );
        }
    };
    if let Err(failure) = start_input_stream(streams.input.as_ref(), &status, options.buffer_size) {
        return finish_failure(false, failure);
    }
    if let Err(error) = streams.output.play() {
        return finish_failure(
            false,
            CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::AudioDeviceError,
                        "could not start the audio output stream",
                    )
                    .with_detail(error.to_string()),
                ],
            },
        );
    }
    println!("Audition: {}", compiled.metadata.name);
    println!("Audio: {} [{}]", selected_audio.name, selected_audio.id);
    if let Some(input) = selected_input.as_ref() {
        println!("Audio input: {} [{}]", input.name, input.id);
    }
    println!("Sample rate: {sample_rate} Hz");
    println!("Device channels: {}", selected_audio.config.channels());
    println!("Requested buffer: {} frames", options.buffer_size);
    println!("Engine latency: {reported_latency_frames} frames");
    if options.looping {
        println!("press Enter to stop");
    } else {
        println!("playing one-shot pattern");
    }
    print_warnings(&diagnostics);

    let session = ScheduledSession {
        _stream: streams.output,
        _input_stream: streams.input,
    };
    let fatal = if options.looping {
        wait_for_enter(&status);
        status.fatal()
    } else {
        while !status.finished() && status.fatal() == audio::FatalStatus::None {
            std::thread::sleep(Duration::from_millis(10));
        }
        status.fatal()
    };
    drop(session);
    print_realtime_status(&status);
    if fatal == audio::FatalStatus::None {
        ExitCode::SUCCESS
    } else {
        let diagnostic = fatal
            .diagnostic()
            .expect("a non-none fatal status has a diagnostic");
        finish_failure(
            false,
            CliFailure {
                code: 3,
                diagnostics: vec![diagnostic],
            },
        )
    }
}

struct ScheduledSession {
    _stream: cpal::Stream,
    _input_stream: Option<cpal::Stream>,
}

fn wait_for_enter(status: &audio::RealtimeStatus) {
    let (stop_sender, stop_receiver) = mpsc::channel();
    let stop_thread = std::thread::spawn(move || {
        let mut input = String::new();
        let result = std::io::stdin().read_line(&mut input);
        let _ = stop_sender.send(result.is_ok());
    });
    while status.fatal() == audio::FatalStatus::None {
        match stop_receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    drop(stop_thread);
}

fn print_realtime_status(status: &audio::RealtimeStatus) {
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

#[allow(clippy::too_many_lines)]
fn start_play(args: &PlayArgs) -> Result<LiveSession, CliFailure> {
    let selected_audio = device::select_audio(
        args.audio_device.as_deref(),
        args.sample_rate,
        args.buffer_size,
    )
    .map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![error.diagnostic],
    })?;
    let selected_midi =
        device::select_midi(args.midi_device.as_deref()).map_err(|error| CliFailure {
            code: 2,
            diagnostics: vec![error.diagnostic],
        })?;
    let sample_rate = selected_audio.config.sample_rate();
    let (compiled, diagnostics) =
        load_and_compile(&args.definition, sample_rate, args.buffer_size)?;
    let input_channels = compiled.required_input_channels();
    let selected_input = select_audio_input(
        args.audio_input_device.as_deref(),
        input_channels,
        sample_rate,
        args.buffer_size,
    )?;
    let instrument_name = compiled.metadata.name.clone();
    let reported_latency_frames = compiled.reported_latency_frames;
    let time_signature =
        parse_time_signature(&args.time_signature).map_err(|error| CliFailure {
            code: 2,
            diagnostics: vec![
                Diagnostic::error(DiagnosticCode::ValueOutOfRange, error)
                    .with_path("time_signature"),
            ],
        })?;
    let macro_cc = parse_macro_cc(&args.macro_cc, &compiled)?;
    let sample_format = device::sample_format_name(selected_audio.config.sample_format());
    let mut runtime = compiled.instantiate();
    let process_spec =
        ProcessSpec::new(f64::from(sample_rate), args.buffer_size, input_channels, 2).map_err(
            |error| CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::ProcessError,
                        "could not prepare the realtime processor",
                    )
                    .with_detail(error.to_string()),
                ],
            },
        )?;
    runtime.prepare(process_spec).map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::ProcessError,
                "could not prepare the realtime processor",
            )
            .with_detail(error.to_string()),
        ],
    })?;

    let events = Arc::new(ArrayQueue::new(audio::REALTIME_EVENT_QUEUE_CAPACITY));
    let status = Arc::new(audio::RealtimeStatus::new());
    let audio_name = selected_audio.name.clone();
    let audio_id = selected_audio.id.clone();
    let midi_name = selected_midi.name.clone();
    let midi_id = selected_midi.id.clone();
    let streams = audio::build_stream(
        &selected_audio,
        selected_input.as_ref(),
        input_channels,
        runtime,
        events.clone(),
        &status,
        args.buffer_size,
        args.tempo,
        time_signature,
    )
    .map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![error.diagnostic],
    })?;
    start_input_stream(streams.input.as_ref(), &status, args.buffer_size)?;
    let midi_connection =
        midi::connect(selected_midi, events, status.clone(), &macro_cc).map_err(|error| {
            CliFailure {
                code: 2,
                diagnostics: vec![error.diagnostic],
            }
        })?;
    streams.output.play().map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::AudioDeviceError,
                "could not start the audio output stream",
            )
            .with_detail(error.to_string()),
        ],
    })?;
    println!("Instrument: {instrument_name}");
    println!("Audio: {audio_name} [{audio_id}]");
    if let Some(input) = selected_input.as_ref() {
        println!("Audio input: {} [{}]", input.name, input.id);
    }
    println!("Sample rate: {sample_rate} Hz");
    println!("Device channels: {}", selected_audio.config.channels());
    println!("Sample format: {sample_format}");
    println!("Requested buffer: {} frames", args.buffer_size);
    println!("Callback frames: measured at shutdown");
    let latency_frames_for_display = u32::try_from(reported_latency_frames).unwrap_or(u32::MAX);
    let latency_ms = f64::from(latency_frames_for_display) * 1_000.0 / f64::from(sample_rate);
    println!("Engine latency: {reported_latency_frames} frames ({latency_ms:.3} ms)");
    println!("MIDI: {midi_name} [{midi_id}]");
    println!("Tempo: {} BPM", args.tempo);
    println!("Time signature: {}", args.time_signature);
    print_warnings(&diagnostics);
    Ok(LiveSession {
        _stream: streams.output,
        _input_stream: streams.input,
        _midi: midi_connection,
        status,
    })
}

fn print_device_inventory(report: &device::DeviceInventoryReport) {
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

fn start_input_stream(
    stream: Option<&cpal::Stream>,
    status: &Arc<audio::RealtimeStatus>,
    startup_frames: usize,
) -> Result<(), CliFailure> {
    let Some(stream) = stream else {
        return Ok(());
    };
    stream.play().map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![
            Diagnostic::error(
                DiagnosticCode::AudioDeviceError,
                "could not start the audio input stream",
            )
            .with_detail(error.to_string()),
        ],
    })?;
    let deadline = Instant::now() + INPUT_STARTUP_DEADLINE;
    while !status.input_ready() && status.fatal() == audio::FatalStatus::None {
        if Instant::now() >= deadline {
            status.set_fatal(audio::FatalStatus::Input);
            return Err(CliFailure {
                code: 2,
                diagnostics: vec![Diagnostic::error(
                    DiagnosticCode::AudioDeviceError,
                    format!(
                        "audio input did not provide {startup_frames} frames before the startup deadline"
                    ),
                )],
            });
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if status.fatal() == audio::FatalStatus::None {
        Ok(())
    } else {
        Err(CliFailure {
            code: 3,
            diagnostics: vec![
                status
                    .fatal()
                    .diagnostic()
                    .expect("fatal status has a diagnostic"),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let result = sonalloy_core::compile_instrument(
            &definition,
            &sonalloy_core::CompileContext {
                definition_base_dir: path.parent().expect("fixture directory").to_path_buf(),
                process_spec: ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid spec"),
            },
        );
        result.instrument.expect("fixture compiles")
    }

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

    #[test]
    fn device_report_serialization_preserves_opaque_ids_and_unknown_buffer_size() {
        let report = device::DeviceInventoryReport {
            audio_inputs: Vec::new(),
            audio_outputs: vec![device::AudioDeviceReport {
                id: "host:opaque-output".to_owned(),
                name: "Output".to_owned(),
                is_default: true,
                default_config: Some(device::AudioConfigReport {
                    sample_rate: 48_000,
                    channels: 2,
                    sample_format: "f32".to_owned(),
                    buffer_size: None,
                }),
            }],
            midi_inputs: vec![device::MidiDeviceReport {
                id: "opaque-midi".to_owned(),
                name: "Keyboard".to_owned(),
            }],
        };

        let value = serde_json::to_value(report).expect("device report serializes");

        assert_eq!(value["audio_outputs"][0]["id"], "host:opaque-output");
        assert_eq!(value["audio_outputs"][0]["default"], true);
        assert!(value["audio_outputs"][0]["default_config"]["buffer_size"].is_null());
        assert_eq!(value["midi_inputs"][0]["id"], "opaque-midi");
    }
}
