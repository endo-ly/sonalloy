use super::time::{Quarter, q};
use super::{Error, Result};

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
    pub(super) root: i32,
    pub(super) bass: i32,
    pub(super) intervals: &'static [i32],
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
    let intervals: &'static [i32] = match quality {
        "maj" => &[0, 4, 7],
        "m" => &[0, 3, 7],
        "dim" => &[0, 3, 6],
        "aug" => &[0, 4, 8],
        "maj7" => &[0, 4, 7, 11],
        "m7" => &[0, 3, 7, 10],
        "7" => &[0, 4, 7, 10],
        "m7b5" => &[0, 3, 6, 10],
        "dim7" => &[0, 3, 6, 9],
        "sus4" => &[0, 5, 7],
        "7sus4" => &[0, 5, 7, 10],
        _ => return Err(Error::new("", "unknown chord quality")),
    };
    Ok(Some(Chord {
        root,
        bass,
        intervals,
    }))
}

pub(super) fn voicing(chord: &Chord, tonic: i32, floor: i32) -> Vec<i32> {
    let bass = floor + (tonic + chord.bass - floor).rem_euclid(12);
    let mut notes = vec![bass];
    for interval in chord.intervals {
        let pitch_class = (tonic + chord.root + interval).rem_euclid(12);
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

pub(super) fn grid(
    text: &str,
    meter: (u8, u8),
    length: &Quarter,
    drums: bool,
) -> Result<Vec<(Quarter, Quarter, String)>> {
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
        for token in tokens {
            cells.push((at.clone(), unit.clone(), token));
            at += &unit;
        }
    }
    if &at != length {
        return Err(Error::new("", "grid length must equal the bar length"));
    }
    Ok(cells)
}
