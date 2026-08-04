use std::f32::consts::FRAC_PI_4;

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
