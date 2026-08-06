pub(super) fn splitmix64_finalizer(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

pub(super) fn unit_f32(value: u64) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    {
        (value >> 40) as f32 / (1_u32 << 24) as f32
    }
}

pub(super) fn bipolar_f32(value: u64) -> f32 {
    unit_f32(value) * 2.0 - 1.0
}
