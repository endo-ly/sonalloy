use std::path::{Path, PathBuf};
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    AdsrDefinition, AssetReference, CompileContext, DiagnosticCode, GeneratorDefinition,
    GeneratorOutputMode, InstrumentDefinition, InstrumentProcessor, ParameterUnit, ProcessBlock,
    ProcessContext, ProcessEvent, ProcessEventKind, ProcessSpec, RenderRequest, ScheduledEvent,
    SpectralDefinition, compile_instrument, render_instrument,
};
use tempfile::TempDir;

fn fixture_directory() -> TempDir {
    tempfile::tempdir().expect("fixture directory creates")
}

fn write_pcm16_wav(path: &Path, samples: &[i16]) {
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

fn write_stereo_pcm16_wav(path: &Path, left: &[i16], right: &[i16]) {
    assert_eq!(left.len(), right.len());
    let payload_len = u32::try_from(left.len() * 4).expect("stereo fixture payload fits RIFF");
    let mut bytes = Vec::with_capacity(44 + left.len() * 4);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + payload_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&192_000_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    for (left, right) in left.iter().zip(right) {
        bytes.extend_from_slice(&left.to_le_bytes());
        bytes.extend_from_slice(&right.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("stereo fixture WAV writes");
}

fn source_samples(frame_count: usize) -> Vec<i16> {
    (0..frame_count)
        .map(|index| {
            #[allow(clippy::cast_precision_loss)]
            let time = index as f32 / 48_000.0;
            #[allow(clippy::cast_precision_loss)]
            let noise = (index * 73 % 997) as f32 / 997.0 * 2.0 - 1.0;
            let value = (std::f32::consts::TAU * 440.0 * time).sin() * 0.4
                + (std::f32::consts::TAU * 1_234.0 * time).sin() * 0.15
                + noise * 0.1;
            #[allow(clippy::cast_possible_truncation)]
            {
                (value * 30_000.0) as i16
            }
        })
        .collect()
}

fn tone_samples(frame_count: usize, frequency_hz: f32) -> Vec<i16> {
    (0..frame_count)
        .map(|index| {
            #[allow(clippy::cast_precision_loss)]
            let time = index as f32 / 48_000.0;
            #[allow(clippy::cast_possible_truncation)]
            {
                (f32::sin(std::f32::consts::TAU * frequency_hz * time) * 24_000.0) as i16
            }
        })
        .collect()
}

fn frequency_energy(samples: &[f32], sample_rate: f64, frequency_hz: f64) -> f64 {
    let mut real = 0.0_f64;
    let mut imaginary = 0.0_f64;
    for (index, sample) in samples.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let phase = std::f64::consts::TAU * frequency_hz * index as f64 / sample_rate;
        real += f64::from(*sample) * phase.cos();
        imaginary -= f64::from(*sample) * phase.sin();
    }
    real.mul_add(real, imaginary * imaginary)
}

fn definition(asset_path: String, fft_size: u16) -> InstrumentDefinition {
    let reference = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/basic-poly-synth.json");
    let mut definition: InstrumentDefinition = serde_json::from_str(
        &std::fs::read_to_string(reference).expect("reference Definition exists"),
    )
    .expect("reference Definition parses");
    definition.layers[0].gain_db = 0.0;
    definition.layers[0].envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.0,
        sustain_level: 1.0,
        release_seconds: 0.01,
    };
    definition.layers[0].generator = GeneratorDefinition::Spectral(SpectralDefinition {
        asset_a: AssetReference {
            path: asset_path,
            sha256: None,
        },
        asset_b: None,
        root_note: 60,
        fft_size,
        position: 0.0,
        freeze: 0.0,
        blur_seconds: 0.0,
        shift_hz: 0.0,
        morph: 0.0,
        phase_reset: true,
    });
    definition.voice_processors.clear();
    definition.global_processors.clear();
    definition.modulation = None;
    definition
}

fn compile(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
) -> Arc<sonalloy_core::CompiledInstrument> {
    compile_at_sample_rate(definition, base_dir, block_size, 48_000.0)
}

fn compile_at_sample_rate(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
    sample_rate: f64,
) -> Arc<sonalloy_core::CompiledInstrument> {
    let result = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec: ProcessSpec::new(sample_rate, block_size, 2).expect("valid process spec"),
        },
    );
    result.instrument.expect("Spectral Definition compiles")
}

fn render(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
    source_frames: usize,
) -> sonalloy_core::RenderedAudio {
    render_note(definition, base_dir, block_size, source_frames, 60)
}

fn render_note(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
    source_frames: usize,
    note_number: u8,
) -> sonalloy_core::RenderedAudio {
    render_note_at_sample_rate(
        definition,
        base_dir,
        block_size,
        source_frames,
        note_number,
        48_000.0,
    )
}

fn render_note_at_sample_rate(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
    source_frames: usize,
    note_number: u8,
    sample_rate: f64,
) -> sonalloy_core::RenderedAudio {
    let fft_size = match &definition.layers[0].generator {
        GeneratorDefinition::Spectral(spectral) => usize::from(spectral.fft_size),
        _ => panic!("fixture must use Spectral"),
    };
    let latency = fft_size - fft_size / 4;
    render_instrument(
        compile_at_sample_rate(definition, base_dir, block_size, sample_rate),
        RenderRequest {
            sample_rate,
            block_size,
            duration_frames: u64::try_from(source_frames + latency * 2)
                .expect("fixture frame count fits render request"),
            tail_frames: 0,
        },
        &[ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number,
                velocity: 110,
            },
        }],
    )
    .expect("Spectral render succeeds")
}

#[test]
fn spectral_definition_validates_and_compiles_the_prepared_contract() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    let samples = source_samples(4_096);
    write_pcm16_wav(&path, &samples);
    let definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    assert!(definition.validate().is_empty());

    let compiled = compile(&definition, directory.path(), 257);
    let sonalloy_core::compiler::CompiledGenerator::Spectral(spectral) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to Spectral");
    };
    let source = spectral.source.as_ref().expect("asset prepares");
    assert_eq!(
        compiled.layers[0].generator.output_mode(),
        GeneratorOutputMode::Mono
    );
    assert_eq!(spectral.fft_size, 1024);
    assert_eq!(spectral.hop_size, 256);
    assert_eq!(source.bin_count, 513);
    assert_eq!(source.latency_frames, 768);
    assert_eq!(spectral.latency_frames, 768);
    assert_eq!(compiled.reported_latency_frames, 768);
    assert_eq!(source.channels, 1);
    assert!(source.magnitudes.iter().all(|value| value.is_finite()));
    assert!(source.phases.iter().all(|value| value.is_finite()));
    assert!(
        source
            .instantaneous_frequencies_hz
            .iter()
            .all(|value| value.is_finite())
    );

    let position = compiled
        .parameter_handle("layer.body.generator.spectral_position")
        .expect("position parameter");
    let descriptor = compiled
        .parameter_descriptor(position)
        .expect("position descriptor");
    assert_eq!(descriptor.unit, ParameterUnit::Normalized);
    assert_relative_eq!(descriptor.min, 0.0);
    assert_relative_eq!(descriptor.max, 1.0);
    assert_relative_eq!(descriptor.smoothing_seconds, 0.010);
    assert!(
        compiled
            .parameter_handle("layer.body.generator.spectral_morph")
            .is_none()
    );
}

#[test]
fn spectral_definition_validates_root_note_fft_and_morph_constraints() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    write_pcm16_wav(&path, &source_samples(1_024));
    let mut definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.root_note = 128;
    spectral.fft_size = 512;
    spectral.morph = 0.5;
    let diagnostics = definition.validate();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[0].generator.spectral.root_note")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[0].generator.spectral.fft_size")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.path.as_deref() == Some("layers[0].generator.spectral.morph")
    }));
}

#[test]
fn spectral_asset_b_adds_the_morph_parameter() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    write_pcm16_wav(&path, &source_samples(1_024));
    let mut definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.asset_b = Some(spectral.asset_a.clone());
    spectral.morph = 0.5;
    assert!(definition.validate().is_empty());
    let compiled = compile(&definition, directory.path(), 257);
    let morph = compiled
        .parameter_handle("layer.body.generator.spectral_morph")
        .expect("morph parameter");
    let descriptor = compiled
        .parameter_descriptor(morph)
        .expect("morph descriptor");
    assert_eq!(descriptor.unit, ParameterUnit::Normalized);
    assert_relative_eq!(descriptor.min, 0.0);
    assert_relative_eq!(descriptor.max, 1.0);
}

#[test]
fn spectral_missing_asset_is_unavailable_without_contributing_latency() {
    let directory = fixture_directory();
    let definition = definition("missing.wav".to_owned(), 1024);
    let compiled = compile(&definition, directory.path(), 257);
    let sonalloy_core::compiler::CompiledGenerator::Spectral(spectral) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to Spectral");
    };
    assert!(spectral.source.is_none());
    assert_eq!(spectral.latency_frames, 0);
    assert_eq!(compiled.reported_latency_frames, 0);
}

#[test]
fn spectral_identity_resynthesis_matches_the_source_after_reported_latency() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    let samples = source_samples(8_192);
    write_pcm16_wav(&path, &samples);
    let definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    let audio = render(&definition, directory.path(), 257, samples.len());
    let latency = 768;
    let comparison_start = latency + 512;
    let comparison_end = latency + samples.len() - 512;
    let mut signal_power = 0.0_f64;
    let mut error_power = 0.0_f64;
    for (source_index, sample) in samples
        .iter()
        .enumerate()
        .skip(512)
        .take(samples.len() - 1_024)
    {
        let expected = f64::from(*sample) / 32_768.0;
        let rendered =
            f64::from(audio.channels[0][latency + source_index]) * f64::from(2.0_f32.sqrt());
        signal_power += expected * expected;
        error_power += (rendered - expected) * (rendered - expected);
    }
    let snr_db = 10.0 * (signal_power / error_power.max(1.0e-20)).log10();
    assert!(comparison_end > comparison_start);
    assert!(snr_db >= 60.0, "identity SNR is {snr_db:.2} dB");
}

#[test]
fn spectral_identity_is_independent_of_host_block_size() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    let samples = source_samples(4_096);
    write_pcm16_wav(&path, &samples);
    let definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    let renders = [32_usize, 64, 257, 1024]
        .into_iter()
        .map(|block_size| render(&definition, directory.path(), block_size, samples.len()))
        .collect::<Vec<_>>();
    for render in renders.iter().skip(1) {
        assert_eq!(renders[0].frames(), render.frames());
        let max_difference = renders[0]
            .channels
            .iter()
            .flatten()
            .zip(render.channels.iter().flatten())
            .map(|(left, right)| (left - right).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            max_difference <= 1.0e-6,
            "maximum difference is {max_difference}"
        );
    }
}

#[test]
fn spectral_latency_places_an_impulse_at_the_reported_offset() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    let mut samples = vec![0_i16; 4_096];
    samples[2_048] = 20_000;
    write_pcm16_wav(&path, &samples);
    let definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    let audio = render(&definition, directory.path(), 257, samples.len());
    let (index, value) = audio.channels[0]
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
        .expect("impulse render has samples");
    assert_eq!(index, 768 + 2_048);
    assert_relative_eq!(
        *value,
        20_000.0 / 32_768.0 / 2.0_f32.sqrt(),
        epsilon = 1.0e-4
    );
}

#[test]
fn spectral_latency_matches_the_fft_contract_for_all_sizes() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    let mut samples = vec![0_i16; 8_192];
    samples[2_048] = 20_000;
    write_pcm16_wav(&path, &samples);
    for (fft_size, latency) in [(1024_u16, 768_usize), (2048, 1_536), (4096, 3_072)] {
        let definition = definition(
            path.file_name().unwrap().to_string_lossy().into_owned(),
            fft_size,
        );
        let compiled = compile(&definition, directory.path(), 257);
        assert_eq!(compiled.reported_latency_frames, latency);
        let audio = render(&definition, directory.path(), 257, samples.len());
        let (index, _) = audio.channels[0]
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
            .expect("impulse render has samples");
        assert_eq!(index, latency + 2_048);
    }
}

#[test]
fn spectral_asset_cache_shares_prepared_audio_between_layers() {
    let directory = fixture_directory();
    let path = directory.path().join("fixture.wav");
    write_pcm16_wav(&path, &source_samples(4_096));
    let mut definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    let mut second_layer = definition.layers[0].clone();
    second_layer.id = "second".to_owned();
    definition.layers.push(second_layer);
    let compiled = compile(&definition, directory.path(), 257);
    let sonalloy_core::compiler::CompiledGenerator::Spectral(first) = &compiled.layers[0].generator
    else {
        panic!("first layer must compile to Spectral");
    };
    let sonalloy_core::compiler::CompiledGenerator::Spectral(second) =
        &compiled.layers[1].generator
    else {
        panic!("second layer must compile to Spectral");
    };
    assert!(Arc::ptr_eq(
        first.source.as_ref().expect("first asset prepares"),
        second.source.as_ref().expect("second asset prepares")
    ));
}

#[test]
fn spectral_preserves_stereo_source_channels() {
    let directory = fixture_directory();
    let path = directory.path().join("stereo.wav");
    let left = source_samples(4_096);
    let right = left
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            #[allow(clippy::cast_precision_loss)]
            let scale = 0.5 + index as f32 / 4_096.0 * 0.25;
            #[allow(clippy::cast_possible_truncation)]
            {
                (f32::from(*sample) * scale) as i16
            }
        })
        .collect::<Vec<_>>();
    write_stereo_pcm16_wav(&path, &left, &right);
    let definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    let compiled = compile(&definition, directory.path(), 257);
    let sonalloy_core::compiler::CompiledGenerator::Spectral(spectral) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to Spectral");
    };
    assert_eq!(
        spectral.source.as_ref().expect("asset prepares").channels,
        2
    );
    assert_eq!(
        compiled.layers[0].generator.output_mode(),
        GeneratorOutputMode::Stereo
    );
    let audio = render(&definition, directory.path(), 257, left.len());
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    let channel_difference = audio.channels[0]
        .iter()
        .zip(&audio.channels[1])
        .map(|(left, right)| (left - right).abs())
        .fold(0.0_f32, f32::max);
    assert!(channel_difference > 0.01);

    let latency = 768;
    let mut signal_power = 0.0_f64;
    let mut error_power = 0.0_f64;
    for source_index in 512..(left.len() - 512) {
        let expected_left = f64::from(left[source_index]) / 32_768.0;
        let rendered_left = f64::from(audio.channels[0][latency + source_index]);
        let expected_right = f64::from(right[source_index]) / 32_768.0;
        let rendered_right = f64::from(audio.channels[1][latency + source_index]);
        signal_power += expected_left * expected_left + expected_right * expected_right;
        error_power +=
            (rendered_left - expected_left).powi(2) + (rendered_right - expected_right).powi(2);
    }
    let snr_db = 10.0 * (signal_power / error_power.max(1.0e-20)).log10();
    assert!(snr_db >= 60.0, "stereo identity SNR is {snr_db:.2} dB");
}

#[test]
fn spectral_position_and_freeze_hold_the_selected_source_frame() {
    let directory = fixture_directory();
    let path = directory.path().join("position.wav");
    let samples = (0..32_768)
        .map(|index| {
            let frequency = if index < 16_384 { 440.0 } else { 880.0 };
            #[allow(clippy::cast_precision_loss)]
            let time = index as f32 / 48_000.0;
            #[allow(clippy::cast_possible_truncation)]
            {
                (f32::sin(std::f32::consts::TAU * frequency * time) * 24_000.0) as i16
            }
        })
        .collect::<Vec<_>>();
    write_pcm16_wav(&path, &samples);
    let mut definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.position = 0.75;
    spectral.freeze = 1.0;
    let audio = render(&definition, directory.path(), 257, samples.len());
    let window_start = 768 + 4_096;
    let window = &audio.channels[0][window_start..window_start + 8_192];
    let high_energy = frequency_energy(window, 48_000.0, 880.0);
    let low_energy = frequency_energy(window, 48_000.0, 440.0);
    assert!(
        high_energy > low_energy * 4.0,
        "position did not select the later segment: high={high_energy}, low={low_energy}"
    );
    assert!(high_energy > 1.0, "freeze output is silent");
}

#[test]
fn spectral_pitch_and_shift_remap_frequency_without_resampling_duration() {
    let directory = fixture_directory();
    let path = directory.path().join("tone.wav");
    let samples = tone_samples(32_768, 440.0);
    write_pcm16_wav(&path, &samples);

    let mut pitch_definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    let pitch_audio = render_note(&pitch_definition, directory.path(), 257, samples.len(), 72);
    let window_start = 768 + 4_096;
    let pitch_window = &pitch_audio.channels[0][window_start..window_start + 8_192];
    let octave_energy = frequency_energy(pitch_window, 48_000.0, 880.0);
    let source_energy = frequency_energy(pitch_window, 48_000.0, 440.0);
    assert!(
        octave_energy > source_energy * 4.0,
        "MIDI pitch was not remapped"
    );

    let GeneratorDefinition::Spectral(spectral) = &mut pitch_definition.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.shift_hz = 300.0;
    let shifted_audio = render(&pitch_definition, directory.path(), 257, samples.len());
    let shifted_window = &shifted_audio.channels[0][window_start..window_start + 8_192];
    let shifted_energy = frequency_energy(shifted_window, 48_000.0, 740.0);
    let unshifted_energy = frequency_energy(shifted_window, 48_000.0, 440.0);
    assert!(
        shifted_energy > unshifted_energy * 4.0,
        "frequency shift was not applied"
    );

    pitch_definition.layers[0].tuning_cents = 1_200.0;
    let tuned_audio = render(&pitch_definition, directory.path(), 257, samples.len());
    let tuned_window = &tuned_audio.channels[0][window_start..window_start + 8_192];
    let tuned_energy = frequency_energy(tuned_window, 48_000.0, 1_180.0);
    assert!(
        tuned_energy > 1.0,
        "layer tuning did not reach the shifted target"
    );
}

#[test]
fn spectral_transform_output_is_independent_of_host_block_size() {
    let directory = fixture_directory();
    let path = directory.path().join("block-size-tone.wav");
    let samples = tone_samples(16_384, 440.0);
    write_pcm16_wav(&path, &samples);
    let mut definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        2048,
    );
    let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.shift_hz = 300.0;
    let renders = [32_usize, 64, 257, 1024]
        .into_iter()
        .map(|block_size| render(&definition, directory.path(), block_size, samples.len()))
        .collect::<Vec<_>>();
    for render in renders.iter().skip(1) {
        let maximum_difference = renders[0]
            .channels
            .iter()
            .flatten()
            .zip(render.channels.iter().flatten())
            .map(|(left, right)| (left - right).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            maximum_difference <= 1.0e-6,
            "maximum transformed difference is {maximum_difference}"
        );
    }
}

#[test]
fn spectral_one_shot_drains_ola_and_finishes_after_source_end() {
    let directory = fixture_directory();
    let path = directory.path().join("short-tone.wav");
    let samples = tone_samples(4_096, 440.0);
    write_pcm16_wav(&path, &samples);
    let definition = definition(
        path.file_name().unwrap().to_string_lossy().into_owned(),
        1024,
    );
    let latency = 768;
    let duration_frames = samples.len() + latency * 2 + 2_048;
    let audio = render_instrument(
        compile(&definition, directory.path(), 257),
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: u64::try_from(duration_frames).expect("duration fits"),
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
    .expect("Spectral render succeeds");
    let tail_start = latency + samples.len() + 1_024;
    let tail_peak = audio.channels[0][tail_start..]
        .iter()
        .copied()
        .map(f32::abs)
        .fold(0.0_f32, f32::max);
    assert!(tail_peak < 1.0e-4, "OLA tail did not finish: {tail_peak}");
}

#[test]
fn spectral_asset_b_is_prepared_and_missing_asset_b_disables_the_layer() {
    let directory = fixture_directory();
    let path_a = directory.path().join("asset-a.wav");
    let path_b = directory.path().join("asset-b.wav");
    write_pcm16_wav(&path_a, &tone_samples(16_384, 440.0));
    write_pcm16_wav(&path_b, &tone_samples(16_384, 880.0));
    let mut definition = definition("asset-a.wav".to_owned(), 1024);
    let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.asset_b = Some(AssetReference {
        path: "asset-b.wav".to_owned(),
        sha256: None,
    });
    spectral.morph = 0.5;

    let compiled = compile(&definition, directory.path(), 257);
    let sonalloy_core::compiler::CompiledGenerator::Spectral(spectral) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to Spectral");
    };
    assert!(spectral.source.is_some());
    assert!(spectral.source_b.is_some());
    assert_eq!(spectral.source.as_ref().expect("asset A").channels, 1);
    assert_eq!(spectral.source_b.as_ref().expect("asset B").channels, 1);

    let mut unavailable = definition.clone();
    let GeneratorDefinition::Spectral(spectral) = &mut unavailable.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.asset_b = Some(AssetReference {
        path: "missing-b.wav".to_owned(),
        sha256: None,
    });
    let unavailable_compiled = compile(&unavailable, directory.path(), 257);
    let sonalloy_core::compiler::CompiledGenerator::Spectral(spectral) =
        &unavailable_compiled.layers[0].generator
    else {
        panic!("definition must compile to Spectral");
    };
    assert!(spectral.source.is_some());
    assert!(spectral.source_b.is_none());
    assert_eq!(spectral.latency_frames, 0);
    assert_eq!(unavailable_compiled.reported_latency_frames, 0);
    let audio = render(&unavailable, directory.path(), 257, 16_384);
    let peak = audio
        .channels
        .iter()
        .flatten()
        .copied()
        .map(f32::abs)
        .fold(0.0_f32, f32::max);
    assert!(
        peak <= 1.0e-8,
        "missing morph source was not disabled: {peak}"
    );
}

#[test]
fn spectral_mismatched_asset_b_channels_are_rejected() {
    let directory = fixture_directory();
    let path_a = directory.path().join("mono.wav");
    let path_b = directory.path().join("stereo.wav");
    let samples = tone_samples(8_192, 440.0);
    write_pcm16_wav(&path_a, &samples);
    write_stereo_pcm16_wav(&path_b, &samples, &tone_samples(8_192, 880.0));
    let mut definition = definition("mono.wav".to_owned(), 1024);
    let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.asset_b = Some(AssetReference {
        path: "stereo.wav".to_owned(),
        sha256: None,
    });
    spectral.morph = 0.5;
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: directory.path().to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, 257, 2).expect("valid process spec"),
        },
    );
    assert!(result.instrument.is_none());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::SpectralPreparationFailed
            && diagnostic.path.as_deref() == Some("layers[0].generator.spectral.asset_b.path")
    }));
}

#[test]
fn spectral_morph_endpoints_select_the_corresponding_source() {
    let directory = fixture_directory();
    let path_a = directory.path().join("morph-a.wav");
    let path_b = directory.path().join("morph-b.wav");
    let source_frames = 32_768;
    write_pcm16_wav(&path_a, &tone_samples(source_frames, 440.0));
    write_pcm16_wav(&path_b, &tone_samples(source_frames, 880.0));
    let mut definition = definition("morph-a.wav".to_owned(), 1024);
    {
        let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator else {
            panic!("fixture must use Spectral");
        };
        spectral.asset_b = Some(AssetReference {
            path: "morph-b.wav".to_owned(),
            sha256: None,
        });
        spectral.morph = 0.0;
    }
    let source_audio = render(&definition, directory.path(), 257, source_frames);
    if let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator {
        spectral.morph = 1.0;
    } else {
        panic!("fixture must use Spectral");
    }
    let morph_audio = render(&definition, directory.path(), 257, source_frames);
    let window_start = 768 + 4_096;
    let source_window = &source_audio.channels[0][window_start..window_start + 8_192];
    let morph_window = &morph_audio.channels[0][window_start..window_start + 8_192];
    assert!(
        frequency_energy(source_window, 48_000.0, 440.0)
            > frequency_energy(source_window, 48_000.0, 880.0) * 4.0
    );
    assert!(
        frequency_energy(morph_window, 48_000.0, 880.0)
            > frequency_energy(morph_window, 48_000.0, 440.0) * 4.0
    );
}

#[test]
fn spectral_morph_of_identical_sources_is_identity_resynthesis() {
    let directory = fixture_directory();
    let path = directory.path().join("identical.wav");
    let samples = source_samples(16_384);
    write_pcm16_wav(&path, &samples);
    let mut definition = definition("identical.wav".to_owned(), 1024);
    let baseline = render(&definition, directory.path(), 257, samples.len());
    let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.asset_b = Some(AssetReference {
        path: "identical.wav".to_owned(),
        sha256: None,
    });
    spectral.morph = 0.5;
    let morphed = render(&definition, directory.path(), 257, samples.len());
    let maximum_difference = baseline
        .channels
        .iter()
        .flatten()
        .zip(morphed.channels.iter().flatten())
        .map(|(left, right)| (left - right).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        maximum_difference <= 1.0e-6,
        "identical-source morph changed the signal: {maximum_difference}"
    );
}

#[test]
fn spectral_morph_sweep_changes_the_active_source() {
    let directory = fixture_directory();
    let path_a = directory.path().join("sweep-a.wav");
    let path_b = directory.path().join("sweep-b.wav");
    let source_frames = 65_536;
    write_pcm16_wav(&path_a, &tone_samples(source_frames, 440.0));
    write_pcm16_wav(&path_b, &tone_samples(source_frames, 880.0));
    let mut definition = definition("sweep-a.wav".to_owned(), 1024);
    let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.asset_b = Some(AssetReference {
        path: "sweep-b.wav".to_owned(),
        sha256: None,
    });
    let compiled = compile(&definition, directory.path(), 257);
    let morph_handle = compiled
        .parameter_handle("layer.body.generator.spectral_morph")
        .expect("morph parameter");
    let audio = render_instrument(
        compiled,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: u64::try_from(source_frames + 2_048).expect("render frame count fits"),
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
                absolute_frame: 32_768,
                kind: ProcessEventKind::ParameterChange {
                    parameter: morph_handle,
                    normalized: 1.0,
                },
            },
        ],
    )
    .expect("Spectral morph sweep renders");
    let before = &audio.channels[0][768 + 4_096..768 + 12_288];
    let after = &audio.channels[0][768 + 40_000..768 + 48_192];
    assert!(frequency_energy(before, 48_000.0, 440.0) > 1.0);
    assert!(frequency_energy(after, 48_000.0, 880.0) > 1.0);
    assert!(
        frequency_energy(after, 48_000.0, 880.0) > frequency_energy(after, 48_000.0, 440.0) * 2.0
    );
}

#[test]
fn spectral_stereo_morph_keeps_left_and_right_independent() {
    let directory = fixture_directory();
    let path_a = directory.path().join("stereo-a.wav");
    let path_b = directory.path().join("stereo-b.wav");
    let source_frames = 16_384;
    write_stereo_pcm16_wav(
        &path_a,
        &tone_samples(source_frames, 440.0),
        &tone_samples(source_frames, 550.0),
    );
    write_stereo_pcm16_wav(
        &path_b,
        &tone_samples(source_frames, 880.0),
        &tone_samples(source_frames, 660.0),
    );
    let mut definition = definition("stereo-a.wav".to_owned(), 1024);
    let GeneratorDefinition::Spectral(spectral) = &mut definition.layers[0].generator else {
        panic!("fixture must use Spectral");
    };
    spectral.asset_b = Some(AssetReference {
        path: "stereo-b.wav".to_owned(),
        sha256: None,
    });
    spectral.morph = 0.5;
    let compiled = compile(&definition, directory.path(), 257);
    let sonalloy_core::compiler::CompiledGenerator::Spectral(spectral) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to Spectral");
    };
    assert_eq!(spectral.output_mode(), GeneratorOutputMode::Stereo);
    assert_eq!(spectral.source_b.as_ref().expect("asset B").channels, 2);
    let audio = render(&definition, directory.path(), 257, source_frames);
    assert!(
        audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    let channel_difference = audio.channels[0]
        .iter()
        .zip(&audio.channels[1])
        .map(|(left, right)| (left - right).abs())
        .fold(0.0_f32, f32::max);
    assert!(channel_difference > 0.01);
}

#[test]
fn spectral_resynthesis_remains_finite_at_supported_sample_rates() {
    let directory = fixture_directory();
    let path = directory.path().join("sample-rates.wav");
    let samples = tone_samples(24_000, 440.0);
    write_pcm16_wav(&path, &samples);
    let definition = definition("sample-rates.wav".to_owned(), 1024);
    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        let audio = render_note_at_sample_rate(
            &definition,
            directory.path(),
            257,
            samples.len(),
            60,
            sample_rate,
        );
        let window_start = 768 + 4_096;
        let window = &audio.channels[0][window_start..window_start + 8_192];
        assert!(window.iter().all(|sample| sample.is_finite()));
        assert!(frequency_energy(window, sample_rate, 440.0) > 1.0);
    }
}

fn example_definition(name: &str) -> (InstrumentDefinition, PathBuf) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments")
        .join(name);
    let definition =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("example Definition exists"))
            .expect("example Definition parses");
    (definition, path)
}

fn assert_hybrid_structure(
    compiled: &sonalloy_core::compiler::CompiledInstrument,
    definition: &InstrumentDefinition,
) {
    assert_eq!(compiled.layers.len(), 4);
    assert!(matches!(
        &compiled.layers[0].generator,
        sonalloy_core::compiler::CompiledGenerator::Spectral(_)
    ));
    assert!(matches!(
        &compiled.layers[1].generator,
        sonalloy_core::compiler::CompiledGenerator::Additive(_)
    ));
    assert!(matches!(
        &compiled.layers[2].generator,
        sonalloy_core::compiler::CompiledGenerator::Sample(_)
    ));
    assert!(matches!(
        &compiled.layers[3].generator,
        sonalloy_core::compiler::CompiledGenerator::Noise(_)
    ));
    assert_eq!(compiled.layers[0].processors.len(), 1);
    assert_eq!(compiled.layers[1].processors.len(), 1);
    assert_eq!(compiled.layers[2].processors.len(), 1);
    assert_eq!(compiled.voice_processors.len(), 2);
    assert_eq!(compiled.global_processors.len(), 2);

    let routes = definition
        .modulation
        .as_ref()
        .expect("hybrid modulation exists")
        .routes
        .iter()
        .map(|route| route.target.as_str())
        .collect::<Vec<_>>();
    for target in [
        "layer.spectral.generator.spectral_position",
        "layer.spectral.generator.spectral_blur",
        "layer.spectral.generator.spectral_shift",
        "layer.spectral.generator.spectral_morph",
        "layer.spectral.processor.spectral_tone.cutoff",
        "voice.processor.voice_tone.cutoff",
        "global.processor.echo.mix",
        "global.processor.space.mix",
    ] {
        assert!(
            routes.contains(&target),
            "missing modulation route {target}"
        );
        assert!(
            compiled.parameter_handle(target).is_some(),
            "missing parameter {target}"
        );
    }
}

fn assert_stereo_finite_and_non_silent(audio: &sonalloy_core::RenderedAudio) {
    assert_eq!(audio.channels.len(), 2);
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
            .any(|sample| sample.abs() > 1.0e-4)
    );
    let stereo_difference = audio.channels[0]
        .iter()
        .zip(&audio.channels[1])
        .map(|(left, right)| (left - right).abs())
        .fold(0.0_f32, f32::max);
    assert!(stereo_difference > 1.0e-4);
}

fn render_hybrid_controls(
    compiled: Arc<sonalloy_core::compiler::CompiledInstrument>,
) -> sonalloy_core::RenderedAudio {
    let morph = compiled
        .parameter_handle("layer.spectral.generator.spectral_morph")
        .expect("morph parameter");
    render_instrument(
        compiled,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 16_384,
            tail_frames: 0,
        },
        &[
            ScheduledEvent {
                absolute_frame: 0,
                kind: ProcessEventKind::NoteOn {
                    note_id: 1,
                    note_number: 60,
                    velocity: 112,
                },
            },
            ScheduledEvent {
                absolute_frame: 4_096,
                kind: ProcessEventKind::ParameterChange {
                    parameter: morph,
                    normalized: 0.85,
                },
            },
            ScheduledEvent {
                absolute_frame: 6_144,
                kind: ProcessEventKind::ModWheel { value: 0.7 },
            },
        ],
    )
    .expect("hybrid render succeeds")
}

fn hybrid_polyphony_events() -> Vec<ScheduledEvent> {
    (0..16)
        .map(|index| ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: index + 1,
                note_number: 48 + u8::try_from(index).expect("voice note fits MIDI"),
                velocity: 96 + u8::try_from(index % 24).expect("velocity fits MIDI"),
            },
        })
        .collect()
}

fn voice_stealing_events() -> [ScheduledEvent; 3] {
    [
        ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 48,
                velocity: 112,
            },
        },
        ScheduledEvent {
            absolute_frame: 64,
            kind: ProcessEventKind::NoteOn {
                note_id: 2,
                note_number: 60,
                velocity: 112,
            },
        },
        ScheduledEvent {
            absolute_frame: 8_192,
            kind: ProcessEventKind::NoteOff { note_id: 2 },
        },
    ]
}

fn render_runtime_note(
    runtime: &mut sonalloy_core::InstrumentRuntime,
    event: ProcessEvent,
) -> Vec<Vec<f32>> {
    let mut left = vec![0.0_f32; 4_096];
    let mut right = vec![0.0_f32; 4_096];
    let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
    runtime
        .process(ProcessBlock {
            frames: 4_096,
            context: ProcessContext {
                absolute_frame: 0,
                tempo_bpm: 120.0,
            },
            events: std::slice::from_ref(&event),
            output: &mut output,
        })
        .expect("runtime block renders");
    vec![left, right]
}

#[test]
fn spectral_reference_definition_exposes_stereo_ab_and_transform_controls() {
    let (definition, path) = example_definition("spectral-generator-reference.json");
    assert!(definition.validate().is_empty());
    let compiled = compile(
        &definition,
        path.parent().expect("instrument directory"),
        257,
    );
    assert_eq!(compiled.performance.polyphony, 16);
    assert_eq!(compiled.layers.len(), 1);
    assert_eq!(
        compiled.layers[0].generator.output_mode(),
        GeneratorOutputMode::Stereo
    );
    let sonalloy_core::compiler::CompiledGenerator::Spectral(spectral) =
        &compiled.layers[0].generator
    else {
        panic!("reference layer must compile to Spectral");
    };
    assert_eq!(spectral.fft_size, 2048);
    assert_eq!(spectral.hop_size, 512);
    assert_eq!(
        spectral.source.as_ref().expect("asset A prepares").channels,
        2
    );
    assert_eq!(
        spectral
            .source_b
            .as_ref()
            .expect("asset B prepares")
            .channels,
        2
    );
    assert!(spectral.parameters.morph.is_some());
    for parameter in [
        "layer.spectral.generator.spectral_position",
        "layer.spectral.generator.spectral_freeze",
        "layer.spectral.generator.spectral_blur",
        "layer.spectral.generator.spectral_shift",
        "layer.spectral.generator.spectral_morph",
    ] {
        assert!(
            compiled.parameter_handle(parameter).is_some(),
            "missing {parameter}"
        );
    }
}

#[test]
fn spectral_hybrid_uses_existing_layers_processors_modulation_and_midi_ready_parameters() {
    let (definition, path) = example_definition("spectral-hybrid-reference.json");
    assert!(definition.validate().is_empty());
    let base_dir = path.parent().expect("instrument directory");
    let compiled = compile(&definition, base_dir, 257);
    assert_hybrid_structure(&compiled, &definition);
    let audio = render_hybrid_controls(compiled);
    assert_stereo_finite_and_non_silent(&audio);
}

#[test]
fn spectral_hybrid_supports_sixteen_voices_voice_stealing_and_reset_determinism() {
    let (definition, path) = example_definition("spectral-hybrid-reference.json");
    let base_dir = path.parent().expect("instrument directory");
    let compiled = compile(&definition, base_dir, 257);
    let audio = render_instrument(
        compiled.clone(),
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 16_384,
            tail_frames: 0,
        },
        &hybrid_polyphony_events(),
    )
    .expect("sixteen-voice render succeeds");
    assert_stereo_finite_and_non_silent(&audio);

    let mut stealing_definition = definition.clone();
    stealing_definition.performance.polyphony = 1;
    let stealing_compiled = compile(&stealing_definition, base_dir, 257);
    let stolen_audio = render_instrument(
        stealing_compiled,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 16_384,
            tail_frames: 0,
        },
        &voice_stealing_events(),
    )
    .expect("voice stealing render succeeds");
    assert_stereo_finite_and_non_silent(&stolen_audio);

    let mut runtime = compiled.instantiate();
    let spec = ProcessSpec::new(48_000.0, 4_096, 2).expect("valid process spec");
    runtime.prepare(spec).expect("runtime prepares");
    let event = ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 112,
        },
    };
    let first = render_runtime_note(&mut runtime, event);
    runtime.reset().expect("runtime resets");
    let second = render_runtime_note(&mut runtime, event);
    assert_eq!(first, second);
}

#[test]
fn spectral_layer_latency_aligns_an_existing_generator_layer() {
    let directory = fixture_directory();
    let path = directory.path().join("silence.wav");
    write_pcm16_wav(&path, &vec![0; 8_192]);
    let mut definition = definition("silence.wav".to_owned(), 2048);
    let reference = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/instruments/basic-poly-synth.json");
    let basic: InstrumentDefinition = serde_json::from_str(
        &std::fs::read_to_string(reference).expect("oscillator reference exists"),
    )
    .expect("oscillator reference parses");
    let mut oscillator = basic.layers[0].clone();
    oscillator.id = "carrier".to_owned();
    oscillator.gain_db = 0.0;
    oscillator.envelope = AdsrDefinition {
        attack_seconds: 0.0,
        decay_seconds: 0.0,
        sustain_level: 1.0,
        release_seconds: 0.01,
    };
    definition.layers.push(oscillator);
    let compiled = compile(&definition, directory.path(), 257);
    assert_eq!(compiled.reported_latency_frames, 1_536);
    let audio = render(&definition, directory.path(), 257, 8_192);
    let first_signal = audio.channels[0]
        .iter()
        .position(|sample| sample.abs() > 1.0e-5)
        .expect("carrier produces output");
    assert!(first_signal >= 1_536);
}
