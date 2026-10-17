use serde_json::{Map, Value};
use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

use super::{Error, Result, Source};

pub(super) fn parse(text: &str, source: &mut Source) -> Result<Value> {
    text.clone_into(&mut source.text);
    let mut parser = Parser::new_from_str(text);
    next(&mut parser)?;
    let (start, _) = next(&mut parser)?;
    if start != Event::DocumentStart {
        return Err(Error::new("", "expected one YAML document"));
    }
    let value = node(&mut parser, source, "", 0)?;
    if next(&mut parser)?.0 != Event::DocumentEnd || next(&mut parser)?.0 != Event::StreamEnd {
        return Err(Error::new("", "expected exactly one YAML document"));
    }
    Ok(value)
}

fn next(parser: &mut Parser<std::str::Chars<'_>>) -> Result<(Event, Marker)> {
    parser.next_token().map_err(|error| {
        let mut result = Error::new("", error.to_string());
        result.line = error.marker().line();
        result.column = error.marker().col() + 1;
        result
    })
}

#[allow(clippy::too_many_lines)]
fn node(
    parser: &mut Parser<std::str::Chars<'_>>,
    source: &mut Source,
    path: &str,
    depth: usize,
) -> Result<Value> {
    if depth > 64 {
        return Err(Error::new(path, "YAML nesting exceeds 64 levels"));
    }
    let (event, mark) = next(parser)?;
    source
        .positions
        .insert(path.into(), (mark.line(), mark.col() + 1));
    source.indices.insert(path.into(), mark.index());
    let fail = |message: &str| {
        let mut error = Error::new(path, message);
        error.line = mark.line();
        error.column = mark.col() + 1;
        error
    };
    let value = match event {
        Event::Scalar(text, style, anchor, tag) => {
            let rational_field = matches!(
                path.rsplit('.').next(),
                Some(
                    "gate"
                        | "duration_steps"
                        | "duration_quarters"
                        | "offset_quarters"
                        | "at_quarters"
                        | "last_bar_quarters"
                )
            );
            if anchor != 0 || tag.is_some() {
                return Err(fail("YAML anchors and tags are not supported"));
            }
            if style == TScalarStyle::Plain && !rational_field {
                match text.as_str() {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    "null" | "~" | "" => Value::Null,
                    _ => serde_json::from_str::<Value>(&text)
                        .ok()
                        .filter(Value::is_number)
                        .unwrap_or(Value::String(text)),
                }
            } else {
                Value::String(text)
            }
        }
        Event::SequenceStart(anchor, tag) => {
            if anchor != 0 || tag.is_some() {
                return Err(fail("YAML anchors and tags are not supported"));
            }
            let mut values = Vec::new();
            while parser.peek().map_err(|e| fail(&e.to_string()))?.0 != Event::SequenceEnd {
                values.push(node(
                    parser,
                    source,
                    &format!("{path}[{}]", values.len()),
                    depth + 1,
                )?);
                if values.len() > 1_000_000 {
                    return Err(fail("sequence exceeds one million entries"));
                }
            }
            next(parser)?;
            Value::Array(values)
        }
        Event::MappingStart(anchor, tag) => {
            if anchor != 0 || tag.is_some() {
                return Err(fail("YAML anchors and tags are not supported"));
            }
            let mut map = Map::new();
            while parser.peek().map_err(|e| fail(&e.to_string()))?.0 != Event::MappingEnd {
                let (key, key_mark) = next(parser)?;
                let Event::Scalar(key, style, 0, None) = key else {
                    return Err(fail("mapping keys must be strings"));
                };
                let numeric_replace = path.ends_with(".replace") && key.parse::<usize>().is_ok();
                if key == "<<"
                    || (!numeric_replace
                        && style == TScalarStyle::Plain
                        && matches!(key.as_str(), "true" | "false" | "null" | "~"))
                    || (!numeric_replace
                        && style == TScalarStyle::Plain
                        && key.parse::<f64>().is_ok())
                {
                    return Err(fail(
                        "mapping keys must be strings; merge keys are not supported",
                    ));
                }
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if map.contains_key(&key) {
                    let mut e = Error::new(child, "duplicate mapping key");
                    e.line = key_mark.line();
                    e.column = key_mark.col() + 1;
                    return Err(e);
                }
                let value = node(parser, source, &child, depth + 1)?;
                map.insert(key, value);
            }
            next(parser)?;
            Value::Object(map)
        }
        Event::Alias(_) => return Err(fail("YAML aliases are not supported")),
        _ => return Err(fail("expected a scalar, sequence, or mapping")),
    };
    if !value.is_object() && !value.is_array() {
        source.values.insert(path.into(), value.clone());
    }
    Ok(value)
}
