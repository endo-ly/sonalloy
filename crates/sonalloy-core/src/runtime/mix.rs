use std::f32::consts::FRAC_PI_4;

/// Convert a normalized pan value to constant-power stereo gains.
#[must_use]
pub(crate) fn constant_power_pan(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
    (angle.cos(), angle.sin())
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
}
