use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::StreamTrait;
use crossbeam_queue::ArrayQueue;
use midir::MidiInputConnection;
use sonalloy_core::{Diagnostic, DiagnosticCode, ProcessSpec, seconds_to_frames};

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

#[derive(Debug, Clone)]
pub(crate) struct ScheduledGainSegment {
    pub(crate) start_tick: u64,
    pub(crate) end_tick: u64,
    pub(crate) gain_db: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct ScheduledGainPlan {
    pub(crate) segments: Vec<ScheduledGainSegment>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScheduledGainFrameSegment {
    pub(crate) start_frame: u64,
    pub(crate) transition_end_frame: u64,
    pub(crate) end_frame: u64,
    pub(crate) from_gain: f32,
    pub(crate) to_gain: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct ScheduledGainTimeline {
    pub(crate) segments: Vec<ScheduledGainFrameSegment>,
    pub(crate) loop_length_frames: u64,
    pub(crate) looping: bool,
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
    prepare: impl FnOnce(
        &sonalloy_core::CompiledInstrument,
    ) -> Result<
        (
            crate::score::events::PerformanceEvents,
            f64,
            Option<ScheduledGainPlan>,
        ),
        (u8, Vec<Diagnostic>),
    >,
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
    let (pattern, gain_db, gain_plan) = prepare(&compiled)?;
    let gain = crate::score::gain_linear(gain_db).ok_or_else(|| {
        (
            2,
            vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "audition gain must be finite",
            )],
        )
    })?;
    #[allow(clippy::cast_possible_truncation)]
    let gain = gain as f32;
    if !gain.is_finite() {
        return Err((
            2,
            vec![Diagnostic::error(
                DiagnosticCode::ValueOutOfRange,
                "audition gain exceeds the audio range",
            )],
        ));
    }
    let input_channels = compiled.required_input_channels();
    let selected_input = select_audio_input(
        options.audio_input_device,
        input_channels,
        sample_rate,
        options.buffer_size,
    )?;
    let gain_timeline = match prepare_scheduled_gain(
        gain_db,
        gain_plan.as_ref(),
        &pattern,
        f64::from(sample_rate),
        options.looping,
    ) {
        Ok(timeline) => timeline,
        Err(detail) => {
            return Err((
                2,
                vec![
                    Diagnostic::error(
                        DiagnosticCode::ValueOutOfRange,
                        "could not prepare scheduled gain changes",
                    )
                    .with_detail(detail),
                ],
            ));
        }
    };
    let compiled_pattern =
        match crate::performance::events::compile(&pattern, &compiled, f64::from(sample_rate)) {
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
    if let Err(error) = runtime.activate() {
        return Err((
            2,
            vec![
                Diagnostic::error(
                    DiagnosticCode::ProcessError,
                    "could not activate the realtime processor",
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
        gain,
        gain_timeline,
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

fn prepare_scheduled_gain(
    base_gain_db: f64,
    plan: Option<&ScheduledGainPlan>,
    pattern: &crate::score::events::PerformanceEvents,
    sample_rate: f64,
    looping: bool,
) -> Result<Option<ScheduledGainTimeline>, String> {
    let Some(plan) = plan else {
        return Ok(None);
    };
    let tempo_changes = crate::performance::events::tempo_points(pattern);
    let frame = |tick| {
        crate::musical_time::tick_to_frame(
            tick,
            pattern.ticks_per_beat,
            &tempo_changes,
            sample_rate,
        )
        .map_err(|error| error.to_string())
    };
    let loop_length_frames = frame(pattern.length_ticks)?;
    let transition_frames = seconds_to_frames(0.005, sample_rate)
        .map_err(|error| error.to_string())?
        .max(1);
    let mut segments = Vec::with_capacity(plan.segments.len());
    let mut previous_gain = None;
    let mut previous_end = 0;
    for segment in &plan.segments {
        if segment.start_tick >= segment.end_tick {
            return Err("scheduled gain segment must have positive duration".to_owned());
        }
        let start_frame = frame(segment.start_tick)?;
        let end_frame = frame(segment.end_tick)?;
        if end_frame <= start_frame || start_frame < previous_end || end_frame > loop_length_frames
        {
            return Err("scheduled gain segment is outside the Pattern timeline".to_owned());
        }
        let to_gain = crate::score::gain_linear(base_gain_db + segment.gain_db)
            .ok_or_else(|| "scheduled gain is not finite".to_owned())?;
        #[allow(clippy::cast_possible_truncation)]
        let to_gain = to_gain as f32;
        if !to_gain.is_finite() {
            return Err("scheduled gain exceeds the audio range".to_owned());
        }
        let from_gain = previous_gain.unwrap_or(to_gain);
        let transition_end_frame = start_frame
            .checked_add(transition_frames)
            .unwrap_or(end_frame)
            .min(end_frame);
        segments.push(ScheduledGainFrameSegment {
            start_frame,
            transition_end_frame,
            end_frame,
            from_gain,
            to_gain,
        });
        previous_gain = Some(to_gain);
        previous_end = end_frame;
    }
    if segments.is_empty() || segments[0].start_frame != 0 {
        return Err("scheduled gain plan must start at Pattern frame zero".to_owned());
    }
    Ok(Some(ScheduledGainTimeline {
        segments,
        loop_length_frames,
        looping,
    }))
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
    runtime.activate().map_err(|error| {
        (
            2,
            vec![
                Diagnostic::error(
                    DiagnosticCode::ProcessError,
                    "could not activate the realtime processor",
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
    let midi_connection = midi::connect(
        selected_midi,
        events,
        status.clone(),
        &macro_cc,
        compiled.parameter_catalog_revision(),
    )
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
    use super::device;

    #[test]
    fn scheduled_section_gain_is_precomputed_on_the_pattern_timeline() {
        let pattern = crate::performance::events::default_pattern();
        let plan = super::ScheduledGainPlan {
            segments: vec![
                super::ScheduledGainSegment {
                    start_tick: 0,
                    end_tick: 960,
                    gain_db: -6.0,
                },
                super::ScheduledGainSegment {
                    start_tick: 960,
                    end_tick: 1_920,
                    gain_db: 6.0,
                },
            ],
        };

        let timeline = super::prepare_scheduled_gain(-10.0, Some(&plan), &pattern, 48_000.0, true)
            .expect("gain plan prepares")
            .expect("gain timeline");

        let quiet = 10.0_f32.powf(-16.0 / 20.0);
        let loud = 10.0_f32.powf(-4.0 / 20.0);
        assert_eq!(timeline.loop_length_frames, 96_000);
        assert!(timeline.looping);
        assert_eq!(timeline.segments[0].start_frame, 0);
        assert_eq!(timeline.segments[0].from_gain, quiet);
        assert_eq!(timeline.segments[0].to_gain, quiet);
        assert_eq!(timeline.segments[1].start_frame, 48_000);
        assert_eq!(timeline.segments[1].transition_end_frame, 48_240);
        assert_eq!(timeline.segments[1].from_gain, quiet);
        assert_eq!(timeline.segments[1].to_gain, loud);
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
