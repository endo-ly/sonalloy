use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use cpal::traits::StreamTrait;
use crossbeam_queue::ArrayQueue;
use midir::MidiInputConnection;
use sonalloy_core::{Diagnostic, DiagnosticCode, InstrumentProcessor, ProcessSpec};

use super::{CliFailure, PlayArgs, finish_failure, load_and_compile, print_warnings};

mod audio;
mod device;
mod midi;

pub(crate) const DEFAULT_BUFFER_SIZE: usize = 256;

struct LiveSession {
    _stream: cpal::Stream,
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
    let (stop_sender, stop_receiver) = mpsc::channel();
    let stop_thread = std::thread::spawn(move || {
        let mut input = String::new();
        let result = std::io::stdin().read_line(&mut input);
        let _ = stop_sender.send(result.is_ok());
    });
    let mut input_error = false;
    while session.status.fatal() == audio::FatalStatus::None {
        match stop_receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(success) => {
                input_error = !success;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let fatal = session.status.fatal();
    if input_error {
        eprintln!("warning: could not read the stop command; stopping");
    }
    if session.status.realtime_denied() {
        eprintln!("warning: realtime scheduling was denied by the audio backend");
    }
    let xruns = session.status.xrun_count();
    if xruns > 0 {
        eprintln!("warning: audio xruns: {xruns}");
    }
    println!("Stopped.");
    println!("XRuns: {xruns}");
    println!(
        "Realtime priority warning: {}",
        if session.status.realtime_denied() {
            "denied"
        } else {
            "none"
        }
    );
    drop(session);
    if fatal == audio::FatalStatus::None {
        let _ = stop_thread.join();
        ExitCode::SUCCESS
    } else {
        drop(stop_thread);
        finish_failure(
            false,
            CliFailure {
                code: 3,
                diagnostics: vec![
                    Diagnostic::error(DiagnosticCode::ProcessError, "realtime session stopped")
                        .with_detail(fatal.label()),
                ],
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
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(CliFailure {
            code: 2,
            diagnostics,
        })
    }
}

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
    let instrument_name = compiled.metadata.name.clone();
    let reported_latency_frames = compiled.reported_latency_frames;
    let sample_format = device::sample_format_name(selected_audio.config.sample_format());
    let mut runtime = compiled.instantiate();
    let process_spec =
        ProcessSpec::new(f64::from(sample_rate), args.buffer_size, 2).map_err(|error| {
            CliFailure {
                code: 2,
                diagnostics: vec![
                    Diagnostic::error(
                        DiagnosticCode::ProcessError,
                        "could not prepare the realtime processor",
                    )
                    .with_detail(error.to_string()),
                ],
            }
        })?;
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
    let stream = audio::build_stream(
        &selected_audio,
        runtime,
        events.clone(),
        status.clone(),
        args.buffer_size,
        args.tempo,
    )
    .map_err(|error| CliFailure {
        code: 2,
        diagnostics: vec![error.diagnostic],
    })?;
    let midi_connection =
        midi::connect(selected_midi, events, status.clone()).map_err(|error| CliFailure {
            code: 2,
            diagnostics: vec![error.diagnostic],
        })?;
    stream.play().map_err(|error| CliFailure {
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
    println!("Sample rate: {sample_rate} Hz");
    println!("Device channels: {}", selected_audio.config.channels());
    println!("Sample format: {sample_format}");
    println!("Requested buffer: {} frames", args.buffer_size);
    println!("Actual callback buffer: backend supplied (may vary)");
    let latency_frames_for_display = u32::try_from(reported_latency_frames).unwrap_or(u32::MAX);
    let latency_ms = f64::from(latency_frames_for_display) * 1_000.0 / f64::from(sample_rate);
    println!("Engine latency: {reported_latency_frames} frames ({latency_ms:.3} ms)");
    println!("MIDI: {midi_name} [{midi_id}]");
    println!("Tempo: {} BPM", args.tempo);
    print_warnings(&diagnostics);
    Ok(LiveSession {
        _stream: stream,
        _midi: midi_connection,
        status,
    })
}

fn print_device_inventory(report: &device::DeviceInventoryReport) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(tempo: f64, buffer_size: usize, sample_rate: Option<u32>) -> PlayArgs {
        PlayArgs {
            definition: "instrument.json".into(),
            audio_device: None,
            midi_device: None,
            sample_rate,
            buffer_size,
            tempo,
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
    fn device_report_serialization_preserves_opaque_ids_and_unknown_buffer_size() {
        let report = device::DeviceInventoryReport {
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
