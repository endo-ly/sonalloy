use std::path::{Path, PathBuf};
use std::sync::Arc;

use approx::assert_relative_eq;
use sonalloy_core::{
    AdsrDefinition, AssetReference, CompileContext, DiagnosticCode, GeneratorDefinition,
    InstrumentDefinition, InstrumentProcessor, MusicalTimeChange, MusicalTimeMap, ProcessBlock,
    ProcessContext, ProcessEvent, ProcessEventKind, ProcessSpec, RenderRequest,
    SamplePlaybackDirection, SampleRegionDefinition, ScheduledEvent, WaveSequenceDefinition,
    WaveSequenceDirection, WaveSequenceDurationDefinition, WaveSequenceStepDefinition,
    WaveSequenceStepPlayback, compile_instrument, render_instrument,
    render_instrument_with_musical_time_map,
};
use tempfile::TempDir;

fn fixture_directory() -> TempDir {
    tempfile::tempdir().expect("fixture directory creates")
}

fn write_wav(path: &Path, stereo: bool, frames: usize, values: impl Fn(usize) -> (f32, f32)) {
    write_wav_at_sample_rate(path, 48_000, stereo, frames, values);
}

fn write_wav_at_sample_rate(
    path: &Path,
    sample_rate: u32,
    stereo: bool,
    frames: usize,
    values: impl Fn(usize) -> (f32, f32),
) {
    let channels = if stereo { 2_u16 } else { 1_u16 };
    let payload_len = u32::try_from(frames * usize::from(channels) * 2).expect("WAV payload fits");
    let mut bytes = Vec::with_capacity(44 + payload_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + payload_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    for frame in 0..frames {
        let (left, right) = values(frame);
        #[allow(clippy::cast_possible_truncation)]
        let left = (left.clamp(-1.0, 1.0) * 30_000.0) as i16;
        bytes.extend_from_slice(&left.to_le_bytes());
        if stereo {
            #[allow(clippy::cast_possible_truncation)]
            let right = (right.clamp(-1.0, 1.0) * 30_000.0) as i16;
            bytes.extend_from_slice(&right.to_le_bytes());
        }
    }
    std::fs::write(path, bytes).expect("fixture WAV writes");
}

fn base_definition(
    steps: Vec<WaveSequenceStepDefinition>,
    direction: WaveSequenceDirection,
    loop_sequence: bool,
    crossfade: f32,
) -> InstrumentDefinition {
    let reference = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/instruments/basic-poly-synth.json");
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
    definition.layers[0].generator = GeneratorDefinition::WaveSequence(WaveSequenceDefinition {
        root_note: 60,
        direction,
        loop_sequence,
        crossfade,
        steps,
    });
    definition.modulation = None;
    definition
}

fn step(
    id: &str,
    asset_path: String,
    duration: WaveSequenceDurationDefinition,
    playback: WaveSequenceStepPlayback,
    playback_direction: SamplePlaybackDirection,
    gain_db: f32,
    pitch_cents: f32,
) -> WaveSequenceStepDefinition {
    WaveSequenceStepDefinition {
        id: id.to_owned(),
        asset: AssetReference {
            path: asset_path,
            sha256: None,
        },
        region: SampleRegionDefinition {
            start_seconds: 0.0,
            end_seconds: Some(0.1),
        },
        duration,
        playback,
        playback_direction,
        gain_db,
        pitch_cents,
    }
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
            process_spec: ProcessSpec::new(sample_rate, block_size, 0, 2)
                .expect("valid process spec"),
        },
    );
    result
        .instrument
        .expect("Wave Sequence Definition compiles")
}

fn render(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
    duration_frames: u64,
) -> sonalloy_core::RenderedAudio {
    render_at_sample_rate(definition, base_dir, block_size, duration_frames, 48_000.0)
}

fn render_at_sample_rate(
    definition: &InstrumentDefinition,
    base_dir: &Path,
    block_size: usize,
    duration_frames: u64,
    sample_rate: f64,
) -> sonalloy_core::RenderedAudio {
    render_instrument(
        compile_at_sample_rate(definition, base_dir, block_size, sample_rate),
        RenderRequest {
            sample_rate,
            block_size,
            duration_frames,
            tail_frames: 0,
        },
        &[ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }],
    )
    .expect("Wave Sequence render succeeds")
}

#[test]
fn wave_sequence_compiles_steps_and_preserves_missing_step_timing() {
    let directory = fixture_directory();
    let first = directory.path().join("first.wav");
    let third = directory.path().join("third.wav");
    write_wav(&first, false, 4_800, |_| (0.2, 0.2));
    write_wav(&third, false, 4_800, |_| (0.6, 0.6));
    let definition = base_definition(
        vec![
            step(
                "first",
                first.file_name().unwrap().to_string_lossy().into_owned(),
                WaveSequenceDurationDefinition::Seconds { value: 0.01 },
                WaveSequenceStepPlayback::Loop,
                SamplePlaybackDirection::Forward,
                0.0,
                0.0,
            ),
            step(
                "missing",
                "missing.wav".to_owned(),
                WaveSequenceDurationDefinition::Seconds { value: 0.02 },
                WaveSequenceStepPlayback::OneShot,
                SamplePlaybackDirection::Forward,
                0.0,
                0.0,
            ),
            step(
                "third",
                third.file_name().unwrap().to_string_lossy().into_owned(),
                WaveSequenceDurationDefinition::Seconds { value: 0.01 },
                WaveSequenceStepPlayback::Loop,
                SamplePlaybackDirection::Forward,
                0.0,
                0.0,
            ),
        ],
        WaveSequenceDirection::Forward,
        false,
        0.0,
    );
    let compiled = compile(&definition, directory.path(), 257);
    let sonalloy_core::compiler::CompiledGenerator::WaveSequence(sequence) =
        &compiled.layers[0].generator
    else {
        panic!("definition must compile to Wave Sequence");
    };
    assert_eq!(sequence.steps.len(), 3);
    assert!(sequence.steps[0].source.is_some());
    assert!(sequence.steps[1].source.is_none());
    assert!(sequence.steps[2].source.is_some());
    assert_eq!(
        compiled.layers[0].generator.output_mode(),
        sonalloy_core::GeneratorOutputMode::Mono
    );

    let audio = render(&definition, directory.path(), 257, 2_000);
    assert_relative_eq!(audio.channels[0][240], 0.13, epsilon = 0.02);
    assert!(
        audio.channels[0][488..1_432]
            .iter()
            .all(|sample| sample.abs() < 2.0e-5)
    );
    assert_relative_eq!(audio.channels[0][479], 0.13, epsilon = 0.02);
    assert_relative_eq!(audio.channels[0][1_448], 0.39, epsilon = 0.02);
}

#[test]
fn wave_sequence_direction_and_ping_pong_are_audible_and_finite() {
    let directory = fixture_directory();
    let first = directory.path().join("first.wav");
    let second = directory.path().join("second.wav");
    let third = directory.path().join("third.wav");
    write_wav(&first, false, 4_800, |_| (0.2, 0.2));
    write_wav(&second, false, 4_800, |_| (0.4, 0.4));
    write_wav(&third, false, 4_800, |_| (0.6, 0.6));
    let names = [
        first.file_name().unwrap().to_string_lossy().into_owned(),
        second.file_name().unwrap().to_string_lossy().into_owned(),
        third.file_name().unwrap().to_string_lossy().into_owned(),
    ];
    let make = |direction| {
        base_definition(
            names
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    step(
                        &format!("step_{index}"),
                        path.clone(),
                        WaveSequenceDurationDefinition::Seconds { value: 0.01 },
                        WaveSequenceStepPlayback::Loop,
                        SamplePlaybackDirection::Forward,
                        0.0,
                        0.0,
                    )
                })
                .collect(),
            direction,
            false,
            0.0,
        )
    };
    let ping_pong = render(
        &make(WaveSequenceDirection::PingPong),
        directory.path(),
        257,
        2_000,
    );
    assert!(
        ping_pong
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        ping_pong.channels[0]
            .iter()
            .any(|sample| sample.abs() > 1.0e-3)
    );
    assert_relative_eq!(ping_pong.channels[0][300], 0.13, epsilon = 0.02);
    assert_relative_eq!(ping_pong.channels[0][700], 0.26, epsilon = 0.02);
    assert_relative_eq!(ping_pong.channels[0][1_100], 0.39, epsilon = 0.02);
    assert_relative_eq!(ping_pong.channels[0][1_500], 0.26, epsilon = 0.02);
}

#[test]
fn wave_sequence_one_shot_keeps_silence_until_step_end() {
    let directory = fixture_directory();
    let short = directory.path().join("short.wav");
    write_wav(&short, false, 96, |_| (0.4, 0.4));
    let mut one_shot = step(
        "one_shot",
        short.file_name().unwrap().to_string_lossy().into_owned(),
        WaveSequenceDurationDefinition::Seconds { value: 0.01 },
        WaveSequenceStepPlayback::OneShot,
        SamplePlaybackDirection::Forward,
        0.0,
        0.0,
    );
    one_shot.region.end_seconds = Some(0.002);
    let definition = base_definition(vec![one_shot], WaveSequenceDirection::Forward, false, 0.0);
    let audio = render(&definition, directory.path(), 257, 600);
    assert!(audio.channels[0][50].abs() > 0.01);
    assert!(
        audio.channels[0][200..480]
            .iter()
            .all(|sample| sample.abs() < 1.0e-5)
    );
}

#[test]
fn wave_sequence_crossfade_mixes_stereo_and_is_block_size_independent() {
    let directory = fixture_directory();
    let mono = directory.path().join("mono.wav");
    let stereo = directory.path().join("stereo.wav");
    write_wav(&mono, false, 4_800, |_| (0.25, 0.25));
    write_wav(&stereo, true, 4_800, |_| (0.5, -0.5));
    let definition = base_definition(
        vec![
            step(
                "mono",
                mono.file_name().unwrap().to_string_lossy().into_owned(),
                WaveSequenceDurationDefinition::Seconds { value: 0.02 },
                WaveSequenceStepPlayback::Loop,
                SamplePlaybackDirection::Forward,
                0.0,
                0.0,
            ),
            step(
                "stereo",
                stereo.file_name().unwrap().to_string_lossy().into_owned(),
                WaveSequenceDurationDefinition::Seconds { value: 0.02 },
                WaveSequenceStepPlayback::Loop,
                SamplePlaybackDirection::Forward,
                -3.0,
                0.0,
            ),
        ],
        WaveSequenceDirection::Forward,
        true,
        0.5,
    );
    let reference = render(&definition, directory.path(), 64, 2_048);
    let candidate_257 = render(&definition, directory.path(), 257, 2_048);
    let candidate = render(&definition, directory.path(), 1_024, 2_048);
    assert_eq!(reference.channels[0].len(), 2_048);
    assert!(
        reference
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(
        reference.channels[0]
            .iter()
            .zip(&reference.channels[1])
            .any(|(left, right)| (left - right).abs() > 0.05)
    );
    assert!(
        reference.channels[0]
            .iter()
            .chain(&reference.channels[1])
            .all(|sample| sample.abs() <= 1.0)
    );
    for (left, candidate_left) in reference.channels[0].iter().zip(&candidate.channels[0]) {
        assert_relative_eq!(*left, *candidate_left, epsilon = 1.0e-6);
    }
    for (left, candidate_left) in reference.channels[0].iter().zip(&candidate_257.channels[0]) {
        assert_relative_eq!(*left, *candidate_left, epsilon = 1.0e-6);
    }
    for (right, candidate_right) in reference.channels[1].iter().zip(&candidate.channels[1]) {
        assert_relative_eq!(*right, *candidate_right, epsilon = 1.0e-6);
    }
    for (right, candidate_right) in reference.channels[1].iter().zip(&candidate_257.channels[1]) {
        assert_relative_eq!(*right, *candidate_right, epsilon = 1.0e-6);
    }
}

#[test]
fn wave_sequence_preserves_time_units_across_sample_rates() {
    let directory = fixture_directory();
    let first = directory.path().join("first.wav");
    let second = directory.path().join("second.wav");
    let definition = base_definition(
        vec![
            step(
                "first",
                first.file_name().unwrap().to_string_lossy().into_owned(),
                WaveSequenceDurationDefinition::Seconds { value: 0.01 },
                WaveSequenceStepPlayback::Loop,
                SamplePlaybackDirection::Forward,
                0.0,
                0.0,
            ),
            step(
                "second",
                second.file_name().unwrap().to_string_lossy().into_owned(),
                WaveSequenceDurationDefinition::Seconds { value: 0.01 },
                WaveSequenceStepPlayback::Loop,
                SamplePlaybackDirection::Forward,
                0.0,
                0.0,
            ),
        ],
        WaveSequenceDirection::Forward,
        false,
        0.0,
    );
    for (sample_rate, source_sample_rate) in [
        (44_100.0_f64, 44_100_u32),
        (48_000.0, 48_000),
        (96_000.0, 96_000),
    ] {
        let fixture_frames =
            usize::try_from(source_sample_rate / 10).expect("fixture frame count fits usize");
        write_wav_at_sample_rate(&first, source_sample_rate, false, fixture_frames, |_| {
            (0.2, 0.2)
        });
        write_wav_at_sample_rate(&second, source_sample_rate, false, fixture_frames, |_| {
            (0.6, 0.6)
        });
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let first_step_frames = (sample_rate * 0.01).round() as usize;
        let audio = render_at_sample_rate(
            &definition,
            directory.path(),
            257,
            u64::try_from(first_step_frames + 257).expect("render frame count fits u64"),
            sample_rate,
        );
        assert_eq!(audio.channels.len(), 2);
        assert!(
            audio
                .channels
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        assert_relative_eq!(
            audio.channels[0][first_step_frames - 1],
            0.13,
            epsilon = 0.02
        );
        assert_relative_eq!(
            audio.channels[0][first_step_frames + 8],
            0.39,
            epsilon = 0.02
        );
    }
}

#[test]
fn wave_sequence_validation_rejects_empty_sequence_and_invalid_duration() {
    let empty = base_definition(Vec::new(), WaveSequenceDirection::Forward, false, 0.0);
    assert!(
        empty
            .validate()
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidSequence)
    );

    let invalid_duration = base_definition(
        vec![step(
            "invalid",
            "missing.wav".to_owned(),
            WaveSequenceDurationDefinition::Seconds { value: 0.0 },
            WaveSequenceStepPlayback::OneShot,
            SamplePlaybackDirection::Forward,
            0.0,
            0.0,
        )],
        WaveSequenceDirection::Forward,
        false,
        0.0,
    );
    assert!(
        invalid_duration
            .validate()
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidStepDuration)
    );
}

#[test]
fn wave_sequence_beats_follow_tempo_changes_without_restarting_the_step() {
    let directory = fixture_directory();
    let first = directory.path().join("first.wav");
    let second = directory.path().join("second.wav");
    write_wav(&first, false, 4_800, |_| (0.2, 0.2));
    write_wav(&second, false, 4_800, |_| (0.6, 0.6));
    let definition = base_definition(
        vec![
            step(
                "first",
                first.file_name().unwrap().to_string_lossy().into_owned(),
                WaveSequenceDurationDefinition::Beats { value: 1.0 },
                WaveSequenceStepPlayback::Loop,
                SamplePlaybackDirection::Forward,
                0.0,
                0.0,
            ),
            step(
                "second",
                second.file_name().unwrap().to_string_lossy().into_owned(),
                WaveSequenceDurationDefinition::Beats { value: 1.0 },
                WaveSequenceStepPlayback::Loop,
                SamplePlaybackDirection::Forward,
                0.0,
                0.0,
            ),
        ],
        WaveSequenceDirection::Forward,
        false,
        0.0,
    );
    let compiled = compile(&definition, directory.path(), 257);
    let musical_time_map = MusicalTimeMap::new(vec![
        MusicalTimeChange {
            absolute_frame: 0,
            tempo_bpm: 120.0,
            time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
        },
        MusicalTimeChange {
            absolute_frame: 12_000,
            tempo_bpm: 60.0,
            time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
        },
    ])
    .expect("tempo map");
    let audio = render_instrument_with_musical_time_map(
        compiled,
        RenderRequest {
            sample_rate: 48_000.0,
            block_size: 257,
            duration_frames: 40_000,
            tail_frames: 0,
        },
        &[ScheduledEvent {
            absolute_frame: 0,
            kind: ProcessEventKind::NoteOn {
                note_id: 1,
                note_number: 60,
                velocity: 100,
            },
        }],
        &musical_time_map,
    )
    .expect("tempo-aware sequence render");
    assert_relative_eq!(audio.channels[0][35_000], 0.13, epsilon = 0.02);
    assert_relative_eq!(audio.channels[0][37_000], 0.39, epsilon = 0.02);
}

#[test]
fn wave_sequence_reset_restarts_the_same_runtime_state() {
    let directory = fixture_directory();
    let asset = directory.path().join("loop.wav");
    write_wav(&asset, false, 4_800, |frame| {
        #[allow(clippy::cast_precision_loss)]
        let phase = frame as f32 / 4_800.0;
        ((std::f32::consts::TAU * phase).sin() * 0.5, 0.0)
    });
    let definition = base_definition(
        vec![step(
            "loop",
            asset.file_name().unwrap().to_string_lossy().into_owned(),
            WaveSequenceDurationDefinition::Seconds { value: 0.05 },
            WaveSequenceStepPlayback::Loop,
            SamplePlaybackDirection::Reverse,
            0.0,
            0.0,
        )],
        WaveSequenceDirection::Forward,
        true,
        0.0,
    );
    let compiled = compile(&definition, directory.path(), 257);
    let mut runtime = compiled.instantiate();
    runtime
        .prepare(ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec"))
        .expect("runtime prepares");
    let event = [ProcessEvent {
        sample_offset: 0,
        kind: ProcessEventKind::NoteOn {
            note_id: 1,
            note_number: 60,
            velocity: 100,
        },
    }];
    let render_block = |runtime: &mut sonalloy_core::InstrumentRuntime, absolute_frame| {
        let mut left = vec![0.0; 257];
        let mut right = vec![0.0; 257];
        let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
        runtime
            .process(ProcessBlock {
                frames: 257,
                context: ProcessContext {
                    absolute_frame,
                    tempo_bpm: 120.0,
                    beat_position: 0.0,
                    bar_position: 0.0,
                    time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                },
                events: &event,
                input: &[],
                output: &mut output,
            })
            .expect("runtime process");
        (left, right)
    };
    let first = render_block(&mut runtime, 0);
    runtime.reset().expect("runtime resets");
    let second = render_block(&mut runtime, 0);
    for (left, right) in first.0.iter().zip(&second.0) {
        assert_relative_eq!(*left, *right, epsilon = 1.0e-6);
    }
    for (left, right) in first.1.iter().zip(&second.1) {
        assert_relative_eq!(*left, *right, epsilon = 1.0e-6);
    }
}

#[test]
fn wave_sequence_reverse_starts_at_the_last_step() {
    let directory = fixture_directory();
    let first = directory.path().join("first.wav");
    let second = directory.path().join("second.wav");
    write_wav(&first, false, 4_800, |_| (0.2, 0.2));
    write_wav(&second, false, 4_800, |_| (0.6, 0.6));
    let definition = base_definition(
        vec![
            step(
                "first",
                first.file_name().unwrap().to_string_lossy().into_owned(),
                WaveSequenceDurationDefinition::Seconds { value: 0.01 },
                WaveSequenceStepPlayback::Loop,
                SamplePlaybackDirection::Forward,
                0.0,
                0.0,
            ),
            step(
                "second",
                second.file_name().unwrap().to_string_lossy().into_owned(),
                WaveSequenceDurationDefinition::Seconds { value: 0.01 },
                WaveSequenceStepPlayback::Loop,
                SamplePlaybackDirection::Forward,
                0.0,
                0.0,
            ),
        ],
        WaveSequenceDirection::Reverse,
        false,
        0.0,
    );
    let audio = render(&definition, directory.path(), 257, 600);
    assert_relative_eq!(audio.channels[0][300], 0.39, epsilon = 0.02);
    assert_relative_eq!(audio.channels[0][550], 0.13, epsilon = 0.02);
}

#[test]
fn wave_sequence_all_missing_is_unavailable_but_compile_is_recoverable() {
    let directory = fixture_directory();
    let definition = base_definition(
        vec![step(
            "missing",
            "missing.wav".to_owned(),
            WaveSequenceDurationDefinition::Seconds { value: 0.1 },
            WaveSequenceStepPlayback::OneShot,
            SamplePlaybackDirection::Forward,
            0.0,
            0.0,
        )],
        WaveSequenceDirection::Forward,
        false,
        0.0,
    );
    let result = compile_instrument(
        &definition,
        &CompileContext {
            definition_base_dir: directory.path().to_path_buf(),
            process_spec: ProcessSpec::new(48_000.0, 257, 0, 2).expect("valid process spec"),
        },
    );
    assert!(result.instrument.is_some());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::AssetNotFound)
    );
}
