use std::fmt;

use sonalloy_core::{TempoChange, TempoMap};

const U64_LIMIT_AS_F64: f64 = 18_446_744_073_709_551_616.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TempoPoint {
    pub(crate) tick: u64,
    pub(crate) bpm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MusicalTimeError {
    InvalidTicksPerBeat,
    InvalidSampleRate,
    InvalidTempo,
    TempoMapEmpty,
    TempoMapMustStartAtZero,
    TempoMapNotSorted,
    TempoFrameCollision,
    FrameOverflow,
}

impl fmt::Display for MusicalTimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidTicksPerBeat => "ticks per beat must be between 1 and 32767",
            Self::InvalidSampleRate => "sample rate must be finite and greater than zero",
            Self::InvalidTempo => "tempo must be finite and greater than zero",
            Self::TempoMapEmpty => "tempo changes must not be empty",
            Self::TempoMapMustStartAtZero => "tempo changes must start at tick zero",
            Self::TempoMapNotSorted => "tempo changes must be strictly ordered by tick",
            Self::TempoFrameCollision => {
                "tempo changes must map to distinct frames at the selected sample rate"
            }
            Self::FrameOverflow => "musical time does not fit in the frame counter",
        };
        formatter.write_str(message)
    }
}

pub(crate) fn validate_tempo_points(
    ticks_per_beat: u16,
    tempo_changes: &[TempoPoint],
) -> Result<(), MusicalTimeError> {
    if ticks_per_beat == 0 || ticks_per_beat > 32_767 {
        return Err(MusicalTimeError::InvalidTicksPerBeat);
    }
    let Some(first) = tempo_changes.first() else {
        return Err(MusicalTimeError::TempoMapEmpty);
    };
    if first.tick != 0 {
        return Err(MusicalTimeError::TempoMapMustStartAtZero);
    }
    if tempo_changes
        .iter()
        .any(|change| !change.bpm.is_finite() || change.bpm <= 0.0)
    {
        return Err(MusicalTimeError::InvalidTempo);
    }
    if tempo_changes
        .windows(2)
        .any(|window| window[0].tick >= window[1].tick)
    {
        return Err(MusicalTimeError::TempoMapNotSorted);
    }
    Ok(())
}

pub(crate) fn tick_to_frame(
    tick: u64,
    ticks_per_beat: u16,
    tempo_changes: &[TempoPoint],
    sample_rate: f64,
) -> Result<u64, MusicalTimeError> {
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return Err(MusicalTimeError::InvalidSampleRate);
    }
    validate_tempo_points(ticks_per_beat, tempo_changes)?;

    let mut cursor_tick = 0_u64;
    let mut cursor_frames = 0.0_f64;
    let mut tempo = tempo_changes[0].bpm;
    for change in tempo_changes.iter().skip(1) {
        if change.tick > tick {
            break;
        }
        cursor_frames += ticks_to_frames(
            change.tick - cursor_tick,
            tempo,
            ticks_per_beat,
            sample_rate,
        );
        cursor_tick = change.tick;
        tempo = change.bpm;
    }
    if tick > cursor_tick {
        cursor_frames += ticks_to_frames(tick - cursor_tick, tempo, ticks_per_beat, sample_rate);
    }
    round_frame(cursor_frames)
}

pub(crate) fn musical_duration_seconds(
    length_ticks: u64,
    ticks_per_beat: u16,
    tempo_changes: &[TempoPoint],
) -> Result<f64, MusicalTimeError> {
    validate_tempo_points(ticks_per_beat, tempo_changes)?;

    let mut cursor_tick = 0_u64;
    let mut seconds = 0.0_f64;
    let mut tempo = tempo_changes[0].bpm;
    for change in tempo_changes.iter().skip(1) {
        if change.tick >= length_ticks {
            break;
        }
        seconds += ticks_to_seconds(change.tick - cursor_tick, tempo, ticks_per_beat);
        cursor_tick = change.tick;
        tempo = change.bpm;
    }
    if length_ticks > cursor_tick {
        seconds += ticks_to_seconds(length_ticks - cursor_tick, tempo, ticks_per_beat);
    }
    if seconds.is_finite() {
        Ok(seconds)
    } else {
        Err(MusicalTimeError::FrameOverflow)
    }
}

pub(crate) fn build_tempo_map(
    ticks_per_beat: u16,
    tempo_changes: &[TempoPoint],
    sample_rate: f64,
) -> Result<TempoMap, MusicalTimeError> {
    validate_tempo_points(ticks_per_beat, tempo_changes)?;
    let mut changes = Vec::with_capacity(tempo_changes.len());
    for change in tempo_changes {
        let absolute_frame =
            tick_to_frame(change.tick, ticks_per_beat, tempo_changes, sample_rate)?;
        if changes
            .last()
            .is_some_and(|previous: &TempoChange| previous.absolute_frame >= absolute_frame)
        {
            return Err(MusicalTimeError::TempoFrameCollision);
        }
        changes.push(TempoChange {
            absolute_frame,
            tempo_bpm: change.bpm,
        });
    }
    TempoMap::new(changes).map_err(|error| match error {
        sonalloy_core::RenderError::TempoMapEmpty => MusicalTimeError::TempoMapEmpty,
        sonalloy_core::RenderError::TempoMapMustStartAtZero => {
            MusicalTimeError::TempoMapMustStartAtZero
        }
        sonalloy_core::RenderError::TempoMapNotSorted => MusicalTimeError::TempoMapNotSorted,
        sonalloy_core::RenderError::InvalidTempo => MusicalTimeError::InvalidTempo,
        _ => MusicalTimeError::FrameOverflow,
    })
}

fn ticks_to_frames(ticks: u64, bpm: f64, ticks_per_beat: u16, sample_rate: f64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let ticks = ticks as f64;
    ticks * 60.0 * sample_rate / bpm / f64::from(ticks_per_beat)
}

fn ticks_to_seconds(ticks: u64, bpm: f64, ticks_per_beat: u16) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let ticks = ticks as f64;
    ticks * 60.0 / bpm / f64::from(ticks_per_beat)
}

fn round_frame(frames: f64) -> Result<u64, MusicalTimeError> {
    let rounded = frames.round();
    if !rounded.is_finite() || !(0.0..U64_LIMIT_AS_F64).contains(&rounded) {
        return Err(MusicalTimeError::FrameOverflow);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frame = rounded as u64;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempo_changes() -> [TempoPoint; 2] {
        [
            TempoPoint {
                tick: 0,
                bpm: 120.0,
            },
            TempoPoint {
                tick: 480,
                bpm: 60.0,
            },
        ]
    }

    #[test]
    fn tick_conversion_accumulates_fractional_frames_before_rounding() {
        let changes = tempo_changes();

        assert_eq!(tick_to_frame(480, 480, &changes, 48_000.0), Ok(24_000));
        assert_eq!(tick_to_frame(960, 480, &changes, 48_000.0), Ok(72_000));
    }

    #[test]
    fn musical_duration_does_not_depend_on_sample_rate() {
        let changes = tempo_changes();

        assert_eq!(musical_duration_seconds(960, 480, &changes), Ok(1.5));
    }

    #[test]
    fn tempo_frame_collisions_are_rejected() {
        let changes = [
            TempoPoint {
                tick: 0,
                bpm: 120.0,
            },
            TempoPoint { tick: 1, bpm: 60.0 },
        ];

        assert_eq!(
            build_tempo_map(480, &changes, 1.0),
            Err(MusicalTimeError::TempoFrameCollision)
        );
    }
}
