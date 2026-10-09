use std::sync::Arc;

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex, num_complex::Complex};

use crate::compiler::convolution::{
    CONVOLUTION_BIN_COUNT, CONVOLUTION_FFT_SIZE, CONVOLUTION_PARTITION_SIZE, PreparedConvolutionIr,
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
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    input_block: [f32; CONVOLUTION_PARTITION_SIZE],
    input_count: usize,
    pending_output: [f32; CONVOLUTION_PARTITION_SIZE],
    pending_index: usize,
    overlap: [f32; CONVOLUTION_PARTITION_SIZE],
    dry_delay: [f32; CONVOLUTION_PARTITION_SIZE],
    dry_position: usize,
    history: Vec<Box<[Complex<f32>]>>,
    history_position: usize,
    /// Sum of the delayed partitions for the block being collected. Built
    /// incrementally so each sample carries an equal share of the work.
    delayed_accumulated: Vec<Complex<f32>>,
    delayed_products_done: usize,
    forward_input: Vec<f32>,
    inverse_output: Vec<f32>,
    fft_buffer: Vec<Complex<f32>>,
    accumulated: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    #[cfg(test)]
    work: Work,
}

#[cfg(test)]
#[derive(Default, Clone, Copy)]
struct Work {
    bin_products: usize,
    fft_transforms: usize,
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
        let mut planner = RealFftPlanner::<f32>::new();
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
    fn new(
        forward: Arc<dyn RealToComplex<f32>>,
        inverse: Arc<dyn ComplexToReal<f32>>,
        partition_count: usize,
    ) -> Self {
        let scratch_len = forward.get_scratch_len().max(inverse.get_scratch_len());
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
                vec![Complex::new(0.0, 0.0); CONVOLUTION_BIN_COUNT].into_boxed_slice();
                partition_count
            ],
            history_position: 0,
            delayed_accumulated: vec![Complex::new(0.0, 0.0); CONVOLUTION_BIN_COUNT],
            delayed_products_done: 0,
            forward_input: vec![0.0; CONVOLUTION_FFT_SIZE],
            inverse_output: vec![0.0; CONVOLUTION_FFT_SIZE],
            fft_buffer: vec![Complex::new(0.0, 0.0); CONVOLUTION_BIN_COUNT],
            accumulated: vec![Complex::new(0.0, 0.0); CONVOLUTION_BIN_COUNT],
            scratch: vec![Complex::new(0.0, 0.0); scratch_len],
            #[cfg(test)]
            work: Work::default(),
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
        self.accumulate_delayed_partitions(spectra);
        if self.input_count == CONVOLUTION_PARTITION_SIZE {
            self.compute_block(spectra)?;
        }
        if output.is_finite() {
            Ok(output)
        } else {
            Err(non_finite())
        }
    }

    /// Schedules bin products rather than whole partitions. The history slot
    /// being written stays untouched until every delayed product is complete.
    fn accumulate_delayed_partitions(&mut self, spectra: &[Box<[Complex<f32>]>]) {
        let product_count = (spectra.len() - 1) * CONVOLUTION_BIN_COUNT;
        let due = (self.input_count * product_count).div_ceil(CONVOLUTION_PARTITION_SIZE);
        while self.delayed_products_done < due {
            let partition_index = 1 + self.delayed_products_done / CONVOLUTION_BIN_COUNT;
            let first_bin = self.delayed_products_done % CONVOLUTION_BIN_COUNT;
            let count = (due - self.delayed_products_done).min(CONVOLUTION_BIN_COUNT - first_bin);
            let bins = first_bin..first_bin + count;
            let history_index =
                (self.history_position + self.history.len() - partition_index) % self.history.len();
            for ((target, input), ir) in self.delayed_accumulated[bins.clone()]
                .iter_mut()
                .zip(self.history[history_index][bins.clone()].iter())
                .zip(spectra[partition_index][bins].iter())
            {
                *target += input * ir;
            }
            self.delayed_products_done += count;
            #[cfg(test)]
            {
                self.work.bin_products += count;
            }
        }
    }

    fn compute_block(&mut self, spectra: &[Box<[Complex<f32>]>]) -> Result<(), ProcessError> {
        self.forward_input[..CONVOLUTION_PARTITION_SIZE].copy_from_slice(&self.input_block);
        self.forward_input[CONVOLUTION_PARTITION_SIZE..].fill(0.0);
        self.forward
            .process_with_scratch(
                &mut self.forward_input,
                &mut self.fft_buffer,
                &mut self.scratch,
            )
            .map_err(|_| invalid_state())?;
        #[cfg(test)]
        {
            self.work.fft_transforms += 1;
        }
        self.history[self.history_position].copy_from_slice(&self.fft_buffer);
        for (((target, delayed), input), ir) in self
            .accumulated
            .iter_mut()
            .zip(self.delayed_accumulated.iter())
            .zip(self.fft_buffer.iter())
            .zip(spectra[0].iter())
        {
            *target = delayed + input * ir;
        }
        self.delayed_accumulated.fill(Complex::new(0.0, 0.0));
        self.delayed_products_done = 0;
        #[cfg(test)]
        {
            self.work.bin_products += self.accumulated.len();
        }
        self.inverse
            .process_with_scratch(
                &mut self.accumulated,
                &mut self.inverse_output,
                &mut self.scratch,
            )
            .map_err(|_| invalid_state())?;
        #[cfg(test)]
        {
            self.work.fft_transforms += 1;
        }
        #[allow(clippy::cast_precision_loss)]
        let fft_scale = CONVOLUTION_FFT_SIZE as f32;
        for index in 0..CONVOLUTION_PARTITION_SIZE {
            self.pending_output[index] =
                self.inverse_output[index] / fft_scale + self.overlap[index];
            self.overlap[index] =
                self.inverse_output[index + CONVOLUTION_PARTITION_SIZE] / fft_scale;
        }
        self.pending_index = 0;
        self.input_count = 0;
        self.history_position = (self.history_position + 1) % self.history.len();
        if self
            .pending_output
            .iter()
            .chain(self.overlap.iter())
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
        self.delayed_accumulated.fill(Complex::new(0.0, 0.0));
        self.delayed_products_done = 0;
        self.forward_input.fill(0.0);
        self.inverse_output.fill(0.0);
        self.fft_buffer.fill(Complex::new(0.0, 0.0));
        self.accumulated.fill(Complex::new(0.0, 0.0));
        self.scratch.fill(Complex::new(0.0, 0.0));
        #[cfg(test)]
        {
            self.work = Work::default();
        }
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
        CONVOLUTION_BIN_COUNT, CONVOLUTION_FFT_SIZE, CONVOLUTION_LATENCY_FRAMES,
        CONVOLUTION_PARTITION_SIZE, PreparedConvolutionIr, PreparedConvolutionSpectra,
        partition_spectra,
    };
    use crate::runtime::modulation::ValueSpan;

    fn impulse_response(samples: &[f32], sample_rate: f32) -> Arc<PreparedConvolutionIr> {
        Arc::new(PreparedConvolutionIr {
            sample_rate: f64::from(sample_rate),
            source_channels: 1,
            source_frames: samples.len(),
            prepared_frames: samples.len(),
            partition_size: CONVOLUTION_PARTITION_SIZE,
            fft_size: CONVOLUTION_FFT_SIZE,
            spectra: PreparedConvolutionSpectra::Mono(partition_spectra(samples)),
        })
    }

    fn constant_span(value: f32) -> ValueSpan {
        ValueSpan::linear(value, value)
    }

    #[test]
    #[ignore = "wall-clock profiling; run with --release --ignored --nocapture"]
    fn profile_small_callbacks() {
        let mut seed = 1_u32;
        let samples: Vec<_> = (0..61_740)
            .map(|index| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                #[allow(clippy::cast_precision_loss)]
                let noise = seed as f32 / u32::MAX as f32 * 2.0 - 1.0;
                #[allow(clippy::cast_precision_loss)]
                let decay = (-8.0 * index as f32 / 61_740.0).exp();
                noise * decay * 0.01
            })
            .collect();
        let prepared = Arc::new(PreparedConvolutionIr {
            sample_rate: 44_100.0,
            source_channels: 2,
            source_frames: 67_200,
            prepared_frames: samples.len(),
            partition_size: CONVOLUTION_PARTITION_SIZE,
            fft_size: CONVOLUTION_FFT_SIZE,
            spectra: PreparedConvolutionSpectra::Stereo {
                left: partition_spectra(&samples),
                right: partition_spectra(&samples.iter().rev().copied().collect::<Vec<_>>()),
            },
        });
        let mut runtimes = vec![
            ConvolutionRuntime::new(Arc::clone(&prepared), 44_100.0).unwrap(),
            ConvolutionRuntime::new(prepared, 44_100.0).unwrap(),
        ]
        .into_boxed_slice();
        let mut times = Vec::new();
        let mut phases = [std::time::Duration::ZERO; 4];
        for block in 0..5_513 {
            let mut left = [[0.125; 64]; 2];
            let mut right = [[0.125; 64]; 2];
            let start = std::time::Instant::now();
            for ((runtime, left), right) in runtimes.iter_mut().zip(&mut left).zip(&mut right) {
                runtime
                    .process(constant_span(0.0), constant_span(0.18), left, right)
                    .unwrap();
            }
            let elapsed = start.elapsed();
            phases[block % 4] = phases[block % 4].max(elapsed);
            times.push(elapsed);
        }
        times.sort_unstable();
        eprintln!(
            "two stereo IRs, 64 frames: median={:?}, p99={:?}, max={:?}, phase maxima={phases:?}",
            times[times.len() / 2],
            times[times.len() * 99 / 100],
            times.last().unwrap()
        );
    }

    #[test]
    fn partitioned_convolution_preserves_latency_and_overlap_add() {
        let mut runtime =
            ConvolutionRuntime::new(impulse_response(&[1.0, 0.5], 48_000.0), 48_000.0)
                .expect("test impulse response prepares");
        let mut left = [0.0; 768];
        let mut right = [0.0; 768];
        let impulse_at = CONVOLUTION_PARTITION_SIZE - 1;
        left[impulse_at] = 1.0;
        right[impulse_at] = 1.0;

        runtime
            .process(
                constant_span(0.0),
                constant_span(1.0),
                &mut left,
                &mut right,
            )
            .expect("convolution processes");

        assert!(
            left[..CONVOLUTION_LATENCY_FRAMES + impulse_at]
                .iter()
                .all(|sample| sample.abs() < 1.0e-6)
        );
        assert!((left[CONVOLUTION_LATENCY_FRAMES + impulse_at] - 1.0).abs() < 1.0e-5);
        assert!((left[CONVOLUTION_LATENCY_FRAMES + impulse_at + 1] - 0.5).abs() < 1.0e-5);
        assert!(
            left.iter()
                .zip(right)
                .all(|(left, right)| left.to_bits() == right.to_bits())
        );
    }

    #[test]
    fn partitioned_convolution_is_independent_of_process_block_splits() {
        #[allow(clippy::cast_precision_loss)]
        let ir_left: Vec<_> = (0..789).map(|i| (i % 17) as f32 / 170.0 - 0.05).collect();
        let ir_right: Vec<_> = ir_left.iter().rev().map(|v| v * 0.5).collect();
        let ir = Arc::new(PreparedConvolutionIr {
            sample_rate: 48_000.0,
            source_channels: 2,
            source_frames: ir_left.len(),
            prepared_frames: ir_left.len(),
            partition_size: CONVOLUTION_PARTITION_SIZE,
            fft_size: CONVOLUTION_FFT_SIZE,
            spectra: PreparedConvolutionSpectra::Stereo {
                left: partition_spectra(&ir_left),
                right: partition_spectra(&ir_right),
            },
        });
        let mut whole_runtime = ConvolutionRuntime::new(Arc::clone(&ir), 48_000.0).unwrap();
        let mut split_runtime = ConvolutionRuntime::new(ir, 48_000.0).unwrap();
        let mut input = vec![0.0; 2_048];
        for (index, sample) in input[..700].iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            {
                *sample = (index % 23) as f32 / 23.0 - 0.5;
            }
        }
        let mut whole = input.clone();
        let mut split = input.clone();
        let mut whole_right: Vec<_> = input.iter().map(|v| v * -0.75).collect();
        let mut split_right = whole_right.clone();

        whole_runtime
            .process(
                constant_span(0.0),
                constant_span(0.18),
                &mut whole,
                &mut whole_right,
            )
            .expect("whole block processes");
        for (left_chunk, right_chunk) in split.chunks_mut(37).zip(split_right.chunks_mut(37)) {
            split_runtime
                .process(
                    constant_span(0.0),
                    constant_span(0.18),
                    left_chunk,
                    right_chunk,
                )
                .expect("split block processes");
        }

        for ((actual, split), (response, input_gain)) in
            [(&whole, &split), (&whole_right, &split_right)]
                .into_iter()
                .zip([(&ir_left, 1.0), (&ir_right, -0.75)])
        {
            for (index, (&actual, &split)) in actual.iter().zip(split).enumerate() {
                let expected = index
                    .checked_sub(CONVOLUTION_LATENCY_FRAMES)
                    .map_or(0.0, |at| {
                        let wet: f32 = response
                            .iter()
                            .take(at + 1)
                            .enumerate()
                            .map(|(lag, value)| value * input[at - lag] * input_gain)
                            .sum();
                        input[at] * input_gain * 0.82 + wet * 0.18
                    });
                assert!(
                    (actual - expected).abs() < 1.0e-5,
                    "sample {index}: {actual} != {expected}"
                );
                assert_eq!(actual.to_bits(), split.to_bits());
            }
        }
    }

    #[test]
    fn delayed_partition_work_is_spread_in_proportion_to_callback_size() {
        // Includes a short IR, the 1.4 second case at 44.1 kHz, and the
        // maximum supported duration at 192 kHz.
        for (sample_rate, frames) in [
            (48_000.0, 2_usize),
            (44_100.0, 61_740),
            (192_000.0, 1_920_000),
        ] {
            let samples = vec![0.25; frames];
            let partition_count = frames.div_ceil(CONVOLUTION_PARTITION_SIZE);
            let prepared = impulse_response(&samples, sample_rate);
            let mut runtime = ConvolutionRuntime::new(prepared, sample_rate)
                .expect("test impulse response prepares");

            let delayed_products = (partition_count - 1) * CONVOLUTION_BIN_COUNT;
            for block_size in [1, 37, 64, 127, 256, 513] {
                runtime.reset();
                let mut left = vec![1.0; block_size];
                let mut right = vec![1.0; block_size];
                for _ in 0..(CONVOLUTION_PARTITION_SIZE * 2).div_ceil(block_size) {
                    let previous = runtime.left.work;
                    let previous_count = runtime.left.input_count;
                    runtime
                        .process(
                            constant_span(0.0),
                            constant_span(1.0),
                            &mut left,
                            &mut right,
                        )
                        .expect("convolution processes");

                    let boundaries = (previous_count + block_size) / CONVOLUTION_PARTITION_SIZE;
                    let products = runtime.left.work.bin_products - previous.bin_products;
                    let budget = (block_size * delayed_products)
                        .div_ceil(CONVOLUTION_PARTITION_SIZE)
                        + boundaries * CONVOLUTION_BIN_COUNT;
                    assert!(
                        products <= budget,
                        "{frames} frames, {block_size} callback: {products} > {budget}"
                    );
                    assert_eq!(
                        runtime.left.work.fft_transforms - previous.fft_transforms,
                        boundaries * 2
                    );
                    assert_eq!(
                        runtime.right.work.bin_products,
                        runtime.left.work.bin_products
                    );
                    assert_eq!(
                        runtime.right.work.fft_transforms,
                        runtime.left.work.fft_transforms
                    );
                    assert_eq!(
                        runtime.left.delayed_products_done,
                        (runtime.left.input_count * delayed_products)
                            .div_ceil(CONVOLUTION_PARTITION_SIZE)
                    );
                }
            }
        }
    }

    #[test]
    fn realtime_processing_reuses_fft_scratch_without_allocations() {
        let ir = impulse_response(&vec![0.001; 61_740], 44_100.0);
        let mut runtime =
            ConvolutionRuntime::new(ir, 44_100.0).expect("test impulse response prepares");
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
