use std::path::{Path, PathBuf};
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    AdsrDefinition, AssetReference, CompileContext, GeneratorDefinition, GeneratorOutputMode,
    InstrumentDefinition, ParameterUnit, ProcessEventKind, ProcessSpec, RenderRequest,
    ScheduledEvent, SpectralDefinition, compile_instrument, render_instrument,
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
    let result = compile_instrument(
        definition,
        &CompileContext {
            definition_base_dir: base_dir.to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, block_size, 2).expect("valid process spec"),
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
    let fft_size = match &definition.layers[0].generator {
        GeneratorDefinition::Spectral(spectral) => usize::from(spectral.fft_size),
        _ => panic!("fixture must use Spectral"),
    };
    let latency = fft_size - fft_size / 4;
    render_instrument(
        compile(definition, base_dir, block_size),
        RenderRequest {
            sample_rate: 48_000.0,
            block_size,
            duration_frames: u64::try_from(source_frames + latency * 2)
                .expect("fixture frame count fits render request"),
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
