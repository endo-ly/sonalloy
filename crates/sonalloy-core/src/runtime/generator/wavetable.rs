use std::sync::Arc;

use crate::compiler::{CompiledWavetable, PreparedWavetable, wavetable_effective_max_frequency};
use crate::generator_parameters::{UNISON_DETUNE, UNISON_SPREAD, WAVETABLE_POSITION};
use crate::process::{ProcessError, ProcessSpec, ProcessorFailureKind};

use super::super::interpolation::cubic_interpolate;
use super::super::mix::mix_component;
use super::super::modulation::{LayerGeneratorTargetSpan, ValueSpan};
use super::validate_generator_span;

struct WavetableComponentRuntime {
    phase: f32,
}

pub(crate) struct WavetableRuntime {
    components: Vec<WavetableComponentRuntime>,
    prepared: Option<Arc<PreparedWavetable>>,
    phase_reset: bool,
    phase: f32,
    unison: Arc<crate::compiler::CompiledUnison>,
}

impl WavetableRuntime {
    pub(super) fn new(
        compiled: &CompiledWavetable,
        _spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let voices = compiled.unison.position_distribution.len();
        if voices == 0 || compiled.unison.phase_distribution.len() != voices {
            return Err(invalid_state());
        }
        let components = compiled
            .unison
            .phase_distribution
            .iter()
            .map(|offset| WavetableComponentRuntime {
                phase: initial_phase(compiled.phase, *offset),
            })
            .collect();
        Ok(Self {
            components,
            prepared: compiled.prepared.clone(),
            phase_reset: compiled.phase_reset,
            phase: compiled.phase,
            unison: Arc::clone(&compiled.unison),
        })
    }

    pub(super) fn start(&mut self) -> Result<(), ProcessError> {
        if self.prepared.is_none() {
            return Err(invalid_state());
        }
        if self.phase_reset {
            self.reset();
        }
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
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if frames == 0 {
            return Ok(());
        }
        let LayerGeneratorTargetSpan::Wavetable {
            position,
            unison_detune,
            unison_spread,
        } = targets
        else {
            return Err(invalid_state());
        };
        let prepared = self.prepared.as_ref().ok_or_else(invalid_state)?;
        if prepared.frame_count == 0
            || prepared.bands.is_empty()
            || prepared
                .bands
                .iter()
                .any(|band| band.frames.len() != prepared.frame_count)
        {
            return Err(invalid_state());
        }
        validate_generator_span(position, WAVETABLE_POSITION)?;
        let detune = unison_detune.unwrap_or(ValueSpan {
            start: 0.0,
            end: 0.0,
        });
        let spread = unison_spread.unwrap_or(ValueSpan {
            start: 0.0,
            end: 0.0,
        });
        validate_generator_span(detune, UNISON_DETUNE)?;
        validate_generator_span(spread, UNISON_SPREAD)?;
        let (base_start, base_end) = base_frequencies(note_number, tuning_start, tuning_end)?;
        let max_frequency = wavetable_effective_max_frequency(sample_rate);

        if self.components.len() == 1 {
            let component = self.components.first_mut().ok_or_else(invalid_state)?;
            render_component(
                frames,
                base_start,
                base_end,
                self.unison.position_distribution[0],
                detune,
                position,
                sample_rate,
                max_frequency,
                prepared,
                &mut component.phase,
                mono,
            )?;
            return ensure_finite(&mono[..frames]);
        }

        left[..frames].fill(0.0);
        right[..frames].fill(0.0);
        for index in 0..self.components.len() {
            let distribution = *self
                .unison
                .position_distribution
                .get(index)
                .ok_or_else(invalid_state)?;
            let component = self.components.get_mut(index).ok_or_else(invalid_state)?;
            render_component(
                frames,
                base_start,
                base_end,
                distribution,
                detune,
                position,
                sample_rate,
                max_frequency,
                prepared,
                &mut component.phase,
                mono,
            )?;
            if !mix_component(
                frames,
                mono,
                &mut left[..frames],
                &mut right[..frames],
                distribution,
                spread,
                self.unison.normalization,
            ) {
                return Err(invalid_state());
            }
        }
        ensure_finite(&left[..frames])?;
        ensure_finite(&right[..frames])
    }

    pub(super) fn reset(&mut self) {
        for (component, offset) in self
            .components
            .iter_mut()
            .zip(self.unison.phase_distribution.iter().copied())
        {
            component.phase = initial_phase(self.phase, offset);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_component(
    frames: usize,
    base_start: f32,
    base_end: f32,
    distribution: f32,
    detune: ValueSpan,
    position: ValueSpan,
    sample_rate: f64,
    max_frequency: f32,
    prepared: &PreparedWavetable,
    phase: &mut f32,
    output: &mut [f32],
) -> Result<(), ProcessError> {
    if output.len() < frames
        || !sample_rate.is_finite()
        || sample_rate <= 0.0
        || !max_frequency.is_finite()
        || max_frequency <= 0.0
    {
        return Err(invalid_state());
    }
    for (index, sample) in output.iter_mut().take(frames).enumerate() {
        let base = ValueSpan {
            start: base_start,
            end: base_end,
        }
        .value_at(index, frames);
        let current_detune = detune.value_at(index, frames);
        let frequency = component_frequency(base, distribution, current_detune, max_frequency)?;
        let current_position = position.value_at(index, frames);
        *sample = read_sample(prepared, *phase, frequency, current_position, sample_rate)?;
        #[allow(clippy::cast_possible_truncation)]
        let increment = (f64::from(frequency) / sample_rate) as f32;
        *phase = (*phase + increment).rem_euclid(1.0);
    }
    Ok(())
}

fn read_sample(
    prepared: &PreparedWavetable,
    phase: f32,
    frequency: f32,
    position: f32,
    sample_rate: f64,
) -> Result<f32, ProcessError> {
    if !phase.is_finite()
        || !frequency.is_finite()
        || frequency <= 0.0
        || !position.is_finite()
        || !sample_rate.is_finite()
        || sample_rate <= 0.0
    {
        return Err(ProcessError::InvalidFrequency);
    }
    let (lower_band, upper_band, band_fraction) = select_band(prepared, frequency, sample_rate)?;
    let (left_frame, right_frame, frame_fraction) = select_frame(prepared, position)?;
    let lower_left = interpolate_frame(prepared, lower_band, left_frame, phase)?;
    let lower_right = interpolate_frame(prepared, lower_band, right_frame, phase)?;
    let upper_left = interpolate_frame(prepared, upper_band, left_frame, phase)?;
    let upper_right = interpolate_frame(prepared, upper_band, right_frame, phase)?;
    let lower = lower_left + (lower_right - lower_left) * frame_fraction;
    let upper = upper_left + (upper_right - upper_left) * frame_fraction;
    let output = lower + (upper - lower) * band_fraction;
    if output.is_finite() {
        Ok(output)
    } else {
        Err(non_finite())
    }
}

fn select_band(
    prepared: &PreparedWavetable,
    frequency: f32,
    sample_rate: f64,
) -> Result<(usize, usize, f32), ProcessError> {
    let first = prepared.bands.first().ok_or_else(invalid_state)?;
    let last_index = prepared
        .bands
        .len()
        .checked_sub(1)
        .ok_or_else(invalid_state)?;
    let last = prepared.bands.get(last_index).ok_or_else(invalid_state)?;
    let allowed = (sample_rate * 0.45 / f64::from(frequency)).max(1.0);
    let first_limit = u32::try_from(first.max_harmonic)
        .map_err(|_| invalid_state())
        .map(f64::from)?;
    let last_limit = u32::try_from(last.max_harmonic)
        .map_err(|_| invalid_state())
        .map(f64::from)?;
    if allowed >= first_limit {
        return Ok((0, 0, 0.0));
    }
    if allowed <= last_limit {
        return Ok((last_index, last_index, 0.0));
    }
    for index in 0..last_index {
        let higher = prepared.bands.get(index).ok_or_else(invalid_state)?;
        let lower = prepared.bands.get(index + 1).ok_or_else(invalid_state)?;
        let higher_limit = u32::try_from(higher.max_harmonic)
            .map_err(|_| invalid_state())
            .map(f64::from)?;
        let lower_limit = u32::try_from(lower.max_harmonic)
            .map_err(|_| invalid_state())
            .map(f64::from)?;
        if allowed <= higher_limit && allowed >= lower_limit {
            if allowed.total_cmp(&higher_limit).is_eq() {
                return Ok((index, index, 0.0));
            }
            if allowed.total_cmp(&lower_limit).is_eq() {
                return Ok((index + 1, index + 1, 0.0));
            }
            let position = allowed.log2();
            let higher_position = higher_limit.log2();
            let lower_position = lower_limit.log2();
            #[allow(clippy::cast_possible_truncation)]
            let fraction =
                ((higher_position - position) / (higher_position - lower_position)) as f32;
            return Ok((index, index + 1, fraction.clamp(0.0, 1.0)));
        }
    }
    Err(invalid_state())
}

fn select_frame(
    prepared: &PreparedWavetable,
    position: f32,
) -> Result<(usize, usize, f32), ProcessError> {
    if !position.is_finite() || !(0.0..=1.0).contains(&position) {
        return Err(ProcessError::InvalidEventValue);
    }
    if prepared.frame_count == 1 {
        return Ok((0, 0, 0.0));
    }
    #[allow(clippy::cast_precision_loss)]
    let frame_position = position * (prepared.frame_count - 1) as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let left = frame_position.floor() as usize;
    let right = (left + 1).min(prepared.frame_count - 1);
    Ok((left, right, frame_position.fract()))
}

fn interpolate_frame(
    prepared: &PreparedWavetable,
    band_index: usize,
    frame_index: usize,
    phase: f32,
) -> Result<f32, ProcessError> {
    let band = prepared.bands.get(band_index).ok_or_else(invalid_state)?;
    let frame = band.frames.get(frame_index).ok_or_else(invalid_state)?;
    #[allow(clippy::cast_precision_loss)]
    let table_position = phase.rem_euclid(1.0) * prepared.frame_length as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let base = table_position.floor() as usize;
    let fraction = table_position.fract();
    let samples = &frame.guarded_samples;
    let offset = base.checked_add(1).ok_or_else(invalid_state)?;
    let p0 = *samples
        .get(offset.checked_sub(1).ok_or_else(invalid_state)?)
        .ok_or_else(invalid_state)?;
    let p1 = *samples.get(offset).ok_or_else(invalid_state)?;
    let p2 = *samples.get(offset + 1).ok_or_else(invalid_state)?;
    let p3 = *samples.get(offset + 2).ok_or_else(invalid_state)?;
    let output = cubic_interpolate(p0, p1, p2, p3, fraction);
    if output.is_finite() {
        Ok(output)
    } else {
        Err(non_finite())
    }
}

fn base_frequencies(
    note_number: u8,
    tuning_start: f32,
    tuning_end: f32,
) -> Result<(f32, f32), ProcessError> {
    let start = crate::compiler::midi_note_frequency(
        note_number,
        crate::compiler::cents_to_ratio(tuning_start),
    );
    let end = crate::compiler::midi_note_frequency(
        note_number,
        crate::compiler::cents_to_ratio(tuning_end),
    );
    if !start.is_finite() || !end.is_finite() || start <= 0.0 || end <= 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    Ok((start, end))
}

fn component_frequency(
    base: f32,
    distribution: f32,
    detune: f32,
    max_frequency: f32,
) -> Result<f32, ProcessError> {
    if !base.is_finite()
        || base <= 0.0
        || !distribution.is_finite()
        || !detune.is_finite()
        || !max_frequency.is_finite()
        || max_frequency <= 0.0
    {
        return Err(ProcessError::InvalidFrequency);
    }
    let ratio = 2.0_f32.powf(distribution * detune / 1200.0);
    let frequency = base * ratio;
    if !frequency.is_finite() || frequency <= 0.0 {
        return Err(ProcessError::InvalidFrequency);
    }
    Ok(frequency.clamp(f32::MIN_POSITIVE, max_frequency))
}

fn initial_phase(base: f32, offset: f32) -> f32 {
    (base + offset).rem_euclid(1.0)
}

fn ensure_finite(samples: &[f32]) -> Result<(), ProcessError> {
    if samples.iter().all(|sample| sample.is_finite()) {
        Ok(())
    } else {
        Err(non_finite())
    }
}

fn non_finite() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::NonFinite,
    }
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::InvalidState,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_position_selects_the_last_frame_at_one() {
        let prepared = PreparedWavetable {
            frame_length: 64,
            frame_count: 3,
            bands: Vec::new().into_boxed_slice(),
            source_metadata: crate::compiler::WavetableSourceMetadata {
                source_sample_rate: 48_000,
                source_channels: 1,
                bits_per_sample: Some(16),
                source_frames: 192,
            },
        };
        assert_eq!(
            select_frame(&prepared, 0.0).expect("first frame"),
            (0, 1, 0.0)
        );
        assert_eq!(
            select_frame(&prepared, 1.0).expect("last frame"),
            (2, 2, 0.0)
        );
    }

    #[test]
    fn band_selection_crossfades_in_log_frequency_space() {
        let prepared = PreparedWavetable {
            frame_length: 64,
            frame_count: 1,
            bands: [32, 16, 8]
                .into_iter()
                .map(|max_harmonic| crate::compiler::PreparedWavetableBand {
                    max_harmonic,
                    frames: Vec::new().into_boxed_slice(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            source_metadata: crate::compiler::WavetableSourceMetadata {
                source_sample_rate: 48_000,
                source_channels: 1,
                bits_per_sample: Some(16),
                source_frames: 64,
            },
        };
        let geometric_mean = (32.0_f64 * 16.0).sqrt();
        #[allow(clippy::cast_possible_truncation)]
        let pair = select_band(
            &prepared,
            (48_000.0 * 0.45 / geometric_mean) as f32,
            48_000.0,
        )
        .expect("band pair");
        assert_eq!(pair.0, 0);
        assert_eq!(pair.1, 1);
        assert!((pair.2 - 0.5).abs() < 1.0e-6);
        assert_eq!(
            select_band(&prepared, 48_000.0 * 0.45 / 16.0, 48_000.0).expect("band boundary"),
            (1, 1, 0.0)
        );
    }
}
