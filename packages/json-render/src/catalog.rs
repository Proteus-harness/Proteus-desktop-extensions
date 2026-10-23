//! A closed, display-only catalog using the public native widget schema.
//! Invalid data returns a content error, allowing the host's raw fallback.

use anyhow::{Context as _, Result, anyhow, bail};
use proteus_extension::ui;
use serde_json::{Map, Value};

const MAX_SOURCE: usize = 256 * 1024;
const MAX_DEPTH: usize = 24;
const MAX_NODES: usize = 2_000;

pub(super) fn render(language: &str, source: &str) -> Result<Value> {
    if source.len() > MAX_SOURCE {
        bail!("visualization source exceeds 256 KiB");
    }
    match language {
        "json-render" => {
            let root: Value = serde_json::from_str(source).context("invalid json-render source")?;
            check(&root, 0, &mut 0)?;
            Ok(root)
        }
        // XML, fragment references and SVG drawing support belong to the
        // shared host Svg primitive. This package never treats source as a
        // path, loads assets, or builds a browser document.
        "svg" if !source.trim().is_empty() => Ok(ui::svg(source, 640., 360.)),
        "svg" => bail!("svg source is empty"),
        _ => bail!("unsupported visualization language {language:?}"),
    }
}

fn object<'a>(value: &'a Value, allowed: &[&str]) -> Result<&'a Map<String, Value>> {
    let map = value
        .as_object()
        .ok_or_else(|| anyhow!("catalog entry must be an object"))?;
    if let Some(key) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        bail!("unsupported field {key:?}");
    }
    Ok(map)
}

fn string<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    map.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("{key} must be a string"))
}

fn array<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a [Value]> {
    map.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow!("{key} must be an array"))
}

fn optional_string(map: &Map<String, Value>, key: &str) -> Result<()> {
    if map.get(key).is_some_and(|v| !v.is_null() && !v.is_string()) {
        bail!("{key} must be a string or null");
    }
    Ok(())
}

fn choice(map: &Map<String, Value>, key: &str, allowed: &[&str]) -> Result<()> {
    if map.contains_key(key) && !allowed.contains(&string(map, key)?) {
        bail!("unsupported {key}");
    }
    Ok(())
}

fn tone(map: &Map<String, Value>) -> Result<()> {
    if !map.contains_key("tone") {
        return Ok(());
    }
    let tone = string(map, "tone")?;
    if ["default", "muted", "accent", "success", "warning", "danger"].contains(&tone)
        || tone.len() == 7
            && tone.starts_with('#')
            && tone.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
    {
        return Ok(());
    }
    bail!("tone must be a semantic name or #rrggbb");
}

fn children(map: &Map<String, Value>, depth: usize, count: &mut usize) -> Result<()> {
    if map.contains_key("children") {
        for child in array(map, "children")? {
            check(child, depth + 1, count)?;
        }
    }
    Ok(())
}

fn check(value: &Value, depth: usize, count: &mut usize) -> Result<()> {
    if depth > MAX_DEPTH {
        bail!("visualization exceeds {MAX_DEPTH} levels");
    }
    *count += 1;
    if *count > MAX_NODES {
        bail!("visualization exceeds {MAX_NODES} widgets");
    }
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("catalog entry needs a string type"))?;
    match kind {
        "stack" => {
            let map = object(value, &["type", "direction", "gap", "children"])?;
            choice(map, "direction", &["column", "row"])?;
            choice(map, "gap", &["none", "small", "medium", "large"])?;
            children(map, depth, count)?;
        }
        "card" => {
            let map = object(value, &["type", "title", "children"])?;
            optional_string(map, "title")?;
            children(map, depth, count)?;
        }
        "text" => {
            let map = object(
                value,
                &["type", "text", "single_line", "tone", "size", "mono"],
            )?;
            string(map, "text")?;
            choice(map, "size", &["small", "normal", "large"])?;
            tone(map)?;
            for key in ["single_line", "mono"] {
                if map.get(key).is_some_and(|v| !v.is_boolean()) {
                    bail!("{key} must be a boolean");
                }
            }
        }
        "metric" => {
            let map = object(value, &["type", "label", "value", "detail", "tone"])?;
            string(map, "label")?;
            string(map, "value")?;
            optional_string(map, "detail")?;
            tone(map)?;
        }
        "table" => {
            let map = object(value, &["type", "columns", "rows"])?;
            let columns = array(map, "columns")?;
            if columns.iter().any(|v| !v.is_string()) {
                bail!("table columns must be strings");
            }
            for row in array(map, "rows")? {
                let row = row
                    .as_array()
                    .ok_or_else(|| anyhow!("table row must be an array"))?;
                if row.len() != columns.len() || row.iter().any(|v| !v.is_string()) {
                    bail!("table rows must contain one string per column");
                }
            }
        }
        "bar_chart" => {
            let map = object(value, &["type", "bars", "unit"])?;
            optional_string(map, "unit")?;
            for bar in array(map, "bars")? {
                let bar = object(bar, &["label", "value", "tone"])?;
                string(bar, "label")?;
                if !bar
                    .get("value")
                    .and_then(Value::as_f64)
                    .is_some_and(|v| v.is_finite() && v >= 0.)
                {
                    bail!("bar value must be finite and non-negative");
                }
                tone(bar)?;
            }
        }
        "tabs" => {
            let map = object(value, &["type", "tabs"])?;
            for tab in array(map, "tabs")? {
                let tab = object(tab, &["label", "count", "children"])?;
                string(tab, "label")?;
                if tab
                    .get("count")
                    .is_some_and(|v| !v.is_null() && !v.is_u64())
                {
                    bail!("tab count must be a non-negative integer or null");
                }
                children(tab, depth, count)?;
            }
        }
        "svg" => {
            let map = object(value, &["type", "source", "width", "height"])?;
            if string(map, "source")?.trim().is_empty() {
                bail!("svg source is empty");
            }
            for key in ["width", "height"] {
                if !map
                    .get(key)
                    .and_then(Value::as_f64)
                    .is_some_and(|v| v.is_finite() && (1. ..=4096.).contains(&v))
                {
                    bail!("svg {key} must be within 1..=4096");
                }
            }
        }
        _ => bail!("unsupported display widget {kind:?}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
