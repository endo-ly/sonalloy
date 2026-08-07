use std::f32::consts::FRAC_PI_4;

use super::modulation::ValueSpan;

/// Convert a normalized pan value to constant-power stereo gains.
#[must_use]
pub(crate) fn constant_power_pan(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
    (angle.cos(), angle.sin())
}

/// Apply a balance control to an already-stereo generator without changing the center image.
#[must_use]
pub(crate) fn stereo_balance(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    if pan <= 0.0 {
        (1.0, (pan.abs() * std::f32::consts::FRAC_PI_2).cos())
    } else {
        ((pan * std::f32::consts::FRAC_PI_2).cos(), 1.0)
    }
}

pub(crate) fn mix_component(
    frames: usize,
    component: &[f32],
    left: &mut [f32],
    right: &mut [f32],
    pan_distribution: f32,
    spread: ValueSpan,
    normalization: f32,
) -> bool {
    if !normalization.is_finite()
        || normalization <= 0.0
        || component.len() < frames
        || left.len() < frames
        || right.len() < frames
    {
        return false;
    }
    let (left_start, right_start) = constant_power_pan(pan_distribution * spread.start);
    let (left_end, right_end) = constant_power_pan(pan_distribution * spread.end);
    let left_gain = ValueSpan {
        start: left_start,
        end: left_end,
    };
    let right_gain = ValueSpan {
        start: right_start,
        end: right_end,
    };
    for (index, sample) in component.iter().take(frames).copied().enumerate() {
        if !mix_component_sample(
            index,
            frames,
            sample,
            left,
            right,
            left_gain,
            right_gain,
            normalization,
        ) {
            return false;
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn mix_component_sample(
    index: usize,
    frames: usize,
    sample: f32,
    left: &mut [f32],
    right: &mut [f32],
    left_gain: ValueSpan,
    right_gain: ValueSpan,
    normalization: f32,
) -> bool {
    if !sample.is_finite()
        || !normalization.is_finite()
        || normalization <= 0.0
        || left.len() <= index
        || right.len() <= index
    {
        return false;
    }
    left[index] += sample * left_gain.value_at(index, frames) * normalization;
    right[index] += sample * right_gain.value_at(index, frames) * normalization;
    left[index].is_finite() && right[index].is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_power_pan_has_equal_center_power() {
        let (left, right) = constant_power_pan(0.0);
        assert!((left - right).abs() < 1.0e-6);
        assert!((left.mul_add(left, right * right) - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn stereo_balance_preserves_center_and_mutes_the_opposite_edge() {
        assert_eq!(stereo_balance(0.0), (1.0, 1.0));
        assert!(stereo_balance(-1.0).1.abs() < 1.0e-6);
        assert!(stereo_balance(1.0).0.abs() < 1.0e-6);
    }
}
