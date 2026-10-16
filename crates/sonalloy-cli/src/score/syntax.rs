use super::time::{Quarter, q};
use super::{Error, Result};

pub(super) struct NoteToken<'a> {
    pub(super) pitch: i32,
    pub(super) velocity: Option<i64>,
    legato: bool,
    pub(super) spelling: &'a str,
}

pub(super) fn note(token: &str) -> Result<NoteToken<'_>> {
    let legato = token.ends_with('~');
    let body = token.strip_suffix('~').unwrap_or(token);
    let (pitch, velocity) = body
        .split_once('@')
        .map_or((body, None), |(p, v)| (p, Some(v)));
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
    Ok(NoteToken {
        pitch: degree(pitch)?,
        velocity,
        legato,
        spelling: token,
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
    let bass = bass.map(degree).transpose()?.unwrap_or(root);
    let tones = chord_quality(quality)?;
    Ok(Some(Chord {
        spelling: text.into(),
        root,
        bass,
        tones,
    }))
}

pub(super) fn voicing(chord: &Chord, tonic: i32, floor: i32) -> Vec<i32> {
    let bass = floor + (tonic + chord.bass - floor).rem_euclid(12);
    let mut notes = vec![bass];
    for tone in &chord.tones {
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
    if chord.bass.rem_euclid(12) != chord.root.rem_euclid(12) {
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
        notes.push(note);
    }
    notes.sort_unstable();
    notes.dedup();
    Ok(notes)
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
    pub(super) tuplet: Option<(Quarter, Quarter)>,
}

pub(super) fn grid(
    text: &str,
    meter: (u8, u8),
    length: &Quarter,
    drums: bool,
) -> Result<Vec<GridCell>> {
    if meter.1 > 16 {
        return Err(Error::new(
            "",
            "use placements for meter denominators above 16",
        ));
    }
    let mut cells = Vec::new();
    let mut at = q(0, 1);
    for group in text.split('|') {
        let group = group.trim();
        let tuplet = group.starts_with('[') && group.ends_with(']');
        let inner = if tuplet {
            &group[1..group.len() - 1]
        } else {
            group
        };
        let tokens: Vec<String> = if drums {
            inner
                .chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| c.to_string())
                .collect()
        } else {
            inner.split_whitespace().map(str::to_owned).collect()
        };
        if tokens.is_empty() {
            return Err(Error::new("", "grid interval must contain cells"));
        }
        let remaining = length - &at;
        let interval = q(4, i64::from(meter.1)).min(remaining);
        if interval <= q(0, 1) {
            return Err(Error::new("", "grid exceeds the bar length"));
        }
        let unit = if tuplet {
            &interval / q(i64::try_from(tokens.len()).unwrap_or(i64::MAX), 1)
        } else {
            q(1, 4)
        };
        if &unit * q(i64::try_from(tokens.len()).unwrap_or(i64::MAX), 1) != interval {
            return Err(Error::new(
                "",
                "grid cell count does not match the meter interval",
            ));
        }
        let group_start = at.clone();
        for token in tokens {
            cells.push(GridCell {
                at: at.clone(),
                duration: unit.clone(),
                token,
                tuplet: tuplet.then(|| (group_start.clone(), interval.clone())),
            });
            at += &unit;
        }
    }
    if &at != length {
        return Err(Error::new("", "grid length must equal the bar length"));
    }
    Ok(cells)
}

pub(super) struct Span {
    pub(super) at: Quarter,
    pub(super) duration: Quarter,
    pub(super) token: String,
    unit: Quarter,
}

pub(super) fn note_spans(
    cells: Vec<(Quarter, Quarter, String)>,
    gate: &Quarter,
    end: &Quarter,
    path: &str,
) -> Result<Vec<Span>> {
    articulate(spans(cells, path)?, gate, end, path)
}

pub(super) fn articulate(
    mut notes: Vec<Span>,
    gate: &Quarter,
    end: &Quarter,
    path: &str,
) -> Result<Vec<Span>> {
    let mut connections = Vec::with_capacity(notes.len());
    for span in &mut notes {
        let connected = note(&span.token).map_err(|e| e.at(path))?.legato;
        if !connected {
            span.duration *= gate;
        }
        connections.push(connected);
    }
    for i in 0..notes.len() {
        if connections[i] {
            let next = notes
                .get(i + 1)
                .ok_or_else(|| Error::new(path, "legato has no following note"))?;
            if &notes[i].at + &notes[i].duration != next.at {
                return Err(Error::new(path, "legato cannot cross a rest or gap"));
            }
            let overlap = q(1, 64).min(next.unit.clone().min(next.duration.clone()) / q(4, 1));
            notes[i].duration = (&next.at + overlap).min(end.clone()) - &notes[i].at;
        }
    }
    Ok(notes)
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
    }

    #[test]
    fn note_attributes_are_strict_and_preserve_spelling() {
        let note = note("bb3'@127~").unwrap();
        assert_eq!(
            (note.pitch, note.velocity, note.legato, note.spelling),
            (14, Some(127), true, "bb3'@127~")
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
