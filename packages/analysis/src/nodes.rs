//! Widget helpers of the analysis page on top of the closed public schema.

use std::borrow::Cow;

use proteus_extension::ui::Node;
pub use proteus_extension::ui::{row, stack as column};
use proteus_extension::{Map, Value, json};

/// Disclosures with these suffixes start revealed.
const OPEN: [&str; 4] = [":args", ":output", ":answer", ":summary"];

/// Initial disclosure policy; native controls remember subsequent toggles.
pub struct Open;

impl Open {
    pub fn is(&self, key: &str) -> bool {
        OPEN.iter().any(|suffix| key.ends_with(suffix))
    }
}

pub fn label(value: impl ToString, tone: &str) -> Value {
    styled_text(value, tone, None)
}

pub fn small(value: impl ToString, tone: &str) -> Value {
    styled_text(value, tone, Some("small"))
}

pub fn large(value: impl ToString) -> Value {
    styled_text(value, "default", Some("large"))
}

fn styled_text(value: impl ToString, tone: &str, size: Option<&str>) -> Value {
    let mut text = value.to_string();
    if text.capacity() != text.len() {
        // Assignment drops the loose allocation before map construction.
        text = text.as_str().to_owned();
    }
    // Keep the original order and avoid growing an ordered map at field four.
    let mut fields = Map::with_capacity(3 + usize::from(size.is_some()));
    fields.insert("type".into(), "text".into());
    fields.insert("text".into(), Value::String(text));
    fields.insert("tone".into(), tone.into());
    if let Some(size) = size {
        fields.insert("size".into(), size.into());
    }
    Value::Object(fields)
}

pub fn card(title: impl ToString, children: Vec<Value>) -> Value {
    json!({"type": "card", "title": title.to_string()}).with("children", Value::Array(children))
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

/// Keep Python's splitlines behavior without storing every line just to size the block.
fn height(body: &str, wrap: bool) -> usize {
    // At most 19 wrapped rows fit below the height cap. Each row has at
    // most 110 four-byte scalars, plus at most one three-byte line ending
    // (CRLF is two bytes). Longer wrapped text cannot fit, whatever its lines.
    const MAX_UNSATURATED_BYTES: usize = 19 * (110 * 4 + 3);
    if wrap && body.len() > MAX_UNSATURATED_BYTES {
        return 360;
    }
    let breaks = [
        '\n', '\r', '\x0b', '\x0c', '\x1c', '\x1d', '\x1e', '\u{85}', '\u{2028}', '\u{2029}',
    ];
    let mut rows = 0;
    let mut after_cr = false;
    for segment in body.split_inclusive(breaks) {
        if after_cr && segment == "\n" {
            after_cr = false;
            continue;
        }
        after_cr = segment.ends_with('\r');
        rows += if wrap {
            let line = segment.strip_suffix(breaks).unwrap_or(segment);
            line.chars().count().div_ceil(110).max(1)
        } else {
            1
        };
        // 19 rows give 358 pixels; the twentieth fills the 360-pixel block.
        if rows >= 20 {
            return 360;
        }
    }
    (rows * 18 + 16).clamp(48, 360)
}

/// A heading that reveals a read-only block; JSON values are pretty-printed, text soft-wraps.
pub fn disclosure(key: &str, title: &str, body: &Value, open: &Open) -> Value {
    let wrap = body.is_string();
    let text = match body.as_str() {
        Some(text) => Cow::Borrowed(text),
        None => Cow::Owned(serde_json::to_string_pretty(body).unwrap_or_default()),
    };
    disclosure_with_text(key, title, text, wrap, open)
}

pub fn disclosure_text(key: &str, title: &str, text: String, open: &Open) -> Value {
    disclosure_with_text(key, title, Cow::Owned(text), true, open)
}

fn disclosure_with_text(
    key: &str,
    title: &str,
    text: Cow<'_, str>,
    wrap: bool,
    open: &Open,
) -> Value {
    let revealed = open.is(key);
    let text = if text.is_empty() {
        Cow::Borrowed("—")
    } else {
        text
    };
    let height = height(&text, wrap);
    let block = Value::Object(Map::from_iter([
        ("type".into(), "code".into()),
        ("id".into(), format!("analysis-{key}").into()),
        ("text".into(), Value::Null),
        ("language".into(), if wrap { "text" } else { "json" }.into()),
        ("height".into(), height.into()),
    ]))
    .maybe("wrap", wrap.then_some(true));
    // The SDK serializes its children into a new value. Fill the large text afterward.
    let mut node =
        proteus_extension::ui::disclosure(key, title, vec![block]).with("open", revealed);
    let text = match text {
        Cow::Owned(text) if text.capacity() == text.len() => text,
        text => text.as_ref().to_owned(),
    };
    *node
        .get_mut("children")
        .and_then(|children| children.get_mut(0))
        .and_then(|block| block.get_mut("text"))
        .expect("disclosure contains the code block's text field") = Value::String(text);
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_follow_wrapped_lines() {
        assert_eq!(height("a\r\nb\n\nc\n", false), 88);
        assert_eq!(height("", true), 48);
        assert_eq!(height(&"x".repeat(1000), true), 196);
        assert_eq!(height(&"x\n".repeat(100), false), 360);
    }

    #[test]
    fn height_preserves_breaks_wrapping_and_saturation() {
        for separator in [
            "\n", "\r", "\r\n", "\x0b", "\x0c", "\x1c", "\x1d", "\x1e", "\u{85}", "\u{2028}",
            "\u{2029}",
        ] {
            assert_eq!(height(&separator.repeat(19), false), 358);
            assert_eq!(height(&separator.repeat(20), true), 360);
            let exact = "🦀".repeat(110);
            assert_eq!(
                height(&format!("{exact}{separator}{exact}{separator}"), true),
                52
            );
            assert_eq!(height(&format!("{exact}x{separator}{exact}"), true), 70);
        }
        assert_eq!(height(&"界".repeat(2090), true), 358);
        assert_eq!(height(&"界".repeat(2091), true), 360);
        assert_eq!(height(&"界".repeat(2091), false), 48);
    }
}
