use std::sync::Arc;

use realfft::num_complex::Complex;

use crate::compiler::CompiledSpectral;
use crate::generator_parameters::{
    SPECTRAL_BLUR, SPECTRAL_FREEZE, SPECTRAL_MORPH, SPECTRAL_POSITION, SPECTRAL_SHIFT,
};
use crate::process::{ProcessError, ProcessSpec};
use crate::spectral::PreparedSpectralAsset;

use super::super::modulation::LayerGeneratorTargetSpan;
use super::{ensure_finite, invalid_state, validate_generator_span};

pub(crate) struct SpectralRuntime {
    source: Option<Arc<PreparedSpectralAsset>>,
    synthesis_plan: Arc<crate::spectral::SpectralSynthesisPlan>,
    phase_reset: bool,
    inverse_input: Vec<Complex<f32>>,
    inverse_output: Vec<f32>,
    inverse_scratch: Vec<Complex<f32>>,
    ola_left: Vec<f32>,
    ola_right: Vec<f32>,
    read_position: usize,
    frame_index: usize,
    samples_until_next_frame: usize,
    output_position: usize,
    total_output_frames: usize,
}

impl SpectralRuntime {
    pub(super) fn new(
        compiled: &CompiledSpectral,
        spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let fft_size = compiled.synthesis_plan.fft_size();
        let hop_size = compiled.synthesis_plan.hop_size();
        if fft_size != compiled.fft_size
            || hop_size != compiled.hop_size
            || compiled.synthesis_plan.bin_count() != fft_size / 2 + 1
        {
            return Err(invalid_state());
        }
        if let Some(source) = &compiled.source {
            if !source.check_layout()
                || source.sample_rate.total_cmp(&spec.sample_rate) != std::cmp::Ordering::Equal
                || source.fft_size != fft_size
                || source.hop_size != hop_size
            {
                return Err(invalid_state());
            }
        }
        let total_output_frames = compiled.source.as_ref().map_or(0, |source| {
            source
                .spectral_frame_count
                .saturating_sub(1)
                .saturating_mul(hop_size)
                .saturating_add(fft_size)
        });
        let ola_capacity = fft_size.saturating_add(hop_size).max(fft_size);
        Ok(Self {
            source: compiled.source.clone(),
            synthesis_plan: Arc::clone(&compiled.synthesis_plan),
            phase_reset: compiled.phase_reset,
            inverse_input: vec![Complex::new(0.0, 0.0); fft_size / 2 + 1],
            inverse_output: vec![0.0; fft_size],
            inverse_scratch: vec![
                Complex::new(0.0, 0.0);
                compiled.synthesis_plan.inverse_scratch_len()
            ],
            ola_left: vec![0.0; ola_capacity],
            ola_right: vec![0.0; ola_capacity],
            read_position: 0,
            frame_index: 0,
            samples_until_next_frame: 0,
            output_position: 0,
            total_output_frames,
        })
    }

    pub(super) fn start(&mut self) -> Result<(), ProcessError> {
        if self.source.is_none() {
            return Err(invalid_state());
        }
        if self.phase_reset {
            self.reset();
        }
        Ok(())
    }

    pub(super) fn intrinsic_latency_frames(&self) -> usize {
        self.source
            .as_ref()
            .map_or(0, |source| source.latency_frames)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &mut self,
        frames: usize,
        targets: LayerGeneratorTargetSpan,
        mono: &mut [f32],
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<bool, ProcessError> {
        let LayerGeneratorTargetSpan::Spectral {
            position,
            freeze,
            blur,
            shift,
            morph,
        } = targets
        else {
            return Err(invalid_state());
        };
        validate_generator_span(position, SPECTRAL_POSITION)?;
        validate_generator_span(freeze, SPECTRAL_FREEZE)?;
        validate_generator_span(blur, SPECTRAL_BLUR)?;
        validate_generator_span(shift, SPECTRAL_SHIFT)?;
        if let Some(morph) = morph {
            validate_generator_span(morph, SPECTRAL_MORPH)?;
        }
        if mono.len() < frames || left.len() < frames || right.len() < frames {
            return Err(invalid_state());
        }
        mono[..frames].fill(0.0);
        left[..frames].fill(0.0);
        right[..frames].fill(0.0);
        let source = self.source.clone().ok_or_else(invalid_state)?;
        if self.output_position >= self.total_output_frames {
            return Ok(true);
        }
        for frame in 0..frames {
            if self.output_position >= self.total_output_frames {
                break;
            }
            if self.samples_until_next_frame == 0 && self.frame_index < source.spectral_frame_count
            {
                self.add_spectral_frame(&source)?;
                self.frame_index += 1;
                self.samples_until_next_frame = self.synthesis_plan.hop_size();
            }
            let left_sample = Self::take_ola_sample(self.read_position, &mut self.ola_left);
            let right_sample = if source.channels == 2 {
                Self::take_ola_sample(self.read_position, &mut self.ola_right)
            } else {
                left_sample
            };
            mono[frame] = (left_sample + right_sample) * 0.5;
            left[frame] = left_sample;
            right[frame] = right_sample;
            self.samples_until_next_frame = self.samples_until_next_frame.saturating_sub(1);
            self.read_position = (self.read_position + 1) % self.ola_left.len();
            self.output_position += 1;
        }
        ensure_finite(&mono[..frames])?;
        ensure_finite(&left[..frames])?;
        ensure_finite(&right[..frames])?;
        Ok(self.output_position >= self.total_output_frames)
    }

    fn add_spectral_frame(&mut self, source: &PreparedSpectralAsset) -> Result<(), ProcessError> {
        self.add_channel_frame(source, 0)?;
        if source.channels == 2 {
            self.add_channel_frame(source, 1)?;
        }
        Ok(())
    }

    fn add_channel_frame(
        &mut self,
        source: &PreparedSpectralAsset,
        channel: usize,
    ) -> Result<(), ProcessError> {
        let frame = self.frame_index;
        let plan = Arc::clone(&self.synthesis_plan);
        for bin in 0..plan.bin_count() {
            let index = source
                .index(channel, frame, bin)
                .ok_or_else(invalid_state)?;
            let magnitude = source
                .magnitudes
                .get(index)
                .copied()
                .ok_or_else(invalid_state)?;
            let phase = source
                .phases
                .get(index)
                .copied()
                .ok_or_else(invalid_state)?;
            let (real, imaginary) = if bin == 0 || bin + 1 == plan.bin_count() {
                (magnitude * phase.cos(), 0.0)
            } else {
                (magnitude * phase.cos(), magnitude * phase.sin())
            };
            self.inverse_input[bin] = Complex::new(real, imaginary);
        }
        plan.inverse(
            &mut self.inverse_input,
            &mut self.inverse_output,
            &mut self.inverse_scratch,
        )
        .map_err(|_| invalid_state())?;
        #[allow(clippy::cast_precision_loss)]
        let normalization = 1.0 / plan.fft_size() as f32;
        let capacity = self.ola_left.len();
        for (offset, sample) in self.inverse_output.iter().copied().enumerate() {
            let index = (self.read_position + offset) % capacity;
            let output = if channel == 0 {
                &mut self.ola_left
            } else if channel == 1 {
                &mut self.ola_right
            } else {
                return Err(invalid_state());
            };
            output[index] += sample * normalization * plan.synthesis_window()[offset];
        }
        Ok(())
    }

    fn take_ola_sample(read_position: usize, buffer: &mut [f32]) -> f32 {
        let sample = buffer[read_position];
        buffer[read_position] = 0.0;
        sample
    }

    pub(super) fn reset(&mut self) {
        self.inverse_input.fill(Complex::new(0.0, 0.0));
        self.inverse_output.fill(0.0);
        self.inverse_scratch.fill(Complex::new(0.0, 0.0));
        self.ola_left.fill(0.0);
        self.ola_right.fill(0.0);
        self.read_position = 0;
        self.frame_index = 0;
        self.samples_until_next_frame = 0;
        self.output_position = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{PreparedAudio, PreparedAudioChannels, SampleMetadata};
    use crate::compiler::CompiledSpectralParameters;
    use crate::parameter::ParameterHandle;
    use crate::runtime::modulation::ValueSpan;
    use crate::spectral::prepare_spectral_asset;

    fn test_runtime(phase_reset: bool) -> SpectralRuntime {
        let samples = (0..4_096)
            .map(|index| {
                #[allow(clippy::cast_precision_loss)]
                let time = index as f32 / 48_000.0;
                (std::f32::consts::TAU * 440.0 * time).sin()
            })
            .collect::<Vec<_>>();
        let audio = PreparedAudio {
            sample_rate: 48_000.0,
            frames: samples.len(),
            source_metadata: SampleMetadata {
                source_sample_rate: 48_000,
                source_channels: 1,
                bits_per_sample: Some(32),
                source_frames: samples.len(),
            },
            channels: PreparedAudioChannels::Mono {
                samples: Arc::from(samples.into_boxed_slice()),
            },
        };
        let source = Arc::new(prepare_spectral_asset(&audio, 1024).expect("spectral source"));
        let synthesis_plan =
            Arc::new(crate::spectral::SpectralSynthesisPlan::new(1024).expect("spectral plan"));
        SpectralRuntime::new(
            &CompiledSpectral {
                source: Some(source),
                asset_a_path: "fixture.wav".to_owned(),
                asset_a_sha256_specified: false,
                asset_b_path: None,
                asset_b_sha256_specified: false,
                root_note: 60,
                fft_size: 1024,
                hop_size: 256,
                phase_reset,
                parameters: CompiledSpectralParameters {
                    position: ParameterHandle::new(0),
                    freeze: ParameterHandle::new(1),
                    blur: ParameterHandle::new(2),
                    shift: ParameterHandle::new(3),
                    morph: None,
                },
                synthesis_plan,
                latency_frames: 768,
            },
            ProcessSpec::new(48_000.0, 64, 2).expect("process spec"),
        )
        .expect("spectral runtime")
    }

    #[test]
    fn spectral_render_does_not_allocate_after_prepare() {
        let mut runtime = test_runtime(true);
        runtime.start().expect("spectral runtime starts");
        let span = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
        let targets = LayerGeneratorTargetSpan::Spectral {
            position: span,
            freeze: span,
            blur: span,
            shift: span,
            morph: None,
        };
        let mut mono = vec![0.0; 64];
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        let allocations = crate::test_allocator::count_allocations(|| {
            runtime
                .render(64, targets, &mut mono, &mut left, &mut right)
                .expect("spectral render");
        });
        assert_eq!(allocations, 0);
    }

    #[test]
    fn phase_reset_controls_retrigger_cursor() {
        let span = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
        let targets = LayerGeneratorTargetSpan::Spectral {
            position: span,
            freeze: span,
            blur: span,
            shift: span,
            morph: None,
        };
        for (phase_reset, expected_position) in [(true, 0_usize), (false, 64_usize)] {
            let mut runtime = test_runtime(phase_reset);
            runtime.start().expect("spectral runtime starts");
            let mut mono = vec![0.0; 64];
            let mut left = vec![0.0; 64];
            let mut right = vec![0.0; 64];
            runtime
                .render(64, targets, &mut mono, &mut left, &mut right)
                .expect("spectral render");
            runtime.start().expect("spectral runtime retriggers");
            assert_eq!(runtime.output_position, expected_position);
        }
    }
}
