mod parse;
mod pattern;
mod render;

pub(crate) use parse::parse_midi;
pub(crate) use pattern::{export_pattern, import_pattern};
pub(crate) use render::read_midi;

pub(crate) const MOD_WHEEL_CONTROLLER: u8 = 1;
pub(crate) const SUSTAIN_PEDAL_CONTROLLER: u8 = 64;

pub(crate) fn normalize_pitch_bend(value: i16) -> f32 {
    if value < 0 {
        f32::from(value) / 8192.0
    } else {
        f32::from(value) / 8191.0
    }
}

pub(crate) fn normalize_control(value: u8) -> f32 {
    f32::from(value) / 127.0
}

pub(crate) fn denormalize_control(value: f32) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let value = (value.clamp(0.0, 1.0) * 127.0).round() as u8;
    value
}

pub(crate) fn denormalize_pitch_bend(value: f32) -> i16 {
    #[allow(clippy::cast_possible_truncation)]
    let value = (value.clamp(-1.0, 1.0)
        * if value.is_sign_negative() {
            8192.0
        } else {
            8191.0
        })
    .round() as i16;
    value.clamp(-8192, 8191)
}

pub(crate) fn tempo_to_microseconds_per_beat(bpm: f64) -> Option<u32> {
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let microseconds = (60_000_000.0 / bpm).round();
    if !microseconds.is_finite() || !(1.0..=16_777_215.0).contains(&microseconds) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(microseconds as u32)
}

pub(crate) fn note_id(channel: u8, note: u8, serial: u32) -> u64 {
    (u64::from(channel) << 56) | (u64::from(note) << 48) | u64::from(serial)
}
