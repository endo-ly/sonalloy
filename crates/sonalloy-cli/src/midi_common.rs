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

pub(crate) fn note_id(channel: u8, note: u8, serial: u32) -> u64 {
    (u64::from(channel) << 56) | (u64::from(note) << 48) | u64::from(serial)
}
