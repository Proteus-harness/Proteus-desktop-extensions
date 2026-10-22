//! Widget helpers of the analysis page on top of the closed public schema.

use std::collections::HashSet;

use proteus_extension::ui::Node;
use proteus_extension::{Value, json};

/// Disclosures with these suffixes start revealed.
const OPEN: [&str; 4] = [":args", ":output", ":answer", ":summary"];

/// Disclosures the user flipped from their initial state.
pub struct Open<'a>(pub &'a HashSet<String>);

impl Open<'_> {
    pub fn is(&self, key: &str) -> bool {
        self.0.contains(key) != OPEN.iter().any(|suffix| key.ends_with(suffix))
    }
}

pub fn label(value: impl ToString, tone: &str) -> Value {
    json!({"type": "text", "text": value.to_string(), "tone": tone})
}

pub fn small(value: impl ToString, tone: &str) -> Value {
    label(value, tone).with("size", "small")
}

pub fn large(value: impl ToString) -> Value {
    label(value, "default").with("size", "large")
}

pub fn row(children: Vec<Value>) -> Value {
    json!({"type": "stack", "direction": "row", "children": children})
}

pub fn column(children: Vec<Value>) -> Value {
    json!({"type": "stack", "direction": "column", "children": children})
}

pub fn card(title: impl ToString, children: Vec<Value>) -> Value {
    json!({"type": "card", "title": title.to_string(), "children": children})
}

pub fn badge(value: impl ToString, tone: &str) -> Value {
    json!({"type": "badge", "label": value.to_string(), "tone": tone})
}

pub fn kv(key: &str, value: impl ToString, tone: &str) -> Value {
    row(vec![label(key, "muted"), label(value, tone)])
}

pub fn ghost(title: impl ToString, action: &str, value: Value, selected: bool) -> Value {
    json!({"type": "button", "label": title.to_string(), "action": action, "value": value, "variant": "ghost"})
        .maybe("selected", selected.then_some(true))
}

/// Lines as Python's `str.splitlines` cuts them.
fn lines(text: &str) -> Vec<&str> {
    let breaks = [
        '\n', '\r', '\x0b', '\x0c', '\x1c', '\x1d', '\x1e', '\u{85}', '\u{2028}', '\u{2029}',
    ];
    let mut result = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find(breaks) {
        result.push(&rest[..index]);
        let skip = if rest[index..].starts_with("\r\n") {
            2
        } else {
            rest[index..].chars().next().map_or(1, char::len_utf8)
        };
        rest = &rest[index + skip..];
    }
    if !rest.is_empty() {
        result.push(rest);
    }
    result
}

fn height(body: &str, wrap: bool) -> usize {
    let lines = lines(body);
    let count = if lines.is_empty() {
        1
    } else if wrap {
        lines
            .iter()
            .map(|line| line.chars().count().div_ceil(110).max(1))
            .sum()
    } else {
        lines.len()
    };
    (count * 18 + 16).clamp(48, 360)
}

/// A heading that reveals a read-only block; JSON values are pretty-printed, text soft-wraps.
pub fn disclosure(key: &str, title: &str, body: &Value, open: &Open) -> Value {
    let revealed = open.is(key);
    let marker = if revealed { "▾ " } else { "▸ " };
    let toggle = row(vec![ghost(
        format!("{marker}{title}"),
        "open",
        Value::from(key),
        false,
    )]);
    if !revealed {
        return toggle;
    }
    let wrap = body.is_string();
    let text = match body.as_str() {
        Some(text) => text.to_owned(),
        None => serde_json::to_string_pretty(body).unwrap_or_default(),
    };
    let text = if text.is_empty() {
        "—".to_owned()
    } else {
        text
    };
    let block = json!({"type": "code", "id": format!("analysis-{key}"), "text": text,
                       "language": if wrap { "text" } else { "json" }, "height": height(&text, wrap)})
    .maybe("wrap", wrap.then_some(true));
    column(vec![toggle, block]).with("gap", "none")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_follow_wrapped_lines() {
        assert_eq!(lines("a\r\nb\n\nc\n"), ["a", "b", "", "c"]);
        assert_eq!(height("", true), 48);
        assert_eq!(height(&"x".repeat(1000), true), 196);
        assert_eq!(height(&"x\n".repeat(100), false), 360);
    }
}
