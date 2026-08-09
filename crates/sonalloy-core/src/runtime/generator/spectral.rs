use std::f64;
use std::sync::Arc;

use realfft::num_complex::Complex;

use crate::compiler::{CompiledSpectral, cents_to_ratio, midi_note_frequency};
use crate::generator_parameters::{
    SPECTRAL_BLUR, SPECTRAL_FREEZE, SPECTRAL_MORPH, SPECTRAL_POSITION, SPECTRAL_SHIFT,
};
use crate::process::{ProcessError, ProcessSpec};
use crate::spectral::PreparedSpectralAsset;

use super::super::modulation::{LayerGeneratorTargetSpan, ValueSpan};
use super::{ensure_finite, invalid_state, validate_generator_span};

const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;

pub(crate) struct SpectralRuntime {
    source: Option<Arc<PreparedSpectralAsset>>,
    synthesis_plan: Arc<crate::spectral::SpectralSynthesisPlan>,
    root_note: u8,
    phase_reset: bool,
    phase_accumulators: Vec<f32>,
    phase_initialized: bool,
    inverse_input: Vec<Complex<f32>>,
    inverse_output: Vec<f32>,
    inverse_scratch: Vec<Complex<f32>>,
    ola_left: Vec<f32>,
    ola_right: Vec<f32>,
    scan_progress: f64,
    read_position: usize,
    samples_until_next_frame: usize,
    output_position: usize,
    last_scheduled_end: usize,
    source_exhausted: bool,
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
        let channels = if let Some(source) = &compiled.source {
            if !source.check_layout()
                || source.sample_rate.total_cmp(&spec.sample_rate) != std::cmp::Ordering::Equal
                || source.fft_size != fft_size
                || source.hop_size != hop_size
            {
                return Err(invalid_state());
            }
            source.channels
        } else {
            1
        };
        let bin_count = fft_size / 2 + 1;
        let phase_accumulator_count = channels.checked_mul(bin_count).ok_or_else(invalid_state)?;
        let ola_capacity = fft_size.saturating_add(hop_size).max(fft_size);
        Ok(Self {
            source: compiled.source.clone(),
            synthesis_plan: Arc::clone(&compiled.synthesis_plan),
            root_note: compiled.root_note,
            phase_reset: compiled.phase_reset,
            phase_accumulators: vec![0.0; phase_accumulator_count],
            phase_initialized: false,
            inverse_input: vec![Complex::new(0.0, 0.0); bin_count],
            inverse_output: vec![0.0; fft_size],
            inverse_scratch: vec![
                Complex::new(0.0, 0.0);
                compiled.synthesis_plan.inverse_scratch_len()
            ],
            ola_left: vec![0.0; ola_capacity],
            ola_right: vec![0.0; ola_capacity],
            scan_progress: 0.0,
            read_position: 0,
            samples_until_next_frame: 0,
            output_position: 0,
            last_scheduled_end: 0,
            source_exhausted: false,
        })
    }

    pub(super) fn start(&mut self) -> Result<(), ProcessError> {
        if self.source.is_none() {
            return Err(invalid_state());
        }
        self.reset_note_state();
        if self.phase_reset {
            self.phase_accumulators.fill(0.0);
            self.phase_initialized = false;
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
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
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
        if frames == 0 {
            return Ok(self.is_finished());
        }
        let tuning = ValueSpan {
            start: tuning_start,
            end: tuning_end,
        };
        let hop_size = self.synthesis_plan.hop_size();
        for offset in 0..frames {
            if self.samples_until_next_frame == 0 {
                if !self.source_exhausted {
                    let position_value = position.value_at(offset, frames);
                    let freeze_value = freeze.value_at(offset, frames);
                    let shift_value = shift.value_at(offset, frames);
                    let tuning_value = tuning.value_at(offset, frames);
                    let pitch_ratio = pitch_ratio(note_number, tuning_value, self.root_note)?;
                    if let Some((lower_frame, upper_frame, frame_mix)) =
                        self.source_frame_position(&source, position_value)
                    {
                        self.add_spectral_frame(
                            &source,
                            lower_frame,
                            upper_frame,
                            frame_mix,
                            pitch_ratio,
                            shift_value,
                            sample_rate,
                        )?;
                        self.last_scheduled_end = self.last_scheduled_end.max(
                            self.output_position
                                .saturating_add(self.synthesis_plan.fft_size()),
                        );
                        self.advance_scan(&source, freeze_value);
                    } else {
                        self.source_exhausted = true;
                    }
                }
                self.samples_until_next_frame = hop_size;
            }
            let left_sample = Self::take_ola_sample(self.read_position, &mut self.ola_left);
            let right_sample = if source.channels == 2 {
                Self::take_ola_sample(self.read_position, &mut self.ola_right)
            } else {
                left_sample
            };
            mono[offset] = (left_sample + right_sample) * 0.5;
            left[offset] = left_sample;
            right[offset] = right_sample;
            self.samples_until_next_frame = self.samples_until_next_frame.saturating_sub(1);
            self.read_position = (self.read_position + 1) % self.ola_left.len();
            self.output_position = self.output_position.saturating_add(1);
        }
        ensure_finite(&mono[..frames])?;
        ensure_finite(&left[..frames])?;
        ensure_finite(&right[..frames])?;
        Ok(self.is_finished())
    }

    fn source_frame_position(
        &self,
        source: &PreparedSpectralAsset,
        position: f32,
    ) -> Option<(usize, usize, f32)> {
        #[allow(clippy::cast_precision_loss)]
        let source_frames = source.source_frames as f64;
        #[allow(clippy::cast_precision_loss)]
        let hop_size = source.hop_size as f64;
        let frame_position = (f64::from(position) + self.scan_progress) * source_frames / hop_size;
        #[allow(clippy::cast_precision_loss)]
        let last_frame = source.spectral_frame_count.saturating_sub(1) as f64;
        if !frame_position.is_finite() || frame_position > last_frame + f64::EPSILON {
            return None;
        }
        let clamped = frame_position.max(0.0).min(last_frame);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let lower = clamped.floor() as usize;
        let upper = lower.saturating_add(1).min(source.spectral_frame_count - 1);
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let mix = (clamped - lower as f64) as f32;
        Some((lower, upper, mix))
    }

    fn advance_scan(&mut self, source: &PreparedSpectralAsset, freeze: f32) {
        #[allow(clippy::cast_precision_loss)]
        let hop_ratio = source.hop_size as f64 / source.source_frames as f64;
        self.scan_progress += hop_ratio * f64::from(1.0 - freeze);
    }

    #[allow(clippy::too_many_arguments)]
    fn add_spectral_frame(
        &mut self,
        source: &PreparedSpectralAsset,
        lower_frame: usize,
        upper_frame: usize,
        frame_mix: f32,
        pitch_ratio: f32,
        shift_hz: f32,
        sample_rate: f64,
    ) -> Result<(), ProcessError> {
        let initialize_phase = !self.phase_initialized;
        if initialize_phase {
            self.initialize_phase_accumulators(source, lower_frame, upper_frame, frame_mix)?;
        }
        let bin_count = self.synthesis_plan.bin_count();
        let nyquist = source.sample_rate * 0.5;
        let plan = Arc::clone(&self.synthesis_plan);
        #[allow(clippy::cast_precision_loss)]
        let normalization = 1.0 / plan.fft_size() as f32;
        let read_position = self.read_position;
        let capacity = self.ola_left.len();
        for channel in 0..source.channels {
            self.inverse_input.fill(Complex::new(0.0, 0.0));
            for bin in 0..bin_count {
                let magnitude = Self::frame_value(
                    source,
                    &source.magnitudes,
                    channel,
                    bin,
                    lower_frame,
                    upper_frame,
                    frame_mix,
                )?;
                let instantaneous_frequency = Self::frame_value(
                    source,
                    &source.instantaneous_frequencies_hz,
                    channel,
                    bin,
                    lower_frame,
                    upper_frame,
                    frame_mix,
                )?;
                if !magnitude.is_finite() || !instantaneous_frequency.is_finite() {
                    return Err(super::non_finite());
                }
                let phase_index = channel * bin_count + bin;
                let mut phase = self
                    .phase_accumulators
                    .get(phase_index)
                    .copied()
                    .ok_or_else(invalid_state)?;
                if !initialize_phase && bin > 0 {
                    let phase_frequency = f64::from(instantaneous_frequency)
                        * f64::from(pitch_ratio)
                        + f64::from(shift_hz);
                    #[allow(clippy::cast_precision_loss)]
                    let phase_advance =
                        f64::from(TAU) * phase_frequency * source.hop_size as f64 / sample_rate;
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        phase = wrap_phase(phase + phase_advance as f32);
                        self.phase_accumulators[phase_index] = phase;
                    }
                }
                let complex = Complex::new(magnitude * phase.cos(), magnitude * phase.sin());
                if bin == 0 {
                    self.inverse_input[0].re += complex.re;
                } else {
                    #[allow(clippy::cast_precision_loss)]
                    let nominal_frequency =
                        bin as f64 * source.sample_rate / source.fft_size as f64;
                    let destination_frequency =
                        nominal_frequency * f64::from(pitch_ratio) + f64::from(shift_hz);
                    if destination_frequency.is_finite()
                        && (0.0..=nyquist).contains(&destination_frequency)
                    {
                        #[allow(clippy::cast_precision_loss)]
                        let destination_bin =
                            destination_frequency * source.fft_size as f64 / source.sample_rate;
                        self.add_fractional_bin(destination_bin, complex);
                    }
                }
            }
            let last_bin = bin_count.saturating_sub(1);
            self.inverse_input[0].im = 0.0;
            self.inverse_input[last_bin].im = 0.0;
            plan.inverse(
                &mut self.inverse_input,
                &mut self.inverse_output,
                &mut self.inverse_scratch,
            )
            .map_err(|_| invalid_state())?;
            for (offset, sample) in self.inverse_output.iter().copied().enumerate() {
                let index = (read_position + offset) % capacity;
                let output = if channel == 0 {
                    &mut self.ola_left
                } else {
                    &mut self.ola_right
                };
                output[index] += sample * normalization * plan.synthesis_window()[offset];
            }
        }
        Ok(())
    }

    fn initialize_phase_accumulators(
        &mut self,
        source: &PreparedSpectralAsset,
        lower_frame: usize,
        upper_frame: usize,
        frame_mix: f32,
    ) -> Result<(), ProcessError> {
        let bin_count = self.synthesis_plan.bin_count();
        for channel in 0..source.channels {
            for bin in 0..bin_count {
                let phase =
                    Self::frame_phase(source, channel, bin, lower_frame, upper_frame, frame_mix)?;
                let index = channel * bin_count + bin;
                self.phase_accumulators[index] = wrap_phase(phase);
            }
        }
        self.phase_initialized = true;
        Ok(())
    }

    fn frame_phase(
        source: &PreparedSpectralAsset,
        channel: usize,
        bin: usize,
        lower_frame: usize,
        upper_frame: usize,
        frame_mix: f32,
    ) -> Result<f32, ProcessError> {
        let lower_index = source
            .index(channel, lower_frame, bin)
            .ok_or_else(invalid_state)?;
        let upper_index = source
            .index(channel, upper_frame, bin)
            .ok_or_else(invalid_state)?;
        let lower = source
            .phases
            .get(lower_index)
            .copied()
            .ok_or_else(invalid_state)?;
        let upper = source
            .phases
            .get(upper_index)
            .copied()
            .ok_or_else(invalid_state)?;
        Ok(wrap_phase(lower + wrap_phase(upper - lower) * frame_mix))
    }

    fn frame_value(
        source: &PreparedSpectralAsset,
        values: &[f32],
        channel: usize,
        bin: usize,
        lower_frame: usize,
        upper_frame: usize,
        frame_mix: f32,
    ) -> Result<f32, ProcessError> {
        let lower_index = source
            .index(channel, lower_frame, bin)
            .ok_or_else(invalid_state)?;
        let upper_index = source
            .index(channel, upper_frame, bin)
            .ok_or_else(invalid_state)?;
        let lower = values.get(lower_index).copied().ok_or_else(invalid_state)?;
        let upper = values.get(upper_index).copied().ok_or_else(invalid_state)?;
        Ok(lower + (upper - lower) * frame_mix)
    }

    fn add_fractional_bin(&mut self, destination_bin: f64, value: Complex<f32>) {
        if !destination_bin.is_finite() {
            return;
        }
        let lower = destination_bin.floor();
        let fraction = destination_bin - lower;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        #[allow(clippy::cast_precision_loss)]
        if lower >= 0.0 && lower < self.inverse_input.len() as f64 {
            let index = lower as usize;
            self.inverse_input[index] += value * (1.0 - fraction as f32);
        }
        let upper = lower + 1.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        #[allow(clippy::cast_precision_loss)]
        if upper >= 0.0 && upper < self.inverse_input.len() as f64 {
            let index = upper as usize;
            self.inverse_input[index] += value * fraction as f32;
        }
    }

    fn is_finished(&self) -> bool {
        self.source_exhausted && self.output_position >= self.last_scheduled_end
    }

    fn take_ola_sample(read_position: usize, buffer: &mut [f32]) -> f32 {
        let sample = buffer[read_position];
        buffer[read_position] = 0.0;
        sample
    }

    pub(super) fn reset(&mut self) {
        self.reset_note_state();
        self.phase_accumulators.fill(0.0);
        self.phase_initialized = false;
    }

    fn reset_note_state(&mut self) {
        self.inverse_input.fill(Complex::new(0.0, 0.0));
        self.inverse_output.fill(0.0);
        self.inverse_scratch.fill(Complex::new(0.0, 0.0));
        self.ola_left.fill(0.0);
        self.ola_right.fill(0.0);
        self.scan_progress = 0.0;
        self.read_position = 0;
        self.samples_until_next_frame = 0;
        self.output_position = 0;
        self.last_scheduled_end = 0;
        self.source_exhausted = false;
    }
}

fn pitch_ratio(note_number: u8, tuning_cents: f32, root_note: u8) -> Result<f32, ProcessError> {
    let note_frequency = midi_note_frequency(note_number, cents_to_ratio(tuning_cents));
    let root_frequency = midi_note_frequency(root_note, 1.0);
    let ratio = note_frequency / root_frequency;
    if ratio.is_finite() && ratio > 0.0 {
        Ok(ratio)
    } else {
        Err(ProcessError::InvalidFrequency)
    }
}

fn wrap_phase(value: f32) -> f32 {
    (value + PI).rem_euclid(TAU) - PI
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

    fn targets(position: f32, freeze: f32, shift: f32) -> LayerGeneratorTargetSpan {
        let position = ValueSpan {
            start: position,
            end: position,
        };
        LayerGeneratorTargetSpan::Spectral {
            position,
            freeze: ValueSpan {
                start: freeze,
                end: freeze,
            },
            blur: ValueSpan {
                start: 0.0,
                end: 0.0,
            },
            shift: ValueSpan {
                start: shift,
                end: shift,
            },
            morph: None,
        }
    }

    #[test]
    fn spectral_render_does_not_allocate_after_prepare() {
        let mut runtime = test_runtime(true);
        runtime.start().expect("spectral runtime starts");
        let mut mono = vec![0.0; 64];
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        let allocations = crate::test_allocator::count_allocations(|| {
            runtime
                .render(
                    64,
                    60,
                    0.0,
                    0.0,
                    48_000.0,
                    targets(0.0, 0.0, 0.0),
                    &mut mono,
                    &mut left,
                    &mut right,
                )
                .expect("spectral render");
        });
        assert_eq!(allocations, 0);
    }

    #[test]
    fn note_on_resets_scan_without_resetting_phase_when_disabled() {
        let mut runtime = test_runtime(false);
        runtime.start().expect("spectral runtime starts");
        let mut mono = vec![0.0; 64];
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        runtime
            .render(
                64,
                60,
                0.0,
                0.0,
                48_000.0,
                targets(0.0, 0.0, 0.0),
                &mut mono,
                &mut left,
                &mut right,
            )
            .expect("spectral render");
        let phase = runtime.phase_accumulators.clone();
        runtime.start().expect("spectral runtime retriggers");
        assert_eq!(runtime.output_position, 0);
        assert!(runtime.scan_progress.abs() <= f64::EPSILON);
        assert_eq!(runtime.phase_accumulators, phase);
    }

    #[test]
    fn position_and_freeze_change_the_source_frame_cursor() {
        let mut runtime = test_runtime(true);
        runtime.start().expect("spectral runtime starts");
        let mut mono = vec![0.0; 64];
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        runtime
            .render(
                64,
                60,
                0.0,
                0.0,
                48_000.0,
                targets(0.5, 1.0, 0.0),
                &mut mono,
                &mut left,
                &mut right,
            )
            .expect("spectral render");
        assert!(runtime.scan_progress.abs() <= f64::EPSILON);
        assert_eq!(runtime.samples_until_next_frame, 192);
        assert!(!runtime.source_exhausted);
    }

    #[test]
    fn fractional_position_interpolates_adjacent_frames() {
        let runtime = test_runtime(true);
        let source = runtime.source.as_ref().expect("spectral source");
        let (lower, upper, mix) = runtime
            .source_frame_position(source, 0.53125)
            .expect("position remains in source");
        assert_eq!(lower, 8);
        assert_eq!(upper, 9);
        assert!((mix - 0.5).abs() <= 1.0e-6);
    }

    #[test]
    fn freeze_keeps_source_frame_but_advances_phase() {
        let mut runtime = test_runtime(true);
        runtime.start().expect("spectral runtime starts");
        let mut mono = vec![0.0; 512];
        let mut left = vec![0.0; 512];
        let mut right = vec![0.0; 512];
        runtime
            .render(
                512,
                60,
                0.0,
                0.0,
                48_000.0,
                targets(0.25, 1.0, 0.0),
                &mut mono,
                &mut left,
                &mut right,
            )
            .expect("spectral render");
        let phase_before = runtime.phase_accumulators[9];
        runtime
            .render(
                256,
                60,
                0.0,
                0.0,
                48_000.0,
                targets(0.25, 1.0, 0.0),
                &mut mono,
                &mut left,
                &mut right,
            )
            .expect("spectral render");
        let phase_after = runtime.phase_accumulators[9];
        assert!((phase_after - phase_before).abs() > 1.0e-4);
        assert!(runtime.scan_progress.abs() <= f64::EPSILON);
    }
}
