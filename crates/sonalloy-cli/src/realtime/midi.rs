use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crossbeam_queue::ArrayQueue;
use midir::MidiInputConnection;
use midly::{MidiMessage, live::LiveEvent};
use sonalloy_core::{ParameterHandle, ProcessEventKind};

use super::audio::{FatalStatus, QueuedEvent, RealtimeStatus};
use super::device::{DeviceError, SelectedMidiDevice};
use crate::midi_common::{
    MOD_WHEEL_CONTROLLER, SUSTAIN_PEDAL_CONTROLLER, normalize_control, normalize_pitch_bend,
    note_id,
};

pub(crate) struct LiveMidiState {
    events: Arc<ArrayQueue<QueuedEvent>>,
    status: Arc<RealtimeStatus>,
    active_notes: HashMap<(u8, u8), VecDeque<u64>>,
    serials: HashMap<(u8, u8), u32>,
    macro_parameters: [Option<ParameterHandle>; 128],
    next_sequence: u64,
}

impl LiveMidiState {
    fn new(
        events: Arc<ArrayQueue<QueuedEvent>>,
        status: Arc<RealtimeStatus>,
        macro_parameters: &[Option<ParameterHandle>; 128],
    ) -> Self {
        Self {
            events,
            status,
            active_notes: HashMap::new(),
            serials: HashMap::new(),
            macro_parameters: *macro_parameters,
            next_sequence: 0,
        }
    }
}

pub(crate) fn connect(
    selected: SelectedMidiDevice,
    events: Arc<ArrayQueue<QueuedEvent>>,
    status: Arc<RealtimeStatus>,
    macro_parameters: &[Option<ParameterHandle>; 128],
) -> Result<MidiInputConnection<LiveMidiState>, DeviceError> {
    let state = LiveMidiState::new(events, status, macro_parameters);
    selected
        .input
        .connect(&selected.port, "sonalloy", handle_message, state)
        .map_err(|error| DeviceError {
            diagnostic: sonalloy_core::Diagnostic::error(
                sonalloy_core::DiagnosticCode::MidiError,
                "could not connect to the MIDI input",
            )
            .with_detail(error.to_string()),
        })
}

fn handle_message(timestamp_us: u64, message: &[u8], state: &mut LiveMidiState) {
    if state.status.fatal() != FatalStatus::None {
        return;
    }
    let Ok(event) = LiveEvent::parse(message) else {
        state.status.set_fatal(FatalStatus::Midi);
        return;
    };
    let LiveEvent::Midi { channel, message } = event else {
        return;
    };
    let channel = channel.as_int();
    match message {
        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
            let note = key.as_int();
            let Some(serial) = next_serial(state, channel, note) else {
                return;
            };
            let note_id = note_id(channel, note, serial);
            state
                .active_notes
                .entry((channel, note))
                .or_default()
                .push_back(note_id);
            enqueue(
                state,
                timestamp_us,
                ProcessEventKind::NoteOn {
                    note_id,
                    note_number: note,
                    velocity: vel.as_int(),
                },
            );
        }
        MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
            let note = key.as_int();
            if let Some(note_id) = state
                .active_notes
                .get_mut(&(channel, note))
                .and_then(VecDeque::pop_front)
            {
                enqueue(state, timestamp_us, ProcessEventKind::NoteOff { note_id });
            }
        }
        MidiMessage::Controller { controller, value }
            if controller.as_int() == SUSTAIN_PEDAL_CONTROLLER =>
        {
            enqueue(
                state,
                timestamp_us,
                ProcessEventKind::SustainPedal {
                    down: value.as_int() >= 64,
                },
            );
        }
        MidiMessage::Controller { controller, value }
            if controller.as_int() == MOD_WHEEL_CONTROLLER =>
        {
            enqueue(
                state,
                timestamp_us,
                ProcessEventKind::ModWheel {
                    value: normalize_control(value.as_int()),
                },
            );
        }
        MidiMessage::PitchBend { bend } => {
            enqueue(
                state,
                timestamp_us,
                ProcessEventKind::PitchBend {
                    value: normalize_pitch_bend(bend.as_int()),
                },
            );
        }
        MidiMessage::ChannelAftertouch { vel } => {
            enqueue(
                state,
                timestamp_us,
                ProcessEventKind::Aftertouch {
                    value: normalize_control(vel.as_int()),
                },
            );
        }
        MidiMessage::Controller { controller, value } => {
            if let Some(parameter) = state.macro_parameters[usize::from(controller.as_int())] {
                enqueue(
                    state,
                    timestamp_us,
                    ProcessEventKind::ParameterChange {
                        parameter,
                        normalized: normalize_control(value.as_int()),
                    },
                );
            }
        }
        MidiMessage::Aftertouch { .. } | MidiMessage::ProgramChange { .. } => {}
    }
}

fn next_serial(state: &mut LiveMidiState, channel: u8, note: u8) -> Option<u32> {
    let serial = state.serials.entry((channel, note)).or_default();
    let current = *serial;
    let Some(next) = serial.checked_add(1) else {
        state.status.set_fatal(FatalStatus::Midi);
        return None;
    };
    *serial = next;
    Some(current)
}

fn enqueue(state: &mut LiveMidiState, timestamp_us: u64, kind: ProcessEventKind) {
    if state.status.fatal() != FatalStatus::None {
        return;
    }
    let Some(next_sequence) = state.next_sequence.checked_add(1) else {
        state.status.set_fatal(FatalStatus::Midi);
        return;
    };
    let event = QueuedEvent {
        timestamp_us,
        sequence: state.next_sequence,
        kind,
    };
    if state.events.push(event).is_err() {
        state.status.set_fatal(FatalStatus::EventQueue);
        return;
    }
    state.next_sequence = next_sequence;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonalloy_core::ProcessEventKind;

    fn state() -> (
        LiveMidiState,
        Arc<ArrayQueue<QueuedEvent>>,
        Arc<RealtimeStatus>,
    ) {
        let events = Arc::new(ArrayQueue::new(
            super::super::audio::REALTIME_EVENT_QUEUE_CAPACITY,
        ));
        let status = Arc::new(RealtimeStatus::new());
        (
            LiveMidiState::new(events.clone(), status.clone(), &[None; 128]),
            events,
            status,
        )
    }

    #[test]
    fn note_offs_match_same_key_note_ons_in_fifo_order() {
        let (mut state, events, status) = state();

        handle_message(10, &[0x90, 60, 100], &mut state);
        handle_message(20, &[0x90, 60, 110], &mut state);
        handle_message(30, &[0x80, 60, 0], &mut state);
        handle_message(40, &[0x80, 60, 0], &mut state);

        let first = events.pop().expect("first event");
        let second = events.pop().expect("second event");
        let third = events.pop().expect("third event");
        let fourth = events.pop().expect("fourth event");
        let ProcessEventKind::NoteOn {
            note_id: first_id, ..
        } = first.kind
        else {
            panic!("expected first note-on");
        };
        let ProcessEventKind::NoteOn {
            note_id: second_id, ..
        } = second.kind
        else {
            panic!("expected second note-on");
        };
        assert_ne!(first_id, second_id);
        assert_eq!(third.kind, ProcessEventKind::NoteOff { note_id: first_id });
        assert_eq!(
            fourth.kind,
            ProcessEventKind::NoteOff { note_id: second_id }
        );
        assert_eq!(first.timestamp_us, 10);
        assert_eq!(third.timestamp_us, 30);
        assert_eq!(status.fatal(), FatalStatus::None);
    }

    #[test]
    fn live_controls_are_normalized_into_core_events() {
        let (mut state, events, status) = state();

        handle_message(10, &[0xB0, SUSTAIN_PEDAL_CONTROLLER, 127], &mut state);
        handle_message(20, &[0xB0, MOD_WHEEL_CONTROLLER, 64], &mut state);
        handle_message(30, &[0xE0, 0, 64], &mut state);
        handle_message(40, &[0xD0, 32], &mut state);

        assert_eq!(
            events.pop().expect("sustain event").kind,
            ProcessEventKind::SustainPedal { down: true }
        );
        assert_eq!(
            events.pop().expect("mod wheel event").kind,
            ProcessEventKind::ModWheel {
                value: normalize_control(64)
            }
        );
        assert_eq!(
            events.pop().expect("pitch bend event").kind,
            ProcessEventKind::PitchBend {
                value: normalize_pitch_bend(0)
            }
        );
        assert_eq!(
            events.pop().expect("aftertouch event").kind,
            ProcessEventKind::Aftertouch {
                value: normalize_control(32)
            }
        );
        assert_eq!(status.fatal(), FatalStatus::None);
    }

    #[test]
    fn mapped_macro_controller_becomes_a_parameter_change() {
        let events = Arc::new(ArrayQueue::new(
            super::super::audio::REALTIME_EVENT_QUEUE_CAPACITY,
        ));
        let status = Arc::new(RealtimeStatus::new());
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/instruments/basic-poly-synth.json");
        let text = std::fs::read_to_string(&path).expect("fixture reads");
        let mut definition: sonalloy_core::InstrumentDefinition =
            serde_json::from_str(&text).expect("fixture parses");
        definition.macros.push(sonalloy_core::MacroDefinition {
            id: "motion".to_owned(),
            name: "Motion".to_owned(),
            default: 0.0,
        });
        let result = sonalloy_core::compile_instrument(
            &definition,
            &sonalloy_core::CompileContext {
                definition_base_dir: path.parent().expect("fixture directory").to_path_buf(),
                process_spec: sonalloy_core::ProcessSpec::new(48_000.0, 64, 0, 2)
                    .expect("valid spec"),
            },
        );
        let compiled = result.instrument.expect("fixture compiles");
        let parameter = compiled
            .parameter_handle("macro.motion")
            .expect("macro parameter");
        let mut macro_parameters = [None; 128];
        macro_parameters[20] = Some(parameter);
        let mut state = LiveMidiState::new(events.clone(), status.clone(), &macro_parameters);

        handle_message(10, &[0xB0, 20, 127], &mut state);

        assert_eq!(
            events.pop().expect("macro event").kind,
            ProcessEventKind::ParameterChange {
                parameter,
                normalized: 1.0,
            }
        );
        assert_eq!(status.fatal(), FatalStatus::None);
    }

    #[test]
    fn malformed_live_message_stops_midi_input() {
        let (mut state, _events, status) = state();

        handle_message(10, &[0x90, 60], &mut state);

        assert_eq!(status.fatal(), FatalStatus::Midi);
    }

    #[test]
    fn queue_overflow_keeps_existing_events_and_sets_fatal_status() {
        let events = Arc::new(ArrayQueue::new(1));
        let status = Arc::new(RealtimeStatus::new());
        let mut state = LiveMidiState::new(events.clone(), status.clone(), &[None; 128]);

        handle_message(10, &[0x90, 60, 100], &mut state);
        handle_message(20, &[0x90, 61, 100], &mut state);

        assert_eq!(status.fatal(), FatalStatus::EventQueue);
        assert!(matches!(
            events.pop().expect("first event remains queued").kind,
            ProcessEventKind::NoteOn {
                note_number: 60,
                ..
            }
        ));
        assert!(events.pop().is_none());
    }
}
