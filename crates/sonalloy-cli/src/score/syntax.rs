use super::time::{Quarter, q};
use super::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pitch<'a> {
    Degree(i32),
    ChordTone(&'a str),
}

pub(super) fn pitch(token: &str) -> Result<Pitch<'_>> {
    if let Some(role) = token.strip_prefix('c') {
        voicing_role(role)?;
        Ok(Pitch::ChordTone(role))
    } else {
        degree(token).map(Pitch::Degree)
    }
}

pub(super) struct NoteToken<'a> {
    pub(super) pitch: Pitch<'a>,
    pub(super) velocity: Option<i64>,
    legato: bool,
}

fn note_parts(token: &str) -> Result<(&str, Option<i64>, bool)> {
    let legato = token.ends_with('~');
    let body = token.strip_suffix('~').unwrap_or(token);
    let (pitch, velocity) = body
        .split_once('@')
        .map_or((body, None), |(p, v)| (p, Some(v)));
    let articulation_start = pitch.find(['^', '!', '_']).unwrap_or(pitch.len());
    let marks = &pitch[articulation_start..];
    for mark in ['^', '!', '_'] {
        if marks.matches(mark).count() > 1 {
            return Err(Error::new("", "duplicate articulation"));
        }
    }
    if !marks.chars().all(|c| matches!(c, '^' | '!' | '_'))
        || (marks.contains('!') && marks.contains('_'))
        || (legato && marks.contains(['!', '_']))
    {
        return Err(Error::new("", "incompatible articulation or legato"));
    }
    let pitch = &pitch[..articulation_start];
    let velocity = velocity
        .map(|v| {
            if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::new("", "expected velocity 1..127 after @"));
            }
            v.parse::<i64>()
                .ok()
                .filter(|v| (1..=127).contains(v))
                .ok_or_else(|| Error::new("", "expected velocity 1..127 after @"))
        })
        .transpose()?;
    Ok((pitch, velocity, legato))
}

pub(super) fn note(token: &str) -> Result<NoteToken<'_>> {
    let (pitch, velocity, legato) = note_parts(token)?;
    Ok(NoteToken {
        pitch: self::pitch(pitch)?,
        velocity,
        legato,
    })
}

pub(super) fn degree(token: &str) -> Result<i32> {
    let bytes = token.as_bytes();
    let mut index = 0;
    let mut accidental = 0;
    if bytes.first() == Some(&b'b') || bytes.first() == Some(&b'#') {
        let c = bytes[0];
        while bytes.get(index) == Some(&c) {
            accidental += if c == b'b' { -1 } else { 1 };
            index += 1;
        }
        if index > 2 {
            return Err(Error::new("", "expected at most two accidentals"));
        }
    }
    let Some(n @ b'1'..=b'7') = bytes.get(index) else {
        return Err(Error::new(
            "",
            "expected degree 1..7 with optional accidentals and octave marks",
        ));
    };
    let semitone = [0, 2, 4, 5, 7, 9, 11][usize::from(*n - b'1')];
    index += 1;
    let suffix = &token[index..];
    if !(suffix.chars().all(|c| c == '\'') || suffix.chars().all(|c| c == ',')) || suffix.len() > 12
    {
        return Err(Error::new(
            "",
            "expected only apostrophes or only commas after the degree",
        ));
    }
    let octaves =
        i32::try_from(suffix.len()).unwrap_or(0) * if suffix.starts_with(',') { -1 } else { 1 };
    Ok(semitone + accidental + 12 * octaves)
}

pub(super) fn tonic(token: &str) -> Result<i32> {
    let mut chars = token.chars();
    let base: i32 = match chars.next() {
        Some('C') => 0,
        Some('D') => 2,
        Some('E') => 4,
        Some('F') => 5,
        Some('G') => 7,
        Some('A') => 9,
        Some('B') => 11,
        _ => return Err(Error::new("", "expected tonic C..B with optional b or #")),
    };
    let suffix: String = chars.collect();
    let delta = match suffix.as_str() {
        "" => 0,
        "b" => -1,
        "#" => 1,
        "bb" => -2,
        "##" => 2,
        _ => return Err(Error::new("", "invalid tonic accidental")),
    };
    Ok((base + delta).rem_euclid(12))
}

#[derive(Debug, Clone)]
pub(super) struct Chord {
    pub(super) spelling: String,
    pub(super) root: i32,
    pub(super) bass: i32,
    slash_bass: bool,
    tones: Vec<ChordTone>,
}

#[derive(Debug, Clone)]
struct ChordTone {
    degree: u8,
    accidental: i32,
    semitones: i32,
    spelling: String,
}

fn chord_tone(text: &str) -> Result<ChordTone> {
    let body = text.trim_start_matches(['b', '#']);
    let prefix = &text[..text.len() - body.len()];
    if prefix.len() > 2 || (prefix.contains('b') && prefix.contains('#')) {
        return Err(Error::new("", "invalid chord accidental"));
    }
    if body.is_empty() || body.starts_with('0') || !body.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::new("", "expected chord degree 1..13"));
    }
    let degree = body
        .parse::<u8>()
        .ok()
        .filter(|n| (1..=13).contains(n))
        .ok_or_else(|| Error::new("", "expected chord degree 1..13"))?;
    let accidental = i32::try_from(prefix.len()).expect("bounded accidental")
        * if prefix.starts_with('b') { -1 } else { 1 };
    let semitones = [0, 2, 4, 5, 7, 9, 11][usize::from((degree - 1) % 7)]
        + 12 * i32::from((degree - 1) / 7)
        + accidental;
    Ok(ChordTone {
        degree,
        accidental,
        semitones,
        spelling: text.into(),
    })
}

fn chord_quality(quality: &str) -> Result<Vec<ChordTone>> {
    let (base, modifiers) = if let Some((base, rest)) = quality.split_once('(') {
        (
            base,
            Some(
                rest.strip_suffix(')')
                    .ok_or_else(|| Error::new("", "unclosed chord modifiers"))?,
            ),
        )
    } else {
        (quality, None)
    };
    if base.is_empty() {
        let direct = modifiers.ok_or_else(|| Error::new("", "expected chord quality"))?;
        return normalize_tones(
            direct
                .split(',')
                .map(chord_tone)
                .collect::<Result<Vec<_>>>()?,
        );
    }
    let (triad, extension) = if let Some(rest) = base.strip_prefix("dim") {
        (["1", "b3", "b5"], rest)
    } else if let Some(rest) = base.strip_prefix("aug") {
        (["1", "3", "#5"], rest)
    } else if base == "sus2" {
        (["1", "2", "5"], "")
    } else if base == "sus4" || base == "7sus4" {
        (["1", "4", "5"], if base == "7sus4" { "7" } else { "" })
    } else if let Some(rest) = base.strip_prefix("maj") {
        (["1", "3", "5"], rest)
    } else if let Some(rest) = base.strip_prefix('m') {
        (["1", "b3", "5"], rest)
    } else {
        (["1", "3", "5"], base)
    };
    let mut tones = triad
        .into_iter()
        .map(chord_tone)
        .collect::<Result<Vec<_>>>()?;
    let extension = if extension == "7b5" {
        tones[2] = chord_tone("b5")?;
        "7"
    } else {
        extension
    };
    let major_seventh = base.starts_with("maj") || extension.starts_with("Maj");
    let extension = extension.strip_prefix("Maj").unwrap_or(extension);
    match extension {
        "" if base == "maj"
            || base == "m"
            || base == "dim"
            || base == "aug"
            || base.starts_with("sus") => {}
        "6" => tones.push(chord_tone("6")?),
        "add9" => tones.push(chord_tone("9")?),
        "7" | "9" | "11" | "13" => {
            tones.push(chord_tone(if base == "dim7" {
                "bb7"
            } else if major_seventh {
                "7"
            } else {
                "b7"
            })?);
            let top = extension.parse::<u8>().expect("known extension");
            for n in [9, 11, 13].into_iter().filter(|n| *n <= top) {
                tones.push(chord_tone(&n.to_string())?);
            }
        }
        _ => return Err(Error::new("", "unknown chord quality")),
    }
    if let Some(modifiers) = modifiers {
        let mut additions = Vec::new();
        let mut alterations = Vec::new();
        let mut omissions = Vec::new();
        for modifier in modifiers.split(',') {
            match modifier {
                "add9" | "add11" | "add13" => additions.push(chord_tone(&modifier[3..])?),
                "b5" | "#5" | "b9" | "#9" | "#11" | "b13" => {
                    alterations.push(chord_tone(modifier)?);
                }
                "omit3" | "omit5" | "omit7" => {
                    omissions.push(modifier[4..].parse::<u8>().expect("known omission"));
                }
                _ => return Err(Error::new("", "unknown chord modifier")),
            }
        }
        tones.extend(additions);
        for tone in &alterations {
            if omissions.contains(&tone.degree) {
                return Err(Error::new("", "chord degree is both altered and omitted"));
            }
            tones.retain(|t| t.degree != tone.degree || t.accidental != 0);
        }
        tones.extend(alterations);
        tones.retain(|tone| !omissions.contains(&tone.degree));
    }
    normalize_tones(tones)
}

fn normalize_tones(mut tones: Vec<ChordTone>) -> Result<Vec<ChordTone>> {
    tones.sort_by_key(|t| (t.degree, t.accidental));
    tones.dedup_by_key(|tone| (tone.degree, tone.accidental));
    if tones.is_empty() {
        return Err(Error::new("", "chord must contain tones"));
    }
    Ok(tones)
}

pub(super) fn chord(text: &str) -> Result<Option<Chord>> {
    if text == "NC" {
        return Ok(None);
    }
    let (body, bass) = text
        .split_once('/')
        .map_or((text, None), |(a, b)| (a, Some(b)));
    let (root, quality) = body
        .split_once(':')
        .ok_or_else(|| Error::new("", "expected degree:quality[/bass] or NC"))?;
    let root = degree(root)?;
    let slash_bass = bass.is_some();
    let bass = bass.map(degree).transpose()?.unwrap_or(root);
    let tones = chord_quality(quality)?;
    Ok(Some(Chord {
        spelling: text.into(),
        root,
        bass,
        slash_bass,
        tones,
    }))
}

pub(super) fn voicing(chord: &Chord, tonic: i32, floor: i32) -> Vec<i32> {
    let bass = floor + (tonic + chord.bass - floor).rem_euclid(12);
    let root = bass + (tonic + chord.root - bass).rem_euclid(12);
    let mut notes = vec![bass];
    for tone in &chord.tones {
        if tone.degree > 7 {
            notes.push(root + tone.semitones);
            continue;
        }
        let pitch_class = (tonic + chord.root + tone.semitones).rem_euclid(12);
        if pitch_class == bass.rem_euclid(12) {
            continue;
        }
        let note = bass + 1 + (pitch_class - bass - 1).rem_euclid(12);
        notes.push(note);
    }
    notes.sort_unstable();
    notes.dedup();
    notes
}

pub(super) fn connected_voicing(
    chord: &Chord,
    tonic: i32,
    floor: i32,
    ceiling: i32,
    previous: Option<&[i32]>,
) -> Result<Vec<i32>> {
    if ceiling <= floor {
        return Err(Error::new("", "voicing ceiling must be above floor"));
    }
    let initial = voicing(chord, tonic, floor);
    let bass = initial[0];
    if bass > ceiling {
        return Err(Error::new("", "voicing range contains no bass"));
    }
    let reference = previous.unwrap_or(&initial[1..]);
    let mut classes: Vec<_> = chord
        .tones
        .iter()
        .map(|t| (tonic + chord.root + t.semitones).rem_euclid(12))
        .filter(|pc| *pc != bass.rem_euclid(12))
        .collect();
    classes.sort_unstable();
    classes.dedup();
    let mut positions_by_class = Vec::with_capacity(classes.len());
    for pc in classes {
        let positions: Vec<_> = (bass + 1..=ceiling)
            .filter(|n| n.rem_euclid(12) == pc)
            .collect();
        if positions.is_empty() {
            return Err(Error::new(
                "",
                "voicing range contains no matching chord tone",
            ));
        }
        positions_by_class.push(positions);
    }
    let mut candidate = Vec::with_capacity(positions_by_class.len());
    let mut best = None;
    search_connected_voicings(&positions_by_class, 0, &mut candidate, reference, &mut best);
    let (_, _, mut best) = best.expect("voicing candidate");
    best.insert(0, bass);
    Ok(best)
}

fn search_connected_voicings(
    positions_by_class: &[Vec<i32>],
    index: usize,
    candidate: &mut Vec<i32>,
    reference: &[i32],
    best: &mut Option<(i32, i32, Vec<i32>)>,
) {
    if index == positions_by_class.len() {
        let distance = candidate
            .iter()
            .map(|note| {
                reference
                    .iter()
                    .map(|previous| (note - previous).abs())
                    .min()
                    .unwrap_or(0)
            })
            .sum::<i32>()
            + reference
                .iter()
                .map(|previous| {
                    candidate
                        .iter()
                        .map(|note| (previous - note).abs())
                        .min()
                        .unwrap_or(0)
                })
                .sum::<i32>();
        let highest = candidate.iter().copied().max().unwrap_or(i32::MIN);
        let better = best
            .as_ref()
            .is_none_or(|(best_distance, best_highest, best_notes)| {
                distance < *best_distance
                    || (distance == *best_distance
                        && (highest < *best_highest
                            || (highest == *best_highest
                                && lexicographically_less_unordered(candidate, best_notes))))
            });
        if better {
            let mut notes = candidate.clone();
            notes.sort_unstable();
            *best = Some((distance, highest, notes));
        }
        return;
    }

    for note in &positions_by_class[index] {
        candidate.push(*note);
        search_connected_voicings(positions_by_class, index + 1, candidate, reference, best);
        candidate.pop();
    }
}

fn lexicographically_less_unordered(candidate: &[i32], sorted: &[i32]) -> bool {
    let mut previous = None;
    for expected in sorted {
        let Some(note) = candidate
            .iter()
            .copied()
            .filter(|note| previous.is_none_or(|previous| *note > previous))
            .min()
        else {
            return false;
        };
        if note != *expected {
            return note < *expected;
        }
        previous = Some(note);
    }
    false
}

pub(super) fn explicit_voicing(
    chord: &Chord,
    tonic: i32,
    floor: i32,
    roles: &[String],
) -> Result<Vec<i32>> {
    if roles.is_empty() {
        return Err(Error::new("", "voicing must contain at least one tone"));
    }
    let bass = floor + (tonic + chord.bass - floor).rem_euclid(12);
    let root = bass + (tonic + chord.root - bass).rem_euclid(12);
    let mut notes = Vec::new();
    if chord.slash_bass {
        notes.push(bass);
    }
    for role in roles {
        let (degree, accidental, octave) = voicing_role(role)?;
        let candidates: Vec<_> = chord
            .tones
            .iter()
            .filter(|t| t.degree == degree && accidental.is_none_or(|a| t.accidental == a))
            .collect();
        let [tone] = candidates.as_slice() else {
            return Err(Error::new(
                "",
                format!(
                    "voicing tone `{role}` is absent or ambiguous in `{}`; available tones: {}",
                    chord.spelling,
                    chord
                        .tones
                        .iter()
                        .map(|tone| tone.spelling.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        };
        let note = root + tone.semitones + octave;
        if !(0..=127).contains(&note) {
            return Err(Error::new("", "voicing is outside MIDI range"));
        }
        if chord.slash_bass && note < bass {
            return Err(Error::new(
                "",
                format!(
                    "voicing tone `{role}` resolves to MIDI {note}, below the slash bass MIDI {bass}"
                ),
            ));
        }
        notes.push(note);
    }
    notes.sort_unstable();
    notes.dedup();
    Ok(notes)
}

pub(super) fn chord_tone_interval(chord: &Chord, role: &str) -> Result<i32> {
    let (degree, accidental, octave) = voicing_role(role)?;
    let candidates: Vec<_> = chord
        .tones
        .iter()
        .filter(|tone| {
            tone.degree == degree && accidental.is_none_or(|value| tone.accidental == value)
        })
        .collect();
    let [tone] = candidates.as_slice() else {
        return Err(Error::new(
            "",
            format!(
                "chord tone `{role}` is absent or ambiguous in `{}`; available tones: {}",
                chord.spelling,
                chord
                    .tones
                    .iter()
                    .map(|tone| tone.spelling.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    };
    tone.semitones
        .checked_add(octave)
        .ok_or_else(|| Error::new("", "chord tone interval is out of range"))
}

pub(super) fn voicing_role(role: &str) -> Result<(u8, Option<i32>, i32)> {
    let body = role.trim_end_matches(['\'', ',']);
    let suffix = &role[body.len()..];
    if suffix.len() > 12 || !(suffix.chars().all(|c| c == '\'') || suffix.chars().all(|c| c == ','))
    {
        return Err(Error::new("", "invalid voicing octave marks"));
    }
    let tone = chord_tone(body)?;
    let accidental = body.starts_with(['b', '#']).then_some(tone.accidental);
    let octave = i32::try_from(suffix.len()).expect("bounded octave")
        * if suffix.starts_with(',') { -12 } else { 12 };
    Ok((tone.degree, accidental, octave))
}

pub(super) fn meter(text: &str) -> Result<(u8, u8)> {
    let (n, d) = text
        .split_once('/')
        .ok_or_else(|| Error::new("", "expected numerator/denominator"))?;
    let n = n
        .parse::<u8>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| Error::new("", "meter numerator must be 1..255"))?;
    let d = d
        .parse::<u8>()
        .ok()
        .filter(|d| d.is_power_of_two())
        .ok_or_else(|| Error::new("", "meter denominator must be a power of two in 1..128"))?;
    Ok((n, d))
}

pub(super) struct GridCell {
    pub(super) at: Quarter,
    pub(super) duration: Quarter,
    pub(super) token: String,
    pub(super) token_offset: usize,
    pub(super) tuplet: Option<(Quarter, Quarter)>,
}

pub(super) fn grid(
    text: &str,
    meter: (u8, u8),
    length: &Quarter,
    drums: bool,
) -> Result<Vec<GridCell>> {
    let cells = pattern(text, meter, &[q(0, 1), length.clone()], &q(0, 1), drums)?;
    if cells.last().map(|c| &c.at + &c.duration).as_ref() != Some(length) {
        return Err(Error::new("", "grid length must equal the bar length"));
    }
    Ok(cells)
}

pub(super) fn pattern(
    text: &str,
    meter: (u8, u8),
    boundaries: &[Quarter],
    start: &Quarter,
    drums: bool,
) -> Result<Vec<GridCell>> {
    if meter.1 > 16 {
        return Err(Error::new(
            "",
            "use placements for meter denominators above 16",
        ));
    }
    PatternParser {
        text,
        boundaries,
        start,
        drums,
        interval: q(4, i64::from(meter.1)),
        bar_length: q(i64::from(meter.0) * 4, i64::from(meter.1)),
        cursor: 0,
        at: start.clone(),
        cells: Vec::new(),
    }
    .parse()
}

struct PatternParser<'a> {
    text: &'a str,
    boundaries: &'a [Quarter],
    start: &'a Quarter,
    drums: bool,
    interval: Quarter,
    bar_length: Quarter,
    cursor: usize,
    at: Quarter,
    cells: Vec<GridCell>,
}

impl PatternParser<'_> {
    fn location(&self) -> (Quarter, Quarter) {
        let bar = self
            .boundaries
            .iter()
            .rev()
            .find(|b| *b <= &self.at)
            .cloned()
            .unwrap_or_else(|| (&self.at / &self.bar_length).floor() * &self.bar_length);
        let bar = if let Some(last) = self.boundaries.last().filter(|last| self.at >= **last) {
            last + ((&self.at - last) / &self.bar_length).floor() * &self.bar_length
        } else {
            bar
        };
        let next = self
            .boundaries
            .iter()
            .find(|b| *b > &self.at)
            .cloned()
            .unwrap_or_else(|| &bar + &self.bar_length);
        (bar, next)
    }

    fn error(&self, offset: usize, message: &str) -> Error {
        let end = self.text[offset..]
            .chars()
            .next()
            .map_or(offset, |c| offset + c.len_utf8());
        Error::new("", message).with_token_location("", offset, &self.text[offset..end])
    }

    fn parse(mut self) -> Result<Vec<GridCell>> {
        let mut assertion = false;
        while self.cursor < self.text.len() {
            let ch = self.text[self.cursor..].chars().next().expect("character");
            if ch.is_whitespace() {
                self.cursor += ch.len_utf8();
                continue;
            }
            if ch == '|' {
                let offset = self.cursor;
                let barline = self.text[self.cursor..].starts_with("||");
                self.cursor += if barline { 2 } else { 1 };
                if self.cells.is_empty() || assertion || self.text[self.cursor..].trim().is_empty()
                {
                    return Err(self.error(offset, "boundary assertion must occur between tokens"));
                }
                let (bar, next) = self.location();
                let boundary = if barline {
                    self.at == bar
                } else {
                    ((&self.at - bar) / &self.interval).is_integer() || self.at == next
                };
                if !boundary {
                    return Err(self.error(
                        offset,
                        "boundary assertion is not at the requested boundary",
                    ));
                }
                assertion = true;
                continue;
            }
            assertion = false;
            if ch == '[' {
                self.tuplet()?;
            } else {
                self.token(ch)?;
            }
        }
        if self.cells.is_empty() {
            return Err(Error::new("", "pattern must contain tokens"));
        }
        Ok(self.cells)
    }

    fn tuplet(&mut self) -> Result<()> {
        let offset = self.cursor;
        let close = self.text[self.cursor + 1..]
            .find(']')
            .map(|i| self.cursor + 1 + i)
            .ok_or_else(|| self.error(offset, "unclosed tuplet"))?;
        let inner = &self.text[self.cursor + 1..close];
        if inner.contains(['[', '|']) {
            return Err(self.error(offset, "invalid tuplet content"));
        }
        let mut tokens = Vec::new();
        if self.drums {
            tokens.extend(
                inner
                    .char_indices()
                    .filter(|(_, c)| !c.is_whitespace())
                    .map(|(i, c)| (c.to_string(), self.cursor + 1 + i)),
            );
        } else {
            let mut pos = 0;
            for token in inner.split_whitespace() {
                let i = inner[pos..].find(token).expect("token") + pos;
                tokens.push((token.to_owned(), self.cursor + 1 + i));
                pos = i + token.len();
            }
        }
        if tokens.is_empty() || tokens.iter().any(|(t, _)| t.contains(':')) {
            return Err(self.error(offset, "tuplet requires tokens without lengths"));
        }
        self.cursor = close + 1;
        let duration = if self.text[self.cursor..].starts_with(':') {
            if self.drums {
                return Err(self.error(offset, "drum tuplets cannot specify a length"));
            }
            self.cursor += 1;
            let end = self.text[self.cursor..]
                .find(|c: char| c.is_whitespace() || c == '|')
                .map_or(self.text.len(), |i| self.cursor + i);
            let duration = token_length(&self.text[self.cursor..end])
                .map_err(|e| e.with_token_location("", offset, &self.text[offset..end]))?;
            self.cursor = end;
            duration
        } else {
            let (bar, next) = self.location();
            if !((&self.at - bar) / &self.interval).is_integer() {
                return Err(self.error(offset, "tuplet must start at an interval boundary"));
            }
            self.interval.clone().min(next - &self.at)
        };
        let unit = &duration / q(i64::try_from(tokens.len()).expect("token count"), 1);
        let group = &self.at - self.start;
        for (token, token_offset) in tokens {
            self.cells.push(GridCell {
                at: &self.at - self.start,
                duration: unit.clone(),
                token,
                token_offset,
                tuplet: Some((group.clone(), duration.clone())),
            });
            self.at += &unit;
        }
        Ok(())
    }

    fn token(&mut self, ch: char) -> Result<()> {
        let offset = self.cursor;
        if ch == ']' {
            return Err(self.error(offset, "unexpected closing bracket"));
        }
        let end = if self.drums {
            offset + ch.len_utf8()
        } else {
            self.text[offset..]
                .find(|c: char| c.is_whitespace() || matches!(c, '|' | '[' | ']'))
                .map_or(self.text.len(), |i| offset + i)
        };
        let raw = &self.text[offset..end];
        let legato = raw.ends_with('~');
        let body = raw.strip_suffix('~').unwrap_or(raw);
        let (token, duration) = if let Some((body, len)) = body.rsplit_once(':') {
            if self.drums {
                return Err(self.error(offset, "drum tokens cannot specify a length"));
            }
            (
                format!("{body}{}", if legato { "~" } else { "" }),
                token_length(len).map_err(|e| e.with_token_location("", offset, raw))?,
            )
        } else {
            (raw.to_owned(), q(1, 4))
        };
        self.cells.push(GridCell {
            at: &self.at - self.start,
            duration: duration.clone(),
            token,
            token_offset: offset,
            tuplet: None,
        });
        self.at += duration;
        self.cursor = end;
        Ok(())
    }
}

fn token_length(text: &str) -> Result<Quarter> {
    if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit() || c == b'/') {
        return Err(Error::new(
            "",
            "expected a positive integer or fraction token length",
        ));
    }
    let length = super::time::rational(&serde_json::Value::String(text.into()), "")?;
    if length <= q(0, 1) {
        return Err(Error::new("", "token length must be positive"));
    }
    Ok(length / q(4, 1))
}

pub(super) struct Span {
    pub(super) at: Quarter,
    pub(super) duration: Quarter,
    pub(super) token: String,
    pub(super) source_path: Option<String>,
    pub(super) token_offset: Option<usize>,
    pub(super) source_bar: Option<usize>,
    unit: Quarter,
    pub(super) shift: Quarter,
}

pub(super) fn note_spans(
    cells: Vec<(Quarter, Quarter, String)>,
    gate: &Quarter,
    end: &Quarter,
    path: &str,
) -> Result<Vec<Span>> {
    articulate(
        spans(cells, path)?,
        gate,
        end,
        path,
        &serde_json::Value::Null,
    )
}

pub(super) fn articulate(
    mut notes: Vec<Span>,
    gate: &Quarter,
    end: &Quarter,
    path: &str,
    settings: &serde_json::Value,
) -> Result<Vec<Span>> {
    let mut connections = Vec::with_capacity(notes.len());
    for span in &mut notes {
        let source_path = span.source_path.as_deref().unwrap_or(path);
        let token_error = |error: Error| {
            let error = if let Some(offset) = span.token_offset {
                error.with_token_location(source_path, offset, &span.token)
            } else {
                error.at(source_path)
            };
            error.with_score_position(span.at.clone(), span.source_bar)
        };
        let connected = note(&span.token).map_err(token_error)?.legato;
        if !connected {
            let articulation = if span.token.contains('!') {
                Some(("staccato", q(1, 2)))
            } else if span.token.contains('_') {
                Some(("tenuto", q(1, 1)))
            } else {
                None
            };
            let selected = if let Some((name, default)) = articulation {
                settings[name]
                    .get("gate")
                    .map_or(Ok(default), |v| super::time::rational(v, source_path))?
            } else {
                gate.clone()
            };
            span.duration *= selected;
        }
        connections.push(connected);
    }
    for i in 0..notes.len() {
        if connections[i] {
            let note = &notes[i];
            let source_path = note.source_path.as_deref().unwrap_or(path);
            let error = |message| {
                let error = Error::new(source_path, message);
                let error = if let Some(offset) = note.token_offset {
                    error.with_token_location(source_path, offset, &note.token)
                } else {
                    error
                };
                error.with_score_position(note.at.clone(), note.source_bar)
            };
            let next = notes
                .get(i + 1)
                .ok_or_else(|| error("legato has no following note"))?;
            if &note.at + &note.duration != next.at {
                return Err(error("legato cannot cross a rest or gap"));
            }
            let overlap = q(1, 64).min(next.unit.clone().min(next.duration.clone()) / q(4, 1));
            notes[i].duration = (&next.at + overlap).min(end.clone()) - &notes[i].at;
        }
    }
    Ok(notes)
}

#[derive(Clone)]
pub(super) struct MelodyCell {
    pub(super) at: Quarter,
    pub(super) duration: Quarter,
    pub(super) token: String,
    pub(super) source_path: String,
    pub(super) token_offset: usize,
    pub(super) source_item: usize,
    pub(super) shift: Quarter,
    pub(super) tuplet: Option<(Quarter, Quarter)>,
}

pub(super) fn spans_with_source(cells: Vec<MelodyCell>) -> Result<Vec<Span>> {
    let mut result: Vec<Span> = Vec::new();
    let mut active = false;
    for MelodyCell {
        at,
        duration: unit,
        token,
        source_path,
        token_offset,
        source_item: source_bar,
        shift,
        ..
    } in cells
    {
        let located_error = |message| {
            Error::new(&source_path, message)
                .with_token_location(&source_path, token_offset, &token)
                .with_score_position(at.clone(), Some(source_bar))
        };
        match token.as_str() {
            "-" => {
                if !active {
                    return Err(located_error("tie has no preceding note"));
                }
                let last = result.last_mut().expect("active note");
                if &last.at + &last.duration != at {
                    return Err(located_error("tie crosses an unnotated gap"));
                }
                last.duration = at + unit - &last.at;
            }
            "." => active = false,
            _ => {
                result.push(Span {
                    at,
                    duration: unit.clone(),
                    unit,
                    token,
                    source_path: Some(source_path),
                    token_offset: Some(token_offset),
                    source_bar: Some(source_bar),
                    shift,
                });
                active = true;
            }
        }
    }
    Ok(result)
}

pub(super) fn spans(cells: Vec<(Quarter, Quarter, String)>, path: &str) -> Result<Vec<Span>> {
    let mut result: Vec<Span> = Vec::new();
    let mut active = false;
    for (at, unit, token) in cells {
        match token.as_str() {
            "-" => {
                if !active {
                    return Err(Error::new(path, "tie has no preceding note"));
                }
                let last = result.last_mut().expect("active note");
                if &last.at + &last.duration != at {
                    return Err(Error::new(path, "tie crosses an unnotated gap"));
                }
                last.duration = at + unit - &last.at;
            }
            "." => active = false,
            _ => {
                result.push(Span {
                    at,
                    duration: unit.clone(),
                    unit,
                    token,
                    source_path: None,
                    token_offset: None,
                    source_bar: None,
                    shift: q(0, 1),
                });
                active = true;
            }
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_qualities_normalize_tones_and_preserve_closed_voicings() {
        for (quality, expected) in [
            ("maj", vec![0, 4, 7]),
            ("m", vec![0, 3, 7]),
            ("dim", vec![0, 3, 6]),
            ("aug", vec![0, 4, 8]),
            ("maj7", vec![0, 4, 7, 11]),
            ("m7", vec![0, 3, 7, 10]),
            ("7", vec![0, 4, 7, 10]),
            ("m7b5", vec![0, 3, 6, 10]),
            ("dim7", vec![0, 3, 6, 9]),
            ("sus4", vec![0, 5, 7]),
            ("7sus4", vec![0, 5, 7, 10]),
        ] {
            let chord = chord(&format!("1:{quality}")).unwrap().unwrap();
            assert_eq!(
                chord.tones.iter().map(|t| t.semitones).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                voicing(&chord, 0, 48),
                expected.iter().map(|n| n + 48).collect::<Vec<_>>()
            );
        }
        for (quality, expected) in [
            ("sus2", vec![0, 2, 7]),
            ("6", vec![0, 4, 7, 9]),
            ("m6", vec![0, 3, 7, 9]),
            ("mMaj7", vec![0, 3, 7, 11]),
            ("9", vec![0, 4, 7, 10, 14]),
            ("maj9", vec![0, 4, 7, 11, 14]),
            ("m9", vec![0, 3, 7, 10, 14]),
            ("11", vec![0, 4, 7, 10, 14, 17]),
            ("maj11", vec![0, 4, 7, 11, 14, 17]),
            ("m11", vec![0, 3, 7, 10, 14, 17]),
            ("13", vec![0, 4, 7, 10, 14, 17, 21]),
            ("maj13", vec![0, 4, 7, 11, 14, 17, 21]),
            ("m13", vec![0, 3, 7, 10, 14, 17, 21]),
            ("add9", vec![0, 4, 7, 14]),
            ("madd9", vec![0, 3, 7, 14]),
        ] {
            let parsed = chord(&format!("1:{quality}")).unwrap().unwrap();
            assert_eq!(
                parsed
                    .tones
                    .iter()
                    .map(|tone| tone.semitones)
                    .collect::<Vec<_>>(),
                expected,
                "{quality}"
            );
        }
        let altered = chord("1:7(b9,#9,#11,omit5,add13)").unwrap().unwrap();
        assert_eq!(
            altered
                .tones
                .iter()
                .map(|t| t.semitones)
                .collect::<Vec<_>>(),
            [0, 4, 10, 13, 15, 18, 21]
        );
        let direct = chord("1:(1,b3,5,b7,#11,bb13)").unwrap().unwrap();
        assert_eq!(direct.tones.last().unwrap().semitones, 19);
        for invalid in [
            "1:7(b5,omit5)",
            "1:7(foo)",
            "1:(14)",
            "1:()",
            "1:7(b9",
            "1:what",
            "1:(+3)",
            "1:(03)",
        ] {
            assert!(chord(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn explicit_voicings_select_chord_roles_and_keep_compound_intervals() {
        let chord = chord("1:m9/5").unwrap().unwrap();
        let roles = ["3", "7", "9'"].map(str::to_owned);
        assert_eq!(
            explicit_voicing(&chord, 0, 48, &roles).unwrap(),
            [55, 63, 70, 86]
        );
        let chord = super::chord("1:7(b9,#9)").unwrap().unwrap();
        assert!(explicit_voicing(&chord, 0, 48, &["9".into()]).is_err());
        assert_eq!(
            explicit_voicing(&chord, 0, 48, &["b9".into(), "#9".into()]).unwrap(),
            [61, 63]
        );
        assert!(explicit_voicing(&chord, 0, 120, &["#9'".into()]).is_err());
        let direct = super::chord("1:(1,b3,5,b7,#11)/5").unwrap().unwrap();
        assert_eq!(
            explicit_voicing(&direct, 0, 48, &["3".into(), "7".into(), "#11'".into()]).unwrap(),
            [55, 63, 70, 90]
        );
        let inversion = super::chord("1:9/3").unwrap().unwrap();
        assert_eq!(
            explicit_voicing(&inversion, 0, 48, &["1".into(), "3".into(), "5".into()]).unwrap(),
            [52, 60, 64, 67]
        );
        assert_eq!(
            explicit_voicing(&inversion, 0, 48, &["9,".into()]).unwrap(),
            [52, 62]
        );
        for role in ["1,", "9,,"] {
            assert!(
                explicit_voicing(&inversion, 0, 48, &[role.into()])
                    .unwrap_err()
                    .message
                    .contains("below the slash bass")
            );
        }
        let root_bass = super::chord("1:maj/1").unwrap().unwrap();
        assert!(explicit_voicing(&root_bass, 0, 48, &["1,".into()]).is_err());
    }

    #[test]
    fn automatic_voicings_preserve_simple_intervals_above_the_bass() {
        for (quality, expected) in [
            ("maj", vec![55, 60, 64]),
            ("m", vec![55, 60, 63]),
            ("dim", vec![55, 60, 63, 66]),
            ("aug", vec![55, 56, 60, 64]),
            ("maj7", vec![55, 59, 60, 64]),
            ("m7", vec![55, 58, 60, 63]),
            ("7", vec![55, 58, 60, 64]),
            ("m7b5", vec![55, 58, 60, 63, 66]),
            ("dim7", vec![55, 57, 60, 63, 66]),
            ("sus4", vec![55, 60, 65]),
            ("7sus4", vec![55, 58, 60, 65]),
        ] {
            let chord = chord(&format!("1:{quality}/5")).unwrap().unwrap();
            assert_eq!(voicing(&chord, 0, 48), expected, "{quality}");
        }
    }

    #[test]
    fn connected_voicings_minimize_distance_with_deterministic_ties() {
        let mut previous = None;
        let mut result = Vec::new();
        for symbol in ["1:maj", "4:maj", "5:maj", "1:maj"] {
            let chord = chord(symbol).unwrap().unwrap();
            let notes = connected_voicing(&chord, 0, 48, 72, previous.as_deref()).unwrap();
            previous = Some(notes[1..].to_vec());
            result.push(notes);
        }
        assert_eq!(
            result,
            [
                vec![48, 52, 55],
                vec![53, 57, 60],
                vec![55, 59, 62],
                vec![48, 55, 64]
            ]
        );
        let chord = chord("1:maj").unwrap().unwrap();
        assert_eq!(
            connected_voicing(&chord, 0, 48, 72, Some(&[58])).unwrap(),
            [48, 52, 55]
        );
        let error = connected_voicing(&chord, 0, 48, 54, None).unwrap_err();
        assert_eq!(
            error.message,
            "voicing range contains no matching chord tone"
        );
    }

    #[test]
    fn connected_voicing_handles_the_full_midi_range_with_bounded_memory() {
        let chord = chord("1:13").unwrap().unwrap();
        let result = connected_voicing(&chord, 0, 0, 127, None).unwrap();

        assert_eq!(result, [0, 4, 7, 10, 14, 17, 21]);
    }

    #[test]
    fn note_attributes_are_strict_and_legato_stays_within_the_next_note() {
        let note = note("bb3'@127~").unwrap();
        assert_eq!(
            (note.pitch, note.velocity, note.legato),
            (Pitch::Degree(14), Some(127), true)
        );
        assert_eq!(super::note("1@1").unwrap().velocity, Some(1));
        let connected = note_spans(
            vec![
                (q(0, 1), q(1, 128), "1~".into()),
                (q(1, 128), q(1, 128), "3".into()),
                (q(1, 64), q(1, 128), "-".into()),
            ],
            &q(1, 1),
            &q(3, 128),
            "line",
        )
        .unwrap();
        assert_eq!(connected[0].duration, q(5, 512));
        for token in [
            "1@0", "1@128", "1@", "1@@80", "1~@80", "1@+80", "1~~", ".@80", "-~",
        ] {
            assert!(super::note(token).is_err(), "{token}");
        }
    }
}
