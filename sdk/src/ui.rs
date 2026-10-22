//! Builders of declarative widgets (PROTOCOL.md, `src/declarative/widget.rs`
//! in the client). Optional fields go through [`Node::with`]:
//! `button("Обновить", "refresh").with("icon", "refresh-cw")`.

use serde_json::{Value, json};

/// Adds optional fields to a node.
pub trait Node {
    fn with(self, key: &str, value: impl Into<Value>) -> Value;
    /// Adds the field only when `value` is `Some`.
    fn maybe(self, key: &str, value: Option<impl Into<Value>>) -> Value;
}

impl Node for Value {
    fn with(mut self, key: &str, value: impl Into<Value>) -> Value {
        if let Value::Object(map) = &mut self {
            map.insert(key.to_owned(), value.into());
        }
        self
    }

    fn maybe(self, key: &str, value: Option<impl Into<Value>>) -> Value {
        match value {
            Some(value) => self.with(key, value),
            None => self,
        }
    }
}

pub fn text(value: impl ToString) -> Value {
    json!({"type": "text", "text": value.to_string()})
}

pub fn muted(value: impl ToString) -> Value {
    text(value).with("tone", "muted")
}

/// Small muted text.
pub fn note(value: impl ToString) -> Value {
    text(value).with("tone", "muted").with("size", "small")
}

pub fn stack(children: Vec<Value>) -> Value {
    json!({"type": "stack", "direction": "column", "children": children})
}

pub fn row(children: Vec<Value>) -> Value {
    json!({"type": "stack", "direction": "row", "children": children})
}

pub fn fill(children: Vec<Value>) -> Value {
    json!({"type": "fill", "children": children})
}

/// Full-height columns; `None` widths share the rest.
pub fn split(children: Vec<Value>, widths: Vec<Option<f64>>) -> Value {
    json!({"type": "split", "children": children, "widths": widths})
}

pub fn card(title: Option<&str>, children: Vec<Value>) -> Value {
    json!({"type": "card", "children": children}).maybe("title", title)
}

pub fn divider() -> Value {
    json!({"type": "divider"})
}

pub fn badge(label: impl ToString) -> Value {
    json!({"type": "badge", "label": label.to_string()})
}

pub fn metric(label: &str, value: impl ToString) -> Value {
    json!({"type": "metric", "label": label, "value": value.to_string()})
}

pub fn progress(value: f64, max: f64) -> Value {
    json!({"type": "progress", "value": value, "max": max})
}

pub fn table(columns: &[&str], rows: Vec<Vec<String>>) -> Value {
    json!({"type": "table", "columns": columns, "rows": rows})
}

/// Key/value pairs as a two-column table.
pub fn kv(rows: Vec<(&str, String)>) -> Value {
    table(
        &["Параметр", "Значение"],
        rows.into_iter()
            .map(|(key, value)| vec![key.to_owned(), value])
            .collect(),
    )
}

pub fn button(label: &str, action: &str) -> Value {
    json!({"type": "button", "label": label, "action": action, "value": null})
}

/// Asks the user before a button or menu item sends its action.
pub fn confirm(title: &str) -> Value {
    json!({"title": title})
}

/// One entry of a `menu` node or of a tree row's context menu.
pub fn item(label: &str, action: &str) -> Value {
    json!({"label": label, "action": action})
}

pub fn menu(items: Vec<Value>) -> Value {
    json!({"type": "menu", "label": "", "items": items})
}

pub fn checkbox(label: &str, checked: bool, action: &str) -> Value {
    json!({"type": "checkbox", "label": label, "checked": checked, "action": action})
}

pub fn input(id: &str, value: &str, placeholder: &str, action: &str) -> Value {
    json!({"type": "input", "id": id, "value": value, "placeholder": placeholder, "action": action})
}

/// A row of ghost buttons where the selected one is highlighted, like tabs.
pub fn segments(action: &str, choices: &[(Value, String)], selected: &Value) -> Value {
    row(choices
        .iter()
        .map(|(value, label)| {
            button(label, action)
                .with("value", value.clone())
                .with("variant", "ghost")
                .with("selected", value == selected)
        })
        .collect())
}

pub fn select(
    id: &str,
    label: &str,
    choices: &[(Value, String)],
    selected: &Value,
    action: &str,
) -> Value {
    let options: Vec<Value> = choices
        .iter()
        .map(
            |(value, label)| json!({"label": label, "value": value, "selected": value == selected}),
        )
        .collect();
    json!({"type": "select", "id": id, "label": label, "action": action, "options": options})
}

pub fn code(id: &str, text: &str, language: &str) -> Value {
    json!({"type": "code", "id": id, "text": text, "language": language})
}

pub fn tree(id: &str, rows: Vec<Value>, action: &str) -> Value {
    json!({"type": "tree", "id": id, "rows": rows, "action": action})
}

/// A context window map: segments of square cells with a legend.
pub fn cells(segments: Vec<Value>) -> Value {
    json!({"type": "cells", "segments": segments})
}
