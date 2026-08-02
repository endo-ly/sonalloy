/// A fixed-rate scalar smoother used at voice boundaries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Smoother {
    current: f32,
    target: f32,
    remaining: usize,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn rounded_frame_count(seconds: f64) -> usize {
    seconds.max(0.0).round() as usize
}

impl Smoother {
    pub(crate) fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            remaining: 0,
        }
    }

    pub(crate) fn reset(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.remaining = 0;
    }

    pub(crate) fn set_target(&mut self, target: f32, frames: usize) {
        self.target = target;
        self.remaining = frames;
        if frames == 0 {
            self.current = target;
        }
    }

    pub(crate) fn next(&mut self) -> f32 {
        if self.remaining == 0 {
            return self.current;
        }
        let difference = self.target - self.current;
        #[allow(clippy::cast_precision_loss)]
        let remaining = self.remaining as f32;
        self.current += difference / remaining;
        self.remaining -= 1;
        if self.remaining == 0 {
            self.current = self.target;
        }
        self.current
    }

    pub(crate) fn span(&mut self, frames: usize) -> (f32, f32) {
        let start = self.current;
        if frames == 0 {
            return (start, start);
        }
        let advance = frames.min(self.remaining);
        if advance == 0 {
            return (start, start);
        }
        #[allow(clippy::cast_precision_loss)]
        let end = if self.remaining == 0 {
            start
        } else {
            let ratio = advance as f32 / self.remaining as f32;
            self.current + (self.target - self.current) * ratio
        };
        self.current = if advance == self.remaining {
            self.target
        } else {
            end
        };
        self.remaining -= advance;
        (start, self.current)
    }

    pub(crate) fn frames_until_target(&self) -> Option<usize> {
        (self.remaining != 0).then_some(self.remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoother_reaches_target_without_overshoot() {
        let mut smoother = Smoother::new(0.0);
        smoother.set_target(1.0, 4);
        let values: Vec<f32> = (0..4).map(|_| smoother.next()).collect();
        assert_eq!(values.last().copied(), Some(1.0));
        assert!(values.windows(2).all(|window| window[0] <= window[1]));
    }
}
