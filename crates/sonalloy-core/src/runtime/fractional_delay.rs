use crate::process::{ProcessError, ProcessorFailureKind};

use super::interpolation::cubic_interpolate;

pub(crate) struct FractionalDelayLine {
    buffer: Vec<f32>,
    write_position: usize,
}

impl FractionalDelayLine {
    pub(crate) fn new(max_delay_frames: usize) -> Self {
        Self {
            buffer: vec![0.0; max_delay_frames.saturating_add(4).max(8)],
            write_position: 0,
        }
    }

    pub(crate) fn read(&self, delay_frames: f32) -> Result<f32, ProcessError> {
        #[allow(clippy::cast_precision_loss)]
        let buffer_length = self.buffer.len() as f32;
        if !delay_frames.is_finite() || delay_frames < 0.0 || delay_frames + 2.0 >= buffer_length {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::InvalidState,
            });
        }
        let length = self.buffer.len();
        // `write_position - delay_frames` may be just below an integer when the
        // delay is fractional.  Keeping the buffer position as an integer and
        // carrying only the interpolation fraction avoids rounding a wrapped
        // float up to `length`.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let delay_ceiling = delay_frames.ceil() as usize;
        #[allow(clippy::cast_precision_loss)]
        let fraction = delay_ceiling as f32 - delay_frames;
        let index = (self.write_position + length - delay_ceiling % length) % length;
        let p0 = self.buffer[(index + length - 1) % length];
        let p1 = self.buffer[index];
        let p2 = self.buffer[(index + 1) % length];
        let p3 = self.buffer[(index + 2) % length];
        let value = cubic_interpolate(p0, p1, p2, p3, fraction);
        if value.is_finite() {
            Ok(value)
        } else {
            Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            })
        }
    }

    pub(crate) fn write(&mut self, value: f32) -> Result<(), ProcessError> {
        if !value.is_finite() {
            return Err(ProcessError::ProcessorFailure {
                kind: ProcessorFailureKind::NonFinite,
            });
        }
        self.buffer[self.write_position] = value;
        self.write_position = (self.write_position + 1) % self.buffer.len();
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_position = 0;
    }

    pub(crate) fn capacity(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::FractionalDelayLine;

    #[test]
    fn integer_delay_reads_the_written_impulse() {
        let mut line = FractionalDelayLine::new(8);
        line.write(1.0).expect("write impulse");
        assert!((line.read(1.0).expect("read delay") - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn reset_clears_the_line() {
        let mut line = FractionalDelayLine::new(8);
        line.write(1.0).expect("write impulse");
        line.reset();
        assert!(line.read(1.0).expect("read reset line").abs() < 1.0e-6);
    }

    #[test]
    fn fractional_reads_wrap_without_exposing_the_buffer_length() {
        let mut line = FractionalDelayLine::new(8);
        for value in 0..line.capacity() {
            #[allow(clippy::cast_precision_loss)]
            let value = value as f32;
            line.write(value).expect("write sample");
        }

        for delay in [f32::EPSILON, 0.0, 0.25, 0.5, 0.75, 1.0, 7.0, 7.25] {
            let value = line.read(delay).expect("boundary read");
            assert!(value.is_finite(), "delay {delay} returned {value}");
        }
    }
}
