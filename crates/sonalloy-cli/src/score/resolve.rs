use super::{Error, Result, array, fields, integer, number, object, required, string, text_field};
use serde_json::Value;

fn validate_voicing(value: &Value, path: &str) -> Result<()> {
    if let Some(roles) = value.get("voicing") {
        let roles = array(roles, path)?;
        if roles.is_empty() {
            return Err(Error::new(path, "voicing must contain at least one tone"));
        }
        for role in roles {
            super::syntax::voicing_role(string(role, path)?).map_err(|e| e.at(path))?;
        }
    }
    Ok(())
}

pub(super) fn inherit(dict: &Value, name: &str, drums: bool, path: &str) -> Result<Value> {
    fn annotated(list: &Value, field: &str, name: &str, path: &str) -> Result<Vec<Value>> {
        Ok(array(list, path)?
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut item = item.clone();
                item.as_object_mut().expect("validated placement").insert(
                    "$origin".to_owned(),
                    Value::String(format!("sections.{name}.{field}[{index}]")),
                );
                item
            })
            .collect())
    }
    fn visit(
        dict: &Value,
        name: &str,
        drums: bool,
        path: &str,
        stack: &mut Vec<String>,
    ) -> Result<Value> {
        if stack.len() >= 128 || stack.iter().any(|s| s == name) {
            return Err(Error::new(path, format!("cyclic reference to {name}")));
        }
        let value = dict
            .get(name)
            .ok_or_else(|| Error::new(path, format!("undefined reference {name}")))?;
        stack.push(name.into());
        let mut result = if let Some(from) = value.get("from") {
            visit(dict, string(from, path)?, drums, path, stack)?
        } else {
            serde_json::json!({})
        };
        for (key, value) in object(value, path)? {
            if key == "from" || key == "add_placements" {
                continue;
            }
            if drums && key == "parts" {
                let target = result
                    .as_object_mut()
                    .expect("mapping")
                    .entry(key.clone())
                    .or_insert_with(|| serde_json::json!({}));
                for (id, part) in object(value, path)? {
                    target[id] = part.clone();
                }
            } else if drums && key == "overrides" {
                let target = result
                    .as_object_mut()
                    .expect("mapping")
                    .entry(key.clone())
                    .or_insert_with(|| serde_json::json!([]));
                target
                    .as_array_mut()
                    .ok_or_else(|| Error::new(path, "expected overrides list"))?
                    .extend(array(value, path)?.iter().cloned());
            } else if !drums && key == "placements" {
                result[key] = Value::Array(annotated(value, "placements", name, path)?);
            } else {
                result[key] = value.clone();
            }
        }
        if !drums {
            if let Some(additions) = value.get("add_placements") {
                let target = result
                    .as_object_mut()
                    .expect("mapping")
                    .entry("placements".to_owned())
                    .or_insert_with(|| serde_json::json!([]));
                target
                    .as_array_mut()
                    .ok_or_else(|| Error::new(path, "expected placements list"))?
                    .extend(annotated(additions, "add_placements", name, path)?);
            }
        }
        stack.pop();
        Ok(result)
    }
    visit(dict, name, drums, path, &mut Vec::new())
}

#[derive(Clone)]
pub(super) struct MelodyItem {
    pub(super) text: String,
    pub(super) path: String,
    pub(super) shift: super::time::Quarter,
    pub(super) source_item: usize,
    pub(super) replaced: Option<Box<MelodyItem>>,
}

pub(super) struct Melody {
    pub(super) items: Vec<MelodyItem>,
    pub(super) chord_bass: bool,
    pub(super) pickup: super::time::Quarter,
}

pub(super) fn melody(dict: &Value, name: &str, path: &str) -> Result<Melody> {
    visit_melody(dict, name, path, &mut Vec::new())
}

fn visit_melody(dict: &Value, name: &str, path: &str, stack: &mut Vec<String>) -> Result<Melody> {
    if stack.len() >= 128 || stack.iter().any(|s| s == name) {
        return Err(Error::new(path, format!("cyclic melody reference {name}")));
    }
    let mut value = dict;
    for key in name.split('.') {
        value = value
            .get(key)
            .ok_or_else(|| Error::new(path, format!("undefined melody {name}")))?;
    }
    let path = &format!("melodies.{name}");
    stack.push(name.into());
    if !value.is_array() {
        fields(
            value,
            &["items", "from", "replace", "relative_to", "pickup_steps"],
            path,
        )?;
        if value.get("items").is_some() == value.get("from").is_some() {
            return Err(Error::new(path, "expected exactly one of items or from"));
        }
        if value.get("replace").is_some() && value.get("from").is_none() {
            return Err(Error::new(path, "replace requires from"));
        }
    }
    let mut result = if let Some(from) = value.get("from") {
        visit_melody(dict, string(from, path)?, path, stack)?
    } else {
        let list = if value.is_array() {
            value
        } else {
            required(value, "items", path)?
        };
        let items = melody_items(dict, list, value.is_array(), path, stack)?;
        Melody {
            items,
            chord_bass: false,
            pickup: super::q(0, 1),
        }
    };
    if let Some(relative) = value.get("relative_to") {
        result.chord_bass = match string(relative, path)? {
            "tonic" => false,
            "chord_bass" => true,
            _ => return Err(Error::new(path, "relative_to must be tonic or chord_bass")),
        };
    }
    if let Some(pickup) = value.get("pickup_steps") {
        result.pickup = super::time::rational(pickup, path)? / super::q(4, 1);
        if result.pickup <= super::q(0, 1) {
            return Err(Error::new(path, "pickup_steps must be positive"));
        }
    }
    if let Some(replace) = value.get("replace") {
        for (key, text) in object(replace, path)? {
            let origin = format!("{path}.replace.{key}");
            let index = key
                .parse::<usize>()
                .ok()
                .filter(|i| *i > 0 && *i <= result.items.len())
                .ok_or_else(|| Error::new(&origin, "replacement item is outside melody"))?;
            let previous = result.items[index - 1].clone();
            result.items[index - 1] = MelodyItem {
                text: string(text, &origin)?.into(),
                path: origin,
                shift: previous.shift.clone(),
                source_item: index,
                replaced: Some(Box::new(previous)),
            };
        }
    }
    stack.pop();
    Ok(result)
}

fn melody_items(
    dict: &Value,
    list: &Value,
    bare: bool,
    path: &str,
    stack: &mut Vec<String>,
) -> Result<Vec<MelodyItem>> {
    let mut items = Vec::new();
    for (index, item) in array(list, path)?.iter().enumerate() {
        let origin = if bare {
            format!("{path}[{index}]")
        } else {
            format!("{path}.items[{index}]")
        };
        if let Some(text) = item.as_str() {
            items.push(MelodyItem {
                text: text.into(),
                path: origin,
                shift: super::q(0, 1),
                source_item: index + 1,
                replaced: None,
            });
        } else {
            fields(item, &["use", "items", "shift_steps"], &origin)?;
            let source = visit_melody(dict, text_field(item, "use", &origin)?, &origin, stack)?;
            let range = array(required(item, "items", &origin)?, &origin)?;
            if range.len() != 2 {
                return Err(Error::new(&origin, "items must be [start,end]"));
            }
            let start = usize::try_from(integer(&range[0], &origin)?).unwrap_or(0);
            let end = usize::try_from(integer(&range[1], &origin)?).unwrap_or(0);
            if start == 0 || end < start || end > source.items.len() {
                return Err(Error::new(&origin, "item range is outside melody"));
            }
            let shift = item
                .get("shift_steps")
                .map_or(Ok(super::q(0, 1)), |v| super::time::rational(v, &origin))?
                / super::q(4, 1);
            for mut item in source.items[start - 1..end].iter().cloned() {
                item.shift += &shift;
                items.push(item);
            }
        }
    }
    if items.is_empty() {
        return Err(Error::new(path, "melody requires at least one item"));
    }
    Ok(items)
}

#[allow(clippy::too_many_lines)]
pub(super) fn validate_structure(doc: &Value) -> Result<()> {
    fields(
        doc,
        &[
            "meta",
            "form",
            "time_changes",
            "sections",
            "melodies",
            "scale_definitions",
            "drums",
            "comp",
            "arps",
            "counters",
            "macros",
            "instruments",
            "transitions",
            "notes",
        ],
        "",
    )?;
    let meta = required(doc, "meta", "")?;
    fields(
        meta,
        &[
            "title",
            "bpm",
            "meter",
            "tuning",
            "reference_octave",
            "gate",
            "swing",
            "master",
            "fade_out_seconds",
        ],
        "meta",
    )?;
    if let Some(value) = meta.get("swing") {
        super::time::Swing::parse(value, "meta.swing")?;
    }
    if let Some(fade) = meta.get("fade_out_seconds") {
        if number(fade, "meta.fade_out_seconds")? < 0.0 {
            return Err(Error::new(
                "meta.fade_out_seconds",
                "fade must be nonnegative",
            ));
        }
    }
    for key in ["title", "meter", "tuning"] {
        text_field(meta, key, "meta")?;
    }
    if text_field(meta, "tuning", "meta")? != "12edo" {
        return Err(Error::new("meta.tuning", "expected 12edo"));
    }
    if let Some(master) = meta.get("master") {
        fields(
            master,
            &["target_lufs", "max_true_peak_db", "target_lra_lu"],
            "meta.master",
        )?;
        for (key, min, max) in [
            ("target_lufs", -70.0, -5.0),
            ("max_true_peak_db", -9.0, 0.0),
        ] {
            let path = format!("meta.master.{key}");
            let value = number(required(master, key, "meta.master")?, &path)?;
            if !(min..=max).contains(&value) {
                return Err(Error::new(path, format!("expected {min}..{max}")));
            }
        }
        if let Some(lra) = master.get("target_lra_lu") {
            if number(lra, "meta.master.target_lra_lu")? < 0.0 {
                return Err(Error::new(
                    "meta.master.target_lra_lu",
                    "expected nonnegative LRA target",
                ));
            }
        }
    }
    for (i, entry) in array(required(doc, "form", "")?, "form")?
        .iter()
        .enumerate()
    {
        fields(
            entry,
            &[
                "section",
                "bars",
                "tonic",
                "scale",
                "bpm",
                "meter",
                "last_bar_quarters",
                "swing",
            ],
            &format!("form[{i}]"),
        )?;
    }
    let sections = required(doc, "sections", "")?;
    validate_sections(sections)?;
    validate_part_gains(sections, required(doc, "instruments", "")?)?;
    validate_instruments(required(doc, "instruments", "")?)?;
    validate_materials(doc)?;
    if let Some(notes) = doc.get("notes") {
        string(notes, "notes")?;
    }
    if let Some(comp) = doc.get("comp") {
        for (name, value) in object(comp, "comp")? {
            if name == "hold" {
                return Err(Error::new("comp.hold", "hold is a reserved comp pattern"));
            }
            string(value, &format!("comp.{name}"))?;
        }
    }
    if let Some(melodies) = doc.get("melodies") {
        validate_melodies(melodies, melodies, "")?;
    }
    for (name, _) in object(sections, "sections")? {
        let path = format!("sections.{name}");
        let section = inherit(sections, name, false, &path)?;
        validate_section_references(doc, &section, &path)?;
    }
    if let Some(drums) = doc.get("drums") {
        for (name, _) in object(drums, "drums")? {
            inherit(drums, name, true, &format!("drums.{name}"))?;
        }
    }
    Ok(())
}

fn validate_melodies(root: &Value, node: &Value, prefix: &str) -> Result<()> {
    for (name, value) in object(node, "melodies")? {
        let name = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        if value.is_array() || value.get("from").is_some() || value.get("items").is_some() {
            melody(root, &name, &format!("melodies.{name}"))?;
        } else {
            validate_melodies(root, value, &name)?;
        }
    }
    Ok(())
}

fn position(v: &Value, path: &str) -> Result<()> {
    fields(v, &["bar", "step", "offset_quarters"], path)?;
    if v.get("step").is_some() && v.get("offset_quarters").is_some() {
        return Err(Error::new(
            path,
            "step and offset_quarters are mutually exclusive",
        ));
    }
    Ok(())
}

fn event(v: &Value, path: &str) -> Result<()> {
    fields(
        v,
        &[
            "kind",
            "macro",
            "at",
            "instrument",
            "instruments",
            "duration_steps",
            "duration_quarters",
            "parameter",
            "control",
            "from",
            "to",
            "native_value",
            "from_value",
            "to_value",
            "down",
            "value",
            "degree",
            "except_instruments",
            "bars",
            "bar",
            "from_step",
            "through_step",
            "velocity_from",
            "velocity_to",
            "early_hits_per_beat",
            "final_hits_per_beat",
            "last_bar_end_beat",
            "riser",
            "velocity",
        ],
        path,
    )?;
    if v.get("kind").is_some() == v.get("macro").is_some() {
        return Err(Error::new(path, "expected exactly one of kind or macro"));
    }
    if let Some(at) = v.get("at") {
        position(at, &format!("{path}.at"))?;
    }
    Ok(())
}

pub(super) fn event_kind(v: &Value, path: &str) -> Result<()> {
    let kind = text_field(v, "kind", path)?;
    let specific: &[&str] = match kind {
        "hit" => &["instrument", "velocity"],
        "hits" => &["instruments", "velocity"],
        "degree" => &["instrument", "degree", "velocity"],
        "silence" => &["except_instruments"],
        "part_window" => &["instrument", "bars"],
        "dynamics" => &["instrument", "from", "to"],
        "step_roll" => &[
            "instrument",
            "bar",
            "from_step",
            "through_step",
            "velocity_from",
            "velocity_to",
        ],
        "drum_roll" => &[
            "instrument",
            "bars",
            "early_hits_per_beat",
            "final_hits_per_beat",
            "last_bar_end_beat",
            "velocity_from",
            "velocity_to",
            "riser",
        ],
        "sustain_pedal" => &["instrument", "down"],
        "pitch_bend" | "mod_wheel" | "aftertouch" => &["instrument", "value"],
        "parameter_change" => &["instrument", "parameter", "native_value"],
        "automation" => &[
            "instrument",
            "parameter",
            "control",
            "from_value",
            "to_value",
        ],
        _ => return Err(Error::new(format!("{path}.kind"), "unknown event kind")),
    };
    if kind == "automation" && v.get("parameter").is_some() == v.get("control").is_some() {
        return Err(Error::new(
            path,
            "automation requires exactly one of parameter or control",
        ));
    }
    if let Some(control) = v.get("control") {
        if !["mod_wheel", "aftertouch", "pitch_bend"].contains(&string(control, path)?) {
            return Err(Error::new(path, "unknown automation control"));
        }
    }
    if kind == "dynamics" {
        for key in ["from", "to"] {
            if !(-126..=126).contains(&integer(required(v, key, path)?, path)?) {
                return Err(Error::new(path, "dynamics values must be -126..126"));
            }
        }
    }
    let mut allowed = vec!["kind"];
    allowed.extend_from_slice(specific);
    for key in specific {
        if !(matches!(*key, "velocity" | "riser" | "except_instruments")
            || kind == "automation" && matches!(*key, "parameter" | "control"))
        {
            required(v, key, path)?;
        }
    }
    if !matches!(kind, "part_window" | "step_roll" | "drum_roll") {
        allowed.push("at");
        required(v, "at", path)?;
    }
    if matches!(
        kind,
        "hit" | "hits" | "degree" | "silence" | "automation" | "dynamics"
    ) {
        allowed.extend(["duration_steps", "duration_quarters"]);
    }
    fields(v, &allowed, path)?;
    if kind == "hits" && array(required(v, "instruments", path)?, path)?.is_empty() {
        return Err(Error::new(path, "hits requires at least one instrument"));
    }
    validate_event_values(v, kind, path)?;
    if matches!(kind, "silence" | "automation" | "dynamics")
        && v.get("duration_steps").is_none()
        && v.get("duration_quarters").is_none()
    {
        return Err(Error::new(
            path,
            "duration_steps or duration_quarters is required",
        ));
    }
    Ok(())
}

fn validate_event_values(v: &Value, kind: &str, path: &str) -> Result<()> {
    if let Some(down) = v.get("down") {
        if !down.is_boolean() {
            return Err(Error::new(format!("{path}.down"), "expected a boolean"));
        }
    }
    for key in ["native_value", "from_value", "to_value"] {
        if let Some(value) = v.get(key) {
            number(value, &format!("{path}.{key}"))?;
        }
    }
    if let Some(value) = v.get("value") {
        let value = number(value, &format!("{path}.value"))?;
        let min = if kind == "pitch_bend" { -1.0 } else { 0.0 };
        if !(min..=1.0).contains(&value) {
            return Err(Error::new(
                format!("{path}.value"),
                format!("expected {min}..1"),
            ));
        }
    }
    Ok(())
}

fn validate_part_gains(sections: &Value, instruments: &Value) -> Result<()> {
    for (name, _) in object(sections, "sections")? {
        let path = format!("sections.{name}");
        let section = inherit(sections, name, false, &path)?;
        let Some(gains) = section.get("part_gains") else {
            continue;
        };
        for (id, value) in object(gains, &format!("{path}.part_gains"))? {
            if instruments.get(id).is_none() {
                return Err(Error::new(
                    format!("{path}.part_gains.{id}"),
                    "part_gains key must name an instrument",
                ));
            }
            number(value, &format!("{path}.part_gains.{id}"))?;
        }
    }
    Ok(())
}

fn validate_sections(sections: &Value) -> Result<()> {
    for (name, section) in object(sections, "sections")? {
        let path = format!("sections.{name}");
        if let Some(velocity) = section.get("velocity") {
            if !(1..=127).contains(&integer(velocity, &format!("{path}.velocity"))?) {
                return Err(Error::new(
                    format!("{path}.velocity"),
                    "expected velocity 1..127",
                ));
            }
        }
        fields(
            section,
            &[
                "from",
                "chords",
                "chord_changes",
                "lines",
                "drums",
                "comp",
                "pad",
                "arp",
                "counter",
                "placements",
                "add_placements",
                "events",
                "velocity",
                "part_gains",
            ],
            &path,
        )?;
        for field in ["pad", "arp", "counter"] {
            if let Some(v) = section.get(field) {
                let allowed = if field == "counter" {
                    vec!["instrument", "pattern", "follows"]
                } else {
                    vec!["instrument", "pattern", "voicing"]
                };
                fields(v, &allowed, &format!("{path}.{field}"))?;
                validate_voicing(v, &format!("{path}.{field}"))?;
            }
        }
        validate_lines(section, &path)?;
        validate_placements(section, &path)?;
        if let Some(events) = section.get("events") {
            for (i, e) in array(events, &path)?.iter().enumerate() {
                event(e, &format!("{path}.events[{i}]"))?;
            }
        }
        if let Some(changes) = section.get("chord_changes") {
            for (i, e) in array(changes, &path)?.iter().enumerate() {
                let p = format!("{path}.chord_changes[{i}]");
                fields(e, &["at", "chord"], &p)?;
                position(required(e, "at", &p)?, &format!("{p}.at"))?;
            }
        }
    }
    Ok(())
}

fn validate_placements(section: &Value, path: &str) -> Result<()> {
    for field in ["placements", "add_placements"] {
        if let Some(list) = section.get(field) {
            for (i, item) in array(list, path)?.iter().enumerate() {
                let p = format!("{path}.{field}[{i}]");
                fields(
                    item,
                    &[
                        "instrument",
                        "at",
                        "melody",
                        "degree",
                        "chord",
                        "register",
                        "tonic",
                        "velocity",
                        "duration_steps",
                        "duration_quarters",
                        "transpose",
                        "gain_db",
                        "mark",
                        "slice_steps",
                        "take_notes",
                        "voicing_floor_midi",
                        "voicing",
                        "add_root_degree",
                    ],
                    &p,
                )?;
                validate_voicing(item, &p)?;
                validate_pitch_settings(item, &p)?;
                position(required(item, "at", &p)?, &format!("{p}.at"))?;
                if let Some(slice) = item.get("slice_steps") {
                    fields(slice, &["start", "count"], &format!("{p}.slice_steps"))?;
                }
            }
        }
    }
    Ok(())
}

fn validate_lines(section: &Value, path: &str) -> Result<()> {
    if let Some(lines) = section.get("lines") {
        for (name, line) in object(lines, path)? {
            let path = format!("{path}.lines.{name}");
            fields(
                line,
                &[
                    "instrument",
                    "melody",
                    "slice_items",
                    "loop",
                    "register",
                    "tonic",
                    "transpose",
                    "gain_db",
                ],
                &path,
            )?;
            validate_pitch_settings(line, &path)?;
            if let Some(value) = line.get("loop") {
                if !value.is_boolean() {
                    return Err(Error::new(&path, "loop must be boolean"));
                }
            }
        }
    }
    Ok(())
}
fn validate_pitch_settings(value: &Value, path: &str) -> Result<()> {
    if let Some(register) = value.get("register") {
        if !["melody", "bass"].contains(&string(register, path)?) {
            return Err(Error::new(
                format!("{path}.register"),
                "register must be melody or bass",
            ));
        }
    }
    if let Some(tonic) = value.get("tonic") {
        super::syntax::tonic(string(tonic, path)?).map_err(|e| e.at(&format!("{path}.tonic")))?;
    }
    Ok(())
}

fn validate_articulations(v: &Value, path: &str) -> Result<()> {
    if let Some(articulations) = v.get("articulations") {
        fields(
            articulations,
            &["accent", "staccato", "tenuto"],
            &format!("{path}.articulations"),
        )?;
        for (name, settings) in object(articulations, path)? {
            let p = format!("{path}.articulations.{name}");
            fields(
                settings,
                if name == "accent" {
                    &["velocity"]
                } else {
                    &["gate", "velocity"]
                },
                &p,
            )?;
            if let Some(value) = settings.get("velocity") {
                if !(-126..=126).contains(&integer(value, &p)?) {
                    return Err(Error::new(&p, "articulation velocity must be -126..126"));
                }
            }
            if let Some(value) = settings.get("gate") {
                let gate = super::time::rational(value, &p)?;
                if gate <= super::q(0, 1) || gate > super::q(1, 1) {
                    return Err(Error::new(
                        &p,
                        "gate must be greater than zero and at most one",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_instruments(instruments: &Value) -> Result<()> {
    if object(instruments, "instruments")?.is_empty() {
        return Err(Error::new(
            "instruments",
            "at least one instrument is required",
        ));
    }
    for (name, v) in object(instruments, "instruments")? {
        fields(
            v,
            &[
                "preset",
                "definition",
                "external",
                "gain_db",
                "pan",
                "role",
                "voicing_floor_midi",
                "voicing_ceiling_midi",
                "articulations",
                "swing",
                "timing_offset_steps",
                "voicing",
                "gate",
                "trigger_note_midi",
                "bass_floor_midi",
                "bass_ceiling_midi",
                "audio_input",
            ],
            &format!("instruments.{name}"),
        )?;
        let count = ["preset", "definition", "external"]
            .iter()
            .filter(|key| v.get(**key).is_some())
            .count();
        if count != 1 || v.get("external").is_some_and(|v| v != &Value::Bool(true)) {
            return Err(Error::new(
                format!("instruments.{name}"),
                "expected exactly one of preset, definition, or external: true",
            ));
        }
        let path = format!("instruments.{name}");
        if let Some(value) = v.get("swing") {
            super::time::Swing::parse(value, &format!("{path}.swing"))?;
        }
        if let Some(value) = v.get("timing_offset_steps") {
            super::time::rational(value, &format!("{path}.timing_offset_steps"))?;
        }
        validate_articulations(v, &path)?;
        validate_voicing(v, &path)?;
        if let Some(value) = v.get("gate") {
            let gate = super::time::rational(value, &format!("{path}.gate"))?;
            if gate <= super::q(0, 1) || gate > super::q(1, 1) {
                return Err(Error::new(
                    format!("{path}.gate"),
                    "gate must be greater than zero and at most one",
                ));
            }
        }
        for key in ["preset", "definition", "role", "audio_input"] {
            if let Some(value) = v.get(key) {
                string(value, &format!("{path}.{key}"))?;
            }
        }
        if let Some(value) = v.get("gain_db") {
            number(value, &format!("{path}.gain_db"))?;
        }
        if let Some(value) = v.get("pan") {
            let pan = number(value, &format!("{path}.pan"))?;
            if !(-1.0..=1.0).contains(&pan) {
                return Err(Error::new(format!("{path}.pan"), "expected pan -1..1"));
            }
        }
        for key in [
            "voicing_floor_midi",
            "voicing_ceiling_midi",
            "trigger_note_midi",
            "bass_floor_midi",
            "bass_ceiling_midi",
        ] {
            if let Some(value) = v.get(key) {
                if !(0..=127).contains(&integer(value, &format!("{path}.{key}"))?) {
                    return Err(Error::new(
                        format!("{path}.{key}"),
                        "expected MIDI note 0..127",
                    ));
                }
            }
        }
        validate_voicing_range(v, &path)?;
        if super::optional_int(v, "bass_floor_midi", 28, &path)?
            > super::optional_int(v, "bass_ceiling_midi", 39, &path)?
        {
            return Err(Error::new(path, "bass floor must not exceed bass ceiling"));
        }
    }
    Ok(())
}
fn validate_voicing_range(v: &Value, path: &str) -> Result<()> {
    if let Some(ceiling) = v.get("voicing_ceiling_midi") {
        if integer(ceiling, path)? <= super::optional_int(v, "voicing_floor_midi", 48, path)? {
            return Err(Error::new(
                format!("{path}.voicing_ceiling_midi"),
                "voicing ceiling must be above floor",
            ));
        }
    }
    Ok(())
}

fn validate_materials(doc: &Value) -> Result<()> {
    for (key, allowed) in [
        (
            "arps",
            vec!["order", "voicing_floor_midi", "retrigger_each_step"],
        ),
        ("counters", vec!["when", "cell"]),
    ] {
        if let Some(dict) = doc.get(key) {
            for (name, v) in object(dict, key)? {
                let path = format!("{key}.{name}");
                fields(v, &allowed, &path)?;
                validate_material(key, v, &path)?;
            }
        }
    }
    if let Some(dict) = doc.get("drums") {
        for (name, v) in object(dict, "drums")? {
            let p = format!("drums.{name}");
            fields(v, &["from", "parts", "overrides"], &p)?;
            let resolved = inherit(dict, name, true, &p)?;
            validate_parts(doc, required(&resolved, "parts", &p)?, &p, true)?;
            if let Some(list) = v.get("overrides") {
                for (i, v) in array(list, &p)?.iter().enumerate() {
                    let p = format!("{p}.overrides[{i}]");
                    fields(v, &["when", "parts"], &p)?;
                    validate_parts(doc, required(v, "parts", &p)?, &p, false)?;
                    validate_bar_condition(required(v, "when", &p)?, &format!("{p}.when"))?;
                }
            }
        }
    }
    if let Some(macros) = doc.get("macros") {
        for (name, v) in object(macros, "macros")? {
            event(v, &format!("macros.{name}"))?;
        }
    }
    if let Some(changes) = doc.get("time_changes") {
        for (i, v) in array(changes, "time_changes")?.iter().enumerate() {
            fields(
                v,
                &["at_quarters", "bpm", "over_quarters"],
                &format!("time_changes[{i}]"),
            )?;
        }
    }
    if let Some(transitions) = doc.get("transitions") {
        for (i, v) in array(transitions, "transitions")?.iter().enumerate() {
            fields(
                v,
                &["from", "to", "method", "description"],
                &format!("transitions[{i}]"),
            )?;
        }
    }
    Ok(())
}

fn validate_bar_condition(v: &Value, path: &str) -> Result<()> {
    fields(v, &["odd", "even", "every", "bars"], path)
}
fn instrument_ref(doc: &Value, value: &Value, path: &str) -> Result<()> {
    let id = string(value, path)?;
    if doc["instruments"].get(id).is_none() {
        return Err(Error::new(path, format!("undefined instrument {id}")));
    }
    Ok(())
}
fn material_ref(doc: &Value, category: &str, value: &Value, path: &str) -> Result<()> {
    let name = string(value, path)?;
    if category == "comp" && name == "hold" {
        return Ok(());
    }
    if doc.get(category).and_then(|v| v.get(name)).is_none() {
        return Err(Error::new(
            path,
            format!("undefined {category} reference {name}"),
        ));
    }
    Ok(())
}
fn validate_parts(doc: &Value, parts: &Value, path: &str, phrases: bool) -> Result<()> {
    for (id, value) in object(parts, path)? {
        instrument_ref(doc, &Value::String(id.clone()), path)?;
        if let Some(list) = value.as_array().filter(|_| phrases) {
            if list.is_empty() {
                return Err(Error::new(path, "drum phrase must not be empty"));
            }
            for item in list {
                string(item, path)?;
            }
        } else {
            string(value, path)?;
        }
    }
    Ok(())
}
fn validate_material(category: &str, value: &Value, path: &str) -> Result<()> {
    match category {
        "arps" => {
            text_field(value, "order", path)?;
            if let Some(retrigger) = value.get("retrigger_each_step") {
                if !retrigger.is_boolean() {
                    return Err(Error::new(path, "retrigger_each_step must be boolean"));
                }
            }
            if let Some(floor) = value.get("voicing_floor_midi") {
                if !(0..=127).contains(&integer(floor, path)?) {
                    return Err(Error::new(path, "voicing floor must be MIDI 0..127"));
                }
            }
        }
        "counters" => {
            if text_field(value, "when", path)? != "silent_full_beat" {
                return Err(Error::new(path, "expected silent_full_beat"));
            }
            let cells: Vec<_> = text_field(value, "cell", path)?
                .split_whitespace()
                .enumerate()
                .map(|(i, token)| {
                    if token != "." && token != "-" {
                        super::syntax::degree(token).map_err(|e| e.at(path))?;
                    }
                    Ok((
                        super::q(
                            i64::try_from(i)
                                .map_err(|_| Error::new(path, "counter cell is too long"))?,
                            1,
                        ),
                        super::q(1, 1),
                        token.into(),
                    ))
                })
                .collect::<Result<_>>()?;
            if cells.is_empty() {
                return Err(Error::new(path, "counter cell must not be empty"));
            }
            let end = super::q(
                i64::try_from(cells.len())
                    .map_err(|_| Error::new(path, "counter cell is too long"))?,
                1,
            );
            super::syntax::note_spans(cells, &super::q(1, 1), &end, path)?;
        }
        _ => unreachable!("material category"),
    }
    Ok(())
}
fn validate_section_references(doc: &Value, section: &Value, path: &str) -> Result<()> {
    if let Some(lines) = section.get("lines") {
        for (name, line) in object(lines, path)? {
            let p = format!("{path}.lines.{name}");
            instrument_ref(doc, required(line, "instrument", &p)?, &p)?;
            melody(&doc["melodies"], text_field(line, "melody", &p)?, &p)?;
        }
    }
    if let Some(counter) = section.get("counter") {
        let p = format!("{path}.counter.follows");
        let follows = array(required(counter, "follows", path)?, &p)?;
        if follows.is_empty() {
            return Err(Error::new(&p, "follows must name at least one line"));
        }
        for name in follows {
            if section["lines"].get(string(name, &p)?).is_none() {
                return Err(Error::new(&p, "undefined melody line"));
            }
        }
    }
    for (field, category) in [("pad", "comp"), ("arp", "arps"), ("counter", "counters")] {
        if let Some(value) = section.get(field) {
            instrument_ref(doc, required(value, "instrument", path)?, path)?;
            material_ref(doc, category, required(value, "pattern", path)?, path)?;
        }
    }
    if let Some(comp) = section.get("comp") {
        for (id, value) in object(comp, path)? {
            instrument_ref(doc, &Value::String(id.clone()), path)?;
            if value.is_string() {
                material_ref(doc, "comp", value, path)?;
            } else {
                fields(value, &["pattern", "voicing"], path)?;
                validate_voicing(value, path)?;
                material_ref(doc, "comp", required(value, "pattern", path)?, path)?;
            }
        }
    }
    if let Some(drums) = section.get("drums") {
        material_ref(doc, "drums", drums, path)?;
    }
    if let Some(list) = section.get("placements") {
        for (i, placement) in array(list, path)?.iter().enumerate() {
            let path = placement
                .get("$origin")
                .and_then(Value::as_str)
                .map_or_else(|| format!("{path}.placements[{i}]"), str::to_owned);
            instrument_ref(doc, required(placement, "instrument", &path)?, &path)?;
            if let Some(name) = placement.get("melody") {
                melody(&doc["melodies"], string(name, &path)?, &path)?;
            }
            if let Some(note) = placement.get("degree") {
                super::syntax::pitch(string(note, &path)?).map_err(|e| e.at(&path))?;
            }
            if let Some(chord) = placement.get("chord") {
                super::syntax::chord(string(chord, &path)?).map_err(|e| e.at(&path))?;
            }
        }
    }
    Ok(())
}
