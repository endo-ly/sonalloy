use serde::Serialize;

/// Musical events and tempo/meter changes on a tick timeline.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct PerformanceEvents {
    pub(crate) name: Option<String>,
    pub(crate) ticks_per_beat: u16,
    pub(crate) length_ticks: u64,
    pub(crate) tempo_changes: Vec<TempoChange>,
    pub(crate) time_signature_changes: Vec<MeterChange>,
    pub(crate) events: Vec<MusicalEvent>,
}

/// Tempo beginning at a musical tick.
#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct TempoChange {
    pub(crate) tick: u64,
    pub(crate) bpm: f64,
}

/// Time signature beginning at a musical tick.
#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct MeterChange {
    pub(crate) tick: u64,
    pub(crate) numerator: u8,
    pub(crate) denominator: u8,
}

/// A note or performance control expressed in musical ticks.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum MusicalEvent {
    Note {
        tick: u64,
        duration_ticks: u64,
        note: u8,
        velocity: u8,
    },
    SustainPedal {
        tick: u64,
        down: bool,
    },
    PitchBend {
        tick: u64,
        value: f32,
    },
    ModWheel {
        tick: u64,
        value: f32,
    },
    Aftertouch {
        tick: u64,
        value: f32,
    },
    ParameterChange {
        tick: u64,
        parameter: String,
        native_value: f32,
    },
    ParameterRamp {
        tick: u64,
        duration_ticks: u64,
        parameter: String,
        from_value: f32,
        to_value: f32,
    },
}

impl MusicalEvent {
    /// Whether this event starts a note.
    pub(crate) const fn is_note(&self) -> bool {
        matches!(self, Self::Note { .. })
    }
}
