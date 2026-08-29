use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::StreamTrait;
use crossbeam_queue::ArrayQueue;
use midir::MidiInputConnection;
use sonalloy_core::{
    Diagnostic, DiagnosticCode, InstrumentProcessor, ProcessSpec, seconds_to_frames,
};

mod audio;
mod device;
mod midi;
mod scheduled;

pub(crate) use audio::{FatalStatus, RealtimeStatus};
pub(crate) use device::DeviceInventoryReport;

pub(crate) fn inventory() -> Result<DeviceInventoryReport, device::DeviceError> {
    device::inventory()
}

pub(crate) const DEFAULT_BUFFER_SIZE: usize = 256;
const INPUT_STARTUP_DEADLINE: Duration = Duration::from_secs(1);

pub(crate) struct LiveSession {
    _stream: cpal::Stream,
    _input_stream: Option<cpal::Stream>,
    _midi: MidiInputConnection<midi::LiveMidiState>,
    pub(crate) status: Arc<audio::RealtimeStatus>,
    pub(crate) instrument_name: String,
    pub(crate) audio_name: String,
    pub(crate) audio_id: String,
    pub(crate) input_name: Option<String>,
    pub(crate) input_id: Option<String>,
    pub(crate) sample_rate: u32,
    pub(crate) device_channels: u16,
    pub(crate) sample_format: String,
    pub(crate) buffer_size: usize,
    pub(crate) reported_latency_frames: usize,
    pub(crate) midi_name: String,
    pub(crate) midi_id: String,
    pub(crate) tempo: f64,
    pub(crate) time_signature: String,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

fn select_audio_input(
    requested_id: Option<&str>,
    required_channels: usize,
    sample_rate: u32,
    buffer_size: usize,
) -> Result<Option<device::SelectedAudioInputDevice>, (u8, Vec<Diagnostic>)> {
    if required_channels == 0 {
        if requested_id.is_some() {
            return Err((
                2,
                vec![Diagnostic::error(
                    DiagnosticCode::AudioDeviceError,
                    "the instrument does not use external audio input",
                )],
            ));
        }
        return Ok(None);
    }
    device::select_audio_input(requested_id, sample_rate, buffer_size, required_channels)
        .map(Some)
        .map_err(|error| (2, vec![error.diagnostic]))
}

pub(crate) struct ScheduledAuditionOptions<'a> {
    pub(crate) definition_path: &'a std::path::Path,
    pub(crate) audio_device: Option<&'a str>,
    pub(crate) audio_input_device: Option<&'a str>,
    pub(crate) requested_sample_rate: Option<u32>,
    pub(crate) buffer_size: usize,
    pub(crate) tail: f64,
    pub(crate) looping: bool,
}

pub(crate) struct PlayOptions<'a> {
    pub(crate) definition_path: &'a Path,
    pub(crate) audio_device: Option<&'a str>,
    pub(crate) audio_input_device: Option<&'a str>,
    pub(crate) midi_device: Option<&'a str>,
    pub(crate) sample_rate: Option<u32>,
    pub(crate) buffer_size: usize,
    pub(crate) tempo: f64,
    pub(crate) time_signature: sonalloy_core::TimeSignature,
    pub(crate) time_signature_display: &'a str,
    pub(crate) macro_cc: &'a [String],
}

pub(crate) struct ScheduledSession {
    _stream: cpal::Stream,
    _input_stream: Option<cpal::Stream>,
    pub(crate) status: Arc<audio::RealtimeStatus>,
    pub(crate) instrument_name: String,
    pub(crate) audio_name: String,
    pub(crate) audio_id: String,
    pub(crate) input_name: Option<String>,
    pub(crate) input_id: Option<String>,
    pub(crate) sample_rate: u32,
    pub(crate) device_channels: u16,
    pub(crate) buffer_size: usize,
    pub(crate) reported_latency_frames: usize,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

#[allow(clippy::too_many_lines)]
pub(crate) fn start_scheduled_audition(
    pattern: &crate::pattern::PatternDefinition,
    mut diagnostics: Vec<Diagnostic>,
    options: &ScheduledAuditionOptions<'_>,
    load: impl FnOnce(
        &Path,
        u32,
        usize,
    ) -> Result<
        (Arc<sonalloy_core::CompiledInstrument>, Vec<Diagnostic>),
        (u8, Vec<Diagnostic>),
    >,
) -> Result<ScheduledSession, (u8, Vec<Diagnostic>)> {
    let selected_audio = match device::select_audio(
        options.audio_device,
        options.requested_sample_rate,
        options.buffer_size,
    ) {
        Ok(device) => device,
        Err(error) => {
            return Err((2, vec![error.diagnostic]));
        }
    };
    let sample_rate = selected_audio.config.sample_rate();
    let (compiled, compile_diagnostics) =
        load(options.definition_path, sample_rate, options.buffer_size)?;
    diagnostics.extend(compile_diagnostics);
    let input_channels = compiled.required_input_channels();
    let selected_input = select_audio_input(
        options.audio_input_device,
        input_channels,
        sample_rate,
        options.buffer_size,
    )?;
    let compiled_pattern = match crate::pattern::compile(pattern, &compiled, f64::from(sample_rate))
    {
        Ok(pattern) => pattern,
        Err(diagnostics) => {
            return Err((2, diagnostics));
        }
    };
    let tail_frames = match seconds_to_frames(options.tail, f64::from(sample_rate)) {
        Ok(frames) => frames,
        Err(error) => {
            return Err((
                2,
                vec![Diagnostic::error(
                    DiagnosticCode::ValueOutOfRange,
                    error.to_string(),
                )],
            ));
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
            return Err((
                2,
                vec![
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "could not prepare scheduled pattern playback",
                    )
                    .with_detail(error.to_string()),
                ],
            ));
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
            return Err((
                2,
                vec![
                    Diagnostic::error(
                        DiagnosticCode::ProcessError,
                        "could not prepare the realtime processor",
                    )
                    .with_detail(error.to_string()),
                ],
            ));
        }
    };
    let mut runtime = compiled.instantiate();
    if let Err(error) = runtime.prepare(process_spec) {
        return Err((
            2,
            vec![
                Diagnostic::error(
                    DiagnosticCode::ProcessError,
                    "could not prepare the realtime processor",
                )
                .with_detail(error.to_string()),
            ],
        ));
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
            return Err((2, vec![error.diagnostic]));
        }
    };
    start_input_stream(streams.input.as_ref(), &status, options.buffer_size)?;
    if let Err(error) = streams.output.play() {
        return Err((
            2,
            vec![
                Diagnostic::error(
                    DiagnosticCode::AudioDeviceError,
                    "could not start the audio output stream",
                )
                .with_detail(error.to_string()),
            ],
        ));
    }
    Ok(ScheduledSession {
        _stream: streams.output,
        _input_stream: streams.input,
        status,
        instrument_name: compiled.metadata.name.clone(),
        audio_name: selected_audio.name,
        audio_id: selected_audio.id,
        input_name: selected_input.as_ref().map(|input| input.name.clone()),
        input_id: selected_input.as_ref().map(|input| input.id.clone()),
        sample_rate,
        device_channels: selected_audio.config.channels(),
        buffer_size: options.buffer_size,
        reported_latency_frames,
        diagnostics,
    })
}

#[allow(clippy::too_many_lines)]
pub(crate) fn start_play(
    options: &PlayOptions<'_>,
    load: impl FnOnce(
        &Path,
        u32,
        usize,
    ) -> Result<
        (Arc<sonalloy_core::CompiledInstrument>, Vec<Diagnostic>),
        (u8, Vec<Diagnostic>),
    >,
    resolve_macro_cc: impl FnOnce(
        &[String],
        &sonalloy_core::CompiledInstrument,
    ) -> Result<
        [Option<sonalloy_core::ParameterHandle>; 128],
        (u8, Vec<Diagnostic>),
    >,
) -> Result<LiveSession, (u8, Vec<Diagnostic>)> {
    let selected_audio = device::select_audio(
        options.audio_device,
        options.sample_rate,
        options.buffer_size,
    )
    .map_err(|error| (2, vec![error.diagnostic]))?;
    let selected_midi =
        device::select_midi(options.midi_device).map_err(|error| (2, vec![error.diagnostic]))?;
    let sample_rate = selected_audio.config.sample_rate();
    let (compiled, diagnostics) = load(options.definition_path, sample_rate, options.buffer_size)?;
    let input_channels = compiled.required_input_channels();
    let selected_input = select_audio_input(
        options.audio_input_device,
        input_channels,
        sample_rate,
        options.buffer_size,
    )?;
    let macro_cc = resolve_macro_cc(options.macro_cc, &compiled)?;
    let mut runtime = compiled.instantiate();
    let process_spec = ProcessSpec::new(
        f64::from(sample_rate),
        options.buffer_size,
        input_channels,
        2,
    )
    .map_err(|error| {
        (
            2,
            vec![
                Diagnostic::error(
                    DiagnosticCode::ProcessError,
                    "could not prepare the realtime processor",
                )
                .with_detail(error.to_string()),
            ],
        )
    })?;
    runtime.prepare(process_spec).map_err(|error| {
        (
            2,
            vec![
                Diagnostic::error(
                    DiagnosticCode::ProcessError,
                    "could not prepare the realtime processor",
                )
                .with_detail(error.to_string()),
            ],
        )
    })?;

    let events = Arc::new(ArrayQueue::new(audio::REALTIME_EVENT_QUEUE_CAPACITY));
    let status = Arc::new(audio::RealtimeStatus::new());
    let midi_name = selected_midi.name.clone();
    let midi_id = selected_midi.id.clone();
    let streams = audio::build_stream(
        &selected_audio,
        selected_input.as_ref(),
        input_channels,
        runtime,
        events.clone(),
        &status,
        options.buffer_size,
        options.tempo,
        options.time_signature,
    )
    .map_err(|error| (2, vec![error.diagnostic]))?;
    start_input_stream(streams.input.as_ref(), &status, options.buffer_size)?;
    let midi_connection = midi::connect(selected_midi, events, status.clone(), &macro_cc)
        .map_err(|error| (2, vec![error.diagnostic]))?;
    streams.output.play().map_err(|error| {
        (
            2,
            vec![
                Diagnostic::error(
                    DiagnosticCode::AudioDeviceError,
                    "could not start the audio output stream",
                )
                .with_detail(error.to_string()),
            ],
        )
    })?;
    Ok(LiveSession {
        _stream: streams.output,
        _input_stream: streams.input,
        _midi: midi_connection,
        status,
        instrument_name: compiled.metadata.name.clone(),
        audio_name: selected_audio.name,
        audio_id: selected_audio.id,
        input_name: selected_input.as_ref().map(|input| input.name.clone()),
        input_id: selected_input.as_ref().map(|input| input.id.clone()),
        sample_rate,
        device_channels: selected_audio.config.channels(),
        sample_format: device::sample_format_name(selected_audio.config.sample_format()),
        buffer_size: options.buffer_size,
        reported_latency_frames: compiled.reported_latency_frames,
        midi_name,
        midi_id,
        tempo: options.tempo,
        time_signature: options.time_signature_display.to_owned(),
        diagnostics,
    })
}
fn start_input_stream(
    stream: Option<&cpal::Stream>,
    status: &Arc<audio::RealtimeStatus>,
    startup_frames: usize,
) -> Result<(), (u8, Vec<Diagnostic>)> {
    let Some(stream) = stream else {
        return Ok(());
    };
    stream.play().map_err(|error| {
        (
            2,
            vec![
                Diagnostic::error(
                    DiagnosticCode::AudioDeviceError,
                    "could not start the audio input stream",
                )
                .with_detail(error.to_string()),
            ],
        )
    })?;
    let deadline = Instant::now() + INPUT_STARTUP_DEADLINE;
    while !status.input_ready() && status.fatal() == audio::FatalStatus::None {
        if Instant::now() >= deadline {
            status.set_fatal(audio::FatalStatus::Input);
            return Err((
                2,
                vec![Diagnostic::error(
                    DiagnosticCode::AudioDeviceError,
                    format!(
                        "audio input did not provide {startup_frames} frames before the startup deadline"
                    ),
                )],
            ));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if status.fatal() == audio::FatalStatus::None {
        Ok(())
    } else {
        Err((
            3,
            vec![
                status
                    .fatal()
                    .diagnostic()
                    .expect("fatal status has a diagnostic"),
            ],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
