use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

use cpal::traits::DeviceTrait;
use cpal::{ErrorKind, FromSample, I24, SampleFormat, SizedSample, Stream, StreamConfig, U24};
use crossbeam_queue::ArrayQueue;
use sonalloy_core::{
    Diagnostic, DiagnosticCode, InstrumentProcessor, InstrumentRuntime, ProcessBlock,
    ProcessContext, ProcessEvent, ProcessEventKind,
};

use super::device::{DeviceError, SelectedAudioDevice};
use super::scheduled::ScheduledEventFeed;

pub(crate) const REALTIME_EVENT_QUEUE_CAPACITY: usize = 4_096;

#[derive(Debug, Clone, Copy)]
pub(crate) struct QueuedEvent {
    pub(crate) timestamp_us: u64,
    pub(crate) sequence: u64,
    pub(crate) kind: ProcessEventKind,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FatalStatus {
    None = 0,
    Process = 1,
    Output = 2,
    Midi = 3,
    EventQueue = 4,
}

impl FatalStatus {
    fn from_raw(value: u8) -> Self {
        match value {
            1 => Self::Process,
            2 => Self::Output,
            3 => Self::Midi,
            4 => Self::EventQueue,
            _ => Self::None,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Process => "process failure",
            Self::Output => "audio output failure",
            Self::Midi => "MIDI input failure",
            Self::EventQueue => "event queue overflow",
        }
    }

    pub(crate) fn diagnostic(self) -> Option<Diagnostic> {
        let (code, message) = match self {
            Self::None => return None,
            Self::Process => (DiagnosticCode::ProcessError, "realtime processing failed"),
            Self::Output => (
                DiagnosticCode::AudioDeviceError,
                "realtime audio output stopped",
            ),
            Self::Midi => (DiagnosticCode::MidiError, "realtime MIDI input stopped"),
            Self::EventQueue => (
                DiagnosticCode::ProcessError,
                "realtime event queue overflow",
            ),
        };
        Some(Diagnostic::error(code, message).with_detail(self.label()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CallbackFrameStats {
    pub(crate) count: u64,
    pub(crate) min: Option<u64>,
    pub(crate) max: Option<u64>,
}

pub(crate) struct RealtimeStatus {
    fatal: AtomicU8,
    realtime_denied: AtomicBool,
    finished: AtomicBool,
    xrun_count: AtomicU64,
    callback_count: AtomicU64,
    callback_frames_min: AtomicU64,
    callback_frames_max: AtomicU64,
}

impl RealtimeStatus {
    pub(crate) fn new() -> Self {
        Self {
            fatal: AtomicU8::new(FatalStatus::None as u8),
            realtime_denied: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            xrun_count: AtomicU64::new(0),
            callback_count: AtomicU64::new(0),
            callback_frames_min: AtomicU64::new(u64::MAX),
            callback_frames_max: AtomicU64::new(0),
        }
    }

    pub(crate) fn set_fatal(&self, status: FatalStatus) {
        let _ = self.fatal.compare_exchange(
            FatalStatus::None as u8,
            status as u8,
            Ordering::Release,
            Ordering::Relaxed,
        );
    }

    pub(crate) fn fatal(&self) -> FatalStatus {
        FatalStatus::from_raw(self.fatal.load(Ordering::Acquire))
    }

    pub(crate) fn mark_realtime_denied(&self) {
        self.realtime_denied.store(true, Ordering::Release);
    }

    pub(crate) fn realtime_denied(&self) -> bool {
        self.realtime_denied.load(Ordering::Acquire)
    }

    pub(crate) fn mark_finished(&self) {
        self.finished.store(true, Ordering::Release);
    }

    pub(crate) fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    pub(crate) fn increment_xruns(&self) {
        self.xrun_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn xrun_count(&self) -> u64 {
        self.xrun_count.load(Ordering::Relaxed)
    }

    pub(crate) fn record_callback_frames(&self, frames: usize) {
        let frames = u64::try_from(frames).unwrap_or(u64::MAX);
        self.callback_count.fetch_add(1, Ordering::Relaxed);
        self.callback_frames_min
            .fetch_min(frames, Ordering::Relaxed);
        self.callback_frames_max
            .fetch_max(frames, Ordering::Relaxed);
    }

    pub(crate) fn callback_frame_stats(&self) -> CallbackFrameStats {
        let count = self.callback_count.load(Ordering::Relaxed);
        CallbackFrameStats {
            count,
            min: (count > 0).then(|| self.callback_frames_min.load(Ordering::Relaxed)),
            max: (count > 0).then(|| self.callback_frames_max.load(Ordering::Relaxed)),
        }
    }
}

enum RealtimeEventFeed {
    Live,
    Scheduled(ScheduledEventFeed),
}

pub(crate) struct AudioEngine {
    runtime: InstrumentRuntime,
    events: Arc<ArrayQueue<QueuedEvent>>,
    feed: RealtimeEventFeed,
    status: Arc<RealtimeStatus>,
    max_block_size: usize,
    tempo_bpm: f64,
    channels: usize,
    left: Vec<f32>,
    right: Vec<f32>,
    queued_events: Vec<QueuedEvent>,
    process_events: Vec<ProcessEvent>,
}

impl AudioEngine {
    pub(crate) fn new(
        runtime: InstrumentRuntime,
        events: Arc<ArrayQueue<QueuedEvent>>,
        status: Arc<RealtimeStatus>,
        max_block_size: usize,
        tempo_bpm: f64,
        channels: usize,
    ) -> Self {
        Self {
            runtime,
            events,
            feed: RealtimeEventFeed::Live,
            status,
            max_block_size,
            tempo_bpm,
            channels,
            left: vec![0.0; max_block_size],
            right: vec![0.0; max_block_size],
            queued_events: Vec::with_capacity(REALTIME_EVENT_QUEUE_CAPACITY),
            process_events: Vec::with_capacity(REALTIME_EVENT_QUEUE_CAPACITY),
        }
    }

    pub(crate) fn new_scheduled(
        runtime: InstrumentRuntime,
        feed: ScheduledEventFeed,
        status: Arc<RealtimeStatus>,
        max_block_size: usize,
        channels: usize,
    ) -> Self {
        let process_event_capacity = feed.max_events_per_block();
        Self {
            runtime,
            events: Arc::new(ArrayQueue::new(1)),
            feed: RealtimeEventFeed::Scheduled(feed),
            status,
            max_block_size,
            tempo_bpm: 120.0,
            channels,
            left: vec![0.0; max_block_size],
            right: vec![0.0; max_block_size],
            queued_events: Vec::with_capacity(1),
            process_events: Vec::with_capacity(process_event_capacity),
        }
    }

    fn process_callback<T>(&mut self, data: &mut [T])
    where
        T: SizedSample + FromSample<f32>,
    {
        data.fill(T::EQUILIBRIUM);
        if self.status.fatal() != FatalStatus::None {
            return;
        }
        if self.channels < 2 || self.max_block_size == 0 {
            self.status.set_fatal(FatalStatus::Output);
            return;
        }
        let Some(frames) = data.len().checked_div(self.channels) else {
            self.status.set_fatal(FatalStatus::Output);
            return;
        };
        if frames * self.channels != data.len() {
            self.status.set_fatal(FatalStatus::Output);
            return;
        }
        self.status.record_callback_frames(frames);

        let mut frame_start = 0;
        while frame_start < frames {
            let block_frames = (frames - frame_start).min(self.max_block_size);
            let absolute_frame = self.runtime.absolute_frame();
            let (block_frames, tempo_bpm) = match self.prepare_block(block_frames) {
                Ok(Some(block)) => block,
                Ok(None) => return,
                Err(()) => {
                    self.status.set_fatal(FatalStatus::Process);
                    return;
                }
            };
            self.left[..block_frames].fill(0.0);
            self.right[..block_frames].fill(0.0);
            let mut output = [
                &mut self.left[..block_frames],
                &mut self.right[..block_frames],
            ];
            let result = self.runtime.process(ProcessBlock {
                frames: block_frames,
                context: ProcessContext {
                    absolute_frame,
                    tempo_bpm,
                },
                events: &self.process_events,
                output: &mut output,
            });
            if result.is_err() {
                self.status.set_fatal(FatalStatus::Process);
                return;
            }
            for frame in 0..block_frames {
                let offset = (frame_start + frame) * self.channels;
                data[offset] = T::from_sample_(self.left[frame]);
                data[offset + 1] = T::from_sample_(self.right[frame]);
            }
            frame_start += block_frames;
            if self.scheduled_finished() {
                self.status.mark_finished();
                return;
            }
        }
    }

    fn prepare_block(&mut self, requested_frames: usize) -> Result<Option<(usize, f64)>, ()> {
        match &mut self.feed {
            RealtimeEventFeed::Live => {
                self.drain_events();
                if self.status.fatal() != FatalStatus::None {
                    return Ok(None);
                }
                Ok(Some((requested_frames, self.tempo_bpm)))
            }
            RealtimeEventFeed::Scheduled(feed) => {
                let absolute_frame = self.runtime.absolute_frame();
                let frames = feed
                    .prepare_block(absolute_frame, requested_frames, &mut self.process_events)
                    .map_err(|_| ())?;
                if frames == 0 {
                    self.status.mark_finished();
                    return Ok(None);
                }
                Ok(Some((frames, feed.tempo_at(absolute_frame))))
            }
        }
    }

    fn scheduled_finished(&self) -> bool {
        match &self.feed {
            RealtimeEventFeed::Live => false,
            RealtimeEventFeed::Scheduled(feed) => feed.is_finished(),
        }
    }

    fn drain_events(&mut self) {
        self.queued_events.clear();
        self.process_events.clear();
        while let Some(event) = self.events.pop() {
            if self.queued_events.len() == REALTIME_EVENT_QUEUE_CAPACITY {
                self.status.set_fatal(FatalStatus::EventQueue);
                return;
            }
            self.queued_events.push(event);
        }
        self.queued_events.sort_unstable_by(|left, right| {
            left.timestamp_us
                .cmp(&right.timestamp_us)
                .then(left.sequence.cmp(&right.sequence))
        });
        for event in &self.queued_events {
            self.process_events.push(ProcessEvent {
                sample_offset: 0,
                kind: event.kind,
            });
        }
    }
}

pub(crate) fn build_stream(
    selected: &SelectedAudioDevice,
    runtime: InstrumentRuntime,
    events: Arc<ArrayQueue<QueuedEvent>>,
    status: Arc<RealtimeStatus>,
    max_block_size: usize,
    tempo_bpm: f64,
) -> Result<Stream, DeviceError> {
    let channels = usize::from(selected.config.channels());
    if channels < 2 {
        return Err(DeviceError {
            diagnostic: sonalloy_core::Diagnostic::error(
                sonalloy_core::DiagnosticCode::AudioDeviceError,
                "the audio output must provide at least two channels",
            ),
        });
    }
    if max_block_size == 0 {
        return Err(DeviceError {
            diagnostic: sonalloy_core::Diagnostic::error(
                sonalloy_core::DiagnosticCode::AudioDeviceError,
                "the realtime block size must be positive",
            ),
        });
    }
    let engine = AudioEngine::new(
        runtime,
        events,
        status.clone(),
        max_block_size,
        tempo_bpm,
        channels,
    );
    build_stream_with_engine(selected, engine, status)
}

pub(crate) fn build_scheduled_stream(
    selected: &SelectedAudioDevice,
    runtime: InstrumentRuntime,
    feed: ScheduledEventFeed,
    status: Arc<RealtimeStatus>,
    max_block_size: usize,
) -> Result<Stream, DeviceError> {
    let channels = usize::from(selected.config.channels());
    if channels < 2 {
        return Err(DeviceError {
            diagnostic: sonalloy_core::Diagnostic::error(
                sonalloy_core::DiagnosticCode::AudioDeviceError,
                "the audio output must provide at least two channels",
            ),
        });
    }
    if max_block_size == 0 {
        return Err(DeviceError {
            diagnostic: sonalloy_core::Diagnostic::error(
                sonalloy_core::DiagnosticCode::AudioDeviceError,
                "the realtime block size must be positive",
            ),
        });
    }
    let engine =
        AudioEngine::new_scheduled(runtime, feed, status.clone(), max_block_size, channels);
    build_stream_with_engine(selected, engine, status)
}

fn build_stream_with_engine(
    selected: &SelectedAudioDevice,
    engine: AudioEngine,
    status: Arc<RealtimeStatus>,
) -> Result<Stream, DeviceError> {
    let config = selected.stream_config;
    let stream = match selected.config.sample_format() {
        SampleFormat::F32 => build_typed_stream::<f32>(&selected.device, config, engine, status),
        SampleFormat::F64 => build_typed_stream::<f64>(&selected.device, config, engine, status),
        SampleFormat::I8 => build_typed_stream::<i8>(&selected.device, config, engine, status),
        SampleFormat::I16 => build_typed_stream::<i16>(&selected.device, config, engine, status),
        SampleFormat::I24 => build_typed_stream::<I24>(&selected.device, config, engine, status),
        SampleFormat::I32 => build_typed_stream::<i32>(&selected.device, config, engine, status),
        SampleFormat::I64 => build_typed_stream::<i64>(&selected.device, config, engine, status),
        SampleFormat::U8 => build_typed_stream::<u8>(&selected.device, config, engine, status),
        SampleFormat::U16 => build_typed_stream::<u16>(&selected.device, config, engine, status),
        SampleFormat::U24 => build_typed_stream::<U24>(&selected.device, config, engine, status),
        SampleFormat::U32 => build_typed_stream::<u32>(&selected.device, config, engine, status),
        SampleFormat::U64 => build_typed_stream::<u64>(&selected.device, config, engine, status),
        _ => {
            return Err(DeviceError {
                diagnostic: sonalloy_core::Diagnostic::error(
                    sonalloy_core::DiagnosticCode::AudioDeviceError,
                    "the selected audio sample format is not supported",
                ),
            });
        }
    };
    stream.map_err(|error| DeviceError {
        diagnostic: sonalloy_core::Diagnostic::error(
            sonalloy_core::DiagnosticCode::AudioDeviceError,
            "could not create the audio output stream",
        )
        .with_detail(error.to_string()),
    })
}

fn build_typed_stream<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut engine: AudioEngine,
    status: Arc<RealtimeStatus>,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let error_status = status;
    device.build_output_stream(
        config,
        move |data: &mut [T], _| engine.process_callback(data),
        move |error| handle_stream_error(&error_status, error.kind()),
        None,
    )
}

fn handle_stream_error(status: &RealtimeStatus, kind: ErrorKind) {
    match kind {
        ErrorKind::RealtimeDenied => status.mark_realtime_denied(),
        ErrorKind::Xrun => status.increment_xruns(),
        _ => status.set_fatal(FatalStatus::Output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonalloy_core::{
        CompileContext, InstrumentProcessor, ProcessSpec, ScheduledEvent, TempoMap, VoiceState,
    };
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::path::PathBuf;

    thread_local! {
        static ALLOCATION_COUNT: Cell<Option<usize>> = const { Cell::new(None) };
    }

    struct CountingAllocator;

    #[global_allocator]
    static ALLOCATOR: CountingAllocator = CountingAllocator;

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            if !pointer.is_null() {
                record_allocation();
            }
            pointer
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc_zeroed(layout) };
            if !pointer.is_null() {
                record_allocation();
            }
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) };
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let pointer = unsafe { System.realloc(pointer, layout, new_size) };
            if !pointer.is_null() {
                record_allocation();
            }
            pointer
        }
    }

    fn record_allocation() {
        ALLOCATION_COUNT.with(|count| {
            if let Some(value) = count.get() {
                count.set(Some(value.saturating_add(1)));
            }
        });
    }

    fn count_allocations(function: impl FnOnce()) -> usize {
        let previous = ALLOCATION_COUNT.with(|count| count.replace(Some(0)));
        assert!(
            previous.is_none(),
            "allocation measurement cannot be nested"
        );
        function();
        ALLOCATION_COUNT.with(|count| {
            count
                .replace(None)
                .expect("allocation measurement was unexpectedly disabled")
        })
    }

    fn prepared_runtime() -> InstrumentRuntime {
        let spec = ProcessSpec::new(48_000.0, 256, 2).expect("valid process spec");
        let definition = crate::default_definition();
        let result = sonalloy_core::compile_instrument(
            &definition,
            &CompileContext {
                definition_base_dir: PathBuf::from("."),
                process_spec: spec,
            },
        );
        let compiled = result.instrument.expect("default compiles");
        let mut runtime = compiled.instantiate();
        runtime.prepare(spec).expect("runtime prepares");
        runtime
    }

    fn prepared_engine(channels: usize) -> AudioEngine {
        AudioEngine::new(
            prepared_runtime(),
            Arc::new(ArrayQueue::new(REALTIME_EVENT_QUEUE_CAPACITY)),
            Arc::new(RealtimeStatus::new()),
            256,
            120.0,
            channels,
        )
    }

    fn prepared_scheduled_engine() -> AudioEngine {
        let feed = ScheduledEventFeed::new(
            crate::pattern::CompiledPattern {
                events: vec![ScheduledEvent {
                    absolute_frame: 0,
                    kind: ProcessEventKind::NoteOn {
                        note_id: 0,
                        note_number: 60,
                        velocity: 100,
                    },
                }],
                tempo_map: TempoMap::constant(120.0).expect("tempo map"),
                length_frames: 48_000,
                one_shot_duration_frames: 48_000,
            },
            0,
            0,
            false,
        )
        .expect("scheduled feed");
        AudioEngine::new_scheduled(
            prepared_runtime(),
            feed,
            Arc::new(RealtimeStatus::new()),
            256,
            2,
        )
    }

    #[test]
    fn stream_errors_keep_realtime_denied_non_fatal_and_count_xruns() {
        let status = RealtimeStatus::new();

        handle_stream_error(&status, ErrorKind::RealtimeDenied);
        handle_stream_error(&status, ErrorKind::Xrun);
        handle_stream_error(&status, ErrorKind::Xrun);

        assert!(status.realtime_denied());
        assert_eq!(status.xrun_count(), 2);
        assert_eq!(status.fatal(), FatalStatus::None);
    }

    #[test]
    fn stream_device_errors_stop_processing() {
        let status = RealtimeStatus::new();

        handle_stream_error(&status, ErrorKind::DeviceNotAvailable);

        assert_eq!(status.fatal(), FatalStatus::Output);
    }

    #[test]
    fn device_changes_are_fatal() {
        let status = RealtimeStatus::new();

        handle_stream_error(&status, ErrorKind::DeviceChanged);

        assert_eq!(status.fatal(), FatalStatus::Output);
    }

    #[test]
    fn fatal_statuses_map_to_their_frontend_diagnostic_categories() {
        assert_eq!(
            FatalStatus::Process
                .diagnostic()
                .expect("process diagnostic")
                .code,
            DiagnosticCode::ProcessError
        );
        assert_eq!(
            FatalStatus::EventQueue
                .diagnostic()
                .expect("queue diagnostic")
                .code,
            DiagnosticCode::ProcessError
        );
        assert_eq!(
            FatalStatus::Output
                .diagnostic()
                .expect("output diagnostic")
                .code,
            DiagnosticCode::AudioDeviceError
        );
        assert_eq!(
            FatalStatus::Midi
                .diagnostic()
                .expect("MIDI diagnostic")
                .code,
            DiagnosticCode::MidiError
        );
        assert!(FatalStatus::None.diagnostic().is_none());
    }

    #[test]
    fn fatal_status_silences_the_next_callback() {
        let mut engine = prepared_engine(2);
        engine.status.set_fatal(FatalStatus::Process);
        let mut data = vec![1.0_f32; 64 * 2];

        engine.process_callback(&mut data);

        assert!(data.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn callback_sizes_are_split_and_absolute_frames_stay_continuous() {
        let mut engine = prepared_engine(2);
        let mut expected_frame = 0_u64;

        for frames in [1_usize, 63, 64, 255, 256, 257, 511, 641, 1024] {
            let mut data = vec![0.0_f32; frames * 2];
            engine.process_callback(&mut data);
            expected_frame += u64::try_from(frames).expect("test frame count fits u64");

            assert_eq!(engine.runtime.absolute_frame(), expected_frame);
            assert_eq!(engine.status.fatal(), FatalStatus::None);
        }

        assert_eq!(
            engine.status.callback_frame_stats(),
            CallbackFrameStats {
                count: 9,
                min: Some(1),
                max: Some(1024),
            }
        );
    }

    #[test]
    fn process_fault_silences_the_current_callback_and_sets_fatal_status() {
        let mut engine = prepared_engine(2);
        engine.max_block_size = 512;
        engine.left.resize(512, 0.0);
        engine.right.resize(512, 0.0);
        let mut data = vec![1.0_f32; 512 * 2];

        engine.process_callback(&mut data);

        assert_eq!(engine.status.fatal(), FatalStatus::Process);
        assert!(data.iter().all(|sample| *sample == 0.0));
    }

    fn assert_sample_conversion<T>()
    where
        T: SizedSample + FromSample<f32> + Copy + PartialEq,
    {
        let mut engine = prepared_engine(2);
        engine
            .events
            .push(QueuedEvent {
                timestamp_us: 0,
                sequence: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 100,
                },
            })
            .expect("queue has capacity");
        let mut data = vec![T::EQUILIBRIUM; 64 * 2];

        engine.process_callback(&mut data);

        assert_eq!(engine.status.fatal(), FatalStatus::None);
        assert!(data.iter().any(|sample| *sample != T::EQUILIBRIUM));
    }

    #[test]
    fn representative_pcm_formats_receive_rendered_audio() {
        assert_sample_conversion::<f32>();
        assert_sample_conversion::<f64>();
        assert_sample_conversion::<i16>();
        assert_sample_conversion::<I24>();
        assert_sample_conversion::<u16>();
        assert_sample_conversion::<U24>();
    }

    #[test]
    fn queue_drain_orders_events_by_timestamp_then_sequence() {
        let mut engine = prepared_engine(2);
        let events = [
            QueuedEvent {
                timestamp_us: 0,
                sequence: 3,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 100,
                },
            },
            QueuedEvent {
                timestamp_us: 0,
                sequence: 1,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
            QueuedEvent {
                timestamp_us: 0,
                sequence: 4,
                kind: ProcessEventKind::SustainPedal { down: true },
            },
            QueuedEvent {
                timestamp_us: 0,
                sequence: 0,
                kind: ProcessEventKind::SustainPedal { down: false },
            },
            QueuedEvent {
                timestamp_us: 0,
                sequence: 2,
                kind: ProcessEventKind::PitchBend { value: 0.0 },
            },
        ];
        for event in events {
            engine.events.push(event).expect("queue has capacity");
        }
        let queued_capacity = engine.queued_events.capacity();
        let process_capacity = engine.process_events.capacity();

        engine.drain_events();

        let kinds = engine
            .process_events
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>();
        assert!(matches!(
            kinds[0],
            ProcessEventKind::SustainPedal { down: false }
        ));
        assert!(matches!(kinds[1], ProcessEventKind::NoteOff { note_id: 1 }));
        assert!(matches!(kinds[2], ProcessEventKind::PitchBend { value } if value == 0.0));
        assert!(matches!(
            kinds[3],
            ProcessEventKind::NoteOn { note_id: 1, .. }
        ));
        assert!(matches!(
            kinds[4],
            ProcessEventKind::SustainPedal { down: true }
        ));
        assert_eq!(engine.queued_events.capacity(), queued_capacity);
        assert_eq!(engine.process_events.capacity(), process_capacity);
    }

    #[test]
    fn queue_drain_preserves_input_order_for_equal_timestamps() {
        let mut engine = prepared_engine(2);
        let events = [
            QueuedEvent {
                timestamp_us: 20,
                sequence: 2,
                kind: ProcessEventKind::SustainPedal { down: false },
            },
            QueuedEvent {
                timestamp_us: 10,
                sequence: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 100,
                },
            },
            QueuedEvent {
                timestamp_us: 20,
                sequence: 1,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
            QueuedEvent {
                timestamp_us: 20,
                sequence: 3,
                kind: ProcessEventKind::SustainPedal { down: true },
            },
        ];
        for event in events {
            engine.events.push(event).expect("queue has capacity");
        }

        engine.drain_events();

        let kinds = engine
            .process_events
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>();
        assert!(matches!(
            kinds.as_slice(),
            [
                ProcessEventKind::NoteOn { .. },
                ProcessEventKind::NoteOff { .. },
                ProcessEventKind::SustainPedal { down: false },
                ProcessEventKind::SustainPedal { down: true },
            ]
        ));
    }

    fn active_voice_count_after(events: &[QueuedEvent]) -> usize {
        let mut engine = prepared_engine(2);
        for event in events {
            engine.events.push(*event).expect("queue has capacity");
        }
        let mut data = vec![0.0_f32; 64 * 2];
        engine.process_callback(&mut data);
        assert_eq!(engine.status.fatal(), FatalStatus::None);
        (0..engine.runtime.voice_count())
            .filter(|&index| engine.runtime.voice_state(index) == Some(VoiceState::Active))
            .count()
    }

    #[test]
    fn same_timestamp_sequence_preserves_note_and_sustain_semantics() {
        let same_time_note_on = QueuedEvent {
            timestamp_us: 100,
            sequence: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        };
        let same_time_note_off = QueuedEvent {
            timestamp_us: 100,
            sequence: 1,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        };
        assert_eq!(
            active_voice_count_after(&[same_time_note_on, same_time_note_off]),
            0
        );

        let note_on = QueuedEvent {
            timestamp_us: 10,
            sequence: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        };
        let note_off = QueuedEvent {
            timestamp_us: 20,
            sequence: 1,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        };
        let sustain_down_after_note_off = QueuedEvent {
            timestamp_us: 20,
            sequence: 2,
            kind: ProcessEventKind::SustainPedal { down: true },
        };
        let sustain_down_before_note_off = QueuedEvent {
            timestamp_us: 20,
            sequence: 1,
            kind: ProcessEventKind::SustainPedal { down: true },
        };
        let note_off_after_sustain = QueuedEvent {
            timestamp_us: 20,
            sequence: 2,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        };

        assert_eq!(
            active_voice_count_after(&[note_on, note_off, sustain_down_after_note_off]),
            0
        );
        assert_eq!(
            active_voice_count_after(&[
                note_on,
                sustain_down_before_note_off,
                note_off_after_sustain,
            ]),
            1
        );
    }

    #[test]
    fn queue_drain_handles_the_fixed_capacity_without_growth() {
        let mut engine = prepared_engine(2);
        for sequence in 0..REALTIME_EVENT_QUEUE_CAPACITY {
            engine
                .events
                .push(QueuedEvent {
                    timestamp_us: 0,
                    sequence: u64::try_from(sequence).expect("test sequence fits u64"),
                    kind: ProcessEventKind::PitchBend { value: 0.0 },
                })
                .expect("queue has fixed capacity");
        }
        let queued_capacity = engine.queued_events.capacity();
        let process_capacity = engine.process_events.capacity();

        engine.drain_events();

        assert_eq!(engine.process_events.len(), REALTIME_EVENT_QUEUE_CAPACITY);
        assert_eq!(engine.queued_events.capacity(), queued_capacity);
        assert_eq!(engine.process_events.capacity(), process_capacity);
        assert!(engine.events.is_empty());
    }

    #[test]
    fn prepared_callback_handles_large_multichannel_buffers_without_allocating() {
        let mut engine = prepared_engine(4);
        engine
            .events
            .push(QueuedEvent {
                timestamp_us: 0,
                sequence: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 100,
                },
            })
            .expect("queue has capacity");
        let mut data = vec![0.0_f32; 641 * 4];
        let queued_capacity = engine.queued_events.capacity();
        let process_capacity = engine.process_events.capacity();

        let allocations = count_allocations(|| engine.process_callback(&mut data));

        assert_eq!(allocations, 0);
        assert_eq!(engine.queued_events.capacity(), queued_capacity);
        assert_eq!(engine.process_events.capacity(), process_capacity);
        assert_eq!(engine.status.fatal(), FatalStatus::None);
        assert!(
            data.chunks_exact(4)
                .any(|frame| frame[0] != 0.0 || frame[1] != 0.0)
        );
        assert!(
            data.chunks_exact(4)
                .all(|frame| frame[2] == 0.0 && frame[3] == 0.0)
        );
    }

    #[test]
    fn scheduled_callback_handles_events_without_allocating() {
        let mut engine = prepared_scheduled_engine();
        let process_capacity = engine.process_events.capacity();
        let mut data = vec![0.0_f32; 641 * 2];

        let allocations = count_allocations(|| engine.process_callback(&mut data));

        assert_eq!(allocations, 0);
        assert_eq!(engine.process_events.capacity(), process_capacity);
        assert_eq!(engine.status.fatal(), FatalStatus::None);
        assert!(data.iter().any(|sample| *sample != 0.0));
    }
}
