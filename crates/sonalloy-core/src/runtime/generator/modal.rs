use sonalloy_dsp_sys::DspModalResonator;

use crate::compiler::CompiledModal;
use crate::parameter::generator::{MODAL_BRIGHTNESS, MODAL_DECAY, MODAL_STRUCTURE};
use crate::process::{ProcessError, ProcessSpec};

use super::super::modulation::LayerGeneratorTargetSpan;
use super::physical_exciter::{
    MIN_PHYSICAL_FREQUENCY_HZ, PhysicalExciterRuntime, valid_physical_frequency,
};
use super::{base_frequencies, ensure_finite, invalid_state, validate_generator_span};

pub(crate) struct ModalRuntime {
    resonator: DspModalResonator,
    exciter: PhysicalExciterRuntime,
    scratch: Vec<f32>,
    sample_rate: f32,
    effective_max_frequency: f32,
}

impl ModalRuntime {
    pub(super) fn new(compiled: &CompiledModal, spec: ProcessSpec) -> Result<Self, ProcessError> {
        #[allow(clippy::cast_possible_truncation)]
        let sample_rate = spec.sample_rate as f32;
        if !sample_rate.is_finite()
            || sample_rate <= 0.0
            || !compiled.effective_max_frequency.is_finite()
            || compiled.effective_max_frequency <= MIN_PHYSICAL_FREQUENCY_HZ
        {
            return Err(invalid_state());
        }
        let mut resonator =
            DspModalResonator::new().map_err(ProcessError::from_modal_resonator_error)?;
        resonator
            .prepare(spec.sample_rate, compiled.mode_count)
            .map_err(ProcessError::from_modal_resonator_error)?;
        Ok(Self {
            resonator,
            exciter: PhysicalExciterRuntime::new(
                compiled.exciter,
                compiled.layer_hash,
                spec.sample_rate,
            )?,
            scratch: vec![0.0; spec.max_block_size],
            sample_rate,
            effective_max_frequency: compiled.effective_max_frequency,
        })
    }

    pub(super) fn start(&mut self, note_id: u64) -> Result<(), ProcessError> {
        self.reset()?;
        self.exciter.start(note_id);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &mut self,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
        targets: LayerGeneratorTargetSpan,
        mono: &mut [f32],
    ) -> Result<(), ProcessError> {
        if frames == 0 {
            return Ok(());
        }
        let LayerGeneratorTargetSpan::Modal {
            structure,
            brightness,
            decay,
        } = targets
        else {
            return Err(invalid_state());
        };
        #[allow(clippy::cast_possible_truncation)]
        let requested_sample_rate = sample_rate as f32;
        if mono.len() < frames
            || self.scratch.len() < frames
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
            || requested_sample_rate.total_cmp(&self.sample_rate).is_ne()
        {
            return Err(invalid_state());
        }
        validate_generator_span(structure, MODAL_STRUCTURE)?;
        validate_generator_span(brightness, MODAL_BRIGHTNESS)?;
        validate_generator_span(decay, MODAL_DECAY)?;
        let (base_start, base_end) = base_frequencies(note_number, tuning_start, tuning_end)?;
        if !valid_physical_frequency(base_start, self.effective_max_frequency)
            || !valid_physical_frequency(base_end, self.effective_max_frequency)
        {
            return Err(ProcessError::InvalidFrequency);
        }
        self.exciter.render(frames, &mut self.scratch[..frames])?;
        self.resonator
            .process_ramp(
                base_start,
                base_end,
                structure.start,
                structure.end,
                brightness.start,
                brightness.end,
                decay.start,
                decay.end,
                &mut self.scratch[..frames],
            )
            .map_err(ProcessError::from_modal_resonator_error)?;
        mono[..frames].copy_from_slice(&self.scratch[..frames]);
        ensure_finite(&mono[..frames])
    }

    pub(super) fn reset(&mut self) -> Result<(), ProcessError> {
        self.resonator
            .reset()
            .map_err(ProcessError::from_modal_resonator_error)?;
        self.exciter.reset();
        self.scratch.fill(0.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{CompiledModalParameters, CompiledPhysicalExciter};
    use crate::parameter::ParameterHandle;
    use crate::runtime::modulation::ValueSpan;

    fn compiled() -> CompiledModal {
        CompiledModal {
            exciter: CompiledPhysicalExciter::Impulse,
            mode_count: 8,
            parameters: CompiledModalParameters {
                structure: ParameterHandle::new(0),
                brightness: ParameterHandle::new(1),
                decay: ParameterHandle::new(2),
            },
            layer_hash: 7,
            effective_max_frequency: 21_600.0,
        }
    }

    #[test]
    fn modal_is_finite_and_reset_repeats() {
        let spec = ProcessSpec::new(48_000.0, 257, 0, 2).expect("spec");
        let mut runtime = ModalRuntime::new(&compiled(), spec).expect("runtime");
        runtime.start(3).expect("start");
        let targets = LayerGeneratorTargetSpan::Modal {
            structure: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
            brightness: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
            decay: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
        };
        let mut first = vec![0.0; 257];
        runtime
            .render(257, 69, 0.0, 0.0, 48_000.0, targets, &mut first)
            .expect("render");
        assert!(first.iter().all(|sample| sample.is_finite()));
        runtime.start(3).expect("restart");
        let mut second = vec![0.0; 257];
        runtime
            .render(257, 69, 0.0, 0.0, 48_000.0, targets, &mut second)
            .expect("render after reset");
        assert_eq!(first, second);
    }

    #[test]
    fn modal_rejects_frequency_outside_the_physical_contract() {
        let spec = ProcessSpec::new(48_000.0, 64, 0, 2).expect("spec");
        let mut runtime = ModalRuntime::new(&compiled(), spec).expect("runtime");
        runtime.start(3).expect("start");
        let targets = LayerGeneratorTargetSpan::Modal {
            structure: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
            brightness: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
            decay: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
        };
        let mut output = [0.0; 64];
        assert_eq!(
            runtime.render(64, 127, 1_200.0, 1_200.0, 48_000.0, targets, &mut output),
            Err(ProcessError::InvalidFrequency)
        );
    }
}
