use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex};

use crate::compiler::convolution::{
    CONVOLUTION_FFT_SIZE, CONVOLUTION_PARTITION_SIZE, PreparedConvolutionIr,
    PreparedConvolutionSpectra,
};
use crate::process::{ProcessError, ProcessorFailureKind};

use super::ValueSpan;

pub(crate) struct ConvolutionRuntime {
    ir: Arc<PreparedConvolutionIr>,
    left: ConvolutionChannel,
    right: ConvolutionChannel,
}

struct ConvolutionChannel {
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    input_block: [f32; CONVOLUTION_PARTITION_SIZE],
    input_count: usize,
    pending_output: [f32; CONVOLUTION_PARTITION_SIZE],
    pending_index: usize,
    overlap: [f32; CONVOLUTION_PARTITION_SIZE],
    dry_delay: [f32; CONVOLUTION_PARTITION_SIZE],
    dry_position: usize,
    history: Vec<Box<[Complex<f32>]>>,
    history_position: usize,
    fft_buffer: Vec<Complex<f32>>,
    accumulated: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl ConvolutionRuntime {
    pub(crate) fn new(
        ir: Arc<PreparedConvolutionIr>,
        sample_rate: f32,
    ) -> Result<Self, ProcessError> {
        if ir.partition_size != CONVOLUTION_PARTITION_SIZE
            || ir.fft_size != CONVOLUTION_FFT_SIZE
            || ir.partition_count() == 0
            || !sample_rate.is_finite()
            || (ir.sample_rate - f64::from(sample_rate)).abs() > 0.01
        {
            return Err(invalid_state());
        }
        let mut planner = FftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(CONVOLUTION_FFT_SIZE);
        let inverse = planner.plan_fft_inverse(CONVOLUTION_FFT_SIZE);
        Ok(Self {
            left: ConvolutionChannel::new(
                Arc::clone(&forward),
                Arc::clone(&inverse),
                ir.partition_count(),
            ),
            right: ConvolutionChannel::new(forward, inverse, ir.partition_count()),
            ir,
        })
    }

    pub(crate) fn process(
        &mut self,
        gain_db: ValueSpan,
        mix: ValueSpan,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(invalid_state());
        }
        let (left_spectra, right_spectra) = match &self.ir.spectra {
            PreparedConvolutionSpectra::Mono(spectra) => (spectra.as_ref(), spectra.as_ref()),
            PreparedConvolutionSpectra::Stereo { left, right } => (left.as_ref(), right.as_ref()),
        };
        for index in 0..left.len() {
            let gain = 10.0_f32.powf(gain_db.value_at(index, left.len()) / 20.0);
            let mix = mix.value_at(index, left.len());
            if !gain.is_finite() || !mix.is_finite() {
                return Err(non_finite());
            }
            let dry_left = self.left.delayed(left[index])?;
            let dry_right = self.right.delayed(right[index])?;
            let wet_left = self.left.process_sample(left[index], left_spectra)? * gain;
            let wet_right = self.right.process_sample(right[index], right_spectra)? * gain;
            left[index] = dry_left * (1.0 - mix) + wet_left * mix;
            right[index] = dry_right * (1.0 - mix) + wet_right * mix;
            if !left[index].is_finite() || !right[index].is_finite() {
                return Err(non_finite());
            }
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.left.reset();
        self.right.reset();
    }
}

impl ConvolutionChannel {
    fn new(forward: Arc<dyn Fft<f32>>, inverse: Arc<dyn Fft<f32>>, partition_count: usize) -> Self {
        let scratch_len = forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len());
        Self {
            forward,
            inverse,
            input_block: [0.0; CONVOLUTION_PARTITION_SIZE],
            input_count: 0,
            pending_output: [0.0; CONVOLUTION_PARTITION_SIZE],
            pending_index: CONVOLUTION_PARTITION_SIZE,
            overlap: [0.0; CONVOLUTION_PARTITION_SIZE],
            dry_delay: [0.0; CONVOLUTION_PARTITION_SIZE],
            dry_position: 0,
            history: vec![
                vec![Complex::new(0.0, 0.0); CONVOLUTION_FFT_SIZE].into_boxed_slice();
                partition_count
            ],
            history_position: 0,
            fft_buffer: vec![Complex::new(0.0, 0.0); CONVOLUTION_FFT_SIZE],
            accumulated: vec![Complex::new(0.0, 0.0); CONVOLUTION_FFT_SIZE],
            scratch: vec![Complex::new(0.0, 0.0); scratch_len],
        }
    }

    fn delayed(&mut self, input: f32) -> Result<f32, ProcessError> {
        if !input.is_finite() {
            return Err(non_finite());
        }
        let delayed = self.dry_delay[self.dry_position];
        self.dry_delay[self.dry_position] = input;
        self.dry_position = (self.dry_position + 1) % CONVOLUTION_PARTITION_SIZE;
        if delayed.is_finite() {
            Ok(delayed)
        } else {
            Err(non_finite())
        }
    }

    fn process_sample(
        &mut self,
        input: f32,
        spectra: &[Box<[Complex<f32>]>],
    ) -> Result<f32, ProcessError> {
        let output = if self.pending_index < CONVOLUTION_PARTITION_SIZE {
            self.pending_output[self.pending_index]
        } else {
            0.0
        };
        if !input.is_finite() {
            return Err(non_finite());
        }
        let had_pending = self.pending_index < CONVOLUTION_PARTITION_SIZE;
        if had_pending {
            self.pending_index += 1;
        }
        self.input_block[self.input_count] = input;
        self.input_count += 1;
        if self.input_count == CONVOLUTION_PARTITION_SIZE {
            self.compute_block(spectra)?;
        }
        if output.is_finite() {
            Ok(output)
        } else {
            Err(non_finite())
        }
    }

    fn compute_block(&mut self, spectra: &[Box<[Complex<f32>]>]) -> Result<(), ProcessError> {
        self.fft_buffer.fill(Complex::new(0.0, 0.0));
        for (target, sample) in self.fft_buffer[..CONVOLUTION_PARTITION_SIZE]
            .iter_mut()
            .zip(self.input_block)
        {
            target.re = sample;
        }
        self.forward
            .process_with_scratch(&mut self.fft_buffer, &mut self.scratch);
        self.history[self.history_position].copy_from_slice(&self.fft_buffer);
        self.accumulated.fill(Complex::new(0.0, 0.0));
        for (partition_index, ir_spectrum) in spectra.iter().enumerate() {
            let history_index =
                (self.history_position + self.history.len() - partition_index) % self.history.len();
            for index in 0..CONVOLUTION_FFT_SIZE {
                self.accumulated[index] += self.history[history_index][index] * ir_spectrum[index];
            }
        }
        self.inverse
            .process_with_scratch(&mut self.accumulated, &mut self.scratch);
        #[allow(clippy::cast_precision_loss)]
        let fft_scale = CONVOLUTION_FFT_SIZE as f32;
        for index in 0..CONVOLUTION_PARTITION_SIZE {
            self.pending_output[index] =
                self.accumulated[index].re / fft_scale + self.overlap[index];
            self.overlap[index] =
                self.accumulated[index + CONVOLUTION_PARTITION_SIZE].re / fft_scale;
        }
        self.pending_index = 0;
        self.input_count = 0;
        self.history_position = (self.history_position + 1) % self.history.len();
        self.input_block.fill(0.0);
        if self
            .pending_output
            .iter()
            .chain(self.input_block.iter())
            .all(|sample| sample.is_finite())
        {
            Ok(())
        } else {
            Err(non_finite())
        }
    }

    fn reset(&mut self) {
        self.input_block.fill(0.0);
        self.input_count = 0;
        self.pending_output.fill(0.0);
        self.pending_index = CONVOLUTION_PARTITION_SIZE;
        self.overlap.fill(0.0);
        self.dry_delay.fill(0.0);
        self.dry_position = 0;
        for partition in &mut self.history {
            partition.fill(Complex::new(0.0, 0.0));
        }
        self.history_position = 0;
        self.fft_buffer.fill(Complex::new(0.0, 0.0));
        self.accumulated.fill(Complex::new(0.0, 0.0));
        self.scratch.fill(Complex::new(0.0, 0.0));
    }
}

fn invalid_state() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::InvalidState,
    }
}

fn non_finite() -> ProcessError {
    ProcessError::ProcessorFailure {
        kind: ProcessorFailureKind::NonFinite,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::ConvolutionRuntime;
    use crate::compiler::convolution::{
        CONVOLUTION_FFT_SIZE, CONVOLUTION_LATENCY_FRAMES, CONVOLUTION_PARTITION_SIZE,
        PreparedConvolutionIr, PreparedConvolutionSpectra, partition_spectra,
    };
    use crate::runtime::modulation::ValueSpan;

    fn impulse_response() -> Arc<PreparedConvolutionIr> {
        Arc::new(PreparedConvolutionIr {
            sample_rate: 48_000.0,
            source_channels: 1,
            source_frames: 2,
            prepared_frames: 2,
            partition_size: CONVOLUTION_PARTITION_SIZE,
            fft_size: CONVOLUTION_FFT_SIZE,
            spectra: PreparedConvolutionSpectra::Mono(partition_spectra(&[1.0, 0.5])),
        })
    }

    fn constant_span(value: f32) -> ValueSpan {
        ValueSpan {
            start: value,
            end: value,
        }
    }

    #[test]
    fn partitioned_convolution_preserves_latency_and_overlap_add() {
        let mut runtime = ConvolutionRuntime::new(impulse_response(), 48_000.0)
            .expect("test impulse response prepares");
        let mut left = [0.0; 512];
        let mut right = [0.0; 512];
        left[0] = 1.0;
        right[0] = 1.0;

        runtime
            .process(
                constant_span(0.0),
                constant_span(1.0),
                &mut left,
                &mut right,
            )
            .expect("convolution processes");

        assert!(
            left[..CONVOLUTION_LATENCY_FRAMES]
                .iter()
                .all(|sample| sample.abs() < 1.0e-6)
        );
        assert!((left[CONVOLUTION_LATENCY_FRAMES] - 1.0).abs() < 1.0e-5);
        assert!((left[CONVOLUTION_LATENCY_FRAMES + 1] - 0.5).abs() < 1.0e-5);
        assert!(
            left.iter()
                .zip(right)
                .all(|(left, right)| left.to_bits() == right.to_bits())
        );
    }

    #[test]
    fn partitioned_convolution_is_independent_of_process_block_splits() {
        let mut whole_runtime =
            ConvolutionRuntime::new(impulse_response(), 48_000.0).expect("whole runtime prepares");
        let mut split_runtime =
            ConvolutionRuntime::new(impulse_response(), 48_000.0).expect("split runtime prepares");
        let mut whole = [0.0; 768];
        let mut split = [0.0; 768];
        whole[0] = 1.0;
        split[0] = 1.0;
        let mut whole_right = whole;
        let mut split_right = split;

        whole_runtime
            .process(
                constant_span(0.0),
                constant_span(1.0),
                &mut whole,
                &mut whole_right,
            )
            .expect("whole block processes");
        for (left_chunk, right_chunk) in split.chunks_mut(37).zip(split_right.chunks_mut(37)) {
            split_runtime
                .process(
                    constant_span(0.0),
                    constant_span(1.0),
                    left_chunk,
                    right_chunk,
                )
                .expect("split block processes");
        }

        assert!(
            whole
                .iter()
                .zip(split)
                .all(|(expected, actual)| (expected - actual).abs() < 1.0e-6)
        );
    }

    #[test]
    fn realtime_processing_reuses_fft_scratch_without_allocations() {
        let mut runtime = ConvolutionRuntime::new(impulse_response(), 48_000.0)
            .expect("test impulse response prepares");
        let mut left = [0.0; 512];
        let mut right = [0.0; 512];
        left[0] = 1.0;
        right[0] = 1.0;
        runtime
            .process(
                constant_span(0.0),
                constant_span(1.0),
                &mut left,
                &mut right,
            )
            .expect("warm-up convolution processes");

        let allocations = crate::test_allocator::count_allocations(|| {
            runtime
                .process(
                    constant_span(0.0),
                    constant_span(1.0),
                    &mut left,
                    &mut right,
                )
                .expect("realtime convolution processes");
        });
        assert_eq!(allocations, 0);
    }
}
