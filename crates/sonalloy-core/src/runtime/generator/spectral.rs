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
const BLUR_SILENCE_THRESHOLD: f32 = 1.0e-5;

#[derive(Clone, Copy)]
struct SpectralFramePosition {
    lower: usize,
    upper: usize,
    mix: f32,
}

#[derive(Clone, Copy)]
struct SpectralBinValues {
    magnitude: f32,
    frequency: f32,
    phase: f32,
}

#[allow(clippy::struct_excessive_bools)]
pub(crate) struct SpectralRuntime {
    source: Option<Arc<PreparedSpectralAsset>>,
    source_b: Option<Arc<PreparedSpectralAsset>>,
    asset_b_required: bool,
    synthesis_plan: Arc<crate::spectral::SpectralSynthesisPlan>,
    root_note: u8,
    phase_reset: bool,
    phase_accumulators: Vec<f32>,
    phase_initialized: bool,
    blur_accumulators: Vec<f32>,
    blur_initialized: bool,
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
            if let Some(source_b) = &compiled.source_b {
                if !source_b.check_layout()
                    || source_b.sample_rate.total_cmp(&spec.sample_rate)
                        != std::cmp::Ordering::Equal
                    || source_b.fft_size != fft_size
                    || source_b.hop_size != hop_size
                    || source_b.channels != source.channels
                {
                    return Err(invalid_state());
                }
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
            source_b: compiled.source_b.clone(),
            asset_b_required: compiled.asset_b_path.is_some(),
            synthesis_plan: Arc::clone(&compiled.synthesis_plan),
            root_note: compiled.root_note,
            phase_reset: compiled.phase_reset,
            phase_accumulators: vec![0.0; phase_accumulator_count],
            phase_initialized: false,
            blur_accumulators: vec![0.0; phase_accumulator_count],
            blur_initialized: false,
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
        if self.source.is_none() || (self.asset_b_required && self.source_b.is_none()) {
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

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
        let source_b = self.source_b.clone();
        if self.asset_b_required && source_b.is_none() {
            return Err(invalid_state());
        }
        if frames == 0 {
            return Ok(self.is_finished());
        }
        let tuning = ValueSpan {
            start: tuning_start,
            end: tuning_end,
        };
        let morph = morph.unwrap_or(ValueSpan {
            start: 0.0,
            end: 0.0,
        });
        let hop_size = self.synthesis_plan.hop_size();
        for offset in 0..frames {
            if self.samples_until_next_frame == 0 {
                let position_value = position.value_at(offset, frames);
                let freeze_value = freeze.value_at(offset, frames);
                let blur_value = blur.value_at(offset, frames);
                let shift_value = shift.value_at(offset, frames);
                let tuning_value = tuning.value_at(offset, frames);
                let morph_value = morph.value_at(offset, frames);
                let pitch_ratio = pitch_ratio(note_number, tuning_value, self.root_note)?;
                let mut frame_scheduled = false;
                if !self.source_exhausted {
                    let normalized_position = f64::from(position_value) + self.scan_progress;
                    if let Some(source_position) =
                        Self::source_frame_position(&source, normalized_position)
                    {
                        let source_b_position = source_b.as_ref().and_then(|source_b| {
                            Self::source_frame_position(source_b, normalized_position)
                        });
                        self.add_spectral_frame(
                            &source,
                            source_b.as_deref(),
                            Some(source_position),
                            source_b_position,
                            pitch_ratio,
                            shift_value,
                            blur_value,
                            morph_value,
                            sample_rate,
                        )?;
                        frame_scheduled = true;
                        self.advance_scan(&source, freeze_value);
                    } else {
                        self.source_exhausted = true;
                    }
                }
                if self.source_exhausted && !frame_scheduled {
                    if self.blur_tail_active(blur_value) {
                        self.add_spectral_frame(
                            &source,
                            source_b.as_deref(),
                            None,
                            None,
                            pitch_ratio,
                            shift_value,
                            blur_value,
                            morph_value,
                            sample_rate,
                        )?;
                        frame_scheduled = true;
                    } else {
                        self.blur_accumulators.fill(0.0);
                    }
                }
                if frame_scheduled {
                    self.last_scheduled_end = self.last_scheduled_end.max(
                        self.output_position
                            .saturating_add(self.synthesis_plan.fft_size()),
                    );
                }
                self.samples_until_next_frame = hop_size;
            }
            let left_sample = Self::take_ola_sample(self.read_position, &mut self.ola_left);
            let right_sample = if source.channels == 2 {
                Self::take_ola_sample(self.read_position, &mut self.ola_right)
            } else {
                left_sample
            };
            mono[offset] = f32::midpoint(left_sample, right_sample);
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
        source: &PreparedSpectralAsset,
        normalized_position: f64,
    ) -> Option<SpectralFramePosition> {
        #[allow(clippy::cast_precision_loss)]
        let source_frames = source.source_frames as f64;
        #[allow(clippy::cast_precision_loss)]
        let hop_size = source.hop_size as f64;
        let frame_position = normalized_position * source_frames / hop_size;
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
        Some(SpectralFramePosition { lower, upper, mix })
    }

    fn advance_scan(&mut self, source: &PreparedSpectralAsset, freeze: f32) {
        #[allow(clippy::cast_precision_loss)]
        let hop_ratio = source.hop_size as f64 / source.source_frames as f64;
        self.scan_progress += hop_ratio * f64::from(1.0 - freeze);
    }

    #[allow(clippy::too_many_arguments)]
    fn add_spectral_frame(
        &mut self,
        source_a: &PreparedSpectralAsset,
        source_b: Option<&PreparedSpectralAsset>,
        source_a_position: Option<SpectralFramePosition>,
        secondary_position: Option<SpectralFramePosition>,
        pitch_ratio: f32,
        shift_hz: f32,
        blur_seconds: f32,
        morph: f32,
        sample_rate: f64,
    ) -> Result<(), ProcessError> {
        let initialize_phase = !self.phase_initialized;
        let initialize_blur = !self.blur_initialized;
        let blur_alpha =
            Self::blur_alpha(blur_seconds, sample_rate, self.synthesis_plan.hop_size())?;
        let bin_count = self.synthesis_plan.bin_count();
        let nyquist = source_a.sample_rate * 0.5;
        let plan = Arc::clone(&self.synthesis_plan);
        #[allow(clippy::cast_precision_loss)]
        let normalization = 1.0 / plan.fft_size() as f32;
        let read_position = self.read_position;
        let capacity = self.ola_left.len();
        for channel in 0..source_a.channels {
            self.inverse_input.fill(Complex::new(0.0, 0.0));
            for bin in 0..bin_count {
                let values = Self::spectral_bin_values(
                    source_a,
                    source_b,
                    source_a_position,
                    secondary_position,
                    channel,
                    bin,
                    morph,
                )?;
                let phase_index = channel * bin_count + bin;
                let magnitude = if initialize_blur || blur_seconds <= 0.0 {
                    self.blur_accumulators[phase_index] = values.magnitude;
                    values.magnitude
                } else {
                    let smoothed = self
                        .blur_accumulators
                        .get_mut(phase_index)
                        .ok_or_else(invalid_state)?;
                    *smoothed += blur_alpha * (values.magnitude - *smoothed);
                    *smoothed
                };
                let mut phase = self
                    .phase_accumulators
                    .get(phase_index)
                    .copied()
                    .ok_or_else(invalid_state)?;
                if initialize_phase {
                    phase = wrap_phase(values.phase);
                    self.phase_accumulators[phase_index] = phase;
                } else if bin > 0 {
                    let phase_frequency =
                        f64::from(values.frequency) * f64::from(pitch_ratio) + f64::from(shift_hz);
                    #[allow(clippy::cast_precision_loss)]
                    let phase_advance =
                        f64::from(TAU) * phase_frequency * source_a.hop_size as f64 / sample_rate;
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
                        bin as f64 * source_a.sample_rate / source_a.fft_size as f64;
                    let destination_frequency =
                        nominal_frequency * f64::from(pitch_ratio) + f64::from(shift_hz);
                    if destination_frequency.is_finite()
                        && (0.0..=nyquist).contains(&destination_frequency)
                    {
                        #[allow(clippy::cast_precision_loss)]
                        let destination_bin =
                            destination_frequency * source_a.fft_size as f64 / source_a.sample_rate;
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
        self.phase_initialized = true;
        self.blur_initialized = true;
        Ok(())
    }

    fn spectral_bin_values(
        source_a: &PreparedSpectralAsset,
        source_b: Option<&PreparedSpectralAsset>,
        source_a_position: Option<SpectralFramePosition>,
        secondary_position: Option<SpectralFramePosition>,
        channel: usize,
        bin: usize,
        morph: f32,
    ) -> Result<SpectralBinValues, ProcessError> {
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let nominal_frequency = bin as f32 * source_a.sample_rate as f32 / source_a.fft_size as f32;
        let magnitude_a = Self::frame_value(
            source_a,
            &source_a.magnitudes,
            channel,
            bin,
            source_a_position,
        )?;
        let frequency_a = Self::frame_value(
            source_a,
            &source_a.instantaneous_frequencies_hz,
            channel,
            bin,
            source_a_position,
        )?;
        let phase_a = Self::frame_phase(source_a, channel, bin, source_a_position)?;
        let Some(source_b) = source_b else {
            return Ok(SpectralBinValues {
                magnitude: magnitude_a,
                frequency: if source_a_position.is_some() {
                    frequency_a
                } else {
                    nominal_frequency
                },
                phase: phase_a,
            });
        };
        let magnitude_b = Self::frame_value(
            source_b,
            &source_b.magnitudes,
            channel,
            bin,
            secondary_position,
        )?;
        let frequency_b = Self::frame_value(
            source_b,
            &source_b.instantaneous_frequencies_hz,
            channel,
            bin,
            secondary_position,
        )?;
        let phase_b = Self::frame_phase(source_b, channel, bin, secondary_position)?;
        if morph <= 0.0 {
            return Ok(SpectralBinValues {
                magnitude: magnitude_a,
                frequency: if source_a_position.is_some() {
                    frequency_a
                } else {
                    nominal_frequency
                },
                phase: phase_a,
            });
        }
        if morph >= 1.0 {
            return Ok(SpectralBinValues {
                magnitude: magnitude_b,
                frequency: if secondary_position.is_some() {
                    frequency_b
                } else {
                    nominal_frequency
                },
                phase: phase_b,
            });
        }
        let weight_a = f64::from(1.0 - morph) * f64::from(magnitude_a) * f64::from(magnitude_a);
        let weight_b = f64::from(morph) * f64::from(magnitude_b) * f64::from(magnitude_b);
        let total_weight = weight_a + weight_b;
        if !total_weight.is_finite() {
            return Err(super::non_finite());
        }
        let frequency = if total_weight > f64::EPSILON {
            #[allow(clippy::cast_possible_truncation)]
            {
                ((weight_a * f64::from(frequency_a) + weight_b * f64::from(frequency_b))
                    / total_weight) as f32
            }
        } else {
            nominal_frequency
        };
        #[allow(clippy::cast_possible_truncation)]
        let magnitude = total_weight.sqrt() as f32;
        Ok(SpectralBinValues {
            magnitude,
            frequency,
            phase: wrap_phase(phase_a + wrap_phase(phase_b - phase_a) * morph),
        })
    }

    fn frame_phase(
        source: &PreparedSpectralAsset,
        channel: usize,
        bin: usize,
        position: Option<SpectralFramePosition>,
    ) -> Result<f32, ProcessError> {
        let Some(position) = position else {
            return Ok(0.0);
        };
        let lower_index = source
            .index(channel, position.lower, bin)
            .ok_or_else(invalid_state)?;
        let upper_index = source
            .index(channel, position.upper, bin)
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
        Ok(wrap_phase(lower + wrap_phase(upper - lower) * position.mix))
    }

    fn frame_value(
        source: &PreparedSpectralAsset,
        values: &[f32],
        channel: usize,
        bin: usize,
        position: Option<SpectralFramePosition>,
    ) -> Result<f32, ProcessError> {
        let Some(position) = position else {
            return Ok(0.0);
        };
        let lower_index = source
            .index(channel, position.lower, bin)
            .ok_or_else(invalid_state)?;
        let upper_index = source
            .index(channel, position.upper, bin)
            .ok_or_else(invalid_state)?;
        let lower = values.get(lower_index).copied().ok_or_else(invalid_state)?;
        let upper = values.get(upper_index).copied().ok_or_else(invalid_state)?;
        Ok(lower + (upper - lower) * position.mix)
    }

    fn blur_alpha(
        blur_seconds: f32,
        sample_rate: f64,
        hop_size: usize,
    ) -> Result<f32, ProcessError> {
        if blur_seconds <= 0.0 {
            return Ok(1.0);
        }
        #[allow(clippy::cast_precision_loss)]
        let exponent = -(hop_size as f64) / (f64::from(blur_seconds) * sample_rate);
        #[allow(clippy::cast_possible_truncation)]
        let alpha = (1.0 - exponent.exp()) as f32;
        if alpha.is_finite() {
            Ok(alpha)
        } else {
            Err(super::non_finite())
        }
    }

    fn blur_tail_active(&self, blur_seconds: f32) -> bool {
        blur_seconds > 0.0 && !self.blur_is_silent()
    }

    fn blur_is_silent(&self) -> bool {
        #[allow(clippy::cast_precision_loss)]
        let threshold = BLUR_SILENCE_THRESHOLD * self.synthesis_plan.fft_size() as f32;
        self.blur_accumulators
            .iter()
            .all(|value| value.abs() <= threshold)
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
        self.source_exhausted
            && self.blur_is_silent()
            && self.output_position >= self.last_scheduled_end
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
        self.blur_accumulators.fill(0.0);
        self.blur_initialized = false;
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
        test_runtime_with(phase_reset, 1024, false, false)
    }

    fn test_runtime_with(
        phase_reset: bool,
        fft_size: usize,
        stereo: bool,
        morph: bool,
    ) -> SpectralRuntime {
        let frame_count = fft_size * 4;
        let left_samples = (0..frame_count)
            .map(|index| {
                #[allow(clippy::cast_precision_loss)]
                let time = index as f32 / 48_000.0;
                (std::f32::consts::TAU * 440.0 * time).sin()
            })
            .collect::<Vec<_>>();
        let right_samples = (0..frame_count)
            .map(|index| {
                #[allow(clippy::cast_precision_loss)]
                let time = index as f32 / 48_000.0;
                (std::f32::consts::TAU * 660.0 * time).sin()
            })
            .collect::<Vec<_>>();
        let make_audio = |left: Vec<f32>, right: Vec<f32>| PreparedAudio {
            sample_rate: 48_000.0,
            frames: frame_count,
            source_metadata: SampleMetadata {
                source_sample_rate: 48_000,
                source_channels: usize::from(stereo) + 1,
                bits_per_sample: Some(32),
                source_frames: frame_count,
            },
            channels: if stereo {
                PreparedAudioChannels::Stereo {
                    left: Arc::from(left.into_boxed_slice()),
                    right: Arc::from(right.into_boxed_slice()),
                }
            } else {
                PreparedAudioChannels::Mono {
                    samples: Arc::from(left.into_boxed_slice()),
                }
            },
        };
        let audio = make_audio(left_samples, right_samples);
        let source = Arc::new(prepare_spectral_asset(&audio, fft_size).expect("spectral source"));
        let source_b = morph.then(|| {
            let left = (0..frame_count)
                .map(|index| {
                    #[allow(clippy::cast_precision_loss)]
                    let time = index as f32 / 48_000.0;
                    (std::f32::consts::TAU * 880.0 * time).sin()
                })
                .collect::<Vec<_>>();
            let right = (0..frame_count)
                .map(|index| {
                    #[allow(clippy::cast_precision_loss)]
                    let time = index as f32 / 48_000.0;
                    (std::f32::consts::TAU * 550.0 * time).sin()
                })
                .collect::<Vec<_>>();
            Arc::new(
                prepare_spectral_asset(&make_audio(left, right), fft_size).expect("morph source"),
            )
        });
        let synthesis_plan =
            Arc::new(crate::spectral::SpectralSynthesisPlan::new(fft_size).expect("spectral plan"));
        SpectralRuntime::new(
            &CompiledSpectral {
                source: Some(source),
                source_b,
                asset_a_path: "fixture.wav".to_owned(),
                asset_a_sha256_specified: false,
                asset_b_path: morph.then(|| "morph.wav".to_owned()),
                asset_b_sha256_specified: false,
                root_note: 60,
                fft_size,
                hop_size: fft_size / 4,
                phase_reset,
                parameters: CompiledSpectralParameters {
                    position: ParameterHandle::new(0),
                    freeze: ParameterHandle::new(1),
                    blur: ParameterHandle::new(2),
                    shift: ParameterHandle::new(3),
                    morph: morph.then(|| ParameterHandle::new(4)),
                },
                synthesis_plan,
                latency_frames: fft_size - fft_size / 4,
            },
            ProcessSpec::new(48_000.0, 64, 2).expect("process spec"),
        )
        .expect("spectral runtime")
    }

    fn targets(position: f32, freeze: f32, shift: f32) -> LayerGeneratorTargetSpan {
        targets_with_blur(position, freeze, 0.0, shift)
    }

    fn targets_with_blur(
        position: f32,
        freeze: f32,
        blur: f32,
        shift: f32,
    ) -> LayerGeneratorTargetSpan {
        targets_with_blur_and_morph(position, freeze, blur, shift, None)
    }

    fn targets_with_blur_and_morph(
        position: f32,
        freeze: f32,
        blur: f32,
        shift: f32,
        morph: Option<f32>,
    ) -> LayerGeneratorTargetSpan {
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
                start: blur,
                end: blur,
            },
            shift: ValueSpan {
                start: shift,
                end: shift,
            },
            morph: morph.map(|value| ValueSpan {
                start: value,
                end: value,
            }),
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
                    targets_with_blur(0.0, 0.0, 0.02, 0.0),
                    &mut mono,
                    &mut left,
                    &mut right,
                )
                .expect("spectral render");
        });
        assert_eq!(allocations, 0);
    }

    #[test]
    fn spectral_sixteen_voice_stereo_morph_render_does_not_allocate() {
        let mut runtimes = (0..16)
            .map(|_| test_runtime_with(true, 2048, true, true))
            .collect::<Vec<_>>();
        for runtime in &mut runtimes {
            runtime.start().expect("spectral runtime starts");
        }
        let mut mono = [0.0_f32; 64];
        let mut left = [0.0_f32; 64];
        let mut right = [0.0_f32; 64];
        let allocations = crate::test_allocator::count_allocations(|| {
            for (index, runtime) in runtimes.iter_mut().enumerate() {
                runtime
                    .render(
                        64,
                        60 + u8::try_from(index).expect("voice index fits"),
                        0.0,
                        0.0,
                        48_000.0,
                        targets_with_blur_and_morph(0.0, 0.0, 0.02, 0.0, Some(0.5)),
                        &mut mono,
                        &mut left,
                        &mut right,
                    )
                    .expect("spectral render");
            }
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
        let position = SpectralRuntime::source_frame_position(source, 0.53125)
            .expect("position remains in source");
        assert_eq!(position.lower, 8);
        assert_eq!(position.upper, 9);
        assert!((position.mix - 0.5).abs() <= 1.0e-6);
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

    #[test]
    fn blur_state_smooths_a_new_source_magnitude() {
        let mut runtime = test_runtime(true);
        let source = runtime.source.clone().expect("spectral source");
        let target_position = SpectralRuntime::source_frame_position(&source, 0.75)
            .expect("blur target remains in source");
        let target_index = source
            .index(0, target_position.lower, 9)
            .expect("target bin exists");
        let target_magnitude = source.magnitudes[target_index];
        runtime.start().expect("spectral runtime starts");
        let mut mono = vec![0.0; 256];
        let mut left = vec![0.0; 256];
        let mut right = vec![0.0; 256];
        runtime
            .render(
                64,
                60,
                0.0,
                0.0,
                48_000.0,
                targets_with_blur(0.0, 0.0, 0.02, 0.0),
                &mut mono,
                &mut left,
                &mut right,
            )
            .expect("initial spectral render");
        let initial_magnitude = runtime.blur_accumulators[9];
        runtime
            .render(
                256,
                60,
                0.0,
                0.0,
                48_000.0,
                targets_with_blur(0.75, 1.0, 0.02, 0.0),
                &mut mono,
                &mut left,
                &mut right,
            )
            .expect("blurred spectral render");
        let smoothed_magnitude = runtime.blur_accumulators[9];
        assert!((smoothed_magnitude - initial_magnitude).abs() > 1.0e-4);
        assert!((smoothed_magnitude - target_magnitude).abs() > 1.0e-4);
    }

    #[test]
    fn blur_tail_keeps_runtime_active_until_the_smoothed_state_decays() {
        let mut runtime = test_runtime(true);
        runtime.start().expect("spectral runtime starts");
        let mut mono = vec![0.0; 128];
        let mut left = vec![0.0; 128];
        let mut right = vec![0.0; 128];
        let mut guard = 0;
        while !runtime.source_exhausted {
            runtime
                .render(
                    128,
                    60,
                    0.0,
                    0.0,
                    48_000.0,
                    targets_with_blur(0.0, 0.0, 0.02, 0.0),
                    &mut mono,
                    &mut left,
                    &mut right,
                )
                .expect("spectral source render");
            guard += 1;
            assert!(guard < 100, "source did not reach its end");
        }
        assert!(!runtime.is_finished());
        let mut finished = false;
        for _ in 0..300 {
            finished = runtime
                .render(
                    128,
                    60,
                    0.0,
                    0.0,
                    48_000.0,
                    targets_with_blur(0.0, 0.0, 0.02, 0.0),
                    &mut mono,
                    &mut left,
                    &mut right,
                )
                .expect("spectral blur tail render");
            if finished {
                break;
            }
        }
        assert!(finished, "blur tail did not drain");
    }
}
