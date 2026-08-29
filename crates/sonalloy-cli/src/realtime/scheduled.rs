use std::fmt;

use sonalloy_core::{
    PreparedMusicalTimeMap, ProcessContext, ProcessEvent, ProcessEventKind, ScheduledEvent,
};

use crate::pattern::{CompiledPattern, loop_note_id};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScheduledFeedError {
    BlockStartDiscontinuity,
    FrameOverflow,
    EventCapacityExceeded,
    IterationOverflow,
    InvalidSampleRate,
}

impl fmt::Display for ScheduledFeedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::BlockStartDiscontinuity => "scheduled block start is discontinuous",
            Self::FrameOverflow => "scheduled frame counter overflow",
            Self::EventCapacityExceeded => "scheduled event scratch capacity is insufficient",
            Self::IterationOverflow => "scheduled loop iteration counter overflow",
            Self::InvalidSampleRate => "scheduled musical-time sample rate is invalid",
        };
        formatter.write_str(message)
    }
}

pub(crate) struct ScheduledEventFeed {
    events: Vec<ScheduledEvent>,
    prepared_musical_time_map: PreparedMusicalTimeMap,
    pattern_length_frames: u64,
    playback_end_frame: Option<u64>,
    looping: bool,
    iteration: u32,
    iteration_start_frame: u64,
    event_index: usize,
    finished: bool,
}

impl ScheduledEventFeed {
    pub(crate) fn new(
        pattern: CompiledPattern,
        tail_frames: u64,
        latency_frames: usize,
        looping: bool,
        sample_rate: f64,
    ) -> Result<Self, ScheduledFeedError> {
        let prepared_musical_time_map = pattern
            .musical_time_map
            .prepare(sample_rate)
            .map_err(|_| ScheduledFeedError::InvalidSampleRate)?;
        let playback_end_frame = if looping {
            None
        } else {
            let latency_frames =
                u64::try_from(latency_frames).map_err(|_| ScheduledFeedError::FrameOverflow)?;
            Some(
                pattern
                    .one_shot_duration_frames
                    .checked_add(tail_frames)
                    .and_then(|frame| frame.checked_add(latency_frames))
                    .ok_or(ScheduledFeedError::FrameOverflow)?,
            )
        };
        Ok(Self {
            events: pattern.events,
            prepared_musical_time_map,
            pattern_length_frames: pattern.length_frames,
            playback_end_frame,
            looping,
            iteration: 0,
            iteration_start_frame: 0,
            event_index: 0,
            finished: false,
        })
    }

    pub(crate) fn max_events_per_block(&self) -> usize {
        if self.looping {
            self.events.len().saturating_mul(2)
        } else {
            self.events.len()
        }
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.finished
    }

    pub(crate) fn prepare_block(
        &mut self,
        block_start_frame: u64,
        requested_frames: usize,
        process_events: &mut Vec<ProcessEvent>,
    ) -> Result<usize, ScheduledFeedError> {
        if self.finished {
            return Ok(0);
        }
        if requested_frames == 0 {
            return Ok(0);
        }
        if self
            .playback_end_frame
            .is_some_and(|end| block_start_frame >= end)
        {
            self.finished = true;
            return Ok(0);
        }
        let expected_start = if self.looping {
            self.iteration_start_frame
        } else {
            0
        };
        if block_start_frame < expected_start {
            return Err(ScheduledFeedError::BlockStartDiscontinuity);
        }
        process_events.clear();
        if self.looping {
            self.advance_boundaries(block_start_frame, process_events)?;
        }

        let mut frames = requested_frames;
        if let Some(end_frame) = self.playback_end_frame {
            let remaining = end_frame
                .checked_sub(block_start_frame)
                .ok_or(ScheduledFeedError::BlockStartDiscontinuity)?;
            frames = frames.min(usize::try_from(remaining).unwrap_or(usize::MAX));
        }
        if self.looping {
            let loop_end = self
                .iteration_start_frame
                .checked_add(self.pattern_length_frames)
                .ok_or(ScheduledFeedError::FrameOverflow)?;
            let remaining = loop_end
                .checked_sub(block_start_frame)
                .ok_or(ScheduledFeedError::BlockStartDiscontinuity)?;
            frames = frames.min(usize::try_from(remaining).unwrap_or(usize::MAX));
        }
        if let Some(next_musical_time_frame) = self.next_musical_time_frame(block_start_frame) {
            let remaining = next_musical_time_frame
                .checked_sub(block_start_frame)
                .ok_or(ScheduledFeedError::BlockStartDiscontinuity)?;
            frames = frames.min(usize::try_from(remaining).unwrap_or(usize::MAX));
        }
        if frames == 0 {
            return Err(ScheduledFeedError::FrameOverflow);
        }
        let block_end = block_start_frame
            .checked_add(u64::try_from(frames).map_err(|_| ScheduledFeedError::FrameOverflow)?)
            .ok_or(ScheduledFeedError::FrameOverflow)?;
        self.collect_events(block_start_frame, block_end, process_events)?;
        if self.playback_end_frame.is_some_and(|end| block_end >= end) {
            self.finished = true;
        }
        Ok(frames)
    }

    pub(crate) fn context_at(&self, absolute_frame: u64) -> ProcessContext {
        let relative_frame = if self.looping {
            absolute_frame.saturating_sub(self.iteration_start_frame)
        } else {
            absolute_frame
        };
        let mut context = self.prepared_musical_time_map.context_at(relative_frame);
        context.absolute_frame = absolute_frame;
        context
    }

    fn next_musical_time_frame(&self, absolute_frame: u64) -> Option<u64> {
        let relative_frame = if self.looping {
            absolute_frame.saturating_sub(self.iteration_start_frame)
        } else {
            absolute_frame
        };
        self.prepared_musical_time_map
            .next_change_after(relative_frame)
            .and_then(|change_frame| self.iteration_start_frame.checked_add(change_frame))
    }

    fn advance_boundaries(
        &mut self,
        block_start_frame: u64,
        process_events: &mut Vec<ProcessEvent>,
    ) -> Result<(), ScheduledFeedError> {
        loop {
            let loop_end = self
                .iteration_start_frame
                .checked_add(self.pattern_length_frames)
                .ok_or(ScheduledFeedError::FrameOverflow)?;
            if block_start_frame < loop_end {
                return Ok(());
            }
            if block_start_frame > loop_end {
                return Err(ScheduledFeedError::BlockStartDiscontinuity);
            }
            while let Some(event) = self.events.get(self.event_index) {
                let event_frame = self
                    .iteration_start_frame
                    .checked_add(event.absolute_frame)
                    .ok_or(ScheduledFeedError::FrameOverflow)?;
                if event_frame != block_start_frame {
                    break;
                }
                self.push_event(event, 0, process_events)?;
                self.event_index += 1;
            }
            self.iteration = self
                .iteration
                .checked_add(1)
                .ok_or(ScheduledFeedError::IterationOverflow)?;
            self.iteration_start_frame = block_start_frame;
            self.event_index = 0;
        }
    }

    fn collect_events(
        &mut self,
        block_start_frame: u64,
        block_end_frame: u64,
        process_events: &mut Vec<ProcessEvent>,
    ) -> Result<(), ScheduledFeedError> {
        while let Some(event) = self.events.get(self.event_index) {
            let event_frame = self
                .iteration_start_frame
                .checked_add(event.absolute_frame)
                .ok_or(ScheduledFeedError::FrameOverflow)?;
            if event_frame < block_start_frame {
                return Err(ScheduledFeedError::BlockStartDiscontinuity);
            }
            if event_frame >= block_end_frame {
                break;
            }
            let offset = usize::try_from(event_frame - block_start_frame)
                .map_err(|_| ScheduledFeedError::FrameOverflow)?;
            self.push_event(event, offset, process_events)?;
            self.event_index += 1;
        }
        Ok(())
    }

    fn push_event(
        &self,
        event: &ScheduledEvent,
        sample_offset: usize,
        process_events: &mut Vec<ProcessEvent>,
    ) -> Result<(), ScheduledFeedError> {
        if process_events.len() == process_events.capacity() {
            return Err(ScheduledFeedError::EventCapacityExceeded);
        }
        let kind = if self.looping {
            remap_loop_note_id(event.kind, self.iteration)
        } else {
            event.kind
        };
        process_events.push(ProcessEvent {
            sample_offset,
            kind,
        });
        Ok(())
    }
}

fn remap_loop_note_id(kind: ProcessEventKind, iteration: u32) -> ProcessEventKind {
    match kind {
        ProcessEventKind::NoteOn {
            note_id,
            note_number,
            velocity,
        } => ProcessEventKind::NoteOn {
            note_id: loop_note_id(
                iteration,
                u32::try_from(note_id).expect("compiled pattern note IDs fit in u32"),
            ),
            note_number,
            velocity,
        },
        ProcessEventKind::NoteOff { note_id } => ProcessEventKind::NoteOff {
            note_id: loop_note_id(
                iteration,
                u32::try_from(note_id).expect("compiled pattern note IDs fit in u32"),
            ),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use sonalloy_core::{
        MusicalTimeChange, MusicalTimeMap, ProcessEventKind, ScheduledEvent, TimeSignature,
    };

    use super::ScheduledEventFeed;
    use crate::pattern::{CompiledPattern, loop_note_id};

    fn pattern(events: Vec<ScheduledEvent>, length_frames: u64) -> CompiledPattern {
        CompiledPattern {
            events,
            musical_time_map: MusicalTimeMap::new(vec![
                MusicalTimeChange {
                    absolute_frame: 0,
                    tempo_bpm: 120.0,
                    time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                },
                MusicalTimeChange {
                    absolute_frame: length_frames / 2,
                    tempo_bpm: 90.0,
                    time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                },
            ])
            .expect("tempo map"),
            length_frames,
            one_shot_duration_frames: length_frames,
        }
    }

    #[test]
    fn events_are_converted_to_sample_offsets_and_block_end_is_exclusive() {
        let mut feed = ScheduledEventFeed::new(
            pattern(
                vec![
                    ScheduledEvent {
                        absolute_frame: 1001,
                        kind: ProcessEventKind::NoteOn {
                            note_id: 0,
                            note_number: 60,
                            velocity: 100,
                        },
                    },
                    ScheduledEvent {
                        absolute_frame: 1255,
                        kind: ProcessEventKind::NoteOff { note_id: 0 },
                    },
                ],
                2_000,
            ),
            0,
            0,
            false,
            48_000.0,
        )
        .expect("feed");
        let mut events = Vec::with_capacity(2);

        let frames = feed.prepare_block(1_000, 256, &mut events).expect("block");

        assert_eq!(frames, 256);
        assert_eq!(events[0].sample_offset, 1);
        assert_eq!(events[1].sample_offset, 256 - 1);
    }

    #[test]
    fn tempo_boundaries_limit_the_process_block() {
        let mut feed = ScheduledEventFeed::new(pattern(Vec::new(), 2_000), 0, 0, false, 48_000.0)
            .expect("feed");
        let mut events = Vec::with_capacity(1);

        let frames = feed.prepare_block(0, 2_000, &mut events).expect("block");

        assert_eq!(frames, 1_000);
        assert!((feed.context_at(0).tempo_bpm - 120.0).abs() < f64::EPSILON);
    }

    #[test]
    fn context_positions_accumulate_tempo_and_meter_changes() {
        let feed = ScheduledEventFeed::new(
            CompiledPattern {
                events: Vec::new(),
                musical_time_map: MusicalTimeMap::new(vec![
                    MusicalTimeChange {
                        absolute_frame: 0,
                        tempo_bpm: 120.0,
                        time_signature: sonalloy_core::DEFAULT_TIME_SIGNATURE,
                    },
                    MusicalTimeChange {
                        absolute_frame: 48_000,
                        tempo_bpm: 60.0,
                        time_signature: TimeSignature {
                            numerator: 3,
                            denominator: 4,
                        },
                    },
                ])
                .expect("tempo and meter map"),
                length_frames: 96_000,
                one_shot_duration_frames: 96_000,
            },
            0,
            0,
            false,
            48_000.0,
        )
        .expect("feed");

        let at_change = feed.context_at(48_000);
        let later = feed.context_at(72_000);

        assert!((at_change.beat_position - 2.0).abs() < f64::EPSILON);
        assert!((at_change.bar_position - 1.0).abs() < f64::EPSILON);
        assert_eq!(at_change.time_signature.numerator, 3);
        assert!((later.beat_position - 2.5).abs() < f64::EPSILON);
        assert!((later.bar_position - (7.0 / 6.0)).abs() < 1.0e-12);
    }

    #[test]
    fn loop_boundary_emits_previous_end_before_next_start_with_unique_ids() {
        let mut feed = ScheduledEventFeed::new(
            constant_pattern(
                vec![
                    ScheduledEvent {
                        absolute_frame: 0,
                        kind: ProcessEventKind::NoteOn {
                            note_id: 0,
                            note_number: 60,
                            velocity: 100,
                        },
                    },
                    ScheduledEvent {
                        absolute_frame: 100,
                        kind: ProcessEventKind::NoteOff { note_id: 0 },
                    },
                ],
                100,
            ),
            0,
            0,
            true,
            48_000.0,
        )
        .expect("feed");
        let mut events = Vec::with_capacity(4);

        let first = feed
            .prepare_block(0, 100, &mut events)
            .expect("first block");
        assert_eq!(first, 100);
        events.clear();
        let second = feed
            .prepare_block(100, 100, &mut events)
            .expect("second block");

        assert_eq!(second, 100);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0].kind,
            ProcessEventKind::NoteOff { note_id: 0 }
        ));
        assert!(matches!(
            events[1].kind,
            ProcessEventKind::NoteOn { note_id, .. } if note_id == loop_note_id(1, 0)
        ));
    }

    fn constant_pattern(events: Vec<ScheduledEvent>, length_frames: u64) -> CompiledPattern {
        CompiledPattern {
            events,
            musical_time_map: MusicalTimeMap::constant(120.0).expect("tempo map"),
            length_frames,
            one_shot_duration_frames: length_frames,
        }
    }
}
