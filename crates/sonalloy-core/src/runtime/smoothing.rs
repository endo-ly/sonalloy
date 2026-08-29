/// A fixed-rate scalar smoother used at voice boundaries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Smoother {
    start: f32,
    current: f32,
    target: f32,
    total: usize,
    elapsed: usize,
    remaining: usize,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn rounded_frame_count(seconds: f64) -> usize {
    seconds.max(0.0).round() as usize
}

impl Smoother {
    pub(crate) fn new(value: f32) -> Self {
        Self {
            start: value,
            current: value,
            target: value,
            total: 0,
            elapsed: 0,
            remaining: 0,
        }
    }

    pub(crate) fn reset(&mut self, value: f32) {
        self.start = value;
        self.current = value;
        self.target = value;
        self.total = 0;
        self.elapsed = 0;
        self.remaining = 0;
    }

    pub(crate) fn set_target(&mut self, target: f32, frames: usize) {
        self.start = self.current;
        self.target = target;
        self.total = frames;
        self.elapsed = 0;
        self.remaining = frames;
        if frames == 0 {
            self.current = target;
        }
    }

    pub(crate) fn current(&self) -> f32 {
        self.current
    }

    pub(crate) fn next(&mut self) -> f32 {
        if self.remaining == 0 {
            return self.current;
        }
        self.elapsed += 1;
        self.remaining -= 1;
        self.current = self.value_at(self.elapsed);
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
        self.elapsed += advance;
        let end = self.value_at(self.elapsed);
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

    #[allow(clippy::cast_precision_loss)]
    fn value_at(&self, elapsed: usize) -> f32 {
        if self.total == 0 {
            return self.target;
        }
        let ratio = elapsed.min(self.total) as f32 / self.total as f32;
        self.start + (self.target - self.start) * ratio
    }
}

#[cfg(test)]
mod tests {
    use super::Smoother;

    #[test]
    fn smoother_reaches_target_without_overshoot() {
        let mut smoother = Smoother::new(0.0);
        smoother.set_target(1.0, 4);
        let values: Vec<f32> = (0..4).map(|_| smoother.next()).collect();
        assert_eq!(values.last().copied(), Some(1.0));
        assert!(values.windows(2).all(|window| window[0] <= window[1]));
    }

    #[test]
    fn smoother_span_is_independent_of_partitioning() {
        let mut whole = Smoother::new(0.0);
        whole.set_target(1.0, 10);
        let whole_span = whole.span(10);

        let mut split = Smoother::new(0.0);
        split.set_target(1.0, 10);
        let first = split.span(3);
        let second = split.span(7);

        assert!((whole_span.0 - first.0).abs() < f32::EPSILON);
        assert!((whole_span.1 - second.1).abs() < f32::EPSILON);
        assert_eq!(split.remaining, 0);
    }
}
