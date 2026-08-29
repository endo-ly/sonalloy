use std::sync::Arc;

use crate::compiler::{CompiledWavetable, PreparedWavetable};
use crate::parameter::generator::{UNISON_DETUNE, UNISON_SPREAD, WAVETABLE_POSITION};
use crate::process::{ProcessError, ProcessSpec};

use super::super::interpolation::cubic_interpolate;
use super::super::mix::mix_component;
use super::super::modulation::{LayerGeneratorTargetSpan, ValueSpan};
use super::{
    base_frequencies, ensure_finite, initial_phase, invalid_state, non_finite,
    validate_generator_span,
};

struct WavetableComponentRuntime {
    phase: f32,
}

pub(crate) struct WavetableRuntime {
    components: Vec<WavetableComponentRuntime>,
    prepared: Option<Arc<PreparedWavetable>>,
    phase_reset: bool,
    phase: f32,
    effective_max_frequency: f32,
    unison: Arc<crate::compiler::CompiledUnison>,
}

impl WavetableRuntime {
    pub(super) fn new(
        compiled: &CompiledWavetable,
        _spec: ProcessSpec,
    ) -> Result<Self, ProcessError> {
        let voices = compiled.unison.position_distribution.len();
        if voices == 0
            || compiled.unison.phase_distribution.len() != voices
            || !compiled.effective_max_frequency.is_finite()
            || compiled.effective_max_frequency <= 0.0
        {
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
            effective_max_frequency: compiled.effective_max_frequency,
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
        let max_frequency = self.effective_max_frequency;

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
        *sample = read_sample(
            prepared,
            *phase,
            frequency,
            current_position,
            sample_rate,
            max_frequency,
        )?;
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
    max_frequency: f32,
) -> Result<f32, ProcessError> {
    if !phase.is_finite()
        || !frequency.is_finite()
        || frequency <= 0.0
        || !position.is_finite()
        || !sample_rate.is_finite()
        || sample_rate <= 0.0
        || !max_frequency.is_finite()
        || max_frequency <= 0.0
    {
        return Err(ProcessError::InvalidFrequency);
    }
    let (lower_band, upper_band, band_fraction) = select_band(prepared, frequency, max_frequency)?;
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
    max_frequency: f32,
) -> Result<(usize, usize, f32), ProcessError> {
    let last_index = prepared
        .bands
        .len()
        .checked_sub(1)
        .ok_or_else(invalid_state)?;
    let harmonic_budget = (f64::from(max_frequency) / f64::from(frequency)).max(1.0);
    let allowed_harmonic = harmonic_budget.floor().max(1.0);

    let mut safe_index = None;
    for (index, band) in prepared.bands.iter().enumerate() {
        let limit = u32::try_from(band.max_harmonic)
            .map_err(|_| invalid_state())
            .map(f64::from)?;
        if limit <= allowed_harmonic {
            safe_index = Some(index);
            break;
        }
    }
    let safe_index = if let Some(index) = safe_index {
        index
    } else {
        let last_limit = u32::try_from(
            prepared
                .bands
                .get(last_index)
                .ok_or_else(invalid_state)?
                .max_harmonic,
        )
        .map_err(|_| invalid_state())
        .map(f64::from)?;
        if last_limit > allowed_harmonic {
            return Err(invalid_state());
        }
        last_index
    };
    if safe_index == last_index {
        return Ok((last_index, last_index, 0.0));
    }

    let safe_limit = u32::try_from(
        prepared
            .bands
            .get(safe_index)
            .ok_or_else(invalid_state)?
            .max_harmonic,
    )
    .map_err(|_| invalid_state())
    .map(f64::from)?;
    let transition_upper = if safe_index == 0 {
        let next_limit = u32::try_from(
            prepared
                .bands
                .get(safe_index + 1)
                .ok_or_else(invalid_state)?
                .max_harmonic,
        )
        .map_err(|_| invalid_state())
        .map(f64::from)?;
        if next_limit <= 0.0 {
            return Err(invalid_state());
        }
        safe_limit * (safe_limit / next_limit)
    } else {
        u32::try_from(
            prepared
                .bands
                .get(safe_index - 1)
                .ok_or_else(invalid_state)?
                .max_harmonic,
        )
        .map_err(|_| invalid_state())
        .map(f64::from)?
    };
    if !safe_limit.is_finite()
        || safe_limit <= 0.0
        || !transition_upper.is_finite()
        || transition_upper <= safe_limit
    {
        return Err(invalid_state());
    }
    if harmonic_budget >= transition_upper {
        return Ok((safe_index, safe_index, 0.0));
    }
    if harmonic_budget <= safe_limit {
        return Ok((safe_index + 1, safe_index + 1, 0.0));
    }

    let position = harmonic_budget.log2();
    let upper_position = transition_upper.log2();
    let safe_position = safe_limit.log2();
    #[allow(clippy::cast_possible_truncation)]
    let fraction = ((upper_position - position) / (upper_position - safe_position)) as f32;
    Ok((safe_index, safe_index + 1, fraction.clamp(0.0, 1.0)))
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
    fn band_selection_crossfades_only_between_safe_bands() {
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
        let max_frequency = 48_000.0_f32 * 0.45;
        let geometric_mean = (64.0_f64 * 32.0).sqrt();
        #[allow(clippy::cast_possible_truncation)]
        let pair = select_band(
            &prepared,
            (f64::from(max_frequency) / geometric_mean) as f32,
            max_frequency,
        )
        .expect("band pair");
        assert_eq!(pair.0, 0);
        assert_eq!(pair.1, 1);
        assert!((pair.2 - 0.5).abs() < 1.0e-6);
        let lower_budget = 24.0_f64;
        #[allow(clippy::cast_possible_truncation)]
        let safe_pair = select_band(
            &prepared,
            (f64::from(max_frequency) / lower_budget) as f32,
            max_frequency,
        )
        .expect("safe band pair");
        assert_eq!(safe_pair.0, 1);
        assert_eq!(safe_pair.1, 2);
        assert!(prepared.bands[safe_pair.0].max_harmonic <= 24);
        assert!(prepared.bands[safe_pair.1].max_harmonic <= 24);
        assert_eq!(
            select_band(&prepared, max_frequency / 16.0, max_frequency).expect("band boundary"),
            (2, 2, 0.0)
        );
    }
}
