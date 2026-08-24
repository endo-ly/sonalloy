pub(crate) mod adsr;
pub(crate) mod fractional_delay;
pub(crate) mod generator;
mod instrument;
pub(crate) mod interpolation;
pub(crate) mod mix;
pub mod modulation;
pub(crate) mod processor;
mod random;
pub(crate) mod sample;
pub(crate) mod smoothing;
pub(crate) mod source;
mod voice;

pub use instrument::InstrumentRuntime;
pub use voice::VoiceState;

use sonalloy_dsp_sys::{DspOscillator, DspOscillatorWaveform};

use crate::process::{InstrumentProcessor, ProcessBlock, ProcessError, ProcessSpec, clear_output};

/// The sine runtime: one `DaisySP` oscillator rendered to stereo.
pub struct SineRuntime {
    frequency_hz: f32,
    oscillator: DspOscillator,
    spec: Option<ProcessSpec>,
    scratch: Vec<f32>,
    absolute_frame: u64,
}

impl SineRuntime {
    /// Create an unprepared sine runtime.
    ///
    /// # Errors
    ///
    /// Returns an error when the frequency is invalid or the native oscillator cannot be
    /// allocated.
    pub fn new(frequency_hz: f32) -> Result<Self, ProcessError> {
        if !frequency_hz.is_finite() || frequency_hz < 0.0 {
            return Err(ProcessError::InvalidFrequency);
        }
        Ok(Self {
            frequency_hz,
            oscillator: DspOscillator::new().map_err(ProcessError::from_dsp_error)?,
            spec: None,
            scratch: Vec::new(),
            absolute_frame: 0,
        })
    }

    /// Return the frequency used by this runtime.
    #[must_use]
    pub fn frequency_hz(&self) -> f32 {
        self.frequency_hz
    }

    /// Return the number of frames processed since preparation or reset.
    #[must_use]
    pub fn absolute_frame(&self) -> u64 {
        self.absolute_frame
    }
}

impl InstrumentProcessor for SineRuntime {
    fn prepare(&mut self, spec: ProcessSpec) -> Result<(), ProcessError> {
        self.spec = None;
        self.scratch.clear();
        self.absolute_frame = 0;
        spec.validate()?;
        if f64::from(self.frequency_hz) > spec.sample_rate * 0.5 {
            return Err(ProcessError::InvalidFrequency);
        }
        self.oscillator
            .prepare(spec.sample_rate, DspOscillatorWaveform::Sine)
            .map_err(ProcessError::from_dsp_error)?;
        self.oscillator
            .reset()
            .map_err(ProcessError::from_dsp_error)?;
        self.scratch.resize(spec.max_block_size, 0.0);
        self.spec = Some(spec);
        self.absolute_frame = 0;
        Ok(())
    }

    fn process(&mut self, block: ProcessBlock<'_>) -> Result<(), ProcessError> {
        clear_output(&mut *block.output, block.frames);
        let spec = self.spec.ok_or(ProcessError::NotPrepared)?;
        block.validate_for(spec)?;

        let next_frame = self
            .absolute_frame
            .checked_add(block.frames as u64)
            .ok_or(ProcessError::FrameOverflow)?;
        if block.context.absolute_frame != self.absolute_frame {
            return Err(ProcessError::ContextDiscontinuity {
                received: block.context.absolute_frame,
                expected: self.absolute_frame,
            });
        }
        if !block.events.is_empty() {
            return Err(ProcessError::EventsUnsupported);
        }

        if let Err(error) = self
            .oscillator
            .process(self.frequency_hz, &mut self.scratch[..block.frames])
            .map_err(ProcessError::from_dsp_error)
        {
            self.spec = None;
            return Err(error);
        }
        for channel in &mut *block.output {
            for (sample, output) in self.scratch[..block.frames]
                .iter()
                .zip((*channel).iter_mut())
            {
                *output = *sample;
            }
        }
        self.absolute_frame = next_frame;
        Ok(())
    }

    fn reset(&mut self) -> Result<(), ProcessError> {
        if self.spec.is_none() {
            return Err(ProcessError::NotPrepared);
        }
        if let Err(error) = self
            .oscillator
            .reset()
            .map_err(ProcessError::from_dsp_error)
        {
            self.spec = None;
            return Err(error);
        }
        self.scratch.fill(0.0);
        self.absolute_frame = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use crate::process::{ProcessContext, ProcessEvent, ProcessEventKind};

    use super::*;

    fn render_blocks(block_size: usize) -> Vec<Vec<f32>> {
        let spec = ProcessSpec::new(48_000.0, block_size, 2).expect("valid process spec");
        let mut runtime = SineRuntime::new(440.0).expect("valid sine runtime");
        runtime.prepare(spec).expect("runtime preparation");

        let mut channels = vec![vec![0.0_f32; 48_000], vec![0.0_f32; 48_000]];
        let mut offset = 0_usize;
        while offset < channels[0].len() {
            let frames = (channels[0].len() - offset).min(block_size);
            let end = offset + frames;
            let (left, right) = channels.split_at_mut(1);
            let mut output: [&mut [f32]; 2] =
                [&mut left[0][offset..end], &mut right[0][offset..end]];
            runtime
                .process(ProcessBlock {
                    frames,
                    context: ProcessContext {
                        absolute_frame: offset as u64,
                        tempo_bpm: 120.0,
                        beat_position: 0.0,
                        bar_position: 0.0,
                        time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                    },
                    events: &[],
                    output: &mut output,
                })
                .expect("runtime process");
            offset = end;
        }
        channels
    }

    #[test]
    fn runtime_handles_variable_blocks() {
        let reference = render_blocks(64);
        for block_size in [257, 1024] {
            let candidate = render_blocks(block_size);
            assert_eq!(candidate[0].len(), 48_000);
            assert_eq!(candidate[1].len(), 48_000);
            assert!(candidate.iter().flatten().all(|sample| sample.is_finite()));
            for (left, right) in reference[0].iter().zip(candidate[0].iter()) {
                assert_relative_eq!(*left, *right, epsilon = 1.0e-6);
            }
            for (left, right) in candidate[0].iter().zip(candidate[1].iter()) {
                assert_relative_eq!(*left, *right, epsilon = 1.0e-7);
            }
        }
    }

    #[test]
    fn reset_restores_the_initial_signal_and_frame() {
        let spec = ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec");
        let mut runtime = SineRuntime::new(440.0).expect("valid sine runtime");
        runtime.prepare(spec).expect("runtime preparation");
        let mut first_left = [0.0_f32; 128];
        let mut first_right = [0.0_f32; 128];
        let mut second_left = [0.0_f32; 128];
        let mut second_right = [0.0_f32; 128];
        let mut output: [&mut [f32]; 2] = [&mut first_left, &mut first_right];
        runtime
            .process(ProcessBlock {
                frames: 128,
                context: ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events: &[],
                output: &mut output,
            })
            .expect("first process");
        assert_eq!(runtime.absolute_frame(), 128);
        runtime.reset().expect("runtime reset");
        assert_eq!(runtime.absolute_frame(), 0);
        let mut reset_output: [&mut [f32]; 2] = [&mut second_left, &mut second_right];
        runtime
            .process(ProcessBlock {
                frames: 128,
                context: ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events: &[],
                output: &mut reset_output,
            })
            .expect("second process");
        for (first, second) in first_left.iter().zip(second_left.iter()) {
            assert_relative_eq!(*first, *second, epsilon = 1.0e-7);
        }
        for (first, second) in first_right.iter().zip(second_right.iter()) {
            assert_relative_eq!(*first, *second, epsilon = 1.0e-7);
        }
    }

    #[test]
    fn failed_prepare_makes_runtime_unprepared() {
        let valid_spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");
        let invalid_frequency_spec =
            ProcessSpec::new(600.0, 64, 2).expect("valid process spec with low sample rate");
        let mut runtime = SineRuntime::new(440.0).expect("valid sine runtime");
        runtime
            .prepare(valid_spec)
            .expect("initial runtime preparation");

        assert_eq!(
            runtime.prepare(invalid_frequency_spec),
            Err(ProcessError::InvalidFrequency)
        );
        {
            let mut left = [1.0_f32; 2];
            let mut right = [1.0_f32; 2];
            let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
            assert_eq!(
                runtime.process(ProcessBlock {
                    frames: 2,
                    context: ProcessContext {
                        absolute_frame: 0,
                        tempo_bpm: 120.0,
                        beat_position: 0.0,
                        bar_position: 0.0,
                        time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                    },
                    events: &[],
                    output: &mut output,
                }),
                Err(ProcessError::NotPrepared)
            );
            assert!(left.iter().all(|sample| sample.abs() < f32::EPSILON));
            assert!(right.iter().all(|sample| sample.abs() < f32::EPSILON));
        }

        runtime.prepare(valid_spec).expect("runtime re-preparation");
        let mut left = [0.0_f32; 2];
        let mut right = [0.0_f32; 2];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        assert!(
            runtime
                .process(ProcessBlock {
                    frames: 2,
                    context: ProcessContext {
                        absolute_frame: 0,
                        tempo_bpm: 120.0,
                        beat_position: 0.0,
                        bar_position: 0.0,
                        time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                    },
                    events: &[],
                    output: &mut output,
                })
                .is_ok()
        );
    }

    #[test]
    fn process_errors_clear_valid_output_ranges() {
        let spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");
        let mut runtime = SineRuntime::new(440.0).expect("valid sine runtime");
        runtime.prepare(spec).expect("runtime preparation");
        let mut left = vec![1.0_f32; 64];
        let mut right = vec![1.0_f32; 64];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        let error = runtime.process(ProcessBlock {
            frames: 64,
            context: ProcessContext {
                absolute_frame: 1,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
            },
            events: &[],
            output: &mut output,
        });
        assert!(matches!(
            error,
            Err(ProcessError::ContextDiscontinuity { .. })
        ));
        assert!(left.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(right.iter().all(|sample| sample.abs() < f32::EPSILON));
    }

    #[test]
    fn unsupported_events_clear_valid_output_ranges() {
        let spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");
        let mut runtime = SineRuntime::new(440.0).expect("valid sine runtime");
        runtime.prepare(spec).expect("runtime preparation");
        let event = ProcessEvent {
            sample_offset: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        };
        let mut left = vec![1.0_f32; 64];
        let mut right = vec![1.0_f32; 64];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        let error = runtime.process(ProcessBlock {
            frames: 64,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
            },
            events: &[event],
            output: &mut output,
        });
        assert_eq!(error, Err(ProcessError::EventsUnsupported));
        assert!(left.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(right.iter().all(|sample| sample.abs() < f32::EPSILON));
    }

    #[test]
    fn process_does_not_write_guard_frames() {
        let spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");
        let mut runtime = SineRuntime::new(440.0).expect("valid sine runtime");
        runtime.prepare(spec).expect("runtime preparation");
        let mut left = vec![0.0_f32; 66];
        let mut right = vec![0.0_f32; 66];
        left[64] = 123.0;
        left[65] = 124.0;
        right[64] = 125.0;
        right[65] = 126.0;
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime
            .process(ProcessBlock {
                frames: 64,
                context: ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events: &[],
                output: &mut output,
            })
            .expect("runtime process");
        assert_eq!(runtime.absolute_frame(), 64);
        assert!((left[64] - 123.0).abs() < f32::EPSILON);
        assert!((left[65] - 124.0).abs() < f32::EPSILON);
        assert!((right[64] - 125.0).abs() < f32::EPSILON);
        assert!((right[65] - 126.0).abs() < f32::EPSILON);
    }

    #[test]
    fn zero_frame_process_is_a_noop() {
        let spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");
        let mut runtime = SineRuntime::new(440.0).expect("valid sine runtime");
        runtime.prepare(spec).expect("runtime preparation");
        let mut left = [1.0_f32; 4];
        let mut right = [1.0_f32; 4];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime
            .process(ProcessBlock {
                frames: 0,
                context: ProcessContext {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
                },
                events: &[],
                output: &mut output,
            })
            .expect("zero-frame process");
        assert_eq!(runtime.absolute_frame(), 0);
        assert!(
            left.iter()
                .all(|sample| (*sample - 1.0).abs() < f32::EPSILON)
        );
        assert!(
            right
                .iter()
                .all(|sample| (*sample - 1.0).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn invalid_shapes_are_silenced_within_available_buffers() {
        let spec = ProcessSpec::new(48_000.0, 64, 2).expect("valid process spec");
        let mut runtime = SineRuntime::new(440.0).expect("valid sine runtime");
        runtime.prepare(spec).expect("runtime preparation");

        let mut left = vec![1.0_f32; 65];
        let mut right = vec![1.0_f32; 65];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        let error = runtime.process(ProcessBlock {
            frames: 65,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
            },
            events: &[],
            output: &mut output,
        });
        assert!(matches!(
            error,
            Err(ProcessError::FrameCountExceedsMaximum { .. })
        ));
        assert!(left.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(right.iter().all(|sample| sample.abs() < f32::EPSILON));

        let mut short_left = vec![1.0_f32; 4];
        let mut short_right = vec![1.0_f32; 4];
        let mut short_output: [&mut [f32]; 2] = [&mut short_left, &mut short_right];
        let error = runtime.process(ProcessBlock {
            frames: 8,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: crate::process::DEFAULT_TIME_SIGNATURE,
            },
            events: &[],
            output: &mut short_output,
        });
        assert!(matches!(
            error,
            Err(ProcessError::OutputBufferTooShort { .. })
        ));
        assert!(short_left.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(short_right.iter().all(|sample| sample.abs() < f32::EPSILON));
    }
}
