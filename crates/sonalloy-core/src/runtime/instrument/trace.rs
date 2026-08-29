#[allow(clippy::wildcard_imports)]
use super::*;
use crate::runtime::modulation;

impl InstrumentRuntime {
    pub(super) fn trace_observation(
        &self,
        handle: ParameterHandle,
        frame: u64,
        sample_rate: f64,
        context: ProcessContext,
        voice_info: Option<TraceVoice>,
        voice: Option<&VoiceRuntime>,
    ) -> Result<TraceObservation, ProcessError> {
        let descriptor = self.compiled.parameter_descriptor(handle).ok_or(
            ProcessError::ParameterHandleOutOfRange {
                handle: handle.index(),
            },
        )?;
        let base_normalized = self
            .parameter_states
            .get(handle.index())
            .ok_or_else(invalid_state)?
            .current();
        let mut routes = Vec::new();
        let mut domain_sum = 0.0;
        for route in self
            .compiled
            .routes_for_checked(handle)
            .ok_or_else(invalid_state)?
        {
            let raw = match route.source {
                CompiledSourceRef::Voice(source) => voice
                    .and_then(|voice| voice.trace_source_value(&self.compiled, source))
                    .ok_or_else(invalid_state)?,
                CompiledSourceRef::Instrument(source) => {
                    self.trace_instrument_source(source, context)?
                }
            };
            let shaped = modulation::curve_value(raw, route.curve);
            let contribution = modulation::route_domain_delta(raw, route.depth, route.curve);
            domain_sum += contribution;
            routes.push(TraceRoute {
                source: trace_source_id(&self.compiled, route.source),
                raw,
                shaped,
                depth: TraceDepth {
                    value: route.depth,
                    unit: descriptor.modulation_unit(),
                },
                contribution: TraceContribution {
                    value: contribution,
                    unit: descriptor.modulation_unit(),
                    factor: (descriptor.scale == ParameterScale::Log2)
                        .then(|| 2.0_f32.powf(contribution)),
                },
            });
        }
        let effective_maximum = self
            .compiled
            .effective_parameter_maximum(handle)
            .ok_or_else(invalid_state)?;
        let evaluated = apply_domain_sum_with_maximum(
            descriptor,
            base_normalized,
            domain_sum,
            effective_maximum,
        )?;
        #[allow(clippy::cast_precision_loss)]
        let seconds = frame as f64 / sample_rate;
        let portamento_offset_cents = voice
            .filter(|_| {
                matches!(descriptor.owner, ParameterOwner::Layer { .. })
                    && descriptor.id.ends_with(".tuning")
            })
            .map(VoiceRuntime::portamento_offset_cents)
            .filter(|offset| offset.total_cmp(&0.0).is_ne());
        let effective_value = portamento_offset_cents.map(|offset| evaluated.final_value + offset);
        Ok(TraceObservation {
            frame,
            seconds,
            parameter: descriptor.id.clone(),
            unit: descriptor.unit,
            voice: voice_info,
            base: evaluated.base,
            routes,
            before_clamp: evaluated.unclamped,
            final_value: evaluated.final_value,
            clamped: evaluated.clamped,
            portamento_offset_cents,
            effective_value,
        })
    }

    fn trace_instrument_source(
        &self,
        handle: crate::compiler::InstrumentSourceHandle,
        context: ProcessContext,
    ) -> Result<f32, ProcessError> {
        let source = self
            .compiled
            .instrument_sources
            .get(handle.index())
            .ok_or_else(invalid_state)?;
        Ok(match &source.source {
            CompiledInstrumentSourceKind::PitchBend => self.pitch_bend.current(),
            CompiledInstrumentSourceKind::ModWheel => self.mod_wheel.current(),
            CompiledInstrumentSourceKind::Aftertouch => self.aftertouch.current(),
            CompiledInstrumentSourceKind::Macro { parameter } => self
                .parameter_states
                .get(parameter.index())
                .ok_or_else(invalid_state)?
                .current(),
            CompiledInstrumentSourceKind::BeatPhase => phase_fraction(context.beat_position),
            CompiledInstrumentSourceKind::BarPhase => phase_fraction(context.bar_position),
            CompiledInstrumentSourceKind::EnvelopeFollower(_) => self
                .instrument_source_states
                .get(handle.index())
                .and_then(Option::as_ref)
                .ok_or_else(invalid_state)?
                .value(),
        })
    }
}

fn trace_source_id(compiled: &CompiledInstrument, source: CompiledSourceRef) -> String {
    match source {
        CompiledSourceRef::Voice(handle) => compiled
            .sources
            .get(handle.index())
            .map_or_else(|| "unknown".to_owned(), |source| source.id.clone()),
        CompiledSourceRef::Instrument(handle) => {
            compiled.instrument_sources.get(handle.index()).map_or_else(
                || "unknown".to_owned(),
                |source| match &source.source {
                    CompiledInstrumentSourceKind::PitchBend => "pitch_bend".to_owned(),
                    CompiledInstrumentSourceKind::ModWheel => "mod_wheel".to_owned(),
                    CompiledInstrumentSourceKind::Aftertouch => "aftertouch".to_owned(),
                    CompiledInstrumentSourceKind::Macro { parameter } => compiled
                        .parameter_descriptor(*parameter)
                        .map_or_else(|| "unknown".to_owned(), |descriptor| descriptor.id.clone()),
                    CompiledInstrumentSourceKind::BeatPhase => "transport_beat_phase".to_owned(),
                    CompiledInstrumentSourceKind::BarPhase => "transport_bar_phase".to_owned(),
                    CompiledInstrumentSourceKind::EnvelopeFollower(_) => source.id.clone(),
                },
            )
        }
    }
}
