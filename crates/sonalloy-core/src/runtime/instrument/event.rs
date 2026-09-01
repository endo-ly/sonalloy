use std::sync::Arc;

use super::super::smoothing::rounded_frame_count;
use super::super::voice::{NoteRequest, PreparedLayerSelection, VoiceState};
use super::{
    HeldNote, MAX_MONOPHONIC_HELD_NOTES, RuntimeGeneration, STEAL_FADE_SECONDS,
    control_smoothing_frames, invalid_state,
};
use crate::compiler::{CompiledGenerator, CompiledPerformanceMode, CompiledSampleZone};
use crate::definition::LayerTriggerEvent;
use crate::process::{ProcessError, ProcessEventKind};

impl RuntimeGeneration {
    pub(super) fn apply_event(
        &mut self,
        event: ProcessEventKind,
        absolute_frame: u64,
        accept_note_ons: bool,
        accept_parameter_changes: bool,
    ) -> Result<(), ProcessError> {
        let spec = self.spec.ok_or(ProcessError::NotPrepared)?;
        match event {
            ProcessEventKind::NoteOn {
                note_id,
                note_number,
                velocity,
            } => {
                if !accept_note_ons {
                    return Ok(());
                }
                let request = NoteRequest::new(note_id, note_number, velocity, absolute_frame);
                match self.compiled.performance.mode {
                    CompiledPerformanceMode::Polyphonic { .. } => {
                        if !self.prepare_note_request(request)? {
                            return Ok(());
                        }
                        let voice_index = self.select_voice();
                        let fade_frames =
                            rounded_frame_count(spec.sample_rate * STEAL_FADE_SECONDS);
                        self.voices
                            .get_mut(voice_index)
                            .ok_or_else(invalid_state)?
                            .request_note(
                                &self.compiled,
                                request,
                                &self.note_layer_selection,
                                fade_frames,
                            )?;
                    }
                    CompiledPerformanceMode::Monophonic { .. } => {
                        self.apply_monophonic_note_on(request)?;
                    }
                }
            }
            ProcessEventKind::NoteOff { note_id } => {
                if matches!(
                    self.compiled.performance.mode,
                    CompiledPerformanceMode::Monophonic { .. }
                ) {
                    self.apply_monophonic_note_off(note_id)?;
                } else {
                    for voice in &mut self.voices {
                        voice.release_note(&self.compiled, note_id, self.sustain_down)?;
                    }
                }
            }
            ProcessEventKind::SustainPedal { down } => {
                if self.sustain_down == down {
                    return Ok(());
                }
                self.sustain_down = down;
                if !down {
                    for voice in &mut self.voices {
                        voice.release_sustain(&self.compiled)?;
                    }
                }
            }
            ProcessEventKind::ParameterChange {
                catalog_revision,
                parameter,
                normalized,
            } => {
                if !accept_parameter_changes
                    || catalog_revision != self.compiled.parameter_catalog_revision()
                {
                    return Ok(());
                }
                let descriptor = self.compiled.parameter_descriptor(parameter).ok_or(
                    ProcessError::ParameterHandleOutOfRange {
                        handle: parameter.index(),
                    },
                )?;
                let frames =
                    rounded_frame_count(f64::from(descriptor.smoothing_seconds) * spec.sample_rate)
                        .max(1);
                self.parameter_states
                    .get_mut(parameter.index())
                    .ok_or_else(invalid_state)?
                    .set_target(normalized, frames);
            }
            ProcessEventKind::PitchBend { value } => {
                self.pitch_bend
                    .set_target(value, control_smoothing_frames(spec.sample_rate));
            }
            ProcessEventKind::ModWheel { value } => {
                self.mod_wheel
                    .set_target(value, control_smoothing_frames(spec.sample_rate));
            }
            ProcessEventKind::Aftertouch { value } => {
                self.aftertouch
                    .set_target(value, control_smoothing_frames(spec.sample_rate));
            }
        }
        Ok(())
    }

    fn apply_monophonic_note_on(&mut self, request: NoteRequest) -> Result<(), ProcessError> {
        if self
            .held_notes
            .iter()
            .any(|note| note.note_id == request.note_id)
        {
            return Err(ProcessError::DuplicateNoteId {
                note_id: request.note_id,
            });
        }
        if self.held_notes.len() >= MAX_MONOPHONIC_HELD_NOTES {
            return Err(ProcessError::MonophonicHeldNoteLimitExceeded {
                limit: MAX_MONOPHONIC_HELD_NOTES,
            });
        }
        let connected = !self.held_notes.is_empty();
        self.held_notes.push(HeldNote {
            note_id: request.note_id,
            note_number: request.note_number,
            velocity: request.velocity,
        });
        if !self.note_has_triggerable_layer(request) {
            self.held_notes.pop();
            return Ok(());
        }
        let CompiledPerformanceMode::Monophonic {
            legato,
            portamento_frames,
        } = self.compiled.performance.mode
        else {
            return Err(invalid_state());
        };
        if connected
            && legato
            && self
                .voices
                .first()
                .is_some_and(|voice| voice.state() == VoiceState::Active)
        {
            self.voices
                .get_mut(0)
                .ok_or_else(invalid_state)?
                .transition_legato(&self.compiled, request, portamento_frames)?;
            return Ok(());
        }
        if !self.prepare_note_request(request)? {
            return Ok(());
        }
        let fade_frames = rounded_frame_count(
            self.spec.ok_or(ProcessError::NotPrepared)?.sample_rate * STEAL_FADE_SECONDS,
        );
        let voice = self.voices.get_mut(0).ok_or_else(invalid_state)?;
        if connected {
            voice.retrigger_monophonic(
                &self.compiled,
                request,
                &self.note_layer_selection,
                portamento_frames,
            )?;
        } else {
            voice.request_note(
                &self.compiled,
                request,
                &self.note_layer_selection,
                fade_frames,
            )?;
        }
        Ok(())
    }

    fn apply_monophonic_note_off(
        &mut self,
        note_id: crate::process::NoteId,
    ) -> Result<(), ProcessError> {
        let Some(index) = self
            .held_notes
            .iter()
            .position(|note| note.note_id == note_id)
        else {
            return Ok(());
        };
        let was_current = index + 1 == self.held_notes.len();
        self.held_notes.remove(index);
        if !was_current {
            return Ok(());
        }
        let CompiledPerformanceMode::Monophonic {
            legato,
            portamento_frames,
        } = self.compiled.performance.mode
        else {
            return Err(invalid_state());
        };
        if let Some(note) = self.held_notes.last().copied() {
            let request = NoteRequest::new(
                note.note_id,
                note.note_number,
                note.velocity,
                self.absolute_frame,
            );
            if legato
                && self
                    .voices
                    .first()
                    .is_some_and(|voice| voice.state() == VoiceState::Active)
            {
                self.voices
                    .get_mut(0)
                    .ok_or_else(invalid_state)?
                    .transition_legato(&self.compiled, request, portamento_frames)?;
            } else if self.prepare_note_request(request)? {
                self.voices
                    .get_mut(0)
                    .ok_or_else(invalid_state)?
                    .retrigger_monophonic(
                        &self.compiled,
                        request,
                        &self.note_layer_selection,
                        portamento_frames,
                    )?;
            }
        } else {
            self.voices
                .get_mut(0)
                .ok_or_else(invalid_state)?
                .release_note(&self.compiled, note_id, self.sustain_down)?;
        }
        Ok(())
    }

    fn note_has_triggerable_layer(&self, note: NoteRequest) -> bool {
        self.compiled.layers.iter().any(|layer| {
            layer.trigger.matches(note.note_number, note.velocity) && layer.generator.is_available()
        })
    }

    fn select_voice(&self) -> usize {
        if let Some((index, _)) = self
            .voices
            .iter()
            .enumerate()
            .find(|(_, voice)| voice.state() == VoiceState::Idle)
        {
            return index;
        }
        if let Some((index, _)) = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.state() == VoiceState::Releasing)
            .min_by(|(_, left), (_, right)| {
                left.estimated_level().total_cmp(&right.estimated_level())
            })
        {
            return index;
        }
        if let Some((index, _)) = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.state() == VoiceState::Active)
            .min_by_key(|(_, voice)| voice.started_at_frame())
        {
            return index;
        }
        self.voices
            .iter()
            .enumerate()
            .min_by_key(|(_, voice)| voice.started_at_frame())
            .map_or(0, |(index, _)| index)
    }

    fn prepare_note_request(&mut self, note: NoteRequest) -> Result<bool, ProcessError> {
        if self.note_layer_selection.len() != self.compiled.layers.len() {
            return Err(invalid_state());
        }
        self.note_layer_selection
            .fill(PreparedLayerSelection::Inactive);
        let mut can_trigger = false;
        let compiled = Arc::clone(&self.compiled);
        for (layer_index, layer) in compiled.layers.iter().enumerate() {
            if !layer.trigger.matches(note.note_number, note.velocity) {
                continue;
            }
            if !layer.generator.is_available() {
                continue;
            }
            match &layer.generator {
                CompiledGenerator::Oscillator(_)
                | CompiledGenerator::Noise(_)
                | CompiledGenerator::PhysicalString(_)
                | CompiledGenerator::Modal(_)
                | CompiledGenerator::Additive(_)
                | CompiledGenerator::Formant(_)
                | CompiledGenerator::Granular(_)
                | CompiledGenerator::WaveSequence(_)
                | CompiledGenerator::Wavetable(_)
                | CompiledGenerator::Spectral(_)
                | CompiledGenerator::OperatorModulation(_) => {
                    *self
                        .note_layer_selection
                        .get_mut(layer_index)
                        .ok_or_else(invalid_state)? = match layer.trigger.event {
                        LayerTriggerEvent::NoteOn => {
                            PreparedLayerSelection::Active { sample_zone: None }
                        }
                        LayerTriggerEvent::NoteOff => {
                            PreparedLayerSelection::Armed { sample_zone: None }
                        }
                    };
                    can_trigger = true;
                }
                CompiledGenerator::Sample(sample) => {
                    if let Some(zone_index) = self.select_sample_zone(
                        layer_index,
                        sample,
                        note.note_number,
                        note.velocity,
                    )? {
                        *self
                            .note_layer_selection
                            .get_mut(layer_index)
                            .ok_or_else(invalid_state)? = match layer.trigger.event {
                            LayerTriggerEvent::NoteOn => PreparedLayerSelection::Active {
                                sample_zone: Some(zone_index),
                            },
                            LayerTriggerEvent::NoteOff => PreparedLayerSelection::Armed {
                                sample_zone: Some(zone_index),
                            },
                        };
                        can_trigger = true;
                    }
                }
            }
        }
        Ok(can_trigger)
    }

    fn select_sample_zone(
        &mut self,
        layer_index: usize,
        sample: &crate::compiler::CompiledSample,
        note_number: u8,
        velocity: u8,
    ) -> Result<Option<usize>, ProcessError> {
        let Some((first_index, first_zone)) = sample
            .zones
            .iter()
            .enumerate()
            .find(|(_, zone)| zone_matches(zone, note_number, velocity))
        else {
            return Ok(None);
        };
        let Some(group_index) = first_zone.group else {
            return Ok(Some(first_index));
        };
        let group = sample.groups.get(group_index).ok_or_else(invalid_state)?;
        let counter = self
            .round_robin_counters
            .get(layer_index)
            .and_then(|counters| counters.get(group_index))
            .copied()
            .ok_or_else(invalid_state)?;
        let matching_count = group
            .enabled_member_zone_indices
            .iter()
            .filter(|index| {
                sample
                    .zones
                    .get(**index)
                    .is_some_and(|zone| zone_matches(zone, note_number, velocity))
            })
            .count();
        if matching_count == 0 {
            return Ok(None);
        }
        let divisor = u64::try_from(matching_count).map_err(|_| invalid_state())?;
        let selected_offset = usize::try_from(counter % divisor).map_err(|_| invalid_state())?;
        let selected = group
            .enabled_member_zone_indices
            .iter()
            .copied()
            .filter(|index| {
                sample
                    .zones
                    .get(*index)
                    .is_some_and(|zone| zone_matches(zone, note_number, velocity))
            })
            .nth(selected_offset)
            .ok_or_else(invalid_state)?;
        let next_counter = counter.wrapping_add(1);
        *self
            .round_robin_counters
            .get_mut(layer_index)
            .and_then(|counters| counters.get_mut(group_index))
            .ok_or_else(invalid_state)? = next_counter;
        Ok(Some(selected))
    }

    pub(super) fn validate_parameter_events(
        &self,
        events: &[crate::process::ProcessEvent],
        accept_parameter_changes: bool,
    ) -> Result<(), ProcessError> {
        if !accept_parameter_changes {
            return Ok(());
        }
        for event in events {
            if let ProcessEventKind::ParameterChange {
                catalog_revision,
                parameter,
                ..
            } = event.kind
                && catalog_revision == self.compiled.parameter_catalog_revision()
                && self.compiled.parameter_descriptor(parameter).is_none()
            {
                return Err(ProcessError::ParameterHandleOutOfRange {
                    handle: parameter.index(),
                });
            }
        }
        Ok(())
    }
}

fn zone_matches(zone: &CompiledSampleZone, note_number: u8, velocity: u8) -> bool {
    zone.is_enabled()
        && (zone.key_min..=zone.key_max).contains(&note_number)
        && (zone.velocity_min..=zone.velocity_max).contains(&velocity)
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use crate::definition::tests::definition;
    use crate::parameter::ParameterHandle;
    use crate::process::ProcessError;
    use crate::process::ProcessEvent;
    use crate::process::{InstrumentProcessor, ProcessBlock, ProcessEventKind, ProcessSpec};
    use crate::runtime::voice::VoiceState;

    use super::super::tests::{
        compiled, modulated_steal_definition, monophonic_definition, phase_runtime,
        phase_runtime_with_waveform, prepare, process, process_parameter_event, runtime,
        runtime_with, traced_source_value,
    };

    #[test]
    fn monophonic_uses_last_note_priority_and_returns_to_held_notes() {
        let definition = monophonic_definition(true, None);
        let mut runtime = runtime_with(&definition);
        prepare(&mut runtime);

        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let second_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 64,
                velocity: 90,
            },
        }];
        let second_off = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 2 },
        }];
        let first_off = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }];

        process(&mut runtime, 64, 0, &first_note);
        assert_eq!(
            runtime.voices[0].trace_identity().map(|value| value.0),
            Some(1)
        );
        process(&mut runtime, 64, 64, &second_note);
        assert_eq!(
            runtime.voices[0].trace_identity().map(|value| value.0),
            Some(2)
        );
        process(&mut runtime, 64, 128, &second_off);
        assert_eq!(
            runtime.voices[0].trace_identity().map(|value| value.0),
            Some(1)
        );
        process(&mut runtime, 64, 192, &first_off);
        assert_eq!(runtime.voices[0].state(), VoiceState::Releasing);
    }

    #[test]
    fn monophonic_legato_ignores_notes_outside_the_layer_trigger() {
        let mut definition = monophonic_definition(true, Some(0.1));
        definition.layers[0].trigger.key_max = 60;
        let mut runtime = runtime_with(&definition);
        prepare(&mut runtime);

        process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 100,
                },
            }],
        );
        process(
            &mut runtime,
            64,
            64,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 72,
                    velocity: 100,
                },
            }],
        );

        assert_eq!(
            runtime.voices[0].trace_identity().map(|value| value.0),
            Some(1)
        );
        process(
            &mut runtime,
            64,
            128,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            }],
        );
        assert_eq!(runtime.voices[0].state(), VoiceState::Releasing);
    }

    #[test]
    fn legato_preserves_source_phase_and_applies_portamento() {
        let mut definition = monophonic_definition(true, Some(0.1));
        definition.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![crate::definition::ModulationSourceDefinition::Lfo(
                crate::definition::LfoDefinition {
                    id: "legato_lfo".to_owned(),
                    waveform: crate::definition::LfoWaveform::Sine,
                    rate: crate::definition::ModulationRateDefinition {
                        value: 0.5,
                        unit: crate::definition::ModulationRateUnit::PerSecond,
                    },
                    phase: 0.0,
                },
            )],
            routes: vec![crate::definition::ModulationRouteDefinition {
                source: "legato_lfo".to_owned(),
                target: "layer.body.tuning".to_owned(),
                depth: crate::definition::ModulationDepthDefinition {
                    value: 120.0,
                    unit: crate::parameter::ModulationUnit::Cents,
                },
                curve: crate::definition::ModulationCurve::Linear,
            }],
        });
        let mut runtime = runtime_with(&definition);
        prepare(&mut runtime);
        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let second_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 67,
                velocity: 100,
            },
        }];

        process(&mut runtime, 64, 0, &first_note);
        let first_phase = traced_source_value(&runtime, 64);
        process(&mut runtime, 64, 64, &second_note);
        let second_phase = traced_source_value(&runtime, 128);

        assert!((second_phase - first_phase).abs() > 1e-5);
        assert_eq!(
            runtime.voices[0].trace_identity().map(|value| value.0),
            Some(2)
        );
        assert!(runtime.voices[0].portamento_offset_cents() < -650.0);
        assert!(runtime.voices[0].portamento_offset_cents() > -700.0);
    }

    #[test]
    fn non_legato_restarts_voice_source_phase() {
        let mut definition = monophonic_definition(false, None);
        definition.modulation = Some(crate::definition::ModulationDefinition {
            sources: vec![crate::definition::ModulationSourceDefinition::Lfo(
                crate::definition::LfoDefinition {
                    id: "retrigger_lfo".to_owned(),
                    waveform: crate::definition::LfoWaveform::Sine,
                    rate: crate::definition::ModulationRateDefinition {
                        value: 0.5,
                        unit: crate::definition::ModulationRateUnit::PerSecond,
                    },
                    phase: 0.0,
                },
            )],
            routes: vec![crate::definition::ModulationRouteDefinition {
                source: "retrigger_lfo".to_owned(),
                target: "layer.body.tuning".to_owned(),
                depth: crate::definition::ModulationDepthDefinition {
                    value: 120.0,
                    unit: crate::parameter::ModulationUnit::Cents,
                },
                curve: crate::definition::ModulationCurve::Linear,
            }],
        });
        let mut runtime = runtime_with(&definition);
        prepare(&mut runtime);
        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let second_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 67,
                velocity: 100,
            },
        }];

        process(&mut runtime, 64, 0, &first_note);
        let first_phase = traced_source_value(&runtime, 64);
        process(&mut runtime, 64, 64, &second_note);
        let second_phase = traced_source_value(&runtime, 128);

        assert!((second_phase - first_phase).abs() < 1e-6);
    }

    #[test]
    fn note_lifecycle_produces_stereo_audio_and_release() {
        let mut runtime = runtime();
        prepare(&mut runtime);
        let on = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 69,
                velocity: 100,
            },
        }];
        let audio = process(&mut runtime, 128, 0, &on);
        assert!(audio.iter().flatten().any(|sample| sample.abs() > 0.01));
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
        let off = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }];
        let _ = process(&mut runtime, 256, 128, &off);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Releasing));
    }

    #[test]
    fn sustain_defers_release_until_the_pedal_is_lifted() {
        let mut runtime = runtime();
        prepare(&mut runtime);
        let note_on = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 0, &note_on);

        let sustain_and_note_off = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::SustainPedal { down: true },
            },
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
        ];
        let _ = process(&mut runtime, 64, 64, &sustain_and_note_off);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));

        let pedal_up = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::SustainPedal { down: false },
        }];
        let _ = process(&mut runtime, 64, 128, &pedal_up);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Releasing));
    }

    #[test]
    fn sustain_does_not_release_a_key_that_is_still_down() {
        let mut runtime = runtime();
        prepare(&mut runtime);
        let note_on = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 0, &note_on);
        let pedal_down = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::SustainPedal { down: true },
        }];
        let _ = process(&mut runtime, 64, 64, &pedal_down);
        let pedal_up = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::SustainPedal { down: false },
        }];
        let _ = process(&mut runtime, 64, 128, &pedal_up);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));

        let note_off = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }];
        let _ = process(&mut runtime, 64, 192, &note_off);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Releasing));
    }

    #[test]
    fn repeated_sustain_state_changes_are_idempotent() {
        let mut runtime = runtime();
        prepare(&mut runtime);
        let note_on = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 0, &note_on);
        let repeated_down = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::SustainPedal { down: true },
            },
            ProcessEvent {
                sample_offset: 1,
                kind: ProcessEventKind::SustainPedal { down: true },
            },
            ProcessEvent {
                sample_offset: 2,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
        ];
        let _ = process(&mut runtime, 64, 64, &repeated_down);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));

        let repeated_up = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::SustainPedal { down: false },
            },
            ProcessEvent {
                sample_offset: 1,
                kind: ProcessEventKind::SustainPedal { down: false },
            },
        ];
        let _ = process(&mut runtime, 64, 128, &repeated_up);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Releasing));
    }

    #[test]
    fn reset_clears_sustain_state() {
        let mut runtime = runtime();
        prepare(&mut runtime);
        let note_on = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 0, &note_on);
        let held_note = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::SustainPedal { down: true },
            },
            ProcessEvent {
                sample_offset: 1,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
        ];
        let _ = process(&mut runtime, 64, 64, &held_note);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));

        runtime.reset().expect("reset");
        let _ = process(&mut runtime, 64, 0, &note_on);
        let note_off = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }];
        let _ = process(&mut runtime, 64, 64, &note_off);

        assert_eq!(runtime.voice_state(0), Some(VoiceState::Releasing));
    }

    #[test]
    fn note_on_reuses_prepared_layer_selection_storage() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let note_layer_capacity = runtime.note_layer_selection.capacity();
        let pending_layer_capacity = runtime.voices[0].pending_layer_selection_capacity();

        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 0, &first_note);

        let second_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 64,
                velocity: 100,
            },
        }];
        let _ = process(&mut runtime, 64, 64, &second_note);

        assert_eq!(runtime.note_layer_selection.capacity(), note_layer_capacity);
        assert_eq!(
            runtime.voices[0].pending_layer_selection_capacity(),
            pending_layer_capacity
        );
    }

    #[test]
    fn note_on_activates_only_layers_matching_the_note() {
        let single_layer = definition();
        let mut layered = single_layer.clone();
        let mut non_matching = layered.layers[0].clone();
        non_matching.id = "non_matching".to_owned();
        non_matching.trigger.key_min = 72;
        non_matching.trigger.key_max = 72;
        non_matching.gain_db = 0.0;
        non_matching.envelope.attack_seconds = 0.0;
        non_matching.envelope.decay_seconds = 0.0;
        non_matching.envelope.sustain_level = 1.0;
        layered.layers.push(non_matching);

        let mut single_runtime = runtime_with(&single_layer);
        let mut layered_runtime = runtime_with(&layered);
        prepare(&mut single_runtime);
        prepare(&mut layered_runtime);
        let event = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }];

        let single_audio = process(&mut single_runtime, 128, 0, &event);
        let layered_audio = process(&mut layered_runtime, 128, 0, &event);

        assert_eq!(single_audio[0], layered_audio[0]);
        assert_eq!(single_audio[1], layered_audio[1]);
    }

    #[test]
    fn releasing_voice_is_selected_before_active_voice() {
        let mut runtime = runtime();
        prepare(&mut runtime);
        let events = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            },
            ProcessEvent {
                sample_offset: 1,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 64,
                    velocity: 127,
                },
            },
            ProcessEvent {
                sample_offset: 2,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
        ];
        let _ = process(&mut runtime, 16, 0, &events);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Releasing));
        assert_eq!(runtime.voice_state(1), Some(VoiceState::Active));
    }

    #[test]
    fn steal_starts_pending_note_when_release_finishes_before_fade() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        source.layers[0].envelope = crate::definition::AdsrDefinition {
            attack_seconds: 0.0,
            decay_seconds: 0.0,
            sustain_level: 1.0,
            release_seconds: 0.001,
        };
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let audio = process(
            &mut runtime,
            64,
            64,
            &[
                ProcessEvent {
                    sample_offset: 0,
                    kind: ProcessEventKind::NoteOff { note_id: 1 },
                },
                ProcessEvent {
                    sample_offset: 47,
                    kind: ProcessEventKind::NoteOn {
                        note_id: 2,
                        note_number: 64,
                        velocity: 127,
                    },
                },
            ],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
        assert!(audio[0][48..].iter().any(|sample| sample.abs() > 1.0e-6));
    }

    #[test]
    fn steal_fade_completes_across_multiple_blocks() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 64,
                    velocity: 127,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::StealFading));
        let empty: [ProcessEvent; 0] = [];
        let _ = process(&mut runtime, 64, 128, &empty);
        let _ = process(&mut runtime, 64, 192, &empty);
        let _ = process(&mut runtime, 64, 256, &empty);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
    }

    #[test]
    fn pending_note_off_cancels_note_before_steal_completion() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[
                ProcessEvent {
                    sample_offset: 0,
                    kind: ProcessEventKind::NoteOn {
                        note_id: 2,
                        note_number: 64,
                        velocity: 127,
                    },
                },
                ProcessEvent {
                    sample_offset: 1,
                    kind: ProcessEventKind::NoteOff { note_id: 2 },
                },
            ],
        );
        let empty: [ProcessEvent; 0] = [];
        let _ = process(&mut runtime, 64, 128, &empty);
        let _ = process(&mut runtime, 64, 192, &empty);
        let _ = process(&mut runtime, 64, 256, &empty);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Idle));
    }

    #[test]
    fn pending_note_off_is_held_by_sustain_until_steal_completes() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[
                ProcessEvent {
                    sample_offset: 0,
                    kind: ProcessEventKind::NoteOn {
                        note_id: 2,
                        note_number: 64,
                        velocity: 127,
                    },
                },
                ProcessEvent {
                    sample_offset: 1,
                    kind: ProcessEventKind::SustainPedal { down: true },
                },
                ProcessEvent {
                    sample_offset: 2,
                    kind: ProcessEventKind::NoteOff { note_id: 2 },
                },
            ],
        );
        let empty: [ProcessEvent; 0] = [];
        let _ = process(&mut runtime, 64, 128, &empty);
        let _ = process(&mut runtime, 64, 192, &empty);
        let _ = process(&mut runtime, 64, 256, &empty);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));

        let pedal_up = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::SustainPedal { down: false },
        }];
        let _ = process(&mut runtime, 64, 320, &pedal_up);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Releasing));
    }

    #[test]
    fn reset_discards_pending_note_during_steal() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        source.layers[0].envelope.attack_seconds = 0.0;
        source.layers[0].envelope.decay_seconds = 0.0;
        source.layers[0].envelope.sustain_level = 1.0;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 64,
                    velocity: 127,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::StealFading));
        runtime.reset().expect("reset");
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Idle));
    }

    #[test]
    fn out_of_range_key_does_not_steal_a_full_voice() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        source.layers[0].trigger.key_min = 60;
        source.layers[0].trigger.key_max = 72;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 59,
                    velocity: 127,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
    }

    #[test]
    fn out_of_range_velocity_does_not_steal_a_full_voice() {
        let mut source = definition();
        source.performance = crate::definition::PerformanceDefinition::Polyphonic {
            polyphony: 1,
            voice_stealing: crate::definition::VoiceStealingDefinition::QuietestReleasingThenOldest,
        };
        source.layers[0].trigger.velocity_min = 64;
        source.layers[0].trigger.velocity_max = 127;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            64,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 64,
                },
            }],
        );
        let _ = process(
            &mut runtime,
            64,
            64,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 60,
                    velocity: 63,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
    }

    #[test]
    fn out_of_range_note_does_not_start_an_idle_voice_and_boundaries_trigger() {
        let mut source = definition();
        source.layers[0].trigger.key_min = 60;
        source.layers[0].trigger.key_max = 60;
        source.layers[0].trigger.velocity_min = 64;
        source.layers[0].trigger.velocity_max = 64;
        let mut runtime = runtime_with(&source);
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            1,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 59,
                    velocity: 64,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Idle));
        let _ = process(
            &mut runtime,
            1,
            1,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 60,
                    velocity: 64,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
    }

    #[test]
    fn steal_fade_continues_parameter_lfo_and_envelope_processing() {
        let dynamic_definition = modulated_steal_definition();
        let mut static_definition = dynamic_definition.clone();
        static_definition.modulation = None;
        let mut dynamic = runtime_with(&dynamic_definition);
        let mut static_runtime = runtime_with(&static_definition);
        prepare(&mut dynamic);
        prepare(&mut static_runtime);
        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let _ = process(&mut dynamic, 256, 0, &first_note);
        let _ = process(&mut static_runtime, 256, 0, &first_note);
        let dynamic_gain = dynamic
            .compiled()
            .parameter_handle("layer.body.gain")
            .expect("body gain parameter");
        let static_gain = static_runtime
            .compiled()
            .parameter_handle("layer.body.gain")
            .expect("body gain parameter");
        let steal_events = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 67,
                    velocity: 110,
                },
            },
            ProcessEvent {
                sample_offset: 32,
                kind: ProcessEventKind::ParameterChange {
                    catalog_revision: dynamic.compiled().parameter_catalog_revision(),
                    parameter: dynamic_gain,
                    normalized: 1.0,
                },
            },
        ];
        let static_events = [
            ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 67,
                    velocity: 110,
                },
            },
            ProcessEvent {
                sample_offset: 32,
                kind: ProcessEventKind::ParameterChange {
                    catalog_revision: static_runtime.compiled().parameter_catalog_revision(),
                    parameter: static_gain,
                    normalized: 1.0,
                },
            },
        ];
        let dynamic_audio = process(&mut dynamic, 256, 256, &steal_events);
        let static_audio = process(&mut static_runtime, 256, 256, &static_events);
        assert_eq!(dynamic.voice_state(0), Some(VoiceState::Active));
        assert_eq!(static_runtime.voice_state(0), Some(VoiceState::Active));
        assert!(
            dynamic_audio
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        assert!(
            dynamic_audio[0][..240]
                .iter()
                .zip(&static_audio[0][..240])
                .any(|(dynamic, static_sample)| (dynamic - static_sample).abs() > 1.0e-5)
        );
    }

    #[test]
    fn steal_completion_inside_a_control_span_starts_the_pending_note() {
        let mut runtime = runtime_with(&modulated_steal_definition());
        prepare(&mut runtime);
        let _ = process(
            &mut runtime,
            256,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        let audio = process(
            &mut runtime,
            256,
            256,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 67,
                    velocity: 110,
                },
            }],
        );
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Active));
        assert!(audio.iter().flatten().all(|sample| sample.is_finite()));
        assert!(audio[0][240..].iter().any(|sample| sample.abs() > 1.0e-6));
        assert!((audio[0][240] - audio[0][239]).abs() < 0.5);
    }

    #[test]
    fn phase_reset_changes_retriggered_note_phase() {
        let mut reset_runtime = phase_runtime(true);
        let mut continue_runtime = phase_runtime(false);
        prepare(&mut reset_runtime);
        prepare(&mut continue_runtime);
        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let _ = process(&mut reset_runtime, 64, 0, &first_note);
        let _ = process(&mut continue_runtime, 64, 0, &first_note);
        let retrigger = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 60,
                velocity: 127,
            },
        }];
        let reset_audio = process(&mut reset_runtime, 257, 64, &retrigger);
        let continue_audio = process(&mut continue_runtime, 257, 64, &retrigger);
        assert!(
            reset_audio[0][240..]
                .iter()
                .zip(&continue_audio[0][240..])
                .any(|(reset, continued)| (reset - continued).abs() > 1.0e-4)
        );
    }

    #[test]
    fn full_reset_restarts_phase_even_when_note_phase_reset_is_disabled() {
        let mut runtime = phase_runtime(false);
        prepare(&mut runtime);
        let note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let first = process(&mut runtime, 64, 0, &note);
        runtime.reset().expect("reset");
        let second = process(&mut runtime, 64, 0, &note);
        for (left, right) in first[0].iter().zip(&second[0]) {
            assert_relative_eq!(*left, *right, epsilon = 1.0e-6);
        }
    }

    #[test]
    fn triangle_retrigger_after_release_matches_first_render() {
        let mut runtime =
            phase_runtime_with_waveform(true, crate::definition::OscillatorWaveform::Triangle);
        prepare(&mut runtime);
        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let first = process(&mut runtime, 64, 0, &first_note);
        let note_off = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }];
        let _ = process(&mut runtime, 64, 64, &note_off);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Idle));

        let second_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 60,
                velocity: 127,
            },
        }];
        let second = process(&mut runtime, 64, 128, &second_note);
        for (first, second) in first[0].iter().zip(&second[0]) {
            assert_relative_eq!(*first, *second, epsilon = 1.0e-6);
        }
    }

    #[test]
    fn triangle_instrument_reset_matches_a_fresh_runtime() {
        let mut runtime =
            phase_runtime_with_waveform(false, crate::definition::OscillatorWaveform::Triangle);
        prepare(&mut runtime);
        let note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let first = process(&mut runtime, 64, 0, &note);
        runtime.reset().expect("reset");
        let second = process(&mut runtime, 64, 0, &note);
        for (first, second) in first[0].iter().zip(&second[0]) {
            assert_relative_eq!(*first, *second, epsilon = 1.0e-6);
        }
    }

    #[test]
    fn triangle_voice_stealing_starts_from_the_compiled_phase() {
        let mut stolen =
            phase_runtime_with_waveform(true, crate::definition::OscillatorWaveform::Triangle);
        let mut direct =
            phase_runtime_with_waveform(true, crate::definition::OscillatorWaveform::Triangle);
        prepare(&mut stolen);
        prepare(&mut direct);
        let first_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let _ = process(&mut stolen, 64, 0, &first_note);

        let pending_note = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 60,
                velocity: 127,
            },
        }];
        let stolen_audio = process(&mut stolen, 256, 64, &pending_note);
        let direct_audio = process(
            &mut direct,
            16,
            0,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 3,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        assert_eq!(stolen.voice_state(0), Some(VoiceState::Active));
        for (stolen_sample, direct_sample) in stolen_audio[0][240..].iter().zip(&direct_audio[0]) {
            assert_relative_eq!(*stolen_sample, *direct_sample, epsilon = 1.0e-6);
        }
    }

    #[test]
    fn phase_reset_disabled_preserves_phase_after_release() {
        let mut runtime = phase_runtime(false);
        prepare(&mut runtime);
        let note_on = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 127,
            },
        }];
        let first = process(&mut runtime, 64, 0, &note_on);
        let note_off = [ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOff { note_id: 1 },
        }];
        let _ = process(&mut runtime, 64, 64, &note_off);
        assert_eq!(runtime.voice_state(0), Some(VoiceState::Idle));

        let continued = process(
            &mut runtime,
            64,
            128,
            &[ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 2,
                    note_number: 60,
                    velocity: 127,
                },
            }],
        );
        assert!(
            continued[0]
                .iter()
                .zip(&first[0])
                .any(|(continued, first)| (continued - first).abs() > 1.0e-4)
        );
    }

    #[test]
    fn invalid_parameter_event_leaves_runtime_state_unchanged() {
        let instrument = compiled(2);
        let mut runtime = instrument.instantiate();
        runtime
            .prepare(ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid spec"))
            .expect("runtime preparation");
        runtime.activate().expect("runtime activation");
        let invalid = ProcessEvent {
            sample_offset: 8,
            kind: ProcessEventKind::ParameterChange {
                catalog_revision: instrument.parameter_catalog_revision(),
                parameter: ParameterHandle::new(instrument.parameters().len()),
                normalized: 0.5,
            },
        };
        let note_on = ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        };
        let mut left = [1.0_f32; 32];
        let mut right = [1.0_f32; 32];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        let error = runtime.process(ProcessBlock {
            frames: 32,
            context: crate::process::ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                transport_state: crate::process::TransportState::Playing,
            },
            events: &[note_on, invalid],
            input: &[],
            output: &mut output,
        });
        assert_eq!(
            error,
            Err(ProcessError::ParameterHandleOutOfRange {
                handle: instrument.parameters().len(),
            })
        );
        assert_eq!(runtime.absolute_frame(), 0);
        assert!(
            (0..runtime.voice_count())
                .all(|index| runtime.voice_state(index) == Some(VoiceState::Idle))
        );
        assert!(left.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(right.iter().all(|sample| sample.abs() < f32::EPSILON));
    }

    #[test]
    fn shared_parameter_state_advances_once_for_any_voice_count() {
        let mut one_voice = super::super::RuntimeGeneration::new(compiled(1));
        let mut eight_voices = super::super::RuntimeGeneration::new(compiled(8));
        let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("valid spec");
        one_voice.prepare(spec).expect("one voice preparation");
        eight_voices.prepare(spec).expect("eight voice preparation");
        let parameter = one_voice.compiled().parameters()[0].id.clone();
        let one_handle = one_voice
            .compiled()
            .parameter_handle(&parameter)
            .expect("parameter handle");
        let eight_handle = eight_voices
            .compiled()
            .parameter_handle(&parameter)
            .expect("parameter handle");
        let one_revision = one_voice.compiled.parameter_catalog_revision();
        let eight_revision = eight_voices.compiled.parameter_catalog_revision();
        process_parameter_event(
            &mut one_voice,
            ProcessEventKind::ParameterChange {
                catalog_revision: one_revision,
                parameter: one_handle,
                normalized: 1.0,
            },
        );
        process_parameter_event(
            &mut eight_voices,
            ProcessEventKind::ParameterChange {
                catalog_revision: eight_revision,
                parameter: eight_handle,
                normalized: 1.0,
            },
        );
        let one_current = one_voice.parameter_states[one_handle.index()].span(0).0;
        let eight_current = eight_voices.parameter_states[eight_handle.index()]
            .span(0)
            .0;
        assert!((one_current - eight_current).abs() < f32::EPSILON);
    }
}
