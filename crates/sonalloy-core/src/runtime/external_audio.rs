use crate::compiler::CompiledEnvelopeFollower;

/// Read-only external audio for one processing range.
#[derive(Clone, Copy)]
pub(crate) struct ExternalAudioBlock<'a> {
    channels: &'a [&'a [f32]],
}

impl<'a> ExternalAudioBlock<'a> {
    pub(crate) fn new(channels: &'a [&'a [f32]]) -> Self {
        Self { channels }
    }

    pub(crate) fn stereo_sample(self, index: usize) -> (f32, f32) {
        match self.channels.len() {
            1 => {
                let sample = self.channels[0][index];
                (sample, sample)
            }
            2 => (self.channels[0][index], self.channels[1][index]),
            _ => unreachable!("external audio was validated before processing"),
        }
    }
}

/// Fixed-size delay used to align external input with a processor carrier.
pub(crate) struct ExternalInputDelay {
    delay_frames: usize,
    left: Vec<f32>,
    right: Vec<f32>,
    position: usize,
}

impl ExternalInputDelay {
    pub(crate) fn new(delay_frames: usize) -> Self {
        Self {
            delay_frames,
            left: vec![0.0; delay_frames],
            right: vec![0.0; delay_frames],
            position: 0,
        }
    }

    pub(crate) fn next(&mut self, external: ExternalAudioBlock<'_>, index: usize) -> (f32, f32) {
        let (left, right) = external.stereo_sample(index);
        if self.delay_frames == 0 {
            return (left, right);
        }
        let aligned = (self.left[self.position], self.right[self.position]);
        self.left[self.position] = left;
        self.right[self.position] = right;
        self.position = (self.position + 1) % self.left.len();
        aligned
    }

    pub(crate) fn reset(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.position = 0;
    }

    pub(crate) fn buffer_bytes(&self) -> usize {
        (self.left.capacity() + self.right.capacity()) * std::mem::size_of::<f32>()
    }
}

/// One sample of a shared external amplitude follower.
pub(crate) struct EnvelopeFollowerRuntime {
    value: f32,
    compiled: CompiledEnvelopeFollower,
}

impl EnvelopeFollowerRuntime {
    pub(crate) fn new(compiled: CompiledEnvelopeFollower) -> Self {
        Self {
            value: 0.0,
            compiled,
        }
    }

    pub(crate) fn next(&mut self, external: ExternalAudioBlock<'_>, index: usize) -> f32 {
        let (left, right) = external.stereo_sample(index);
        let target = (left.abs().max(right.abs()) * self.compiled.input_gain_linear).min(1.0);
        let coefficient = if target > self.value {
            self.compiled.attack_coeff
        } else {
            self.compiled.release_coeff
        };
        self.value = target + coefficient * (self.value - target);
        self.value
    }

    pub(crate) fn reset(&mut self) {
        self.value = 0.0;
    }

    pub(crate) fn value(&self) -> f32 {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::{EnvelopeFollowerRuntime, ExternalAudioBlock, ExternalInputDelay};
    use crate::compiler::CompiledEnvelopeFollower;

    #[test]
    fn external_input_delay_preserves_zero_and_positive_delays() {
        let left = [1.0, 2.0, 3.0, 4.0];
        let right = [-1.0, -2.0, -3.0, -4.0];
        let channels = [&left[..], &right[..]];
        let external = ExternalAudioBlock::new(&channels);

        let mut direct = ExternalInputDelay::new(0);
        let direct_output = (0..left.len())
            .map(|index| direct.next(external, index))
            .collect::<Vec<_>>();
        assert_eq!(
            direct_output,
            vec![(1.0, -1.0), (2.0, -2.0), (3.0, -3.0), (4.0, -4.0)]
        );

        let mut one_frame_delayed = ExternalInputDelay::new(1);
        let one_frame_output = (0..left.len())
            .map(|index| one_frame_delayed.next(external, index))
            .collect::<Vec<_>>();
        assert_eq!(
            one_frame_output,
            vec![(0.0, 0.0), (1.0, -1.0), (2.0, -2.0), (3.0, -3.0)]
        );

        let mut delayed = ExternalInputDelay::new(2);
        let delayed_output = (0..left.len())
            .map(|index| delayed.next(external, index))
            .collect::<Vec<_>>();
        assert_eq!(
            delayed_output,
            vec![(0.0, 0.0), (0.0, 0.0), (1.0, -1.0), (2.0, -2.0)]
        );

        delayed.reset();
        assert_eq!(delayed.next(external, 0), (0.0, 0.0));
    }

    #[test]
    fn envelope_follower_is_linked_and_clamped() {
        let compiled = CompiledEnvelopeFollower {
            attack_coeff: 0.0,
            release_coeff: 0.0,
            input_gain_linear: 2.0,
        };
        let left = [2.0];
        let right = [0.5];
        let channels = [&left[..], &right[..]];
        let mut follower = EnvelopeFollowerRuntime::new(compiled);

        assert_relative_eq!(follower.next(ExternalAudioBlock::new(&channels), 0), 1.0);
        follower.reset();
        assert_relative_eq!(follower.value(), 0.0);
    }
}
