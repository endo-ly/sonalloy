pub(crate) fn cubic_interpolate(p0: f32, p1: f32, p2: f32, p3: f32, fraction: f32) -> f32 {
    let a = 0.5 * (p2 - p0);
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
    ((c.mul_add(fraction, b)).mul_add(fraction, a)).mul_add(fraction, p1)
}
