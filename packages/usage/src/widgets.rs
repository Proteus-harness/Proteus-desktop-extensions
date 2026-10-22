//! Small widget builders of the usage page.

use proteus_extension::ui::{self, Node};
use proteus_extension::{Value, json};

pub fn label(value: impl ToString, tone: &str) -> Value {
    json!({"type": "text", "text": value.to_string(), "tone": tone})
}

pub fn sized(value: impl ToString, tone: &str, size: &str) -> Value {
    label(value, tone).with("size", size)
}

pub fn badge(value: impl ToString, tone: &str) -> Value {
    ui::badge(value).with("tone", tone)
}

pub fn metric(name: &str, value: impl ToString, detail: Option<&str>) -> Value {
    json!({"type": "metric", "label": name, "value": value.to_string(), "detail": detail})
}

/// Cell text of a loosely typed value, as Python's `str` shows it.
pub fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        other => other.to_string(),
    }
}
