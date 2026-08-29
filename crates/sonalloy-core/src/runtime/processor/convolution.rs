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
