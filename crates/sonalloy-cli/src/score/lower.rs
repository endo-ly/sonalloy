use num_traits::ToPrimitive;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use super::time::{Quarter, ppq, rational, tick};
use super::{
    Error, MeterChange, MusicalEvent, Occurrence, PartWindow, PerformanceEvents, Result, ScoreLane,
    ScorePlan, TempoChange, array, integer, number, object, optional_int, optional_number, q,
    required, string, text_field,
};
use super::{resolve, syntax};

#[derive(Clone)]
struct Section {
    name: String,
    value: Value,
    start: Quarter,
    length: Quarter,
    bars: Vec<Quarter>,
    meter: (u8, u8),
    tonic: i32,
    scale: Option<String>,
    tonic_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Musical,
    Drum,
    Roll,
}

#[derive(Clone)]
struct Event {
    id: String,
    gain: f64,
    at: Quarter,
    duration: Quarter,
    kind: MusicalEvent,
    source: String,
    origin: Origin,
    spelling: Option<String>,
}

struct Builder<'a> {
    doc: &'a Value,
    events: Vec<Event>,
    harmony: Vec<(Quarter, Option<syntax::Chord>, i32)>,
    gate: Quarter,
    octave: i32,
    transforms: Vec<(Quarter, Value, String)>,
    part_windows: Vec<(String, Quarter, Quarter, Quarter, Quarter)>,
}

pub(super) fn lower(doc: &Value) -> Result<ScorePlan> {
    let meta = &doc["meta"];
    let bpm = number(required(meta, "bpm", "meta")?, "meta.bpm")?;
    if bpm <= 0.0 {
        return Err(Error::new("meta.bpm", "expected positive BPM"));
    }
    let meter =
        syntax::meter(text_field(meta, "meter", "meta")?).map_err(|e| e.at("meta.meter"))?;
    let gate = meta
        .get("gate")
        .map_or(Ok(q(1, 1)), |v| rational(v, "meta.gate"))?;
    if gate <= q(0, 1) || gate > q(1, 1) {
        return Err(Error::new(
            "meta.gate",
            "gate must be greater than zero and at most one",
        ));
    }
    let octave = i32::try_from(optional_int(meta, "reference_octave", 4, "meta")?)
        .map_err(|_| Error::new("meta.reference_octave", "octave is out of range"))?;
    if !(-1..=9).contains(&octave) {
        return Err(Error::new("meta.reference_octave", "octave must be -1..9"));
    }
    let (sections, start, mut tempos, meters) = resolve_form(doc, bpm, meter)?;
    if let Some(definitions) = doc.get("scale_definitions") {
        for (name, notes) in object(definitions, "scale_definitions")? {
            for note in array(notes, name)? {
                syntax::degree(string(note, name)?)
                    .map_err(|e| e.at(&format!("scale_definitions.{name}")))?;
            }
        }
    }
    if let Some(changes) = doc.get("time_changes") {
        for (i, v) in array(changes, "time_changes")?.iter().enumerate() {
            let p = format!("time_changes[{i}]");
            let at = rational(required(v, "at_quarters", &p)?, &p)?;
            let bpm = number(required(v, "bpm", &p)?, &p)?;
            if at < q(0, 1) || at >= start || bpm <= 0.0 || tempos.iter().any(|(t, _)| t == &at) {
                return Err(Error::new(
                    p,
                    "tempo change must be unique, positive, and within the score",
                ));
            }
            tempos.push((at, bpm));
        }
    }
    tempos.sort_by(|a, b| a.0.cmp(&b.0));
    let mut builder = Builder {
        doc,
        events: Vec::new(),
        harmony: Vec::new(),
        gate,
        octave,
        transforms: Vec::new(),
        part_windows: Vec::new(),
    };
    expand_declared_events(doc, &mut builder)?;
    for section in &sections {
        builder.chords(section)?;
    }
    for section in &sections {
        builder.section(section)?;
    }
    builder.resolve_rolls()?;
    for (start, value, path) in std::mem::take(&mut builder.transforms) {
        let section = sections
            .iter()
            .find(|s| s.start == start)
            .expect("resolved occurrence");
        builder.transform(section, &value, &path)?;
    }
    builder.apply_part_windows();
    builder.validate_controls()?;
    let timeline = compile_timeline(meta, &builder, &sections, &start, &tempos, &meters)?;
    let part_windows = compile_part_windows(
        std::mem::take(&mut builder.part_windows),
        timeline.ticks_per_beat,
    )?;
    let lanes = compile_lanes(builder.events, &timeline, &start)?;
    let occurrences = inspect_sections(&sections, &builder.harmony, &timeline)?;
    Ok(ScorePlan {
        source: super::Source::default(),
        title: text_field(meta, "title", "meta")?.into(),
        instruments: object(&doc["instruments"], "instruments")?.clone(),
        lanes,
        sections: occurrences,
        length_ticks: timeline.length_ticks,
        ticks_per_beat: timeline.ticks_per_beat,
        master: meta.get("master").cloned(),
        fade_out_seconds: optional_number(meta, "fade_out_seconds", 0.0, "meta")?,
        timeline,
        part_windows,
    })
}

fn compile_part_windows(
    windows: Vec<(String, Quarter, Quarter, Quarter, Quarter)>,
    ticks_per_beat: u16,
) -> Result<Vec<PartWindow>> {
    windows
        .into_iter()
        .map(
            |(instrument, section_start, section_end, window_start, window_end)| {
                Ok(PartWindow {
                    instrument,
                    section_start_tick: tick(&section_start, ticks_per_beat)?,
                    section_end_tick: tick(&section_end, ticks_per_beat)?,
                    start_tick: tick(&window_start, ticks_per_beat)?,
                    end_tick: tick(&window_end, ticks_per_beat)?,
                })
            },
        )
        .collect()
}

fn expand_declared_events(doc: &Value, builder: &mut Builder<'_>) -> Result<()> {
    if let Some(macros) = doc.get("macros") {
        for (name, value) in object(macros, "macros")? {
            let path = format!("macros.{name}");
            resolve::event_kind(&builder.macro_event(value, &path, &mut Vec::new())?, &path)?;
        }
    }
    for (name, _) in object(&doc["sections"], "sections")? {
        let section = resolve::inherit(&doc["sections"], name, false, &format!("sections.{name}"))?;
        if let Some(events) = section.get("events") {
            for (i, event) in array(events, name)?.iter().enumerate() {
                let path = format!("sections.{name}.events[{i}]");
                let event = builder.macro_event(event, &path, &mut Vec::new())?;
                resolve::event_kind(&event, &path)?;
                builder.event_targets(&event, &path)?;
            }
        }
    }
    Ok(())
}

fn event_tick(event: &MusicalEvent) -> u64 {
    match event {
        MusicalEvent::Note { tick, .. }
        | MusicalEvent::SustainPedal { tick, .. }
        | MusicalEvent::PitchBend { tick, .. }
        | MusicalEvent::ModWheel { tick, .. }
        | MusicalEvent::Aftertouch { tick, .. }
        | MusicalEvent::ParameterChange { tick, .. }
        | MusicalEvent::ParameterRamp { tick, .. } => *tick,
    }
}

type PartWindowRanges = BTreeMap<(String, Quarter, Quarter), (Quarter, Quarter)>;

fn merged_part_window_ranges(
    part_windows: &[(String, Quarter, Quarter, Quarter, Quarter)],
) -> PartWindowRanges {
    let mut ranges = PartWindowRanges::new();
    for (instrument, section_start, section_end, window_start, window_end) in part_windows {
        ranges
            .entry((
                instrument.clone(),
                section_start.clone(),
                section_end.clone(),
            ))
            .and_modify(|(start, end)| {
                *start = start.clone().max(window_start.clone());
                *end = end.clone().min(window_end.clone());
            })
            .or_insert((window_start.clone(), window_end.clone()));
    }
    ranges
}

impl Builder<'_> {
    fn apply_part_windows(&mut self) {
        let ranges = merged_part_window_ranges(&self.part_windows);
        if ranges.is_empty() {
            return;
        }

        let mut events = Vec::with_capacity(self.events.len());
        for mut event in std::mem::take(&mut self.events) {
            let event_ranges = ranges
                .iter()
                .filter(|((instrument, _, _), _)| instrument == &event.id)
                .map(
                    |((_, section_start, section_end), (window_start, window_end))| {
                        (
                            section_start.clone(),
                            section_end.clone(),
                            window_start.clone(),
                            window_end.clone(),
                        )
                    },
                )
                .collect::<Vec<_>>();
            if event_ranges.is_empty() {
                events.push(event);
                continue;
            }

            let ramp_values = match &event.kind {
                MusicalEvent::ParameterRamp {
                    from_value,
                    to_value,
                    ..
                } => Some((*from_value, *to_value)),
                _ => None,
            };
            if let Some((from_value, to_value)) = ramp_values {
                if event.duration <= q(0, 1) {
                    events.push(event);
                    continue;
                }
                let ramp_end = &event.at + &event.duration;
                for (start, end, from, to) in
                    quarter_ramp_segments(&event.at, &ramp_end, from_value, to_value, &event_ranges)
                {
                    let mut segment = event.clone();
                    segment.at = start;
                    segment.duration = &end - &segment.at;
                    if let MusicalEvent::ParameterRamp {
                        from_value,
                        to_value,
                        ..
                    } = &mut segment.kind
                    {
                        *from_value = from;
                        *to_value = to;
                    }
                    events.push(segment);
                }
                continue;
            }

            let mut keep = true;
            for (section_start, section_end, window_start, window_end) in &event_ranges {
                match &mut event.kind {
                    MusicalEvent::Note { .. } => {
                        if event.at < *section_start || event.at >= *section_end {
                            continue;
                        }
                        if event.at < *window_start || event.at >= *window_end {
                            keep = false;
                            break;
                        }
                        if &event.at + &event.duration > *window_end {
                            event.duration = window_end - &event.at;
                        }
                    }
                    _ => {
                        if event.at >= *section_start
                            && event.at < *section_end
                            && (event.at < *window_start || event.at >= *window_end)
                        {
                            keep = false;
                            break;
                        }
                    }
                }
            }
            if keep {
                events.push(event);
            }
        }
        self.events = events;
    }

    fn event_targets(&self, event: &Value, path: &str) -> Result<()> {
        for key in ["instrument", "riser"] {
            if let Some(value) = event.get(key) {
                self.instrument(string(value, &format!("{path}.{key}"))?)?;
            }
        }
        for key in ["instruments", "except_instruments"] {
            if let Some(list) = event.get(key) {
                for value in array(list, path)? {
                    self.instrument(string(value, path)?)?;
                }
            }
        }
        Ok(())
    }
    fn validate_controls(&self) -> Result<()> {
        let mut controls: std::collections::BTreeMap<(&str, u64, String), Vec<&Event>> =
            std::collections::BTreeMap::new();
        for event in &self.events {
            let key = match &event.kind {
                MusicalEvent::Note { .. } => continue,
                MusicalEvent::SustainPedal { .. } => "sustain".into(),
                MusicalEvent::PitchBend { .. } => "bend".into(),
                MusicalEvent::ModWheel { .. } => "wheel".into(),
                MusicalEvent::Aftertouch { .. } => "aftertouch".into(),
                MusicalEvent::ParameterChange { parameter, .. }
                | MusicalEvent::ParameterRamp { parameter, .. } => format!("parameter.{parameter}"),
            };
            controls
                .entry((&event.id, event.gain.to_bits(), key))
                .or_default()
                .push(event);
        }
        for events in controls.values_mut() {
            events.sort_by(|a, b| a.at.cmp(&b.at));
            for pair in events.windows(2) {
                if pair[0].at == pair[1].at || &pair[0].at + &pair[0].duration > pair[1].at {
                    return Err(Error::new(
                        &pair[1].source,
                        "conflicting controls target the same instrument and parameter",
                    ));
                }
            }
        }
        Ok(())
    }
    fn instrument(&self, id: &str) -> Result<&Value> {
        self.doc["instruments"]
            .get(id)
            .ok_or_else(|| Error::new(format!("instruments.{id}"), "undefined instrument"))
    }
    #[allow(clippy::too_many_arguments)]
    fn note(
        &mut self,
        id: &str,
        at: Quarter,
        duration: Quarter,
        note: i32,
        velocity: i64,
        gain: f64,
        path: &str,
        origin: Origin,
    ) -> Result<()> {
        if self.events.len() >= 1_000_000 {
            return Err(Error::new(
                path,
                "expanded score exceeds one million events",
            ));
        }
        let instrument = self.instrument(id)?;
        let gain =
            gain + optional_number(instrument, "gain_db", 0.0, &format!("instruments.{id}"))?;
        if !(0..=127).contains(&note)
            || !(1..=127).contains(&velocity)
            || duration <= q(0, 1)
            || !gain.is_finite()
            || crate::performance::gain_linear(gain)
                .is_none_or(|linear| linear > f64::from(f32::MAX))
        {
            return Err(Error::new(
                path,
                "expected MIDI note 0..127, velocity 1..127, positive duration, and finite gain",
            ));
        }
        self.events.push(Event {
            id: id.into(),
            gain,
            at,
            duration,
            kind: MusicalEvent::Note {
                tick: 0,
                duration_ticks: 0,
                note: u8::try_from(note).expect("range checked"),
                velocity: u8::try_from(velocity).expect("range checked"),
            },
            source: path.into(),
            origin,
            spelling: None,
        });
        Ok(())
    }
    fn trigger(
        &mut self,
        id: &str,
        at: Quarter,
        duration: Quarter,
        velocity: i64,
        path: &str,
        origin: Origin,
    ) -> Result<()> {
        let instrument_path = format!("instruments.{id}.trigger_note_midi");
        let instrument = self.instrument(id)?;
        let note = instrument.get("trigger_note_midi").ok_or_else(|| {
            Error::new(
                path,
                format!(
                    "instrument `{id}` requires `{instrument_path}` (MIDI 0..127) for a trigger at score quarter {at}"
                ),
            )
        })?;
        let note = integer(note, &instrument_path)?;
        if !(0..=127).contains(&note) {
            return Err(Error::new(
                path,
                format!(
                    "`{instrument_path}` must be MIDI 0..127 for a trigger at score quarter {at}"
                ),
            ));
        }
        self.note(
            id,
            at,
            duration,
            i32::try_from(note).map_err(|_| Error::new(path, "trigger note is out of range"))?,
            velocity,
            0.0,
            path,
            origin,
        )
    }
    fn resolve_rolls(&mut self) -> Result<()> {
        let mut roll_positions = BTreeSet::new();
        for event in &self.events {
            if event.origin == Origin::Roll
                && !roll_positions.insert((event.id.clone(), event.at.clone()))
            {
                return Err(Error::new(&event.source, "overlapping drum rolls"));
            }
        }
        self.events.retain(|event| {
            event.origin != Origin::Drum
                || !roll_positions.contains(&(event.id.clone(), event.at.clone()))
        });
        Ok(())
    }
    fn position(s: &Section, v: &Value, path: &str, allow_end: bool) -> Result<Quarter> {
        let bar = if v["bar"] == "last" {
            s.bars.len()
        } else {
            usize::try_from(integer(required(v, "bar", path)?, path)?)
                .map_err(|_| Error::new(path, "bar must be positive"))?
        };
        if bar == 0 || bar > s.bars.len() {
            return Err(Error::new(path, "bar is outside the section"));
        }
        let offset = if let Some(v) = v.get("offset_quarters") {
            rational(v, path)?
        } else {
            q(optional_int(v, "step", 1, path)? - 1, 4)
        };
        if offset < q(0, 1)
            || (offset > s.bars[bar - 1]
                || (offset == s.bars[bar - 1] && !(allow_end && bar == s.bars.len())))
        {
            return Err(Error::new(
                path,
                "step or offset must be within the selected bar",
            ));
        }
        Ok(s.bars[..bar - 1].iter().cloned().sum::<Quarter>() + offset)
    }
    fn duration(v: &Value, path: &str) -> Result<Quarter> {
        if v.get("duration_steps").is_some() && v.get("duration_quarters").is_some() {
            return Err(Error::new(
                path,
                "duration_steps and duration_quarters are mutually exclusive",
            ));
        }
        let duration = if let Some(v) = v.get("duration_quarters") {
            rational(v, path)?
        } else if let Some(v) = v.get("duration_steps") {
            rational(v, path)? / q(4, 1)
        } else {
            q(1, 4)
        };
        if duration <= q(0, 1) {
            return Err(Error::new(path, "duration must be positive"));
        }
        Ok(duration)
    }
    fn chords(&mut self, s: &Section) -> Result<()> {
        let path = format!("sections.{}.chords", s.name);
        let mut cursor = s.start.clone();
        if let Some(chords) = s.value.get("chords") {
            let chords = array(chords, &path)?;
            if chords.len() != s.bars.len() {
                return Err(Error::new(
                    path,
                    "chord list must contain one entry per bar",
                ));
            }
            for (bar, length) in chords.iter().zip(&s.bars) {
                let parts: Vec<_> = string(bar, &path)?.split_whitespace().collect();
                if parts.is_empty() || parts.len() > 2 {
                    return Err(Error::new(&path, "expected one or two chords per bar"));
                }
                let unit = length / q(i64::try_from(parts.len()).expect("length"), 1);
                for text in parts {
                    self.harmony.push((
                        cursor.clone(),
                        syntax::chord(text).map_err(|e| e.at(&path))?,
                        s.tonic,
                    ));
                    cursor += &unit;
                }
            }
        }
        if let Some(changes) = s.value.get("chord_changes") {
            for (i, v) in array(changes, &path)?.iter().enumerate() {
                let p = format!("sections.{}.chord_changes[{i}]", s.name);
                let at = &s.start + Self::position(s, required(v, "at", &p)?, &p, false)?;
                if self.harmony.iter().any(|(t, _, _)| t == &at) {
                    return Err(Error::new(p, "conflicting chord changes at the same time"));
                }
                self.harmony.push((
                    at,
                    syntax::chord(text_field(v, "chord", &p)?).map_err(|e| e.at(&p))?,
                    s.tonic,
                ));
            }
        }
        self.harmony.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(())
    }
    fn harmony_at(&self, at: &Quarter, next: bool, path: &str) -> Result<(&syntax::Chord, i32)> {
        let entry = if next {
            self.harmony.iter().find(|(t, _, _)| t > at)
        } else {
            self.harmony.iter().rev().find(|(t, _, _)| t <= at)
        };
        let (_, chord, tonic) =
            entry.ok_or_else(|| Error::new(path, "no chord exists at the requested time"))?;
        Ok((
            chord
                .as_ref()
                .ok_or_else(|| Error::new(path, "bass reference is invalid over NC"))?,
            *tonic,
        ))
    }
    fn bass_note(&self, id: &str, token: &str, at: &Quarter, path: &str) -> Result<i32> {
        let base = token.trim_end_matches(['\'', ',']);
        let suffix = &token[base.len()..];
        if suffix.len() > 12
            || !(suffix.chars().all(|c| c == '\'') || suffix.chars().all(|c| c == ','))
        {
            return Err(Error::new(path, "mixed bass octave marks"));
        }
        let (chord, tonic) = self.harmony_at(at, base == "N", path)?;
        let instrument = self.instrument(id)?;
        let floor = optional_int(instrument, "bass_floor_midi", 28, path)?;
        let ceiling = optional_int(instrument, "bass_ceiling_midi", 39, path)?;
        let pc = i64::from(tonic + chord.bass);
        let note = floor + (pc - floor).rem_euclid(12);
        if note > ceiling {
            return Err(Error::new(path, "bass range contains no matching pitch"));
        }
        let fifth = match base {
            "R" | "N" => 0,
            "R5" => 7,
            _ => return Err(Error::new(path, "unknown bass token")),
        };
        i32::try_from(
            note + fifth
                + 12 * i64::try_from(suffix.len()).unwrap_or(128)
                    * if suffix.starts_with(',') { -1 } else { 1 },
        )
        .map_err(|_| Error::new(path, "bass pitch is out of range"))
    }
    fn section(&mut self, s: &Section) -> Result<()> {
        let path = format!("sections.{}", s.name);
        let velocity = optional_int(&s.value, "velocity", 100, &path)?;
        let mut lead_intervals = Vec::new();
        if let Some(lead) = s.value.get("lead") {
            let before = self.events.len();
            self.melody(
                s,
                lead,
                &q(0, 1),
                velocity,
                0,
                0.0,
                &format!("{path}.lead"),
                true,
            )?;
            for e in &self.events[before..] {
                lead_intervals.push((e.at.clone(), &e.at + &e.duration));
            }
            if let Some(layers) = lead.get("layers") {
                for (i, layer) in array(layers, &path)?.iter().enumerate() {
                    let mut copied = lead.clone();
                    copied["instrument"] = required(layer, "instrument", &path)?.clone();
                    self.melody(
                        s,
                        &copied,
                        &q(0, 1),
                        velocity,
                        signed_int(layer, "transpose", 0, &path)?,
                        optional_number(layer, "gain_db", 0.0, &path)?,
                        &format!("{path}.lead.layers[{i}]"),
                        true,
                    )?;
                }
            }
        }
        if let Some(drums) = s.value.get("drums") {
            self.drums(s, string(drums, &path)?, &format!("{path}.drums"))?;
        }
        if let Some(bass) = s.value.get("bass") {
            self.bass(s, bass, velocity, &format!("{path}.bass"))?;
        }
        if let Some(comp) = s.value.get("comp") {
            for (id, pattern) in object(comp, &path)? {
                self.comp(
                    s,
                    id,
                    string(pattern, &path)?,
                    velocity,
                    &format!("{path}.comp.{id}"),
                )?;
            }
        }
        if let Some(pad) = s.value.get("pad") {
            self.comp(
                s,
                text_field(pad, "instrument", &path)?,
                text_field(pad, "pattern", &path)?,
                velocity,
                &format!("{path}.pad"),
            )?;
        }
        if let Some(arp) = s.value.get("arp") {
            self.arp(s, arp, velocity, &format!("{path}.arp"))?;
        }
        if let Some(counter) = s.value.get("counter") {
            self.counter(
                s,
                counter,
                velocity,
                &lead_intervals,
                &format!("{path}.counter"),
            )?;
        }
        if let Some(list) = s.value.get("placements") {
            for (i, v) in array(list, &path)?.iter().enumerate() {
                let origin = v
                    .get("$origin")
                    .and_then(Value::as_str)
                    .map_or_else(|| format!("{path}.placements[{i}]"), str::to_owned);
                self.placement(s, v, velocity, &origin)?;
            }
        }
        let mut transforms = Vec::new();
        if let Some(events) = s.value.get("events") {
            for (i, v) in array(events, &path)?.iter().enumerate() {
                let p = format!("{path}.events[{i}]");
                let v = self.macro_event(v, &p, &mut Vec::new())?;
                resolve::event_kind(&v, &p)?;
                if matches!(v["kind"].as_str(), Some("silence" | "part_window")) {
                    transforms.push((v, p));
                } else {
                    self.event(s, &v, velocity, &p)?;
                }
            }
        }
        for (v, p) in transforms {
            self.transforms.push((s.start.clone(), v, p));
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn melody(
        &mut self,
        s: &Section,
        v: &Value,
        offset: &Quarter,
        velocity: i64,
        transpose: i32,
        gain: f64,
        path: &str,
        full: bool,
    ) -> Result<()> {
        let name = text_field(v, "melody", path)?;
        let mut bars = resolve::melody(&self.doc["melodies"], name, path)?;
        if let Some(range) = v.get("slice_bars") {
            let (start, end) = bar_range(range, bars.len(), path)?;
            bars = bars[start - 1..end].to_vec();
        }
        if full && bars.len() != s.bars.len() {
            return Err(Error::new(
                path,
                "melody length must match section bars; use slice_bars for a shorter passage",
            ));
        }
        let id = text_field(v, "instrument", path)?;
        self.instrument(id)?;
        let mut cursor = q(0, 1);
        let mut cells = Vec::new();
        for (i, bar) in bars.iter().enumerate() {
            let length = s
                .bars
                .get(i)
                .cloned()
                .unwrap_or_else(|| q(i64::from(s.meter.0) * 4, i64::from(s.meter.1)));
            let mut value = bar;
            let mut shift = q(0, 1);
            while let Some(b) = value.get("resolved_bar") {
                shift += q(optional_int(value, "shift_steps", 0, path)?, 4);
                value = b;
            }
            for (at, unit, token) in syntax::grid(string(value, path)?, s.meter, &length, false)
                .map_err(|e| e.at(path))?
            {
                cells.push((&cursor + at + &shift, unit, token));
            }
            cursor += length;
        }
        let spans = spans(cells, path)?;
        let mut slice_start = q(0, 1);
        let mut slice_end = cursor.clone();
        if let Some(slice) = v.get("slice_steps") {
            slice_start = q(optional_int(slice, "start", 1, path)? - 1, 4);
            slice_end = &slice_start + q(optional_int(slice, "count", 0, path)?, 4);
            if slice_start < q(0, 1) || slice_end <= slice_start || slice_end > cursor {
                return Err(Error::new(path, "invalid slice_steps range"));
            }
        }
        let take = optional_int(v, "take_notes", i64::MAX, path)?;
        if take <= 0 {
            return Err(Error::new(path, "take_notes must be positive"));
        }
        let mut count = 0;
        for (at, duration, token) in spans {
            if at < slice_start || at >= slice_end {
                continue;
            }
            if count >= take {
                break;
            }
            count += 1;
            let duration = duration.min(&slice_end - &at) * &self.gate;
            let at = &s.start + offset + at - &slice_start;
            let pitch = (self.octave + 1) * 12
                + s.tonic
                + syntax::degree(&token).map_err(|e| e.at(path))?;
            let pitch = transposed(pitch, transpose, path)?;
            self.note(
                id,
                at,
                duration,
                pitch,
                velocity,
                gain,
                path,
                Origin::Musical,
            )?;
            self.events.last_mut().expect("emitted note").spelling = Some(token);
        }
        Ok(())
    }
    fn placement(&mut self, s: &Section, v: &Value, velocity: i64, path: &str) -> Result<()> {
        validate_placement_shape(v, path)?;
        let offset = Self::position(s, required(v, "at", path)?, path, false)?;
        let id = text_field(v, "instrument", path)?;
        let velocity = optional_int(v, "velocity", velocity, path)?;
        let transpose = signed_int(v, "transpose", 0, path)?;
        let gain = optional_number(v, "gain_db", 0.0, path)?;
        if v.get("melody").is_some() {
            return self.melody(s, v, &offset, velocity, transpose, gain, path, false);
        }
        let at = &s.start + offset;
        let duration = Self::duration(v, path)?;
        if let Some(degree) = v.get("degree") {
            let note = (self.octave + 1) * 12
                + s.tonic
                + syntax::degree(string(degree, path)?).map_err(|e| e.at(path))?;
            let note = transposed(note, transpose, path)?;
            self.note(
                id,
                at,
                duration,
                note,
                velocity,
                gain,
                path,
                Origin::Musical,
            )?;
            self.events.last_mut().expect("emitted note").spelling =
                Some(string(degree, path)?.into());
        } else if let Some(bass) = v.get("bass_note") {
            let note = transposed(
                self.bass_note(id, string(bass, path)?, &at, path)?,
                transpose,
                path,
            )?;
            self.note(
                id,
                at,
                duration,
                note,
                velocity,
                gain,
                path,
                Origin::Musical,
            )?;
        } else if let Some(chord) =
            syntax::chord(text_field(v, "chord", path)?).map_err(|e| e.at(path))?
        {
            let floor = signed_int(
                v,
                "voicing_floor_midi",
                optional_int(self.instrument(id)?, "voicing_floor_midi", 48, path)?,
                path,
            )?;
            let mut notes = syntax::voicing(&chord, s.tonic, floor);
            if let Some(root) = v.get("add_root_degree") {
                notes.push(
                    (self.octave + 1) * 12
                        + s.tonic
                        + syntax::degree(string(root, path)?).map_err(|e| e.at(path))?,
                );
            }
            for note in notes {
                self.note(
                    id,
                    at.clone(),
                    duration.clone(),
                    transposed(note, transpose, path)?,
                    velocity,
                    gain,
                    path,
                    Origin::Musical,
                )?;
            }
        }
        Ok(())
    }
    fn drums(&mut self, s: &Section, name: &str, path: &str) -> Result<()> {
        let groove = resolve::inherit(&self.doc["drums"], name, true, path)?;
        let mut cursor = s.start.clone();
        for (i, length) in s.bars.iter().enumerate() {
            let mut parts = object(required(&groove, "parts", path)?, path)?.clone();
            let mut overridden = BTreeSet::new();
            if let Some(overrides) = groove.get("overrides") {
                for v in array(overrides, path)? {
                    if condition(&v["when"], i + 1, s.bars.len(), path)? {
                        for (id, pattern) in object(&v["parts"], path)? {
                            if !overridden.insert(id.clone()) {
                                return Err(Error::new(
                                    path,
                                    "multiple overrides target the same instrument and bar",
                                ));
                            }
                            parts.insert(id.clone(), pattern.clone());
                        }
                    }
                }
            }
            for (id, pattern) in parts {
                self.instrument(&id)?;
                for (at, unit, token) in
                    syntax::grid(string(&pattern, path)?, s.meter, length, true)
                        .map_err(|e| e.at(path))?
                {
                    let velocity = match token.as_str() {
                        "." => continue,
                        "o" => 64,
                        "x" => 96,
                        "X" => 120,
                        _ => return Err(Error::new(path, "drum cells must be ., o, x, or X")),
                    };
                    self.trigger(
                        &id,
                        &cursor + at,
                        unit * &self.gate,
                        velocity,
                        path,
                        Origin::Drum,
                    )?;
                }
            }
            cursor += length;
        }
        Ok(())
    }
    fn bass(&mut self, s: &Section, v: &Value, velocity: i64, path: &str) -> Result<()> {
        let name = text_field(v, "pattern", path)?;
        let bass = self.doc["bass"]
            .get(name)
            .ok_or_else(|| Error::new(path, "undefined bass pattern"))?;
        let mut base_parts = object(required(bass, "parts", path)?, path)?.clone();
        if let Some(parts) = v.get("add_parts") {
            for (id, p) in object(parts, path)? {
                if base_parts.insert(id.clone(), p.clone()).is_some() {
                    return Err(Error::new(
                        path,
                        "add_parts must name a new bass instrument",
                    ));
                }
            }
        }
        let mut cursor = s.start.clone();
        let mut patterns: std::collections::BTreeMap<String, Vec<(Quarter, Quarter, String)>> =
            std::collections::BTreeMap::new();
        for (i, length) in s.bars.iter().enumerate() {
            let mut parts = base_parts.clone();
            let mut overridden = BTreeSet::new();
            if let Some(list) = s.value.get("bass_overrides") {
                for v in array(list, path)? {
                    if bar_number(&v["bar"], s.bars.len(), path)? == i + 1 {
                        for (id, p) in object(&v["parts"], path)? {
                            if !overridden.insert(id.clone()) {
                                return Err(Error::new(path, "overlapping bass overrides"));
                            }
                            parts.insert(id.clone(), p.clone());
                        }
                    }
                }
            }
            for (id, p) in parts {
                self.instrument(&id)?;
                let mut cells = syntax::grid(string(&p, path)?, s.meter, length, false)
                    .map_err(|e| e.at(path))?;
                for (_, _, token) in &mut cells {
                    if let Some(choice) = bass.get("choices").and_then(|v| v.get(token.as_str())) {
                        *token = text_field(
                            choice,
                            if (i + 1) % 2 == 1 { "odd" } else { "even" },
                            path,
                        )?
                        .into();
                    }
                }
                patterns.entry(id).or_default().extend(
                    cells
                        .into_iter()
                        .map(|(at, unit, token)| (&cursor + at, unit, token)),
                );
            }
            cursor += length;
        }
        for (id, cells) in patterns {
            for (at, duration, token) in spans(cells, path)? {
                let note = self.bass_note(&id, &token, &at, path)?;
                self.note(
                    &id,
                    at,
                    duration * &self.gate,
                    note,
                    velocity,
                    0.0,
                    path,
                    Origin::Musical,
                )?;
            }
        }
        Ok(())
    }
    fn comp(&mut self, s: &Section, id: &str, name: &str, velocity: i64, path: &str) -> Result<()> {
        let pattern = self.doc["comp"]
            .get(name)
            .ok_or_else(|| Error::new(path, "undefined comp pattern"))?;
        let floor = signed_int(self.instrument(id)?, "voicing_floor_midi", 48, path)?;
        let mut cursor = s.start.clone();
        for length in &s.bars {
            let cells = syntax::grid(string(pattern, path)?, s.meter, length, false)
                .map_err(|e| e.at(path))?;
            for (at, duration, token) in spans(cells, path)? {
                if token != "x" {
                    return Err(Error::new(path, "comp cells must be x, -, or ."));
                }
                let at = &cursor + at;
                let end = &at + &duration;
                let mut changes = vec![at.clone()];
                if name == "hold" {
                    changes.extend(
                        self.harmony
                            .iter()
                            .filter(|(t, _, _)| t > &at && t < &end)
                            .map(|(t, _, _)| t.clone()),
                    );
                }
                changes.push(end);
                for pair in changes.windows(2) {
                    let Some((_, Some(chord), tonic)) =
                        self.harmony.iter().rev().find(|(t, _, _)| t <= &pair[0])
                    else {
                        continue;
                    };
                    let notes = syntax::voicing(chord, *tonic, floor);
                    for note in notes {
                        self.note(
                            id,
                            pair[0].clone(),
                            (&pair[1] - &pair[0]) * &self.gate,
                            note,
                            velocity,
                            0.0,
                            path,
                            Origin::Musical,
                        )?;
                    }
                }
            }
            cursor += length;
        }
        Ok(())
    }
    fn arp(&mut self, s: &Section, v: &Value, velocity: i64, path: &str) -> Result<()> {
        let id = text_field(v, "instrument", path)?;
        let pattern = self.doc["arps"]
            .get(text_field(v, "pattern", path)?)
            .ok_or_else(|| Error::new(path, "undefined arp pattern"))?;
        let retrigger = pattern.get("retrigger_each_step").map_or(Ok(true), |v| {
            v.as_bool()
                .ok_or_else(|| Error::new(path, "retrigger_each_step must be boolean"))
        })?;
        let floor = signed_int(
            pattern,
            "voicing_floor_midi",
            optional_int(self.instrument(id)?, "voicing_floor_midi", 48, path)?,
            path,
        )?;
        let mut cursor = s.start.clone();
        let mut cells = Vec::new();
        for length in &s.bars {
            for (at, unit, token) in
                syntax::grid(text_field(pattern, "order", path)?, s.meter, length, false)
                    .map_err(|e| e.at(path))?
            {
                cells.push((&cursor + &at, unit, token));
            }
            cursor += length;
        }
        let mut pending: Vec<(Quarter, Quarter, i32)> = Vec::new();
        for (at, duration, token) in spans(cells, path)? {
            let index = token
                .parse::<usize>()
                .ok()
                .filter(|n| *n < 128)
                .ok_or_else(|| {
                    Error::new(
                        path,
                        "arp order cells must be numbers below 128, '.', or '-'",
                    )
                })?;
            let Some((_, Some(chord), tonic)) =
                self.harmony.iter().rev().find(|(t, _, _)| t <= &at)
            else {
                continue;
            };
            let notes = syntax::voicing(chord, *tonic, floor);
            let note = notes[index % notes.len()]
                + 12 * i32::try_from(index / notes.len()).expect("arp index below 128");
            if !retrigger
                && pending.last().is_some_and(|(previous, duration, pitch)| {
                    *pitch == note && previous + duration == at
                })
            {
                pending.last_mut().expect("previous note").1 += duration;
            } else {
                pending.push((at, duration, note));
            }
        }
        for (at, duration, note) in pending {
            self.note(
                id,
                at,
                duration * &self.gate,
                note,
                velocity,
                0.0,
                path,
                Origin::Musical,
            )?;
        }
        Ok(())
    }
    fn counter(
        &mut self,
        s: &Section,
        v: &Value,
        velocity: i64,
        lead: &[(Quarter, Quarter)],
        path: &str,
    ) -> Result<()> {
        let id = text_field(v, "instrument", path)?;
        let pattern = self.doc["counters"]
            .get(text_field(v, "pattern", path)?)
            .ok_or_else(|| Error::new(path, "undefined counter pattern"))?;
        if text_field(pattern, "when", path)? != "lead_silent_full_beat" {
            return Err(Error::new(path, "expected lead_silent_full_beat"));
        }
        let cell: Vec<_> = text_field(pattern, "cell", path)?
            .split_whitespace()
            .collect();
        self.instrument(id)?;
        if cell.is_empty() {
            return Err(Error::new(path, "counter cell must not be empty"));
        }
        let beat = q(4, i64::from(s.meter.1));
        let unit = &beat
            / q(
                i64::try_from(cell.len())
                    .map_err(|_| Error::new(path, "counter cell is too long"))?,
                1,
            );
        let mut at = s.start.clone();
        let end = &s.start + &s.length;
        while at < end {
            let beat_end = (&at + &beat).min(end.clone());
            if &beat_end - &at == beat && lead.iter().all(|(a, b)| b <= &at || a >= &beat_end) {
                for (i, token) in cell.iter().enumerate() {
                    let note = (self.octave + 1) * 12
                        + s.tonic
                        + syntax::degree(token).map_err(|e| e.at(path))?;
                    self.note(
                        id,
                        &at + &unit * q(i64::try_from(i).expect("bounded cell index"), 1),
                        &unit * &self.gate,
                        note,
                        velocity,
                        0.0,
                        path,
                        Origin::Musical,
                    )?;
                }
            }
            at += &beat;
        }
        Ok(())
    }
    fn macro_event(&self, v: &Value, path: &str, stack: &mut Vec<String>) -> Result<Value> {
        let Some(name) = v.get("macro") else {
            return Ok(v.clone());
        };
        let name = string(name, path)?;
        if stack.iter().any(|s| s == name) {
            return Err(Error::new(path, "cyclic macro reference"));
        }
        stack.push(name.into());
        let base = self.doc["macros"]
            .get(name)
            .ok_or_else(|| Error::new(path, "undefined macro"))?;
        let mut result = self.macro_event(base, path, stack)?;
        for (k, v) in object(v, path)? {
            if k != "macro" {
                result[k] = v.clone();
            }
        }
        stack.pop();
        Ok(result)
    }
    #[allow(clippy::too_many_lines)]
    fn event(&mut self, s: &Section, v: &Value, velocity: i64, path: &str) -> Result<()> {
        let kind = text_field(v, "kind", path)?;
        if matches!(kind, "step_roll" | "drum_roll") {
            return self.roll(s, v, path);
        }
        let offset = if let Some(at) = v.get("at") {
            Self::position(s, at, path, !matches!(kind, "hit" | "hits" | "degree"))?
        } else {
            q(0, 1)
        };
        let at = &s.start + offset;
        let duration = Self::duration(v, path)?;
        let velocity = optional_int(v, "velocity", velocity, path)?;
        if kind == "degree" {
            let mut placement = v.clone();
            placement.as_object_mut().expect("mapping").remove("kind");
            if placement.get("at").is_none() {
                placement["at"] = serde_json::json!({"bar":1,"step":1});
            }
            return self.placement(s, &placement, velocity, path);
        }
        if kind == "hit" || kind == "hits" {
            let ids = if kind == "hit" {
                vec![text_field(v, "instrument", path)?.to_owned()]
            } else {
                array(required(v, "instruments", path)?, path)?
                    .iter()
                    .map(|v| string(v, path).map(str::to_owned))
                    .collect::<Result<Vec<_>>>()?
            };
            for id in ids {
                self.trigger(
                    &id,
                    at.clone(),
                    duration.clone(),
                    velocity,
                    path,
                    Origin::Musical,
                )?;
            }
            return Ok(());
        }
        let id = text_field(v, "instrument", path)?;
        let gain = optional_number(self.instrument(id)?, "gain_db", 0.0, path)?;
        let control = |key: &str| -> Result<f32> {
            let n = number(required(v, key, path)?, path)?;
            if !(if kind == "pitch_bend" { -1.0 } else { 0.0 }..=1.0).contains(&n) {
                return Err(Error::new(
                    path,
                    "control value is outside its allowed range",
                ));
            }
            #[allow(clippy::cast_possible_truncation)]
            let value = n as f32;
            Ok(value)
        };
        let native = |key: &str| -> Result<f32> {
            let n = number(required(v, key, path)?, path)?;
            #[allow(clippy::cast_possible_truncation)]
            let n = n as f32;
            if !n.is_finite() {
                return Err(Error::new(path, "parameter value must be finite"));
            }
            Ok(n)
        };
        let event = match kind {
            "sustain_pedal" => MusicalEvent::SustainPedal {
                tick: 0,
                down: required(v, "down", path)?
                    .as_bool()
                    .ok_or_else(|| Error::new(path, "down must be boolean"))?,
            },
            "pitch_bend" => MusicalEvent::PitchBend {
                tick: 0,
                value: control("value")?,
            },
            "mod_wheel" => MusicalEvent::ModWheel {
                tick: 0,
                value: control("value")?,
            },
            "aftertouch" => MusicalEvent::Aftertouch {
                tick: 0,
                value: control("value")?,
            },
            "parameter_change" => MusicalEvent::ParameterChange {
                tick: 0,
                parameter: text_field(v, "parameter", path)?.into(),
                native_value: native("native_value")?,
            },
            "automation" => MusicalEvent::ParameterRamp {
                tick: 0,
                duration_ticks: 0,
                parameter: text_field(v, "parameter", path)?.into(),
                from_value: native("from_value")?,
                to_value: native("to_value")?,
            },
            _ => return Err(Error::new(path, "unknown event kind")),
        };
        let duration = if kind == "automation" {
            duration
        } else {
            q(0, 1)
        };
        self.events.push(Event {
            id: id.into(),
            gain,
            at,
            duration,
            kind: event,
            source: path.into(),
            origin: Origin::Musical,
            spelling: None,
        });
        Ok(())
    }
    #[allow(clippy::too_many_lines)]
    fn roll(&mut self, s: &Section, v: &Value, path: &str) -> Result<()> {
        let id = text_field(v, "instrument", path)?;
        let mut times = Vec::new();
        let start;
        if v["kind"] == "step_roll" {
            let bar = bar_number(required(v, "bar", path)?, s.bars.len(), path)?;
            let from = optional_int(v, "from_step", 1, path)?;
            let through = integer(required(v, "through_step", path)?, path)?;
            if from < 1 || through < from {
                return Err(Error::new(path, "invalid roll step range"));
            }
            start = s.bars[..bar - 1].iter().cloned().sum::<Quarter>();
            for step in from..=through {
                let at = &start + q(step - 1, 4);
                if at >= &start + &s.bars[bar - 1] {
                    return Err(Error::new(path, "roll step is outside bar"));
                }
                times.push((at, q(1, 4)));
            }
        } else {
            let bars = if v["bars"] == "all" {
                s.bars.len()
            } else {
                usize::try_from(integer(required(v, "bars", path)?, path)?)
                    .map_err(|_| Error::new(path, "invalid roll bars"))?
            };
            if bars == 0 || bars > s.bars.len() {
                return Err(Error::new(path, "roll bars exceed section"));
            }
            start = s.bars[..s.bars.len() - bars]
                .iter()
                .cloned()
                .sum::<Quarter>();
            let mut cursor = start.clone();
            for i in s.bars.len() - bars..s.bars.len() {
                let final_bar = i + 1 == s.bars.len();
                let hits = integer(
                    required(
                        v,
                        if final_bar {
                            "final_hits_per_beat"
                        } else {
                            "early_hits_per_beat"
                        },
                        path,
                    )?,
                    path,
                )?;
                let beats = if final_bar {
                    integer(required(v, "last_bar_end_beat", path)?, path)?
                } else {
                    i64::from(s.meter.0)
                };
                if !(1..=128).contains(&hits) || beats < 0 || beats > i64::from(s.meter.0) {
                    return Err(Error::new(path, "invalid roll density or end beat"));
                }
                let unit = q(4, i64::from(s.meter.1) * hits);
                for n in 0..beats * hits {
                    let at = &cursor + &unit * q(n, 1);
                    if at >= &cursor + &s.bars[i] {
                        return Err(Error::new(path, "roll exceeds partial bar"));
                    }
                    times.push((at, unit.clone()));
                }
                cursor += &s.bars[i];
            }
        }
        let from = integer(required(v, "velocity_from", path)?, path)?;
        let to = integer(required(v, "velocity_to", path)?, path)?;
        if !(1..=127).contains(&from) || !(1..=127).contains(&to) {
            return Err(Error::new(path, "roll velocities must be 1..127"));
        }
        let total = times.len().saturating_sub(1).max(1);
        for (i, (at, duration)) in times.into_iter().enumerate() {
            let velocity = (q(from, 1)
                + q(to - from, 1)
                    * q(
                        i64::try_from(i).expect("bounded roll length"),
                        i64::try_from(total).expect("bounded roll length"),
                    ))
            .round()
            .to_integer()
            .to_i64()
            .expect("velocity remains 1..127");
            self.trigger(
                id,
                &s.start + at,
                duration * &self.gate,
                velocity,
                path,
                Origin::Roll,
            )?;
        }
        if let Some(riser) = v.get("riser") {
            self.trigger(
                string(riser, path)?,
                &s.start + start,
                q(1, 4),
                100,
                path,
                Origin::Musical,
            )?;
        }
        Ok(())
    }
    fn transform(&mut self, s: &Section, v: &Value, path: &str) -> Result<()> {
        let kind = text_field(v, "kind", path)?;
        let section_end = &s.start + &s.length;
        let (id, begin, end, except) = if kind == "silence" {
            let begin = &s.start + Self::position(s, required(v, "at", path)?, path, false)?;
            let end = &begin + Self::duration(v, path)?;
            if end > section_end {
                return Err(Error::new(path, "silence exceeds section"));
            }
            let except = if let Some(v) = v.get("except_instruments") {
                array(v, path)?
                    .iter()
                    .map(|v| string(v, path).map(str::to_owned))
                    .collect::<Result<Vec<_>>>()?
            } else {
                Vec::new()
            };
            for id in &except {
                self.instrument(id)?;
            }
            (None, begin, end, except)
        } else {
            let id = text_field(v, "instrument", path)?.to_owned();
            self.instrument(&id)?;
            let (a, b) = bar_range(required(v, "bars", path)?, s.bars.len(), path)?;
            let begin = &s.start + s.bars[..a - 1].iter().cloned().sum::<Quarter>();
            let end = &s.start + s.bars[..b].iter().cloned().sum::<Quarter>();
            self.part_windows.push((
                id.clone(),
                s.start.clone(),
                section_end.clone(),
                begin.clone(),
                end.clone(),
            ));
            (Some(id), begin, end, Vec::new())
        };
        if kind == "part_window" {
            return Ok(());
        }
        self.events.retain_mut(|e| {
            if !matches!(e.kind, MusicalEvent::Note { .. })
                || &e.at + &e.duration <= s.start
                || e.at >= section_end
            {
                return true;
            }
            if let Some(id) = &id {
                if &e.id != id {
                    return true;
                }
                if e.at < begin || e.at >= end {
                    return false;
                }
                if &e.at + &e.duration > end {
                    e.duration = &end - &e.at;
                }
            } else if !except.contains(&e.id) {
                if e.at >= begin && e.at < end {
                    return false;
                }
                if e.at < begin && &e.at + &e.duration > begin {
                    e.duration = &begin - &e.at;
                }
            }
            true
        });
        Ok(())
    }
}

fn spans(
    cells: Vec<(Quarter, Quarter, String)>,
    path: &str,
) -> Result<Vec<(Quarter, Quarter, String)>> {
    let mut result: Vec<(Quarter, Quarter, String)> = Vec::new();
    let mut active = false;
    for (at, unit, token) in cells {
        match token.as_str() {
            "-" => {
                if !active {
                    return Err(Error::new(path, "tie has no preceding note"));
                }
                let last = result.last_mut().expect("active note");
                if &last.0 + &last.1 != at {
                    return Err(Error::new(path, "tie crosses an unnotated gap"));
                }
                last.1 = at + unit - &last.0;
            }
            "." => active = false,
            _ => {
                result.push((at, unit, token));
                active = true;
            }
        }
    }
    Ok(result)
}
fn bar_range(v: &Value, length: usize, path: &str) -> Result<(usize, usize)> {
    let range = array(v, path)?;
    if range.len() != 2 {
        return Err(Error::new(path, "expected [start,end] bar range"));
    }
    let a = bar_number(&range[0], length, path)?;
    let b = bar_number(&range[1], length, path)?;
    if a > b {
        return Err(Error::new(path, "bar range is reversed"));
    }
    Ok((a, b))
}
fn bar_number(v: &Value, length: usize, path: &str) -> Result<usize> {
    if v == "last" {
        return Ok(length);
    }
    let n =
        usize::try_from(integer(v, path)?).map_err(|_| Error::new(path, "bar must be positive"))?;
    if n == 0 || n > length {
        return Err(Error::new(path, "bar is outside section"));
    }
    Ok(n)
}
fn condition(v: &Value, bar: usize, length: usize, path: &str) -> Result<bool> {
    let map = object(v, path)?;
    if map.len() != 1 {
        return Err(Error::new(
            path,
            "override requires exactly one bar condition",
        ));
    }
    if let Some(odd) = v.get("odd") {
        if odd != &Value::Bool(true) {
            return Err(Error::new(path, "odd must be true"));
        }
        return Ok(bar % 2 == 1);
    }
    if let Some(every) = v.get("every") {
        let n = integer(every, path)?;
        if n <= 0 {
            return Err(Error::new(path, "every must be positive"));
        }
        return Ok(bar
            % usize::try_from(n).map_err(|_| Error::new(path, "bar condition is too large"))?
            == 0);
    }
    Ok(bar_number(required(v, "bar", path)?, length, path)? == bar)
}

fn quarter_ramp_segments(
    start: &Quarter,
    end: &Quarter,
    from: f32,
    to: f32,
    ranges: &[(Quarter, Quarter, Quarter, Quarter)],
) -> Vec<(Quarter, Quarter, f32, f32)> {
    let mut boundaries = vec![start.clone(), end.clone()];
    for (section_start, section_end, window_start, window_end) in ranges {
        boundaries.extend(
            [section_start, section_end, window_start, window_end]
                .into_iter()
                .filter(|time| start < *time && *time < end)
                .cloned(),
        );
    }
    boundaries.sort();
    boundaries.dedup();

    boundaries
        .windows(2)
        .filter_map(|interval| {
            let interval_start = &interval[0];
            let interval_end = &interval[1];
            let allowed =
                ranges
                    .iter()
                    .find_map(|(section_start, section_end, window_start, window_end)| {
                        (interval_start >= section_start && interval_start < section_end).then_some(
                            interval_start >= window_start && interval_start < window_end,
                        )
                    });
            if allowed == Some(false) {
                return None;
            }
            Some((
                interval_start.clone(),
                interval_end.clone(),
                quarter_ramp_value(start, end, interval_start, from, to),
                quarter_ramp_value(start, end, interval_end, from, to),
            ))
        })
        .collect()
}

fn quarter_ramp_value(start: &Quarter, end: &Quarter, at: &Quarter, from: f32, to: f32) -> f32 {
    if at == start {
        return from;
    }
    if at == end {
        return to;
    }
    let ratio = ((at - start) / (end - start))
        .to_f64()
        .expect("ramp interpolation ratio is between zero and one");
    (f64::from(from) + (f64::from(to) - f64::from(from)) * ratio)
        .to_f32()
        .expect("interpolated ramp value is within the f32 range")
}

fn validate_placement_shape(v: &Value, path: &str) -> Result<()> {
    let targets = ["melody", "degree", "chord", "bass_note"]
        .iter()
        .filter(|key| v.get(**key).is_some())
        .count();
    if targets != 1 {
        return Err(Error::new(
            path,
            "placement requires exactly one of melody, degree, chord, bass_note",
        ));
    }
    if v.get("melody").is_some()
        && (v.get("duration_steps").is_some() || v.get("duration_quarters").is_some())
    {
        return Err(Error::new(
            path,
            "melody placements use the source durations; use slice_steps to select a passage",
        ));
    }
    if v.get("melody").is_none()
        && (v.get("slice_steps").is_some() || v.get("take_notes").is_some())
    {
        return Err(Error::new(
            path,
            "slice_steps and take_notes require melody",
        ));
    }
    if v.get("chord").is_none()
        && (v.get("voicing_floor_midi").is_some() || v.get("add_root_degree").is_some())
    {
        return Err(Error::new(path, "voicing fields require chord"));
    }
    Ok(())
}

fn signed_int(v: &Value, key: &str, default: i64, path: &str) -> Result<i32> {
    let value = optional_int(v, key, default, path)?;
    if key == "voicing_floor_midi" && !(0..=127).contains(&value) {
        return Err(Error::new(
            format!("{path}.{key}"),
            "voicing floor must be MIDI 0..127",
        ));
    }
    i32::try_from(value).map_err(|_| {
        Error::new(
            format!("{path}.{key}"),
            "integer is outside the signed 32-bit range",
        )
    })
}

fn compile_lanes(
    events: Vec<Event>,
    timeline: &PerformanceEvents,
    end: &Quarter,
) -> Result<Vec<ScoreLane>> {
    let mut lanes: Vec<ScoreLane> = Vec::new();
    for mut event in events {
        if event.at < q(0, 1)
            || &event.at + &event.duration > *end
            || event.at > *end
            || (event.at == *end && event.duration > q(0, 1))
        {
            return Err(Error::new(
                event.source,
                "event must fit within the score timeline",
            ));
        }
        let at = tick(&event.at, timeline.ticks_per_beat)?;
        let duration = tick(&event.duration, timeline.ticks_per_beat)?;
        match &mut event.kind {
            MusicalEvent::Note {
                tick,
                duration_ticks,
                ..
            }
            | MusicalEvent::ParameterRamp {
                tick,
                duration_ticks,
                ..
            } => {
                *tick = at;
                *duration_ticks = duration;
            }
            MusicalEvent::SustainPedal { tick, .. }
            | MusicalEvent::PitchBend { tick, .. }
            | MusicalEvent::ModWheel { tick, .. }
            | MusicalEvent::Aftertouch { tick, .. }
            | MusicalEvent::ParameterChange { tick, .. } => *tick = at,
        }
        let index = lanes
            .iter()
            .position(|lane| lane.id == event.id && lane.gain_db.to_bits() == event.gain.to_bits())
            .unwrap_or_else(|| {
                lanes.push(ScoreLane {
                    id: event.id.clone(),
                    gain_db: event.gain,
                    pattern: timeline.clone(),
                    sources: Vec::new(),
                });
                lanes.len() - 1
            });
        lanes[index].pattern.events.push(event.kind);
        lanes[index].sources.push(super::EventSource {
            path: event.source,
            spelling: event.spelling,
            tick: at,
            at_quarters: event.at.to_string(),
            duration_quarters: event.duration.to_string(),
            line: 0,
            column: 0,
            section: String::new(),
            bar: 0,
            step: String::new(),
        });
    }
    for lane in &mut lanes {
        lane.pattern.events.sort_by_key(event_tick);
        lane.sources.sort_by_key(|s| s.tick);
    }
    Ok(lanes)
}

type FormTimeline = (
    Vec<Section>,
    Quarter,
    Vec<(Quarter, f64)>,
    Vec<(Quarter, (u8, u8))>,
);
#[allow(clippy::too_many_lines)]
fn resolve_form(doc: &Value, mut bpm: f64, mut meter: (u8, u8)) -> Result<FormTimeline> {
    let mut sections = Vec::new();
    let mut start = q(0, 1);
    let mut tempos = vec![(q(0, 1), bpm)];
    let mut meters = vec![(q(0, 1), meter)];
    let form = array(&doc["form"], "form")?;
    if form.is_empty() {
        return Err(Error::new("form", "form must not be empty"));
    }
    for (i, entry) in form.iter().enumerate() {
        let path = format!("form[{i}]");
        let name = text_field(entry, "section", &path)?;
        let value = resolve::inherit(&doc["sections"], name, false, &format!("sections.{name}"))?;
        let count = integer(required(entry, "bars", &path)?, &format!("{path}.bars"))?;
        if !(1..=100_000).contains(&count) {
            return Err(Error::new(&path, "bars must be 1..100000"));
        }
        if let Some(v) = entry.get("bpm") {
            bpm = number(v, &format!("{path}.bpm"))?;
            if bpm <= 0.0 {
                return Err(Error::new(&path, "expected positive BPM"));
            }
            if i == 0 {
                tempos[0].1 = bpm;
            } else {
                tempos.push((start.clone(), bpm));
            }
        }
        if let Some(v) = entry.get("meter") {
            meter = syntax::meter(string(v, &path)?).map_err(|e| e.at(&path))?;
            if i == 0 {
                meters[0].1 = meter;
            } else {
                meters.push((start.clone(), meter));
            }
        }
        let bar_length = q(i64::from(meter.0) * 4, i64::from(meter.1));
        let mut bars = vec![bar_length.clone(); usize::try_from(count).expect("positive count")];
        if let Some(last) = entry.get("last_bar_quarters") {
            let length = rational(last, &format!("{path}.last_bar_quarters"))?;
            if length <= q(0, 1) || length >= bar_length {
                return Err(Error::new(
                    &path,
                    "last_bar_quarters must be positive and shorter than a complete bar",
                ));
            }
            *bars.last_mut().expect("bars") = length;
        }
        let length: Quarter = bars.iter().cloned().sum();
        let tonic_name = text_field(entry, "tonic", &path)?.to_owned();
        let tonic = syntax::tonic(&tonic_name).map_err(|e| e.at(&format!("{path}.tonic")))?;
        let scale = entry
            .get("scale")
            .map(|_| text_field(entry, "scale", &path))
            .transpose()?
            .map(str::to_owned);
        if let Some(scale) = &scale {
            let known = [
                "ionian",
                "dorian",
                "phrygian",
                "lydian",
                "mixolydian",
                "aeolian",
                "locrian",
                "harmonic_minor",
                "melodic_minor",
                "major_pentatonic",
                "minor_pentatonic",
                "whole_tone",
            ];
            if !known.contains(&scale.as_str())
                && doc
                    .get("scale_definitions")
                    .and_then(|d| d.get(scale))
                    .is_none()
            {
                return Err(Error::new(
                    format!("{path}.scale"),
                    "unknown scale; declare it in scale_definitions",
                ));
            }
        }
        sections.push(Section {
            name: name.into(),
            value,
            start: start.clone(),
            length: length.clone(),
            bars,
            meter,
            tonic,
            scale,
            tonic_name,
        });
        start += length;
    }
    Ok((sections, start, tempos, meters))
}

fn compile_timeline(
    meta: &Value,
    builder: &Builder<'_>,
    sections: &[Section],
    start: &Quarter,
    tempos: &[(Quarter, f64)],
    meters: &[(Quarter, (u8, u8))],
) -> Result<PerformanceEvents> {
    let mut denominators = vec![start];
    for event in &builder.events {
        denominators.extend([&event.at, &event.duration]);
    }
    for (t, _) in tempos {
        denominators.push(t);
    }
    for (t, _) in meters {
        denominators.push(t);
    }
    for section in sections {
        denominators.extend([&section.start, &section.length]);
        denominators.extend(section.bars.iter());
    }
    for (t, _, _) in &builder.harmony {
        denominators.push(t);
    }
    let resolution = ppq(denominators)?;
    let length_ticks = tick(start, resolution)?;
    let tempo_changes: Vec<_> = tempos
        .iter()
        .map(|(at, bpm)| {
            Ok(TempoChange {
                tick: tick(at, resolution)?,
                bpm: *bpm,
            })
        })
        .collect::<Result<_>>()?;
    let time_signature_changes: Vec<_> = meters
        .iter()
        .map(|(at, (n, d))| {
            Ok(MeterChange {
                tick: tick(at, resolution)?,
                numerator: *n,
                denominator: *d,
            })
        })
        .collect::<Result<_>>()?;
    Ok(PerformanceEvents {
        name: Some(text_field(meta, "title", "meta")?.into()),
        ticks_per_beat: resolution,
        length_ticks,
        tempo_changes,
        time_signature_changes,
        events: Vec::new(),
    })
}

fn inspect_sections(
    sections: &[Section],
    harmony: &[(Quarter, Option<syntax::Chord>, i32)],
    timeline: &PerformanceEvents,
) -> Result<Vec<Occurrence>> {
    let resolution = timeline.ticks_per_beat;
    sections
        .iter()
        .map(|s| {
            Ok(Occurrence {
                section: s.name.clone(),
                bars: s.bars.len(),
                tonic: s.tonic_name.clone(),
                scale: s.scale.clone(),
                start_tick: tick(&s.start, resolution)?,
                end_tick: tick(&(&s.start + &s.length), resolution)?,
                bpm: timeline
                    .tempo_changes
                    .iter()
                    .rev()
                    .find(|t| t.tick <= tick(&s.start, resolution).expect("resolved section tick"))
                    .expect("initial tempo")
                    .bpm,
                meter: format!("{}/{}", s.meter.0, s.meter.1),
                chords: harmony
                    .iter()
                    .filter(|(at, _, _)| at >= &s.start && at < &(&s.start + &s.length))
                    .map(|(at, chord, _)| {
                        Ok(super::ChordChange {
                            tick: tick(at, resolution)?,
                            chord: chord
                                .as_ref()
                                .map_or_else(|| "NC".into(), |c| c.spelling.clone()),
                        })
                    })
                    .collect::<Result<_>>()?,
            })
        })
        .collect::<Result<_>>()
}

fn transposed(note: i32, semitones: i32, path: &str) -> Result<i32> {
    note.checked_add(semitones)
        .ok_or_else(|| Error::new(path, "transposed note is outside the MIDI range"))
}
