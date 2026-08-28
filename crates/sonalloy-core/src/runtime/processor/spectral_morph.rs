use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

use crate::compiler::{
    CompiledSpectralMorphProcessor, SPECTRAL_MORPH_FFT_SIZE, SPECTRAL_MORPH_HOP_SIZE,
};
use crate::process::{ProcessError, ProcessSpec, ProcessorFailureKind};
use crate::runtime::external_audio::{ExternalAudioBlock, ExternalInputDelay};
use crate::spectral::{build_analysis_window, build_synthesis_window};

use super::ValueSpan;

const INITIAL_ANALYSIS_START: i64 = -3 * 256;

pub(crate) struct SpectralMorphRuntime {
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    analysis_window: Vec<f32>,
    synthesis_window: Vec<f32>,
    carrier_history: Vec<f32>,
    carrier_history_right: Vec<f32>,
    external_history: Vec<f32>,
    ola_left: Vec<f32>,
    ola_right: Vec<f32>,
    forward_input: Vec<f32>,
    carrier_spectrum: Vec<Complex<f32>>,
    carrier_spectrum_right: Vec<Complex<f32>>,
    external_spectrum: Vec<Complex<f32>>,
    inverse_spectrum: Vec<Complex<f32>>,
    inverse_spectrum_right: Vec<Complex<f32>>,
    inverse_output: Vec<f32>,
    forward_scratch: Vec<Complex<f32>>,
    inverse_scratch: Vec<Complex<f32>>,
    external_input: ExternalInputDelay,
    write_position: usize,
    frames_seen: usize,
    next_analysis_start: i64,
}

impl SpectralMorphRuntime {
    pub(crate) fn new(compiled: &CompiledSpectralMorphProcessor, _spec: ProcessSpec) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(SPECTRAL_MORPH_FFT_SIZE);
        let inverse = planner.plan_fft_inverse(SPECTRAL_MORPH_FFT_SIZE);
        let bin_count = SPECTRAL_MORPH_FFT_SIZE / 2 + 1;
        Self {
            forward: Arc::clone(&forward),
            inverse: Arc::clone(&inverse),
            analysis_window: build_analysis_window(SPECTRAL_MORPH_FFT_SIZE),
            synthesis_window: build_synthesis_window(
                SPECTRAL_MORPH_FFT_SIZE,
                SPECTRAL_MORPH_HOP_SIZE,
            ),
            carrier_history: vec![0.0; SPECTRAL_MORPH_FFT_SIZE],
            carrier_history_right: vec![0.0; SPECTRAL_MORPH_FFT_SIZE],
            external_history: vec![0.0; SPECTRAL_MORPH_FFT_SIZE],
            ola_left: vec![0.0; SPECTRAL_MORPH_FFT_SIZE * 4],
            ola_right: vec![0.0; SPECTRAL_MORPH_FFT_SIZE * 4],
            forward_input: vec![0.0; SPECTRAL_MORPH_FFT_SIZE],
            carrier_spectrum: forward.make_output_vec(),
            carrier_spectrum_right: forward.make_output_vec(),
            external_spectrum: forward.make_output_vec(),
            inverse_spectrum: vec![Complex::new(0.0, 0.0); bin_count],
            inverse_spectrum_right: vec![Complex::new(0.0, 0.0); bin_count],
            inverse_output: inverse.make_output_vec(),
            forward_scratch: forward.make_scratch_vec(),
            inverse_scratch: inverse.make_scratch_vec(),
            external_input: ExternalInputDelay::new(compiled.external_input_alignment_frames),
            write_position: 0,
            frames_seen: 0,
            next_analysis_start: INITIAL_ANALYSIS_START,
        }
    }

    pub(crate) fn process(
        &mut self,
        morph: ValueSpan,
        output_gain_db: ValueSpan,
        external: ExternalAudioBlock<'_>,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), ProcessError> {
        if left.len() != right.len() {
            return Err(invalid_state());
        }
        for index in 0..left.len() {
            let (external_left, external_right) = self.external_input.next(external, index);
            self.carrier_history[self.write_position] = left[index];
            self.carrier_history_right[self.write_position] = right[index];
            self.external_history[self.write_position] =
                f32::midpoint(external_left, external_right);
            let analysis_end = self
                .next_analysis_start
                .checked_add(
                    i64::try_from(SPECTRAL_MORPH_FFT_SIZE)
                        .map_err(|_| ProcessError::FrameOverflow)?,
                )
                .ok_or(ProcessError::FrameOverflow)?;
            let available_frames = i64::try_from(self.frames_seen.saturating_add(1))
                .map_err(|_| ProcessError::FrameOverflow)?;
            if available_frames >= analysis_end {
                self.analyze_frame(self.next_analysis_start, morph.value_at(index, left.len()))?;
                self.next_analysis_start = self
                    .next_analysis_start
                    .checked_add(
                        i64::try_from(SPECTRAL_MORPH_HOP_SIZE)
                            .map_err(|_| ProcessError::FrameOverflow)?,
                    )
                    .ok_or(ProcessError::FrameOverflow)?;
            }
            let output_index = self.frames_seen % self.ola_left.len();
            let gain = db_to_linear(output_gain_db.value_at(index, left.len()));
            left[index] = self.ola_left[output_index] * gain;
            right[index] = self.ola_right[output_index] * gain;
            self.ola_left[output_index] = 0.0;
            self.ola_right[output_index] = 0.0;
            self.write_position = (self.write_position + 1) % SPECTRAL_MORPH_FFT_SIZE;
            self.frames_seen = self.frames_seen.saturating_add(1);
            if !gain.is_finite() || !left[index].is_finite() || !right[index].is_finite() {
                return Err(non_finite());
            }
        }
        Ok(())
    }

    fn analyze_frame(&mut self, start: i64, morph: f32) -> Result<(), ProcessError> {
        Self::forward_frame(
            &*self.forward,
            &mut self.forward_input,
            &mut self.forward_scratch,
            &self.analysis_window,
            &self.carrier_history,
            start,
            &mut self.carrier_spectrum,
        )?;
        Self::forward_frame(
            &*self.forward,
            &mut self.forward_input,
            &mut self.forward_scratch,
            &self.analysis_window,
            &self.carrier_history_right,
            start,
            &mut self.carrier_spectrum_right,
        )?;
        Self::forward_frame(
            &*self.forward,
            &mut self.forward_input,
            &mut self.forward_scratch,
            &self.analysis_window,
            &self.external_history,
            start,
            &mut self.external_spectrum,
        )?;
        let carrier_energy = spectrum_energy(&self.carrier_spectrum);
        let carrier_energy_right = spectrum_energy(&self.carrier_spectrum_right);
        let external_energy = spectrum_energy(&self.external_spectrum);
        let morph = morph.clamp(0.0, 1.0);
        let bin_count = self.inverse_spectrum.len();
        for (index, ((carrier, external), output)) in self
            .carrier_spectrum
            .iter()
            .zip(&self.external_spectrum)
            .zip(&mut self.inverse_spectrum)
            .enumerate()
        {
            *output =
                morph_spectrum_value(*carrier, *external, morph, carrier_energy, external_energy);
            clear_real_fft_imaginary_bin(output, index, bin_count);
        }
        let output_start = usize::try_from(
            start
                .checked_add(
                    i64::try_from(SPECTRAL_MORPH_FFT_SIZE)
                        .map_err(|_| ProcessError::FrameOverflow)?,
                )
                .ok_or(ProcessError::FrameOverflow)?,
        )
        .map_err(|_| ProcessError::FrameOverflow)?;
        Self::inverse_frame(
            &*self.inverse,
            &mut self.inverse_spectrum,
            &mut self.inverse_output,
            &mut self.inverse_scratch,
            &self.synthesis_window,
            output_start,
            &mut self.ola_left,
        )?;
        for (index, ((carrier, external), output)) in self
            .carrier_spectrum_right
            .iter()
            .zip(&self.external_spectrum)
            .zip(&mut self.inverse_spectrum_right)
            .enumerate()
        {
            *output = morph_spectrum_value(
                *carrier,
                *external,
                morph,
                carrier_energy_right,
                external_energy,
            );
            clear_real_fft_imaginary_bin(output, index, bin_count);
        }
        Self::inverse_frame(
            &*self.inverse,
            &mut self.inverse_spectrum_right,
            &mut self.inverse_output,
            &mut self.inverse_scratch,
            &self.synthesis_window,
            output_start,
            &mut self.ola_right,
        )?;
        Ok(())
    }

    fn forward_frame(
        forward: &dyn RealToComplex<f32>,
        input: &mut [f32],
        scratch: &mut [Complex<f32>],
        window: &[f32],
        history: &[f32],
        start: i64,
        spectrum: &mut [Complex<f32>],
    ) -> Result<(), ProcessError> {
        for index in 0..SPECTRAL_MORPH_FFT_SIZE {
            let source_frame = start
                .checked_add(i64::try_from(index).map_err(|_| ProcessError::FrameOverflow)?)
                .ok_or(ProcessError::FrameOverflow)?;
            input[index] = usize::try_from(source_frame)
                .ok()
                .map_or(0.0, |position| history[position % SPECTRAL_MORPH_FFT_SIZE])
                * window[index];
        }
        forward
            .process_with_scratch(input, spectrum, scratch)
            .map_err(|_| invalid_state())
    }

    fn inverse_frame(
        inverse: &dyn ComplexToReal<f32>,
        spectrum: &mut [Complex<f32>],
        output: &mut [f32],
        scratch: &mut [Complex<f32>],
        window: &[f32],
        output_start: usize,
        ola: &mut [f32],
    ) -> Result<(), ProcessError> {
        inverse
            .process_with_scratch(spectrum, output, scratch)
            .map_err(|_| invalid_state())?;
        for index in 0..SPECTRAL_MORPH_FFT_SIZE {
            let output_index = (output_start + index) % ola.len();
            #[allow(clippy::cast_precision_loss)]
            let sample = output[index] / SPECTRAL_MORPH_FFT_SIZE as f32;
            ola[output_index] += sample * window[index];
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.carrier_history.fill(0.0);
        self.carrier_history_right.fill(0.0);
        self.external_history.fill(0.0);
        self.ola_left.fill(0.0);
        self.ola_right.fill(0.0);
        self.forward_input.fill(0.0);
        self.carrier_spectrum.fill(Complex::new(0.0, 0.0));
        self.carrier_spectrum_right.fill(Complex::new(0.0, 0.0));
        self.external_spectrum.fill(Complex::new(0.0, 0.0));
        self.inverse_spectrum.fill(Complex::new(0.0, 0.0));
        self.inverse_spectrum_right.fill(Complex::new(0.0, 0.0));
        self.inverse_output.fill(0.0);
        self.forward_scratch.fill(Complex::new(0.0, 0.0));
        self.inverse_scratch.fill(Complex::new(0.0, 0.0));
        self.external_input.reset();
        self.write_position = 0;
        self.frames_seen = 0;
        self.next_analysis_start = INITIAL_ANALYSIS_START;
    }
}

fn db_to_linear(value: f32) -> f32 {
    10.0_f32.powf(value / 20.0)
}

fn morph_spectrum_value(
    carrier: Complex<f32>,
    external: Complex<f32>,
    morph: f32,
    carrier_energy: f32,
    external_energy: f32,
) -> Complex<f32> {
    let carrier_magnitude = carrier.norm();
    let scale = (carrier_energy / external_energy).min(8.0);
    let normalized_external = external.norm() * scale;
    let magnitude = carrier_magnitude * (1.0 - morph) + normalized_external * morph;
    let phase = carrier.im.atan2(carrier.re);
    Complex::new(magnitude * phase.cos(), magnitude * phase.sin())
}

fn spectrum_energy(spectrum: &[Complex<f32>]) -> f32 {
    spectrum
        .iter()
        .map(Complex::norm_sqr)
        .sum::<f32>()
        .sqrt()
        .max(1.0e-9)
}

fn clear_real_fft_imaginary_bin(value: &mut Complex<f32>, index: usize, bin_count: usize) {
    if index == 0 || index + 1 == bin_count {
        value.im = 0.0;
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
