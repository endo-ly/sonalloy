use serde::Serialize;

use crate::parameter::{ModulationUnit, ParameterHandle, ParameterUnit};
use crate::process::{ProcessContext, ProcessError};
use crate::runtime::InstrumentRuntime;

/// Maximum number of observations retained by one offline trace.
pub const MAX_TRACE_OBSERVATIONS: usize = 100_000;

/// Request for selected-parameter observations during an offline render.
#[derive(Debug, Clone)]
pub struct TraceRequest {
    /// Parameters to observe in request order.
    pub parameters: Vec<ParameterHandle>,
    /// Period between regular observations in frames.
    pub every_frames: usize,
}

/// Machine-readable trace report grouped by selected parameter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderTraceReport {
    /// Regular observation period.
    pub every_frames: usize,
    /// Reports in the order requested by the caller.
    pub parameters: Vec<TraceParameterReport>,
}

/// Trace observations for one selected parameter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceParameterReport {
    /// Canonical parameter identifier.
    pub parameter: String,
    /// Native target unit.
    pub unit: ParameterUnit,
    /// Observations in timeline and voice order.
    pub observations: Vec<TraceObservation>,
}

/// One runtime parameter observation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceObservation {
    /// Public frame after which the state was sampled.
    pub frame: u64,
    /// Public time in seconds.
    pub seconds: f64,
    /// Canonical parameter identifier.
    pub parameter: String,
    /// Native target unit.
    pub unit: ParameterUnit,
    /// Voice identity, or null for a global target.
    pub voice: Option<TraceVoice>,
    /// Native base value before route evaluation.
    pub base: f32,
    /// Route details in Definition order.
    pub routes: Vec<TraceRoute>,
    /// Native value before the target clamp.
    pub before_clamp: f32,
    /// Native value after the target clamp.
    #[serde(rename = "final")]
    pub final_value: f32,
    /// Whether the target clamp changed the value.
    pub clamped: bool,
    /// Runtime pitch glide offset added after Layer Tuning route evaluation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portamento_offset_cents: Option<f32>,
}

/// Identity and state of a voice at one trace point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TraceVoice {
    /// Prepared voice index.
    pub index: usize,
    /// Frontend note identity.
    pub note_id: u64,
    /// MIDI note number.
    pub note_number: u8,
    /// MIDI velocity.
    pub velocity: u8,
    /// Current voice lifecycle state.
    pub state: TraceVoiceState,
}

/// Public voice lifecycle state used by Trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceVoiceState {
    /// Note is held or sustaining.
    Active,
    /// Note Off has started release.
    Releasing,
    /// Voice is fading before a pending note starts.
    StealFading,
}

/// One route's source and direct-depth contribution.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceRoute {
    /// Source identifier.
    pub source: String,
    /// Raw source value before curve shaping.
    pub raw: f32,
    /// Source value after curve shaping.
    pub shaped: f32,
    /// Definition-level depth.
    pub depth: TraceDepth,
    /// Contribution in the target modulation domain.
    pub contribution: TraceContribution,
}

/// Definition-level depth included in a trace record.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TraceDepth {
    /// Signed depth.
    pub value: f32,
    /// Depth unit.
    pub unit: ModulationUnit,
}

/// One route's signed domain delta and logarithmic factor when applicable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TraceContribution {
    /// Signed domain delta.
    pub value: f32,
    /// Domain unit.
    pub unit: ModulationUnit,
    /// Multiplicative factor for Log2 targets.
    pub factor: Option<f32>,
}

pub(crate) struct TraceCollector {
    target_handles: Vec<ParameterHandle>,
    sample_rate: f64,
    last_public_frame: Option<u64>,
    observation_count: usize,
    report: RenderTraceReport,
}

pub(crate) enum TraceCollectError {
    Process(ProcessError),
    LimitExceeded { observed: usize, limit: usize },
}

impl From<ProcessError> for TraceCollectError {
    fn from(error: ProcessError) -> Self {
        Self::Process(error)
    }
}

impl TraceCollector {
    pub(crate) fn new(
        request: &TraceRequest,
        compiled: &crate::compiler::CompiledInstrument,
    ) -> Self {
        let parameters = request
            .parameters
            .iter()
            .map(|handle| {
                let descriptor = compiled
                    .parameter_descriptor(*handle)
                    .expect("trace parameter handle must be valid");
                TraceParameterReport {
                    parameter: descriptor.id.clone(),
                    unit: descriptor.unit,
                    observations: Vec::new(),
                }
            })
            .collect();
        Self {
            target_handles: request.parameters.clone(),
            sample_rate: compiled.process_sample_rate,
            last_public_frame: None,
            observation_count: 0,
            report: RenderTraceReport {
                every_frames: request.every_frames,
                parameters,
            },
        }
    }

    pub(crate) fn observe(
        &mut self,
        runtime: &InstrumentRuntime,
        frame: u64,
        context: ProcessContext,
    ) -> Result<(), TraceCollectError> {
        if self.last_public_frame == Some(frame) {
            return Ok(());
        }
        let observations =
            runtime.trace_snapshots(&self.target_handles, frame, self.sample_rate, context)?;
        let Some(observation_count) = self.observation_count.checked_add(observations.len()) else {
            return Err(TraceCollectError::LimitExceeded {
                observed: usize::MAX,
                limit: MAX_TRACE_OBSERVATIONS,
            });
        };
        if observation_count > MAX_TRACE_OBSERVATIONS {
            return Err(TraceCollectError::LimitExceeded {
                observed: observation_count,
                limit: MAX_TRACE_OBSERVATIONS,
            });
        }
        for (handle, observation) in observations {
            let Some(index) = self
                .target_handles
                .iter()
                .position(|candidate| *candidate == handle)
            else {
                return Err(TraceCollectError::Process(ProcessError::ProcessorFailure {
                    kind: crate::process::ProcessorFailureKind::InvalidState,
                }));
            };
            self.report
                .parameters
                .get_mut(index)
                .ok_or(ProcessError::ProcessorFailure {
                    kind: crate::process::ProcessorFailureKind::InvalidState,
                })?
                .observations
                .push(observation);
        }
        self.observation_count = observation_count;
        self.last_public_frame = Some(frame);
        Ok(())
    }

    pub(crate) fn finish(self) -> RenderTraceReport {
        self.report
    }
}
