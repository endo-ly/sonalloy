use std::f32::consts::FRAC_PI_4;

/// Convert a normalized pan value to constant-power stereo gains.
#[must_use]
pub(crate) fn constant_power_pan(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
    (angle.cos(), angle.sin())
}

/// Convert normalized MIDI velocity to the explicit P1 gain response.
#[must_use]
pub(crate) fn velocity_gain(velocity: u8, amount: f32) -> f32 {
    let normalized = f32::from(velocity) / 127.0;
    (1.0 - amount) + amount * normalized
}

/// Lower a filter cutoff for lower velocities.
#[must_use]
pub(crate) fn velocity_cutoff(cutoff_hz: f32, velocity: u8, octaves: f32) -> f32 {
    let normalized = f32::from(velocity) / 127.0;
    let reduction = (1.0 - normalized) * octaves;
    cutoff_hz * 2.0_f32.powf(-reduction)
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
    fn velocity_response_has_expected_endpoints() {
        assert!((velocity_gain(127, 0.75) - 1.0).abs() < 1.0e-6);
        assert!((velocity_gain(1, 0.75) - 0.25).abs() < 0.01);
        assert!((velocity_cutoff(1_000.0, 127, 2.0) - 1_000.0).abs() < 1.0e-6);
        assert!((velocity_cutoff(1_000.0, 1, 2.0) - 250.0).abs() < 5.0);
    }
}
