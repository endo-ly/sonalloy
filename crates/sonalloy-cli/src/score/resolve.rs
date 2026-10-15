use super::{Error, Result, array, fields, integer, number, object, required, string, text_field};
use serde_json::Value;

pub(super) fn inherit(dict: &Value, name: &str, drums: bool, path: &str) -> Result<Value> {
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
            if key == "from" {
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
            } else {
                result[key] = value.clone();
            }
        }
        stack.pop();
        Ok(result)
    }
    visit(dict, name, drums, path, &mut Vec::new())
}

pub(super) fn melody(dict: &Value, name: &str, path: &str) -> Result<Vec<Value>> {
    fn visit(dict: &Value, name: &str, path: &str, stack: &mut Vec<String>) -> Result<Vec<Value>> {
        if stack.iter().any(|s| s == name) {
            return Err(Error::new(path, format!("cyclic melody reference {name}")));
        }
        let mut value = dict;
        for key in name.split('.') {
            value = value
                .get(key)
                .ok_or_else(|| Error::new(path, format!("undefined melody {name}")))?;
        }
        stack.push(name.into());
        let result = if let Some(list) = value.as_array() {
            let mut bars = Vec::new();
            for item in list {
                if item.is_string() {
                    bars.push(item.clone());
                    continue;
                }
                fields(item, &["use", "bars", "shift_steps"], path)?;
                let referenced = visit(dict, text_field(item, "use", path)?, path, stack)?;
                let range = array(required(item, "bars", path)?, path)?;
                if range.len() != 2 {
                    return Err(Error::new(path, "bars must be [start,end]"));
                }
                let start = range[0]
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n > 0)
                    .ok_or_else(|| Error::new(path, "bar start must be positive"))?;
                let end = range[1]
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| Error::new(path, "bar end must be positive"))?;
                if start > end || end > referenced.len() {
                    return Err(Error::new(path, "melody slice is outside source bars"));
                }
                let shift = item.get("shift_steps").cloned().unwrap_or(Value::from(0));
                for bar in &referenced[start - 1..end] {
                    bars.push(serde_json::json!({"resolved_bar":bar,"shift_steps":shift}));
                }
            }
            bars
        } else {
            fields(value, &["from", "replace"], path)?;
            let mut bars = visit(dict, text_field(value, "from", path)?, path, stack)?;
            if let Some(replace) = value.get("replace") {
                for (key, bar) in object(replace, path)? {
                    let index = key
                        .parse::<usize>()
                        .ok()
                        .filter(|n| *n > 0 && *n <= bars.len())
                        .ok_or_else(|| Error::new(path, "replacement bar is outside melody"))?;
                    string(bar, path)?;
                    bars[index - 1] = bar.clone();
                }
            }
            bars
        };
        stack.pop();
        Ok(result)
    }
    visit(dict, name, path, &mut Vec::new())
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
            "bass",
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
            "master",
            "fade_out_seconds",
        ],
        "meta",
    )?;
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
            ],
            &format!("form[{i}]"),
        )?;
    }
    let sections = required(doc, "sections", "")?;
    validate_sections(sections)?;
    validate_instruments(required(doc, "instruments", "")?)?;
    validate_materials(doc)?;
    if let Some(notes) = doc.get("notes") {
        string(notes, "notes")?;
    }
    if let Some(comp) = doc.get("comp") {
        for (name, value) in object(comp, "comp")? {
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
        if value.is_array() || value.get("from").is_some() || value.get("replace").is_some() {
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
        "automation" => &["instrument", "parameter", "from_value", "to_value"],
        _ => return Err(Error::new(format!("{path}.kind"), "unknown event kind")),
    };
    let mut allowed = vec!["kind"];
    allowed.extend_from_slice(specific);
    for key in specific {
        if !matches!(*key, "velocity" | "riser" | "except_instruments") {
            required(v, key, path)?;
        }
    }
    if !matches!(kind, "part_window" | "step_roll" | "drum_roll") {
        allowed.push("at");
        required(v, "at", path)?;
    }
    if matches!(kind, "hit" | "hits" | "degree" | "silence" | "automation") {
        allowed.extend(["duration_steps", "duration_quarters"]);
    }
    fields(v, &allowed, path)?;
    if kind == "hits" && array(required(v, "instruments", path)?, path)?.is_empty() {
        return Err(Error::new(path, "hits requires at least one instrument"));
    }
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
    if matches!(kind, "silence" | "automation")
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
                "lead",
                "drums",
                "bass",
                "bass_overrides",
                "comp",
                "pad",
                "arp",
                "counter",
                "answers",
                "placements",
                "events",
                "velocity",
            ],
            &path,
        )?;
        for field in ["pad", "arp", "counter"] {
            if let Some(v) = section.get(field) {
                fields(v, &["instrument", "pattern"], &format!("{path}.{field}"))?;
            }
        }
        validate_lead(section, &path)?;
        if let Some(bass) = section.get("bass") {
            fields(bass, &["pattern", "add_parts"], &format!("{path}.bass"))?;
        }
        for field in ["answers", "placements"] {
            if let Some(list) = section.get(field) {
                for (i, item) in array(list, &path)?.iter().enumerate() {
                    let p = format!("{path}.{field}[{i}]");
                    fields(
                        item,
                        &[
                            "instrument",
                            "at",
                            "melody",
                            "degree",
                            "chord",
                            "bass_note",
                            "velocity",
                            "duration_steps",
                            "duration_quarters",
                            "transpose",
                            "gain_db",
                            "mark",
                            "slice_steps",
                            "take_notes",
                            "voicing_floor_midi",
                            "add_root_degree",
                        ],
                        &p,
                    )?;
                    position(required(item, "at", &p)?, &format!("{p}.at"))?;
                    if let Some(slice) = item.get("slice_steps") {
                        fields(slice, &["start", "count"], &format!("{p}.slice_steps"))?;
                    }
                }
            }
        }
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
        if let Some(overrides) = section.get("bass_overrides") {
            for (i, e) in array(overrides, &path)?.iter().enumerate() {
                fields(e, &["bar", "parts"], &format!("{path}.bass_overrides[{i}]"))?;
            }
        }
    }
    Ok(())
}

fn validate_lead(section: &Value, path: &str) -> Result<()> {
    if let Some(lead) = section.get("lead") {
        fields(
            lead,
            &["instrument", "melody", "slice_bars", "layers"],
            &format!("{path}.lead"),
        )?;
        if let Some(layers) = lead.get("layers") {
            for (i, layer) in array(layers, path)?.iter().enumerate() {
                fields(
                    layer,
                    &["instrument", "transpose", "gain_db"],
                    &format!("{path}.lead.layers[{i}]"),
                )?;
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
                "role",
                "voicing_floor_midi",
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
        for key in ["preset", "definition", "role", "audio_input"] {
            if let Some(value) = v.get(key) {
                string(value, &format!("{path}.{key}"))?;
            }
        }
        if let Some(value) = v.get("gain_db") {
            number(value, &format!("{path}.gain_db"))?;
        }
        for key in [
            "voicing_floor_midi",
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
        if super::optional_int(v, "bass_floor_midi", 28, &path)?
            > super::optional_int(v, "bass_ceiling_midi", 39, &path)?
        {
            return Err(Error::new(path, "bass floor must not exceed bass ceiling"));
        }
    }
    Ok(())
}
fn validate_materials(doc: &Value) -> Result<()> {
    for (key, allowed) in [
        ("bass", vec!["parts", "choices"]),
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
                validate_material(doc, key, v, &path)?;
            }
        }
    }
    if let Some(dict) = doc.get("drums") {
        for (name, v) in object(dict, "drums")? {
            let p = format!("drums.{name}");
            fields(v, &["from", "parts", "overrides"], &p)?;
            let resolved = inherit(dict, name, true, &p)?;
            validate_parts(doc, required(&resolved, "parts", &p)?, &p)?;
            if let Some(list) = v.get("overrides") {
                for (i, v) in array(list, &p)?.iter().enumerate() {
                    let p = format!("{p}.overrides[{i}]");
                    fields(v, &["when", "parts"], &p)?;
                    validate_parts(doc, required(v, "parts", &p)?, &p)?;
                    fields(
                        required(v, "when", &p)?,
                        &["odd", "every", "bar"],
                        &format!("{p}.when"),
                    )?;
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
            fields(v, &["at_quarters", "bpm"], &format!("time_changes[{i}]"))?;
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

fn instrument_ref(doc: &Value, value: &Value, path: &str) -> Result<()> {
    let id = string(value, path)?;
    if doc["instruments"].get(id).is_none() {
        return Err(Error::new(path, format!("undefined instrument {id}")));
    }
    Ok(())
}
fn material_ref(doc: &Value, category: &str, value: &Value, path: &str) -> Result<()> {
    let name = string(value, path)?;
    if doc.get(category).and_then(|v| v.get(name)).is_none() {
        return Err(Error::new(
            path,
            format!("undefined {category} reference {name}"),
        ));
    }
    Ok(())
}
fn validate_parts(doc: &Value, parts: &Value, path: &str) -> Result<()> {
    for (id, value) in object(parts, path)? {
        instrument_ref(doc, &Value::String(id.clone()), path)?;
        string(value, path)?;
    }
    Ok(())
}
fn validate_material(doc: &Value, category: &str, value: &Value, path: &str) -> Result<()> {
    match category {
        "bass" => {
            validate_parts(doc, required(value, "parts", path)?, path)?;
            if let Some(choices) = value.get("choices") {
                for (name, choice) in object(choices, path)? {
                    let path = format!("{path}.choices.{name}");
                    fields(choice, &["odd", "even"], &path)?;
                    text_field(choice, "odd", &path)?;
                    text_field(choice, "even", &path)?;
                }
            }
        }
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
            if text_field(value, "when", path)? != "lead_silent_full_beat" {
                return Err(Error::new(path, "expected lead_silent_full_beat"));
            }
            for note in text_field(value, "cell", path)?.split_whitespace() {
                super::syntax::degree(note).map_err(|e| e.at(path))?;
            }
        }
        _ => unreachable!("material category"),
    }
    Ok(())
}
fn validate_section_references(doc: &Value, section: &Value, path: &str) -> Result<()> {
    if let Some(lead) = section.get("lead") {
        instrument_ref(doc, required(lead, "instrument", path)?, path)?;
        melody(&doc["melodies"], text_field(lead, "melody", path)?, path)?;
        if let Some(layers) = lead.get("layers") {
            for layer in array(layers, path)? {
                instrument_ref(doc, required(layer, "instrument", path)?, path)?;
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
            material_ref(doc, "comp", value, path)?;
        }
    }
    if let Some(bass) = section.get("bass") {
        material_ref(doc, "bass", required(bass, "pattern", path)?, path)?;
        if let Some(parts) = bass.get("add_parts") {
            validate_parts(doc, parts, path)?;
        }
    }
    if let Some(drums) = section.get("drums") {
        material_ref(doc, "drums", drums, path)?;
    }
    for field in ["answers", "placements"] {
        if let Some(list) = section.get(field) {
            for (i, placement) in array(list, path)?.iter().enumerate() {
                let path = format!("{path}.{field}[{i}]");
                instrument_ref(doc, required(placement, "instrument", &path)?, &path)?;
                if let Some(name) = placement.get("melody") {
                    melody(&doc["melodies"], string(name, &path)?, &path)?;
                }
                if let Some(note) = placement.get("degree") {
                    super::syntax::degree(string(note, &path)?).map_err(|e| e.at(&path))?;
                }
                if let Some(chord) = placement.get("chord") {
                    super::syntax::chord(string(chord, &path)?).map_err(|e| e.at(&path))?;
                }
            }
        }
    }
    Ok(())
}
