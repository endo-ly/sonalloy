use crate::compiler::CompiledNoise;
use crate::definition::NoiseColor;
use crate::parameter::generator::NOISE_CORRELATION;
use crate::process::{ProcessError, ProcessorFailureKind};

use super::super::modulation::ValueSpan;
use super::super::random::{bipolar_f32, splitmix64_finalizer};
use super::validate_generator_span;

const STREAM_SHARED: u64 = 0x7368_6172_6564_0001;
const STREAM_LEFT: u64 = 0x6c65_6674_0000_0002;
const STREAM_RIGHT: u64 = 0x7269_6768_7400_0003;
const PINK_ROWS: usize = 16;
const BROWN_ZERO_THRESHOLD: f32 = f32::EPSILON;

pub(crate) struct NoiseRuntime {
    color: NoiseColor,
    seed: u64,
    layer_hash: u64,
    brown_coefficient: f32,
    shared: NoiseStream,
    left: NoiseStream,
    right: NoiseStream,
}

impl NoiseRuntime {
    pub(super) fn new(compiled: &CompiledNoise) -> Self {
        Self {
            color: compiled.color,
            seed: compiled.seed,
            layer_hash: compiled.layer_hash,
            brown_coefficient: compiled.brown_coefficient,
            shared: NoiseStream::new(1),
            left: NoiseStream::new(2),
            right: NoiseStream::new(3),
        }
    }

    pub(super) fn start(&mut self, note_id: u64) {
        self.shared.reset(stream_seed(
            self.seed,
            self.layer_hash,
            note_id,
            STREAM_SHARED,
        ));
        self.left.reset(stream_seed(
            self.seed,
            self.layer_hash,
            note_id,
            STREAM_LEFT,
        ));
        self.right.reset(stream_seed(
            self.seed,
            self.layer_hash,
            note_id,
            STREAM_RIGHT,
        ));
    }

    pub(super) fn render(
        &mut self,
        frames: usize,
        correlation: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        validate_correlation(correlation)?;
        for index in 0..frames {
            #[allow(clippy::cast_precision_loss)]
            let correlation = correlation.value_at(index, frames);
            let shared = self.shared.next(self.color, self.brown_coefficient);
            let independent_left = self.left.next(self.color, self.brown_coefficient);
            let independent_right = self.right.next(self.color, self.brown_coefficient);
            let shared_gain = correlation.sqrt();
            let independent_gain = (1.0 - correlation).sqrt();
            left[index] = shared.mul_add(shared_gain, independent_left * independent_gain);
            right[index] = shared.mul_add(shared_gain, independent_right * independent_gain);
        }
        if left[..frames]
            .iter()
            .chain(&right[..frames])
            .all(|sample| sample.is_finite())
        {
            Ok(())
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            })
        }
    }

    pub(super) fn reset(&mut self) {
        self.shared.reset(1);
        self.left.reset(2);
        self.right.reset(3);
    }
}

fn validate_correlation(correlation: ValueSpan) -> Result<(), ProcessError> {
    validate_generator_span(correlation, NOISE_CORRELATION)
}

struct NoiseStream {
    random: Prng,
    pink_rows: [f32; PINK_ROWS],
    pink_sum: f32,
    pink_counter: u32,
    brown_state: f32,
}

impl NoiseStream {
    fn new(seed: u64) -> Self {
        Self {
            random: Prng::new(seed),
            pink_rows: [0.0; PINK_ROWS],
            pink_sum: 0.0,
            pink_counter: 0,
            brown_state: 0.0,
        }
    }

    fn reset(&mut self, seed: u64) {
        self.random = Prng::new(seed);
        self.pink_rows.fill(0.0);
        self.pink_sum = 0.0;
        self.pink_counter = 0;
        self.brown_state = 0.0;
    }

    fn next(&mut self, color: NoiseColor, brown_coefficient: f32) -> f32 {
        let white = self.random.next_bipolar();
        match color {
            NoiseColor::White => white,
            NoiseColor::Pink => self.next_pink(white),
            NoiseColor::Brown => {
                let input_gain = 1.0 - brown_coefficient;
                self.brown_state = brown_coefficient.mul_add(self.brown_state, input_gain * white);
                if self.brown_state.abs() < BROWN_ZERO_THRESHOLD {
                    self.brown_state = 0.0;
                }
                self.brown_state
            }
        }
    }

    fn next_pink(&mut self, white: f32) -> f32 {
        self.pink_counter = self.pink_counter.wrapping_add(1);
        let row = usize::try_from(self.pink_counter.trailing_zeros())
            .unwrap_or(usize::MAX)
            .min(PINK_ROWS - 1);
        let replacement = self.random.next_bipolar();
        self.pink_sum += replacement - self.pink_rows[row];
        self.pink_rows[row] = replacement;
        (self.pink_sum + white) * (1.0 / 17.0)
    }
}

struct Prng {
    state: u64,
}

impl Prng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            },
        }
    }

    fn next_bipolar(&mut self) -> f32 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        bipolar_f32(splitmix64_finalizer(self.state))
    }
}

fn stream_seed(seed: u64, layer_hash: u64, note_id: u64, stream: u64) -> u64 {
    splitmix64_finalizer(seed ^ layer_hash ^ note_id ^ stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::CompiledNoise;
    use crate::parameter::ParameterHandle;

    fn compiled_noise(color: NoiseColor, seed: u64) -> CompiledNoise {
        CompiledNoise {
            color,
            seed,
            correlation: ParameterHandle::new(0),
            layer_hash: 0x1234_5678_9abc_def0,
            brown_coefficient: 0.997,
        }
    }

    #[test]
    fn colors_produce_finite_bounded_samples() {
        for color in [NoiseColor::White, NoiseColor::Pink, NoiseColor::Brown] {
            let mut stream = NoiseStream::new(42);
            let samples: Vec<_> = (0..4096).map(|_| stream.next(color, 0.997)).collect();
            assert!(samples.iter().all(|sample| sample.is_finite()));
            assert!(samples.iter().all(|sample| (-1.0..=1.0).contains(sample)));
        }
    }

    #[test]
    fn negligible_brown_state_returns_to_zero() {
        let mut stream = NoiseStream::new(42);
        stream.brown_state = BROWN_ZERO_THRESHOLD * 0.5;
        let sample = stream.next(NoiseColor::Brown, 1.0);
        assert_eq!(stream.brown_state.to_bits(), 0.0_f32.to_bits());
        assert_eq!(sample.to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn seed_mixing_is_stable_and_note_specific() {
        let first = stream_seed(7, 11, 13, STREAM_SHARED);
        assert_eq!(first, stream_seed(7, 11, 13, STREAM_SHARED));
        assert_ne!(first, stream_seed(7, 11, 14, STREAM_SHARED));
        assert_ne!(first, stream_seed(7, 11, 13, STREAM_LEFT));
    }

    #[test]
    fn correlation_endpoints_and_reset_are_deterministic() {
        let span = ValueSpan {
            start: 1.0,
            end: 1.0,
        };
        let mut correlated = NoiseRuntime::new(&compiled_noise(NoiseColor::White, 7));
        correlated.start(13);
        let mut left = vec![0.0; 512];
        let mut right = vec![0.0; 512];
        correlated
            .render(512, span, &mut left, &mut right)
            .expect("correlated noise renders");
        assert_eq!(left, right);

        let mut independent = NoiseRuntime::new(&compiled_noise(NoiseColor::White, 7));
        independent.start(13);
        let mut independent_left = vec![0.0; 512];
        let mut independent_right = vec![0.0; 512];
        independent
            .render(
                512,
                ValueSpan {
                    start: 0.0,
                    end: 0.0,
                },
                &mut independent_left,
                &mut independent_right,
            )
            .expect("independent noise renders");
        assert!(
            independent_left
                .iter()
                .zip(&independent_right)
                .any(|(left, right)| left.to_bits() != right.to_bits())
        );

        correlated.reset();
        correlated.start(13);
        let mut repeated_left = vec![0.0; 512];
        let mut repeated_right = vec![0.0; 512];
        correlated
            .render(512, span, &mut repeated_left, &mut repeated_right)
            .expect("reset noise renders");
        assert_eq!(left, repeated_left);
        assert_eq!(right, repeated_right);
    }

    #[test]
    fn block_partition_does_not_change_the_noise_sequence() {
        let compiled = compiled_noise(NoiseColor::Pink, 99);
        let span = ValueSpan {
            start: 0.4,
            end: 0.4,
        };
        let mut one_block = NoiseRuntime::new(&compiled);
        one_block.start(5);
        let mut whole_left = vec![0.0; 257];
        let mut whole_right = vec![0.0; 257];
        one_block
            .render(257, span, &mut whole_left, &mut whole_right)
            .expect("whole block renders");

        let mut split = NoiseRuntime::new(&compiled);
        split.start(5);
        let mut split_left = vec![0.0; 257];
        let mut split_right = vec![0.0; 257];
        let mut offset = 0;
        for length in [32, 64, 161] {
            split
                .render(
                    length,
                    span,
                    &mut split_left[offset..offset + length],
                    &mut split_right[offset..offset + length],
                )
                .expect("split block renders");
            offset += length;
        }
        assert_eq!(whole_left, split_left);
        assert_eq!(whole_right, split_right);
    }
}
