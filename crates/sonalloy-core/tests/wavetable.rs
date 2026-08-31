use std::path::{Path, PathBuf};
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    AdsrDefinition, AssetReference, CompileContext, DiagnosticCode, GeneratorDefinition,
    InstrumentDefinition, InstrumentProcessor, ProcessBlock, ProcessContext, ProcessEventKind,
    ProcessSpec, RenderRequest, ScheduledEvent, UnisonDefinition, WavetableDefinition,
    compile_instrument, render_instrument,
};
use tempfile::TempDir;

fn fixture_directory() -> TempDir {
    tempfile::tempdir().expect("fixture directory creates")
}

fn write_pcm16_wav(path: &PathBuf, samples: &[i16]) {
    let payload_len = u32::try_from(samples.len() * 2).expect("fixture payload fits RIFF");
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + payload_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&96_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("fixture WAV writes");
}

fn table_samples() -> Vec<i16> {
    (0..3)
        .flat_map(|frame| {
            (0..64).map(move |index| {
                let index = u16::try_from(index).expect("table index fits");
                let phase = f32::from(index) / 64.0;
                let value = match frame {
                    0 => (std::f32::consts::TAU * phase).sin(),
                    1 => {
                        (std::f32::consts::TAU * phase).sin()
                            + 0.3 * (std::f32::consts::TAU * phase * 3.0).sin()
                    }
                    _ => 2.0 * (2.0 * phase - (2.0 * phase + 0.5).floor()).abs() - 1.0,
                };
                #[allow(clippy::cast_possible_truncation)]
                {
                    (value.clamp(-1.0, 1.0) * 30_000.0) as i16
                }
            })
        })
        .collect()
}

fn warning_samples() -> Vec<i16> {
    let silent = std::iter::repeat_n(0_i16, 64);
    let offset = (0..64).map(|index| {
        let phase = f32::from(u16::try_from(index).expect("table index fits")) / 64.0;
        let value = (std::f32::consts::TAU * phase).sin() * 0.4 + 0.02;
        #[allow(clippy::cast_possible_truncation)]
        {
            (value * 30_000.0) as i16
        }
    });
    silent.chain(offset).collect()
}

fn definition(asset_path: String, unison: Option<UnisonDefinition>) -> InstrumentDefinition {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/basic-poly-synth.json");
    let mut definition: InstrumentDefinition =
        serde_json::from_str(&std::fs::read_to_string(path).expect("reference Definition exists"))
            .expect("reference Definition parses");
    definition.layers[0].gain_db = 0.0;
    definition.layers[0].envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.0,
        sustain_level: 1.0,
        release_seconds: 0.01,
    };
    definition.layers[0].generator = GeneratorDefinition::Wavetable(WavetableDefinition {
        asset: AssetReference {
            path: asset_path,
            sha256: None,
        },
        frame_length: 64,
        position: 0.0,
        phase_reset: true,
        phase: 0.0,
        unison,
    });
    definition.modulation = None;
    definition
}

fn compile(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
) -> Arc<sonalloy_core::CompiledInstrument> {
    let result = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, block_size, 0, 2).expect("valid process spec"),
        },
    );
    result.instrument.expect("Wavetable compiles")
}

fn render(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
) -> sonalloy_core::RenderedAudio {
    let compiled = compile(definition, base_dir, block_size);
    render_instrument(
        compiled,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size,
            duration_frames: 2_048,
            tail_frames: 0,
        },
        &[
            ScheduledEvent {
                absolute_frame: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 110,
                },
            },
            ScheduledEvent {
                absolute_frame: 1_024,
                kind: ProcessEventKind::NoteOff { note_id: 1 },
            },
        ],
    )
    .expect("Wavetable renders")
}

#[test]
fn wavetable_compiles_band_tables_and_renders_at_the_correct_pitch() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    write_pcm16_wav(&path, &table_samples());
    let base_dir = directory.path();
    let definition = definition(path.file_name().unwrap().to_str().unwrap().to_owned(), None);
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec"),
        },
    );
    let compiled = result.instrument.expect("Wavetable compiles");
    let sonalloy_core::compiler::CompiledGenerator::Wavetable(wavetable) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to a Wavetable");
    };
    let prepared = wavetable.prepared.as_ref().expect("asset prepares");
    assert_eq!(prepared.frame_length, 64);
    assert_eq!(prepared.frame_count, 3);
    assert_eq!(prepared.bands.len(), 6);
    assert_eq!(prepared.source_metadata.source_sample_rate, 48_000);
    let first_frame = &prepared.bands[0].frames[0].guarded_samples;
    assert_relative_eq!(first_frame[0], first_frame[64], epsilon = 1.0e-6);
    assert_relative_eq!(first_frame[65], first_frame[1], epsilon = 1.0e-6);
    assert_relative_eq!(first_frame[66], first_frame[2], epsilon = 1.0e-6);
    let wide_frame = &prepared.bands[0].frames[1].guarded_samples;
    let narrow_band = prepared
        .bands
        .iter()
        .find(|band| band.max_harmonic == 1)
        .expect("sine band");
    let narrow_frame = &narrow_band.frames[1].guarded_samples;
    assert!(harmonic_amplitude(wide_frame, 64, 3) > 0.2);
    assert!(harmonic_amplitude(narrow_frame, 64, 3) < 1.0e-4);
    assert_eq!(
        compiled
            .parameter_handle("layer.body.generator.wavetable_position")
            .expect("position parameter"),
        wavetable.parameters.position
    );
    let position_descriptor = compiled
        .parameter_descriptor(wavetable.parameters.position)
        .expect("position descriptor");
    assert_relative_eq!(position_descriptor.min, 0.0);
    assert_relative_eq!(position_descriptor.max, 1.0);
    assert_relative_eq!(position_descriptor.smoothing_seconds, 0.010);
    let audio = render(&definition, base_dir, 257);
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .any(|sample| sample.abs() > 0.01)
    );
    let pitch_audio = render_instrument(
        compiled,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 24_576,
            tail_frames: 0,
        },
        &[ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 60,
                velocity: 110,
            },
        }],
    )
    .expect("static Wavetable pitch renders");
    let segment = &pitch_audio.channels[0][2_048..22_048];
    let crossings = segment
        .windows(2)
        .filter(|samples| samples[0] <= 0.0 && samples[1] > 0.0)
        .count();
    let crossing_count = u16::try_from(crossings).expect("crossing count fits");
    let segment_frames = u16::try_from(segment.len()).expect("segment length fits");
    let estimated_frequency = f32::from(crossing_count) * 48_000.0 / f32::from(segment_frames);
    assert!((estimated_frequency - 261.6256).abs() < 2.0);
}

fn harmonic_amplitude(guarded_samples: &[f32], frame_length: usize, harmonic: usize) -> f32 {
    let samples = &guarded_samples[1..=frame_length];
    #[allow(clippy::cast_precision_loss)]
    let frame_length_f32 = frame_length as f32;
    let (real, imaginary) =
        samples
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(real, imaginary), (index, sample)| {
                #[allow(clippy::cast_precision_loss)]
                let angle =
                    std::f32::consts::TAU * harmonic as f32 * index as f32 / frame_length_f32;
                (
                    real + sample * angle.cos(),
                    imaginary - sample * angle.sin(),
                )
            });
    2.0 * (real.mul_add(real, imaginary * imaginary)).sqrt() / frame_length_f32
}

#[test]
fn wavetable_position_and_unison_change_the_compiled_output_mode_and_signal() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    write_pcm16_wav(&path, &table_samples());
    let base_dir = directory.path();
    let file_name = path.file_name().unwrap().to_str().unwrap().to_owned();
    let first = definition(file_name.clone(), None);
    let mut last = definition(file_name.clone(), None);
    if let GeneratorDefinition::Wavetable(wavetable) = &mut last.layers[0].generator {
        wavetable.position = 1.0;
    }
    let first_audio = render(&first, base_dir, 32);
    let last_audio = render(&last, base_dir, 32);
    assert!(
        first_audio.channels[0]
            .iter()
            .zip(&last_audio.channels[0])
            .any(|(left, right)| (left - right).abs() > 0.01)
    );

    let stereo = definition(
        file_name.clone(),
        Some(UnisonDefinition {
            voices: 5,
            detune_cents: 14.0,
            stereo_spread: 0.8,
            phase_spread: 0.5,
        }),
    );
    let compiled = compile(&stereo, base_dir, 257);
    assert_eq!(
        compiled.layers[0].generator.output_mode(),
        sonalloy_core::compiler::GeneratorOutputMode::Stereo
    );
    let sonalloy_core::compiler::CompiledGenerator::Wavetable(wavetable) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to a Wavetable");
    };
    assert_relative_eq!(
        compiled
            .parameter_descriptor(wavetable.parameters.unison_detune.expect("detune handle"))
            .expect("detune descriptor")
            .smoothing_seconds,
        0.010
    );
    let audio = render_instrument(
        compiled,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 2_048,
            tail_frames: 0,
        },
        &[ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 48,
                velocity: 110,
            },
        }],
    )
    .expect("stereo Wavetable renders");
    assert!(
        audio.channels[0]
            .iter()
            .zip(&audio.channels[1])
            .any(|(left, right)| (left - right).abs() > 0.001)
    );
    let eight = definition(
        file_name,
        Some(UnisonDefinition {
            voices: 8,
            detune_cents: 18.0,
            stereo_spread: 0.9,
            phase_spread: 0.25,
        }),
    );
    let eight_audio = render(&eight, base_dir, 257);
    assert!(
        eight_audio.channels[0]
            .iter()
            .any(|sample| sample.abs() > 0.01)
    );
}

#[test]
fn wavetable_frame_warnings_keep_audible_assets_available() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    write_pcm16_wav(&path, &warning_samples());
    let base_dir = directory.path();
    let definition = definition(path.file_name().unwrap().to_str().unwrap().to_owned(), None);
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec"),
        },
    );
    assert!(result.instrument.is_some());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::WavetableSilentFrame)
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::WavetableDcOffset)
    );
}

#[test]
fn wavetable_preparation_is_shared_for_identical_asset_references() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    write_pcm16_wav(&path, &table_samples());
    let base_dir = directory.path();
    let file_name = path.file_name().unwrap().to_str().unwrap().to_owned();
    let mut definition = definition(file_name, None);
    let mut second = definition.layers[0].clone();
    second.id = "second".to_owned();
    definition.layers.push(second);
    let compiled = compile(&definition, base_dir, 257);
    let sonalloy_core::compiler::CompiledGenerator::Wavetable(first) =
        &compiled.layers[0].generator
    else {
        panic!("first layer must be a Wavetable");
    };
    let sonalloy_core::compiler::CompiledGenerator::Wavetable(second) =
        &compiled.layers[1].generator
    else {
        panic!("second layer must be a Wavetable");
    };
    assert!(Arc::ptr_eq(
        first.prepared.as_ref().expect("first prepares"),
        second.prepared.as_ref().expect("second prepares")
    ));
}

#[test]
fn wavetable_layout_errors_are_compile_errors() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    write_pcm16_wav(&path, &[0_i16; 63]);
    let base_dir = directory.path();
    let definition = definition(path.file_name().unwrap().to_str().unwrap().to_owned(), None);
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec"),
        },
    );
    assert!(result.instrument.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::WavetableLayoutInvalid)
    );
}

#[test]
fn wavetable_output_is_stable_across_block_sizes_and_reset() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    write_pcm16_wav(&path, &table_samples());
    let base_dir = directory.path();
    let definition = definition(path.file_name().unwrap().to_str().unwrap().to_owned(), None);
    let first = render(&definition, base_dir, 32);
    let second = render(&definition, base_dir, 257);
    for (left, right) in first.channels[0].iter().zip(&second.channels[0]) {
        assert_relative_eq!(*left, *right, epsilon = 1.0e-6);
    }
    let compiled = compile(&definition, base_dir, 257);
    let mut runtime = compiled.instantiate();
    let spec = ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec");
    runtime.prepare(spec).expect("runtime prepares");
    runtime.activate().expect("runtime activates");
    let mut first_left = vec![0.0; 256];
    let mut first_right = vec![0.0; 256];
    let mut output: [&mut [f32]; 2] = [&mut first_left, &mut first_right];
    runtime
        .process(ProcessBlock {
            frames: 256,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                transport_state: sonalloy_core::TransportState::Playing,
            },
            events: &[sonalloy_core::ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 110,
                },
            }],
            input: &[],
            output: &mut output,
        })
        .expect("first block renders");
    runtime.reset().expect("runtime resets");
    let mut reset_left = vec![0.0; 256];
    let mut reset_right = vec![0.0; 256];
    let mut reset_output: [&mut [f32]; 2] = [&mut reset_left, &mut reset_right];
    runtime
        .process(ProcessBlock {
            frames: 256,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
                beat_position: 0.0,
                bar_position: 0.0,
                time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                transport_state: sonalloy_core::TransportState::Playing,
            },
            events: &[sonalloy_core::ProcessEvent {
                sample_offset: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 110,
                },
            }],
            input: &[],
            output: &mut reset_output,
        })
        .expect("reset block renders");
    for (left, right) in first_left.iter().zip(&reset_left) {
        assert_relative_eq!(*left, *right, epsilon = 1.0e-6);
    }
}

#[test]
fn missing_wavetable_asset_leaves_other_layers_available() {
    let directory = fixture_directory();
    let base_dir = directory.path();
    let missing = "missing.wav".to_owned();
    let mut definition = definition(missing, None);
    let mut fallback = definition.layers[0].clone();
    fallback.id = "fallback".to_owned();
    fallback.generator =
        sonalloy_core::GeneratorDefinition::Oscillator(sonalloy_core::OscillatorDefinition {
            waveform: sonalloy_core::OscillatorWaveform::Sine,
            phase_reset: true,
            phase: 0.0,
            hard_sync: None,
            waveshaping: None,
            phase_distortion: None,
            wavefold: None,
            feedback: None,
            unison: None,
        });
    definition.layers.push(fallback);
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec"),
        },
    );
    let compiled = result
        .instrument
        .expect("missing asset remains recoverable");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == DiagnosticCode::AssetNotFound })
    );
    let audio = render_instrument(
        compiled,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 512,
            tail_frames: 0,
        },
        &[ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 110,
            },
        }],
    )
    .expect("fallback layer renders");
    assert!(audio.channels[0].iter().any(|sample| sample.abs() > 0.01));
}
