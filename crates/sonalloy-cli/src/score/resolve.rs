use super::{Error, Result, array, fields, object, required, string, text_field};
use serde_json::Value;

pub(super) fn inherit(dict: &Value, name: &str, drums: bool, path: &str) -> Result<Value> {
    fn visit(
        dict: &Value,
        name: &str,
        drums: bool,
        path: &str,
        stack: &mut Vec<String>,
    ) -> Result<Value> {
        if stack.iter().any(|s| s == name) {
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
                    .filter(|n| *n > 0)
                    .ok_or_else(|| Error::new(path, "bar start must be positive"))?
                    as usize;
                let end = range[1]
                    .as_u64()
                    .ok_or_else(|| Error::new(path, "bar end must be positive"))?
                    as usize;
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
    for (name, section) in object(sections, "sections")? {
        let path = format!("sections.{name}");
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
        if let Some(lead) = section.get("lead") {
            fields(
                lead,
                &["instrument", "melody", "slice_bars", "layers"],
                &format!("{path}.lead"),
            )?;
            if let Some(layers) = lead.get("layers") {
                for (i, layer) in array(layers, &path)?.iter().enumerate() {
                    fields(
                        layer,
                        &["instrument", "transpose", "gain_db"],
                        &format!("{path}.lead.layers[{i}]"),
                    )?;
                }
            }
        }
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
    for (name, v) in object(required(doc, "instruments", "")?, "instruments")? {
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
    }
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
                fields(v, &allowed, &format!("{key}.{name}"))?;
            }
        }
    }
    if let Some(dict) = doc.get("drums") {
        for (name, v) in object(dict, "drums")? {
            let p = format!("drums.{name}");
            fields(v, &["from", "parts", "overrides"], &p)?;
            if let Some(list) = v.get("overrides") {
                for (i, v) in array(list, &p)?.iter().enumerate() {
                    let p = format!("{p}.overrides[{i}]");
                    fields(v, &["when", "parts"], &p)?;
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
    if let Some(notes) = doc.get("notes") {
        string(notes, "notes")?;
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
