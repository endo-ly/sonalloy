use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use thiserror::Error;

use super::super::external_audio::ExternalAudioBlock;
use super::{RuntimeGeneration, VoiceState};
use crate::compiler::CompiledInstrument;
use crate::parameter::ParameterCatalogRevision;
use crate::process::{
    InstrumentProcessor, ProcessBlock, ProcessError, ProcessEventKind, ProcessSpec, clear_output,
};

/// Maximum number of live voice generations retained by one runtime.
pub const MAX_LIVE_GENERATIONS: usize = 8;

const MAX_RETIRED_GENERATIONS: usize = MAX_LIVE_GENERATIONS - 1;
const MAX_RECLAIMABLE_RESOURCES: usize = MAX_LIVE_GENERATIONS * 2;
const GLOBAL_RECONFIG_FADE_SECONDS: f64 = 0.005;

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: crate::process::ProcessorFailureKind::InvalidState,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn global_transition_frames(sample_rate: f64) -> usize {
    (sample_rate * GLOBAL_RECONFIG_FADE_SECONDS)
        .round()
        .max(1.0) as usize
}

#[allow(clippy::cast_precision_loss)]
fn global_transition_position(elapsed_frames: usize, index: usize, total_frames: usize) -> f32 {
    elapsed_frames
        .saturating_add(index.saturating_add(1))
        .min(total_frames) as f32
        / total_frames as f32
}

/// Identity assigned to a prepared voice generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GenerationId(u64);

impl GenerationId {
    pub(crate) const fn initial() -> Self {
        Self(1)
    }

    const fn unassigned() -> Self {
        Self(0)
    }

    /// Return the monotonically increasing numeric identity.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Lifecycle state of a runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    /// No process resources have been prepared.
    Unprepared,
    /// Process resources are ready but audio processing has not started.
    Prepared,
    /// The runtime accepts process blocks.
    Active,
    /// A fatal process error occurred and the runtime must be re-prepared.
    Faulted,
}

/// Reason a prepared update requires stream reactivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactivationReason {
    /// The candidate reports a different fixed latency.
    LatencyChanged,
    /// The external input bus count changed.
    InputChannelsChanged,
}

/// Error returned while preparing a control-side runtime update.
#[derive(Debug, Error, PartialEq)]
pub enum PrepareUpdateError {
    /// The candidate process specification or compiled instrument is invalid.
    #[error("could not prepare update: {0}")]
    Process(#[from] ProcessError),
}

/// Error returned when a prepared update cannot be published at a block boundary.
#[derive(Debug, Error, PartialEq)]
pub enum PublishError {
    /// The runtime has not been prepared.
    #[error("runtime is not prepared")]
    NotPrepared,
    /// The runtime is not accepting audio-side updates.
    #[error("runtime is not active")]
    NotActive,
    /// The update has already been published or destroyed.
    #[error("prepared update has already been consumed")]
    UpdateConsumed,
    /// The candidate process specification cannot be used by the active stream.
    #[error("prepared update process specification is incompatible")]
    ProcessSpecMismatch,
    /// The update must be applied after stream reactivation.
    #[error(
        "prepared update requires reactivation: {reason:?} ({current_latency_frames} -> {candidate_latency_frames} frames)"
    )]
    RequiresReactivation {
        /// Why the host must reactivate the stream.
        reason: ReactivationReason,
        /// Latency of the active runtime.
        current_latency_frames: usize,
        /// Latency of the candidate runtime.
        candidate_latency_frames: usize,
    },
    /// The runtime is retaining the maximum number of retired generations.
    #[error("live generation capacity is exhausted")]
    CapacityExceeded,
    /// Reclaimable resources must be taken before another update is published.
    #[error("reclaimable resource capacity is exhausted")]
    ReclaimCapacityExceeded,
    /// A global processor transition is already in progress.
    #[error("global processor transition is busy")]
    TransitionBusy,
    /// The generation identity counter reached its limit.
    #[error("generation id overflow")]
    GenerationIdOverflow,
    /// An internal runtime invariant was violated.
    #[error("runtime invariant is invalid")]
    InvalidState,
}

/// Metadata returned after a successful update publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishOutcome {
    /// Newly active generation identity.
    pub generation_id: GenerationId,
    /// Parameter catalog revision used by the active generation.
    pub parameter_catalog_revision: ParameterCatalogRevision,
    /// Fixed output latency in frames.
    pub reported_latency_frames: usize,
    /// Required external input channel count.
    pub required_input_channels: usize,
}

/// A fully prepared candidate runtime that can be moved to the audio side.
pub struct PreparedInstrumentUpdate {
    generation: Option<RuntimeGeneration>,
    spec: ProcessSpec,
}

impl PreparedInstrumentUpdate {
    /// Prepare all DSP resources for a compiled candidate on the control side.
    ///
    /// # Errors
    ///
    /// Returns an error when the process specification or any runtime resource is invalid.
    pub fn prepare(
        compiled: Arc<CompiledInstrument>,
        spec: ProcessSpec,
    ) -> Result<Self, PrepareUpdateError> {
        let mut generation = RuntimeGeneration::new(compiled);
        generation.prepare_inner(spec)?;
        generation.id = GenerationId::unassigned();
        Ok(Self {
            generation: Some(generation),
            spec,
        })
    }

    /// Return the candidate compiled instrument.
    #[must_use]
    pub fn compiled(&self) -> Option<&Arc<CompiledInstrument>> {
        self.generation.as_ref().map(RuntimeGeneration::compiled)
    }

    /// Return whether the update has been consumed by a successful publish.
    #[must_use]
    pub fn is_consumed(&self) -> bool {
        self.generation.is_none()
    }
}

/// Resources whose destruction is deferred to the control thread.
pub struct ReclaimableRuntimeResource {
    inner: ReclaimableRuntimeResourceInner,
}

#[allow(clippy::large_enum_variant)]
enum ReclaimableRuntimeResourceInner {
    Generation(RuntimeGeneration),
    GlobalProcessor(super::super::processor::StereoProcessorChain),
}

impl ReclaimableRuntimeResource {
    fn generation(generation: RuntimeGeneration) -> Self {
        Self {
            inner: ReclaimableRuntimeResourceInner::Generation(generation),
        }
    }

    fn global_processor(processor: super::super::processor::StereoProcessorChain) -> Self {
        Self {
            inner: ReclaimableRuntimeResourceInner::GlobalProcessor(processor),
        }
    }

    /// Return the kind of resource held for deferred destruction.
    #[must_use]
    pub fn kind(&self) -> ReclaimableRuntimeResourceKind {
        match &self.inner {
            ReclaimableRuntimeResourceInner::Generation(generation) => {
                let _ = generation;
                ReclaimableRuntimeResourceKind::Generation
            }
            ReclaimableRuntimeResourceInner::GlobalProcessor(processor) => {
                let _ = processor;
                ReclaimableRuntimeResourceKind::GlobalProcessor
            }
        }
    }
}

/// Kind of a deferred runtime resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimableRuntimeResourceKind {
    /// A retired voice generation.
    Generation,
    /// A global processor chain replaced during reconfiguration.
    GlobalProcessor,
}

struct GenerationAudio {
    left: Vec<f32>,
    right: Vec<f32>,
}

impl GenerationAudio {
    fn new(max_block_size: usize) -> Self {
        Self {
            left: vec![0.0; max_block_size],
            right: vec![0.0; max_block_size],
        }
    }
}

struct GlobalTransition {
    old_generation: GenerationId,
    total_frames: usize,
    elapsed_frames: usize,
}

/// Runtime lifecycle and block-boundary generation manager.
pub struct InstrumentRuntime {
    compiled: Arc<CompiledInstrument>,
    state: RuntimeState,
    spec: Option<ProcessSpec>,
    absolute_frame: u64,
    next_generation_id: u64,
    active: Option<RuntimeGeneration>,
    retired: Vec<RuntimeGeneration>,
    reclaimable: Vec<ReclaimableRuntimeResource>,
    generation_audio: Vec<GenerationAudio>,
    mix_left: Vec<f32>,
    mix_right: Vec<f32>,
    old_global_left: Vec<f32>,
    old_global_right: Vec<f32>,
    new_global_left: Vec<f32>,
    new_global_right: Vec<f32>,
    global_transition: Option<GlobalTransition>,
    stale_parameter_events: u64,
}

impl InstrumentRuntime {
    /// Create an unprepared runtime from a compiled instrument.
    #[must_use]
    pub fn new(compiled: Arc<CompiledInstrument>) -> Self {
        Self {
            compiled,
            state: RuntimeState::Unprepared,
            spec: None,
            absolute_frame: 0,
            next_generation_id: 2,
            active: None,
            retired: Vec::with_capacity(MAX_RETIRED_GENERATIONS),
            reclaimable: Vec::with_capacity(MAX_RECLAIMABLE_RESOURCES),
            generation_audio: Vec::new(),
            mix_left: Vec::new(),
            mix_right: Vec::new(),
            old_global_left: Vec::new(),
            old_global_right: Vec::new(),
            new_global_left: Vec::new(),
            new_global_right: Vec::new(),
            global_transition: None,
            stale_parameter_events: 0,
        }
    }

    /// Return the current lifecycle state.
    #[must_use]
    pub fn state(&self) -> RuntimeState {
        self.state
    }

    /// Return the current active generation identity, or zero while unprepared.
    #[must_use]
    pub fn generation_id(&self) -> GenerationId {
        self.active
            .as_ref()
            .map_or(GenerationId::unassigned(), RuntimeGeneration::generation_id)
    }

    /// Return the immutable compiled configuration currently selected by the runtime.
    #[must_use]
    pub fn compiled(&self) -> &Arc<CompiledInstrument> {
        &self.compiled
    }

    /// Return the number of voices in the active generation.
    #[must_use]
    pub fn voice_count(&self) -> usize {
        self.active
            .as_ref()
            .map_or(0, RuntimeGeneration::voice_count)
    }

    /// Return one active voice state for diagnostics and tests.
    #[must_use]
    pub fn voice_state(&self, index: usize) -> Option<VoiceState> {
        self.active.as_ref()?.voice_state(index)
    }

    /// Return the runtime's absolute frame position.
    #[must_use]
    pub fn absolute_frame(&self) -> u64 {
        self.absolute_frame
    }

    /// Return the number of stale parameter events ignored by the active runtime.
    #[must_use]
    pub fn stale_parameter_event_count(&self) -> u64 {
        self.stale_parameter_events
    }

    /// Prepare process resources on the control side.
    ///
    /// # Errors
    ///
    /// Returns a process error without replacing the current prepared runtime when preparation
    /// fails.
    pub fn prepare(&mut self, spec: ProcessSpec) -> Result<(), ProcessError> {
        if self.state == RuntimeState::Active {
            return Err(ProcessError::NotActive);
        }
        let mut generation = RuntimeGeneration::new(Arc::clone(&self.compiled));
        generation.prepare_inner(spec)?;
        generation.id = GenerationId::initial();
        self.active = Some(generation);
        self.retired.clear();
        self.reclaimable.clear();
        self.prepare_audio_scratch(spec.max_block_size);
        self.spec = Some(spec);
        self.absolute_frame = 0;
        self.next_generation_id = 2;
        self.global_transition = None;
        self.state = RuntimeState::Prepared;
        Ok(())
    }

    fn prepare_audio_scratch(&mut self, max_block_size: usize) {
        self.generation_audio = (0..MAX_LIVE_GENERATIONS)
            .map(|_| GenerationAudio::new(max_block_size))
            .collect();
        self.mix_left.resize(max_block_size, 0.0);
        self.mix_right.resize(max_block_size, 0.0);
        self.old_global_left.resize(max_block_size, 0.0);
        self.old_global_right.resize(max_block_size, 0.0);
        self.new_global_left.resize(max_block_size, 0.0);
        self.new_global_right.resize(max_block_size, 0.0);
    }

    /// Activate a prepared runtime immediately before audio processing starts.
    ///
    /// # Errors
    ///
    /// Returns `NotPrepared` unless the runtime has prepared resources.
    pub fn activate(&mut self) -> Result<(), ProcessError> {
        if self.active.is_none() || self.spec.is_none() {
            return Err(ProcessError::NotPrepared);
        }
        if self.state != RuntimeState::Prepared {
            return Err(ProcessError::NotActive);
        }
        self.state = RuntimeState::Active;
        Ok(())
    }

    /// Deactivate after the host stops the audio stream while retaining DSP resources.
    ///
    /// # Errors
    ///
    /// Returns `NotActive` unless the runtime is active.
    pub fn deactivate(&mut self) -> Result<(), ProcessError> {
        if self.state != RuntimeState::Active {
            return Err(ProcessError::NotActive);
        }
        self.state = RuntimeState::Prepared;
        Ok(())
    }

    /// Prepare a candidate update without affecting the active runtime.
    ///
    /// # Errors
    ///
    /// Returns an error when the candidate cannot construct every required runtime resource.
    pub fn prepare_update(
        compiled: Arc<CompiledInstrument>,
        spec: ProcessSpec,
    ) -> Result<PreparedInstrumentUpdate, PrepareUpdateError> {
        PreparedInstrumentUpdate::prepare(compiled, spec)
    }

    /// Publish a prepared update at the start of an audio block.
    ///
    /// # Errors
    ///
    /// Returns an error when the lifecycle, process specification, latency, or fixed capacity
    /// contract prevents publication. The active runtime and update remain unchanged on error.
    ///
    /// # Panics
    ///
    /// This method does not panic when the runtime's lifecycle and capacity invariants hold.
    pub fn publish_prepared(
        &mut self,
        update: &mut PreparedInstrumentUpdate,
    ) -> Result<PublishOutcome, PublishError> {
        if self.state != RuntimeState::Active {
            return Err(if self.spec.is_some() {
                PublishError::NotActive
            } else {
                PublishError::NotPrepared
            });
        }
        let candidate = update
            .generation
            .as_ref()
            .ok_or(PublishError::UpdateConsumed)?;
        let active = self.active.as_ref().ok_or(PublishError::InvalidState)?;
        if self.global_transition.is_some() {
            return Err(PublishError::TransitionBusy);
        }
        let current_latency = active.compiled.reported_latency_frames();
        let candidate_latency = candidate.compiled.reported_latency_frames();
        if current_latency != candidate_latency {
            return Err(PublishError::RequiresReactivation {
                reason: ReactivationReason::LatencyChanged,
                current_latency_frames: current_latency,
                candidate_latency_frames: candidate_latency,
            });
        }
        if active.compiled.required_input_channels() != candidate.compiled.required_input_channels()
        {
            return Err(PublishError::RequiresReactivation {
                reason: ReactivationReason::InputChannelsChanged,
                current_latency_frames: current_latency,
                candidate_latency_frames: candidate_latency,
            });
        }
        if self.spec != Some(update.spec) {
            return Err(PublishError::ProcessSpecMismatch);
        }
        if self.retired.len() >= MAX_RETIRED_GENERATIONS {
            return Err(PublishError::CapacityExceeded);
        }
        let global_changed =
            active.compiled.global_processors != candidate.compiled.global_processors;
        let reclaim_slots_needed = self
            .retired
            .len()
            .saturating_add(1)
            .saturating_add(usize::from(global_changed));
        if self.reclaimable.len().saturating_add(reclaim_slots_needed) > MAX_RECLAIMABLE_RESOURCES {
            return Err(PublishError::ReclaimCapacityExceeded);
        }
        let generation_id = GenerationId(self.next_generation_id);
        let next_generation_id = self
            .next_generation_id
            .checked_add(1)
            .ok_or(PublishError::GenerationIdOverflow)?;
        let mut candidate = update
            .generation
            .take()
            .ok_or(PublishError::UpdateConsumed)?;
        let mut active = self.active.take().ok_or(PublishError::InvalidState)?;
        let candidate_compiled = Arc::clone(candidate.compiled());
        candidate.id = generation_id;
        candidate.absolute_frame = self.absolute_frame;
        candidate.inherit_performance_controls(&active);
        if !global_changed {
            candidate.swap_global_processors(&mut active);
        }
        let old_generation_id = active.id;
        self.retired.push(active);
        self.active = Some(candidate);
        self.compiled = candidate_compiled;
        self.next_generation_id = next_generation_id;
        if global_changed {
            let total_frames = global_transition_frames(self.compiled.process_sample_rate);
            self.global_transition = Some(GlobalTransition {
                old_generation: old_generation_id,
                total_frames,
                elapsed_frames: 0,
            });
        }
        Ok(PublishOutcome {
            generation_id,
            parameter_catalog_revision: self.compiled.parameter_catalog_revision(),
            reported_latency_frames: self.compiled.reported_latency_frames(),
            required_input_channels: self.compiled.required_input_channels(),
        })
    }

    /// Move one deferred resource to the caller without destroying it.
    pub fn take_reclaimable(&mut self) -> Option<ReclaimableRuntimeResource> {
        self.reclaimable.pop()
    }

    fn validate_events(
        &mut self,
        events: &[crate::process::ProcessEvent],
    ) -> Result<(), ProcessError> {
        let active = self.active.as_ref().ok_or(ProcessError::NotPrepared)?;
        let revision = active.compiled.parameter_catalog_revision();
        for (event_index, event) in events.iter().enumerate() {
            match event.kind {
                ProcessEventKind::ParameterChange {
                    catalog_revision, ..
                } if catalog_revision != revision => {
                    self.stale_parameter_events = self.stale_parameter_events.saturating_add(1);
                }
                ProcessEventKind::ParameterChange { parameter, .. }
                    if active.compiled.parameter_descriptor(parameter).is_none() =>
                {
                    return Err(ProcessError::ParameterHandleOutOfRange {
                        handle: parameter.index(),
                    });
                }
                ProcessEventKind::NoteOn { note_id, .. } => {
                    let already_held = active.contains_note_id(note_id)
                        || self.retired.iter().any(|generation| generation.contains_note_id(note_id))
                        || events[..event_index].iter().any(|previous| {
                            matches!(previous.kind, ProcessEventKind::NoteOn { note_id: previous_id, .. } if previous_id == note_id)
                        });
                    if already_held {
                        return Err(ProcessError::DuplicateNoteId { note_id });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn process_inner(&mut self, block: &mut ProcessBlock<'_>) -> Result<(), ProcessError> {
        self.active
            .as_ref()
            .ok_or(ProcessError::NotPrepared)?
            .validate_process_block(block, true)?;
        for retired in &self.retired {
            retired.validate_process_block(block, false)?;
        }
        self.validate_events(block.events)?;
        let next_frame = self
            .absolute_frame
            .checked_add(block.frames as u64)
            .ok_or(ProcessError::FrameOverflow)?;
        if self.retired.is_empty() && self.global_transition.is_none() {
            self.active
                .as_mut()
                .ok_or(ProcessError::NotPrepared)?
                .process_inner(block)?;
            self.absolute_frame = next_frame;
            return Ok(());
        }
        self.process_multigeneration(block, next_frame)
    }

    fn process_multigeneration(
        &mut self,
        block: &mut ProcessBlock<'_>,
        next_frame: u64,
    ) -> Result<(), ProcessError> {
        self.mix_left[..block.frames].fill(0.0);
        self.mix_right[..block.frames].fill(0.0);
        if block.frames == 0 {
            self.absolute_frame = next_frame;
            return Ok(());
        }

        let generation_count = self.retired.len() + 1;
        for audio in &mut self.generation_audio[..generation_count] {
            audio.left[..block.frames].fill(0.0);
            audio.right[..block.frames].fill(0.0);
        }

        let mut cursor = 0;
        let mut event_index = 0;
        while cursor < block.frames {
            cursor = self.process_multigeneration_span(block, cursor, &mut event_index)?;
        }
        self.absolute_frame = next_frame;
        self.finish_transition(block.frames);
        self.reclaim_idle_generations();
        Ok(())
    }

    fn process_multigeneration_span(
        &mut self,
        block: &mut ProcessBlock<'_>,
        cursor: usize,
        event_index: &mut usize,
    ) -> Result<usize, ProcessError> {
        let event_start = *event_index;
        while *event_index < block.events.len()
            && block.events[*event_index].sample_offset == cursor
        {
            *event_index += 1;
        }
        let events = &block.events[event_start..*event_index];
        if !events.is_empty() {
            let absolute_frame = block
                .context
                .absolute_frame
                .checked_add(cursor as u64)
                .ok_or(ProcessError::FrameOverflow)?;
            self.active
                .as_mut()
                .ok_or(ProcessError::NotPrepared)?
                .apply_events_at(events, absolute_frame, true, true)?;
            for retired in &mut self.retired {
                retired.apply_events_at(events, absolute_frame, false, false)?;
            }
        }

        let mut end = block.frames;
        if let Some(active) = self.active.as_ref() {
            end = end.min(active.next_span_end(block, cursor, *event_index)?);
        }
        for retired in &self.retired {
            end = end.min(retired.next_span_end(block, cursor, *event_index)?);
        }
        let context = block.context;
        let input = block.input;
        let mut generation_index = 0;
        if let Some(active) = self.active.as_mut() {
            Self::process_generation_span(
                active,
                context,
                input,
                cursor,
                end,
                &mut self.generation_audio[generation_index],
            )?;
            generation_index += 1;
        }
        for retired in &mut self.retired {
            Self::process_generation_span(
                retired,
                context,
                input,
                cursor,
                end,
                &mut self.generation_audio[generation_index],
            )?;
            generation_index += 1;
        }
        for audio in &self.generation_audio[..generation_index] {
            for index in cursor..end {
                self.mix_left[index] += audio.left[index];
                self.mix_right[index] += audio.right[index];
            }
        }
        let external_channels = [
            input
                .first()
                .and_then(|channel| channel.get(cursor..end))
                .unwrap_or(&[]),
            input
                .get(1)
                .and_then(|channel| channel.get(cursor..end))
                .unwrap_or(&[]),
        ];
        let external = ExternalAudioBlock::new(&external_channels[..input.len().min(2)]);
        self.process_global_span(context.tempo_bpm, external, cursor, end)?;
        let (left_channels, right_channels) = block.output.split_at_mut(1);
        let left = left_channels
            .first_mut()
            .and_then(|channel| channel.get_mut(cursor..end))
            .ok_or_else(invalid_state)?;
        let right = right_channels
            .first_mut()
            .and_then(|channel| channel.get_mut(cursor..end))
            .ok_or_else(invalid_state)?;
        left.copy_from_slice(&self.mix_left[cursor..end]);
        right.copy_from_slice(&self.mix_right[cursor..end]);
        Ok(end)
    }

    fn process_generation_span(
        generation: &mut RuntimeGeneration,
        context: crate::process::ProcessContext,
        input: &[&[f32]],
        start: usize,
        end: usize,
        audio: &mut GenerationAudio,
    ) -> Result<(), ProcessError> {
        let mut output = [&mut audio.left[..end], &mut audio.right[..end]];
        generation.render_voice_span(context, input, start, end, &mut output)?;
        generation.evaluate_global_targets_for_span(end - start)
    }

    #[allow(clippy::cast_precision_loss)]
    fn process_global_span(
        &mut self,
        tempo_bpm: f64,
        external: ExternalAudioBlock<'_>,
        start: usize,
        end: usize,
    ) -> Result<(), ProcessError> {
        let Some(active) = self.active.as_mut() else {
            return Err(ProcessError::NotPrepared);
        };
        if let Some(transition) = self.global_transition.as_ref() {
            let old_generation = transition.old_generation;
            let elapsed_frames = transition.elapsed_frames;
            let total_frames = transition.total_frames;
            let old = self
                .retired
                .iter_mut()
                .find(|generation| generation.generation_id() == old_generation)
                .ok_or(PublishError::InvalidState)
                .map_err(|_| ProcessError::ProcessorFailure {
                    kind: crate::process::ProcessorFailureKind::InvalidState,
                })?;
            self.old_global_left[start..end].copy_from_slice(&self.mix_left[start..end]);
            self.old_global_right[start..end].copy_from_slice(&self.mix_right[start..end]);
            self.new_global_left[start..end].copy_from_slice(&self.mix_left[start..end]);
            self.new_global_right[start..end].copy_from_slice(&self.mix_right[start..end]);
            old.process_global(
                tempo_bpm,
                external,
                &mut self.old_global_left[start..end],
                &mut self.old_global_right[start..end],
            )?;
            active.process_global(
                tempo_bpm,
                external,
                &mut self.new_global_left[start..end],
                &mut self.new_global_right[start..end],
            )?;
            for index in start..end {
                let position = global_transition_position(elapsed_frames, index, total_frames);
                let old_gain = (position * FRAC_PI_2).cos();
                let new_gain = (position * FRAC_PI_2).sin();
                self.mix_left[index] =
                    self.old_global_left[index] * old_gain + self.new_global_left[index] * new_gain;
                self.mix_right[index] = self.old_global_right[index] * old_gain
                    + self.new_global_right[index] * new_gain;
            }
        } else {
            active.process_global(
                tempo_bpm,
                external,
                &mut self.mix_left[start..end],
                &mut self.mix_right[start..end],
            )?;
        }
        Ok(())
    }

    fn finish_transition(&mut self, frames: usize) {
        let Some(transition) = self.global_transition.as_mut() else {
            return;
        };
        transition.elapsed_frames = transition.elapsed_frames.saturating_add(frames);
        if transition.elapsed_frames < transition.total_frames {
            return;
        }
        let old_id = transition.old_generation;
        self.global_transition = None;
        if let Some(old) = self
            .retired
            .iter_mut()
            .find(|generation| generation.generation_id() == old_id)
            && let Some(global) = old.take_global()
        {
            self.reclaimable
                .push(ReclaimableRuntimeResource::global_processor(global));
        }
    }

    fn reclaim_idle_generations(&mut self) {
        let transition_id = self
            .global_transition
            .as_ref()
            .map(|value| value.old_generation);
        let mut index = 0;
        while index < self.retired.len() {
            let generation = &self.retired[index];
            if generation.is_idle() && Some(generation.generation_id()) != transition_id {
                let generation = self.retired.remove(index);
                self.reclaimable
                    .push(ReclaimableRuntimeResource::generation(generation));
            } else {
                index += 1;
            }
        }
    }

    /// Reset all live generations without destroying their resources on the audio thread.
    ///
    /// # Errors
    ///
    /// Returns `NotPrepared` when no process resources exist.
    pub fn reset(&mut self) -> Result<(), ProcessError> {
        if self.spec.is_none() {
            return Err(ProcessError::NotPrepared);
        }
        let active = self.active.as_mut().ok_or(ProcessError::NotPrepared)?;
        if let Err(error) = active.reset_for_runtime() {
            self.state = RuntimeState::Faulted;
            return Err(error);
        }
        while let Some(generation) = self.retired.pop() {
            self.reclaimable
                .push(ReclaimableRuntimeResource::generation(generation));
        }
        self.global_transition = None;
        self.absolute_frame = 0;
        self.mix_left.fill(0.0);
        self.mix_right.fill(0.0);
        Ok(())
    }

    /// Return trace observations for the active generation.
    pub(crate) fn trace_snapshots(
        &self,
        handles: &[crate::parameter::ParameterHandle],
        frame: u64,
        sample_rate: f64,
        context: crate::process::ProcessContext,
    ) -> Result<
        Vec<(
            crate::parameter::ParameterHandle,
            crate::trace::TraceObservation,
        )>,
        ProcessError,
    > {
        self.active
            .as_ref()
            .ok_or(ProcessError::NotPrepared)?
            .trace_snapshots(handles, frame, sample_rate, context)
    }
}

impl InstrumentProcessor for InstrumentRuntime {
    fn prepare(&mut self, spec: ProcessSpec) -> Result<(), ProcessError> {
        InstrumentRuntime::prepare(self, spec)
    }

    fn activate(&mut self) -> Result<(), ProcessError> {
        InstrumentRuntime::activate(self)
    }

    fn process(&mut self, mut block: ProcessBlock<'_>) -> Result<(), ProcessError> {
        clear_output(&mut *block.output, block.frames);
        if self.state != RuntimeState::Active {
            return Err(if self.spec.is_some() {
                ProcessError::NotActive
            } else {
                ProcessError::NotPrepared
            });
        }
        let result = self.process_inner(&mut block);
        if result.is_err() {
            clear_output(&mut *block.output, block.frames);
            self.state = RuntimeState::Faulted;
        }
        result
    }

    fn reset(&mut self) -> Result<(), ProcessError> {
        InstrumentRuntime::reset(self)
    }

    fn deactivate(&mut self) -> Result<(), ProcessError> {
        InstrumentRuntime::deactivate(self)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{InstrumentRuntime, global_transition_position};
    use crate::process::{InstrumentProcessor, ProcessEvent, ProcessSpec};
    use crate::runtime::instrument::tests::runtime;

    #[test]
    fn publish_process_and_reclaim_take_do_not_allocate() {
        let generation = runtime();
        let compiled = Arc::clone(generation.compiled());
        let mut runtime = InstrumentRuntime::new(compiled);
        let spec = ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec");
        runtime.prepare(spec).expect("runtime prepares");
        runtime.activate().expect("runtime activates");
        let mut update = InstrumentRuntime::prepare_update(Arc::clone(runtime.compiled()), spec)
            .expect("update prepares");
        let allocations = crate::test_allocator::count_allocations(|| {
            runtime
                .publish_prepared(&mut update)
                .expect("update publishes");
        });
        assert_eq!(allocations, 0);

        let no_events: [ProcessEvent; 0] = [];
        let allocations = crate::test_allocator::count_allocations(|| {
            let mut left = [0.0_f32; 64];
            let mut right = [0.0_f32; 64];
            let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
            runtime
                .process(crate::process::ProcessBlock {
                    frames: 64,
                    context: crate::process::ProcessContext {
                        absolute_frame: 0,
                        tempo_bpm: 120.0,
                        beat_position: 0.0,
                        bar_position: 0.0,
                        time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                        transport_state: crate::process::TransportState::Playing,
                    },
                    events: &no_events,
                    input: &[],
                    output: &mut output,
                })
                .expect("runtime processes");
        });
        assert_eq!(allocations, 0);

        runtime.reset().expect("runtime resets");
        let mut reclaimed = None;
        let allocations = crate::test_allocator::count_allocations(|| {
            reclaimed = runtime.take_reclaimable();
        });
        assert_eq!(allocations, 0);
        assert!(reclaimed.is_some());
        drop(reclaimed);
    }

    #[test]
    fn global_transition_position_is_continuous_across_quantum_spans() {
        let total_frames = 240;
        let first_span_end = global_transition_position(0, 31, total_frames);
        let second_span_start = global_transition_position(0, 32, total_frames);
        let second_block_start = global_transition_position(64, 0, total_frames);
        let span_positions: Vec<_> = (0..64)
            .map(|index| global_transition_position(0, index, total_frames))
            .collect();

        assert!((first_span_end - 32.0 / 240.0).abs() < 1.0e-7);
        assert!((second_span_start - 33.0 / 240.0).abs() < 1.0e-7);
        assert!(second_span_start > first_span_end);
        assert!(
            span_positions
                .windows(2)
                .all(|window| window[1] > window[0])
        );
        assert!(second_block_start > global_transition_position(0, 63, total_frames));
    }
}
