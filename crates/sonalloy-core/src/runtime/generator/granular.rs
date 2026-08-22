use std::sync::Arc;

use crate::asset::{PreparedAudio, PreparedAudioChannels};
use crate::compiler::CompiledGranular;
use crate::generator_parameters::{
    GRAIN_DENSITY, GRAIN_PAN_SPREAD, GRAIN_PITCH, GRAIN_RANDOMNESS, GRAIN_SIZE,
    GRANULAR_GRAIN_POOL_LIMIT, GRANULAR_POSITION,
};
use crate::process::ProcessError;

use super::super::interpolation::cubic_interpolate;
use super::super::mix::{constant_power_pan, stereo_balance};
use super::super::modulation::{LayerGeneratorTargetSpan, ValueSpan};
use super::super::random::{bipolar_f32, splitmix64_finalizer};
use super::{ensure_finite, invalid_state, playback_ratio, validate_generator_span};

const POSITION_STREAM: u64 = 0x706f_7369_7469_6f6e;
const PAN_STREAM: u64 = 0x7061_6e00_0000_0001;

#[derive(Debug, Clone, Copy)]
struct GrainState {
    active: bool,
    source_position: f64,
    source_increment: f64,
    age_frames: usize,
    length_frames: usize,
    pan: f32,
    left_gain: f32,
    right_gain: f32,
}

impl GrainState {
    const fn inactive() -> Self {
        Self {
            active: false,
            source_position: 0.0,
            source_increment: 0.0,
            age_frames: 0,
            length_frames: 0,
            pan: 0.0,
            left_gain: 0.0,
            right_gain: 0.0,
        }
    }
}

/// Runtime state for one prepared Granular Generator.
pub(crate) struct GranularRuntime {
    source: Option<Arc<PreparedAudio>>,
    root_note: u8,
    start_frame: usize,
    end_frame: usize,
    seed: u64,
    layer_hash: u64,
    grain_pool_limit: usize,
    grains: [GrainState; GRANULAR_GRAIN_POOL_LIMIT],
    note_id: u64,
    grain_serial: u64,
    scheduler_phase: f64,
}

impl GranularRuntime {
    pub(super) fn new(compiled: &CompiledGranular) -> Result<Self, ProcessError> {
        if !(1..=GRANULAR_GRAIN_POOL_LIMIT).contains(&compiled.grain_pool_limit)
            || (compiled.source.is_some() && compiled.start_frame >= compiled.end_frame)
        {
            return Err(invalid_state());
        }
        Ok(Self {
            source: compiled.source.clone(),
            root_note: compiled.root_note,
            start_frame: compiled.start_frame,
            end_frame: compiled.end_frame,
            seed: compiled.seed,
            layer_hash: compiled.layer_hash,
            grain_pool_limit: compiled.grain_pool_limit,
            grains: [GrainState::inactive(); GRANULAR_GRAIN_POOL_LIMIT],
            note_id: 0,
            grain_serial: 0,
            scheduler_phase: 0.0,
        })
    }

    pub(super) fn start(&mut self, note_id: u64) -> Result<(), ProcessError> {
        if self.source.is_none() || self.end_frame - self.start_frame < 2 {
            return Err(invalid_state());
        }
        self.note_id = note_id;
        self.grain_serial = 0;
        self.scheduler_phase = 1.0;
        self.grains.fill(GrainState::inactive());
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
        let LayerGeneratorTargetSpan::Granular {
            position,
            grain_size,
            density,
            pitch,
            randomness,
            pan_spread,
        } = targets
        else {
            return Err(invalid_state());
        };
        if frames == 0 {
            return Ok(());
        }
        if mono.len() < frames
            || left.len() < frames
            || right.len() < frames
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
        {
            return Err(invalid_state());
        }
        validate_generator_span(position, GRANULAR_POSITION)?;
        validate_generator_span(grain_size, GRAIN_SIZE)?;
        validate_generator_span(density, GRAIN_DENSITY)?;
        validate_generator_span(pitch, GRAIN_PITCH)?;
        validate_generator_span(randomness, GRAIN_RANDOMNESS)?;
        validate_generator_span(pan_spread, GRAIN_PAN_SPREAD)?;
        if self.source.is_none() || self.end_frame - self.start_frame < 2 {
            return Err(invalid_state());
        }
        if !tuning_start.is_finite() || !tuning_end.is_finite() {
            return Err(invalid_state());
        }
        left[..frames].fill(0.0);
        right[..frames].fill(0.0);
        mono[..frames].fill(0.0);

        for frame in 0..frames {
            if self.scheduler_phase >= 1.0 {
                self.spawn_grain(
                    frame,
                    frames,
                    note_number,
                    tuning_start,
                    tuning_end,
                    sample_rate,
                    position,
                    grain_size,
                    pitch,
                    randomness,
                    pan_spread,
                )?;
                self.scheduler_phase -= self.scheduler_phase.floor();
            }
            let (frame_left, frame_right) = self.render_frame()?;
            left[frame] = frame_left;
            right[frame] = frame_right;
            mono[frame] = f32::midpoint(frame_left, frame_right);
            let current_density = density.value_at(frame, frames);
            #[allow(clippy::cast_precision_loss)]
            {
                self.scheduler_phase += f64::from(current_density) / sample_rate;
            }
        }
        ensure_finite(&left[..frames])?;
        ensure_finite(&right[..frames])?;
        ensure_finite(&mono[..frames])
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::too_many_arguments
    )]
    fn spawn_grain(
        &mut self,
        frame: usize,
        frames: usize,
        note_number: u8,
        tuning_start: f32,
        tuning_end: f32,
        sample_rate: f64,
        position: ValueSpan,
        grain_size: ValueSpan,
        pitch: ValueSpan,
        randomness: ValueSpan,
        pan_spread: ValueSpan,
    ) -> Result<(), ProcessError> {
        let grain_serial = self.grain_serial;
        self.grain_serial = self.grain_serial.wrapping_add(1);
        let source_length = self.end_frame.saturating_sub(self.start_frame);
        if source_length < 2 {
            return Err(invalid_state());
        }
        let position = position.value_at(frame, frames).clamp(0.0, 1.0);
        let grain_size = grain_size.value_at(frame, frames);
        let grain_pitch = pitch.value_at(frame, frames);
        let randomness = randomness.value_at(frame, frames).clamp(0.0, 1.0);
        let pan_spread = pan_spread.value_at(frame, frames).clamp(0.0, 1.0);
        let tuning = tuning_start + (tuning_end - tuning_start) * frame_ratio(frame, frames);
        let note_ratio = playback_ratio(
            note_number,
            self.root_note,
            crate::compiler::cents_to_ratio(tuning + grain_pitch),
        );
        if !note_ratio.is_finite() || note_ratio <= 0.0 {
            return Err(ProcessError::InvalidFrequency);
        }
        let requested_length = rounded_frame_count(f64::from(grain_size) * sample_rate).max(1);
        let source_increment = note_ratio;
        let available_source_span = source_length.saturating_sub(1) as f64;
        #[allow(clippy::cast_precision_loss)]
        let maximum_length = (available_source_span / source_increment).floor() as usize + 1;
        let length_frames = requested_length.min(maximum_length.max(1));
        #[allow(clippy::cast_precision_loss)]
        let maximum_start = self.end_frame as f64
            - 1.0
            - source_increment * (length_frames.saturating_sub(1) as f64);
        let maximum_start = maximum_start.max(self.start_frame as f64);
        let random_position = bipolar_f32(random_value(
            self.seed,
            self.layer_hash,
            self.note_id,
            grain_serial,
            POSITION_STREAM,
        ));
        let normalized_position = if randomness == 0.0 {
            position
        } else {
            (position + random_position * randomness).rem_euclid(1.0)
        };
        let source_position = self.start_frame as f64
            + (maximum_start - self.start_frame as f64) * f64::from(normalized_position);
        let pan = bipolar_f32(random_value(
            self.seed,
            self.layer_hash,
            self.note_id,
            grain_serial,
            PAN_STREAM,
        )) * pan_spread;
        let (left_gain, right_gain) = constant_power_pan(pan);
        let slot = self
            .grains
            .iter()
            .take(self.grain_pool_limit)
            .position(|grain| !grain.active)
            .or_else(|| {
                self.grains
                    .iter()
                    .take(self.grain_pool_limit)
                    .enumerate()
                    .max_by_key(|(_, grain)| grain.age_frames)
                    .map(|(index, _)| index)
            })
            .ok_or_else(invalid_state)?;
        let grain = self.grains.get_mut(slot).ok_or_else(invalid_state)?;
        grain.active = true;
        grain.source_position = source_position;
        grain.source_increment = source_increment;
        grain.age_frames = 0;
        grain.length_frames = length_frames;
        grain.pan = pan;
        grain.left_gain = left_gain;
        grain.right_gain = right_gain;
        Ok(())
    }

    fn render_frame(&mut self) -> Result<(f32, f32), ProcessError> {
        let source = self.source.as_ref().ok_or_else(invalid_state)?;
        let normalization = self.window_power_normalization();
        let mut left = 0.0;
        let mut right = 0.0;
        for grain in self.grains.iter_mut().take(self.grain_pool_limit) {
            if !grain.active {
                continue;
            }
            let window = hann_window(grain.age_frames, grain.length_frames);
            let (source_left, source_right) = read_frame(
                source,
                grain.source_position,
                self.start_frame,
                self.end_frame,
            );
            let (grain_left, grain_right) = match &source.channels {
                PreparedAudioChannels::Mono { .. } => (
                    source_left * grain.left_gain,
                    source_left * grain.right_gain,
                ),
                PreparedAudioChannels::Stereo { .. } => {
                    let (left_gain, right_gain) = stereo_balance(grain.pan);
                    (source_left * left_gain, source_right * right_gain)
                }
            };
            left += grain_left * window * normalization;
            right += grain_right * window * normalization;
            grain.source_position += grain.source_increment;
            grain.age_frames = grain.age_frames.saturating_add(1);
            if grain.age_frames >= grain.length_frames {
                grain.active = false;
            }
        }
        if left.is_finite() && right.is_finite() {
            Ok((left, right))
        } else {
            Err(invalid_state())
        }
    }

    fn window_power_normalization(&self) -> f32 {
        let window_power = self
            .grains
            .iter()
            .take(self.grain_pool_limit)
            .filter(|grain| grain.active)
            .map(|grain| {
                let window = hann_window(grain.age_frames, grain.length_frames);
                window * window
            })
            .sum::<f32>();
        if window_power <= 0.0 {
            0.0
        } else {
            1.0 / window_power.max(1.0).sqrt()
        }
    }

    pub(super) fn reset(&mut self) {
        self.grains.fill(GrainState::inactive());
        self.note_id = 0;
        self.grain_serial = 0;
        self.scheduler_phase = 0.0;
    }
}

fn random_value(seed: u64, layer_hash: u64, note_id: u64, serial: u64, stream: u64) -> u64 {
    splitmix64_finalizer(
        seed ^ layer_hash.rotate_left(17)
            ^ note_id.rotate_left(31)
            ^ serial.rotate_left(47)
            ^ stream,
    )
}

fn frame_ratio(frame: usize, frames: usize) -> f32 {
    if frames == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        {
            frame as f32 / frames as f32
        }
    }
}

fn rounded_frame_count(seconds: f64) -> usize {
    if !seconds.is_finite() || seconds <= 0.0 {
        1
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            seconds.round() as usize
        }
    }
}

fn hann_window(age_frames: usize, length_frames: usize) -> f32 {
    if length_frames <= 1 {
        return 1.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let phase = age_frames.min(length_frames - 1) as f32 / (length_frames - 1) as f32;
    0.5 - 0.5 * (std::f32::consts::TAU * phase).cos()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn read_frame(
    source: &PreparedAudio,
    position: f64,
    start_frame: usize,
    end_frame: usize,
) -> (f32, f32) {
    let left = match &source.channels {
        PreparedAudioChannels::Mono { samples } => {
            cubic_sample(samples, position, start_frame, end_frame)
        }
        PreparedAudioChannels::Stereo { left, .. } => {
            cubic_sample(left, position, start_frame, end_frame)
        }
    };
    let right = match &source.channels {
        PreparedAudioChannels::Mono { .. } => left,
        PreparedAudioChannels::Stereo { right, .. } => {
            cubic_sample(right, position, start_frame, end_frame)
        }
    };
    (left, right)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn cubic_sample(source: &[f32], position: f64, start_frame: usize, end_frame: usize) -> f32 {
    if source.is_empty()
        || !position.is_finite()
        || start_frame >= end_frame
        || position < start_frame as f64
    {
        return 0.0;
    }
    let base = position.floor() as isize;
    let fraction = position.fract() as f32;
    let p0 = sample_at(source, base - 1, start_frame, end_frame);
    let p1 = sample_at(source, base, start_frame, end_frame);
    let p2 = sample_at(source, base + 1, start_frame, end_frame);
    let p3 = sample_at(source, base + 2, start_frame, end_frame);
    cubic_interpolate(p0, p1, p2, p3, fraction)
}

fn sample_at(source: &[f32], index: isize, start_frame: usize, end_frame: usize) -> f32 {
    let start = isize::try_from(start_frame).unwrap_or(isize::MAX);
    let end = isize::try_from(end_frame.saturating_sub(1)).unwrap_or(isize::MAX);
    usize::try_from(index.clamp(start, end))
        .ok()
        .and_then(|source_index| source.get(source_index))
        .copied()
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::SampleMetadata;

    fn source(values: &[f32]) -> Arc<PreparedAudio> {
        Arc::new(PreparedAudio {
            sample_rate: 48_000.0,
            frames: values.len(),
            source_metadata: SampleMetadata {
                source_sample_rate: 48_000,
                source_channels: 1,
                bits_per_sample: Some(16),
                source_frames: values.len(),
            },
            channels: PreparedAudioChannels::Mono {
                samples: Arc::from(values.to_vec()),
            },
        })
    }

    fn compiled() -> CompiledGranular {
        compiled_with_source(source(
            &(0..1024)
                .map(|value| f32::from(u16::try_from(value).expect("fixture value fits")) / 1024.0)
                .collect::<Vec<_>>(),
        ))
    }

    fn compiled_with_source(source: Arc<PreparedAudio>) -> CompiledGranular {
        let position = crate::parameter::ParameterHandle::new(0);
        let end_frame = source.frames;
        CompiledGranular {
            source: Some(source),
            asset_path: "test.wav".to_owned(),
            asset_sha256_specified: false,
            root_note: 60,
            start_frame: 0,
            end_frame,
            parameters: crate::compiler::CompiledGranularParameters {
                position,
                grain_size: position,
                density: position,
                pitch: position,
                randomness: position,
                pan_spread: position,
            },
            seed: 1,
            layer_hash: 2,
            grain_pool_limit: GRANULAR_GRAIN_POOL_LIMIT,
        }
    }

    fn constant_compiled() -> CompiledGranular {
        compiled_with_source(source(&vec![0.5; 48_000]))
    }

    #[test]
    fn hann_window_is_zero_at_both_boundaries_and_one_at_the_center() {
        assert!((hann_window(0, 9)).abs() < 1.0e-6);
        assert!((hann_window(8, 9)).abs() < 1.0e-6);
        assert!((hann_window(4, 9) - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn random_value_is_stable_for_the_same_grain_identity() {
        let first = random_value(1, 2, 3, 4, POSITION_STREAM);
        let second = random_value(1, 2, 3, 4, POSITION_STREAM);
        assert_eq!(first, second);
        assert_ne!(first, random_value(1, 2, 3, 5, POSITION_STREAM));
    }

    #[test]
    fn position_one_uses_the_region_end_side_without_randomness() {
        let definition = compiled();
        let mut runtime = GranularRuntime::new(&definition).expect("runtime prepares");
        runtime.start(9).expect("runtime starts");
        runtime
            .spawn_grain(
                0,
                1,
                60,
                0.0,
                0.0,
                48_000.0,
                ValueSpan {
                    start: 1.0,
                    end: 1.0,
                },
                ValueSpan {
                    start: 0.005,
                    end: 0.005,
                },
                ValueSpan {
                    start: 0.0,
                    end: 0.0,
                },
                ValueSpan {
                    start: 0.0,
                    end: 0.0,
                },
                ValueSpan {
                    start: 0.0,
                    end: 0.0,
                },
            )
            .expect("grain starts");
        assert!((runtime.grains[0].source_position - 784.0).abs() < 1.0e-6);
    }

    #[test]
    fn granular_runtime_reuses_a_fixed_pool_and_emits_finite_audio() {
        let definition = compiled();
        let mut runtime = GranularRuntime::new(&definition).expect("runtime prepares");
        runtime.start(9).expect("runtime starts");
        let zero = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
        let targets = LayerGeneratorTargetSpan::Granular {
            position: zero,
            grain_size: ValueSpan {
                start: 0.05,
                end: 0.05,
            },
            density: ValueSpan {
                start: 20.0,
                end: 20.0,
            },
            pitch: zero,
            randomness: zero,
            pan_spread: zero,
        };
        let mut mono = vec![0.0; 128];
        let mut left = vec![0.0; 128];
        let mut right = vec![0.0; 128];
        runtime
            .render(
                128, 60, 0.0, 0.0, 48_000.0, targets, &mut mono, &mut left, &mut right,
            )
            .expect("granular render");
        assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
        assert!(left.iter().any(|sample| sample.abs() > 0.0));
    }

    #[test]
    fn grain_window_normalization_does_not_jump_at_grain_boundaries() {
        let definition = constant_compiled();
        let mut runtime = GranularRuntime::new(&definition).expect("runtime prepares");
        runtime.start(9).expect("runtime starts");
        let zero = ValueSpan {
            start: 0.0,
            end: 0.0,
        };
        let targets = LayerGeneratorTargetSpan::Granular {
            position: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
            grain_size: ValueSpan {
                start: 0.08,
                end: 0.08,
            },
            density: ValueSpan {
                start: 24.0,
                end: 24.0,
            },
            pitch: zero,
            randomness: zero,
            pan_spread: zero,
        };
        let frames = 12_000;
        let mut mono = vec![0.0; frames];
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        runtime
            .render(
                frames, 60, 0.0, 0.0, 48_000.0, targets, &mut mono, &mut left, &mut right,
            )
            .expect("granular render");

        let max_delta = left
            .windows(2)
            .map(|samples| (samples[1] - samples[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            max_delta < 0.01,
            "grain boundary discontinuity is too large: {max_delta}"
        );
    }

    #[test]
    fn granular_runtime_process_path_does_not_allocate() {
        let definition = compiled();
        let mut runtime = GranularRuntime::new(&definition).expect("runtime prepares");
        runtime.start(9).expect("runtime starts");
        let targets = LayerGeneratorTargetSpan::Granular {
            position: ValueSpan {
                start: 0.5,
                end: 0.5,
            },
            grain_size: ValueSpan {
                start: 0.05,
                end: 0.05,
            },
            density: ValueSpan {
                start: 100.0,
                end: 100.0,
            },
            pitch: ValueSpan {
                start: 0.0,
                end: 0.0,
            },
            randomness: ValueSpan {
                start: 1.0,
                end: 1.0,
            },
            pan_spread: ValueSpan {
                start: 1.0,
                end: 1.0,
            },
        };
        let mut mono = vec![0.0; 128];
        let mut left = vec![0.0; 128];
        let mut right = vec![0.0; 128];
        let allocations = crate::test_allocator::count_allocations(|| {
            for _ in 0..32 {
                runtime
                    .render(
                        128, 60, 0.0, 0.0, 48_000.0, targets, &mut mono, &mut left, &mut right,
                    )
                    .expect("granular render");
            }
        });
        assert_eq!(allocations, 0);
    }
}
