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

/// Empty room in a row that pushes the following items to its end.
pub fn spacer() -> Value {
    json!({"type": "spacer"})
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

/// Asks for one line of text before a button or menu item sends its
/// action; the action gets `{value, text}`. Optional fields: `placeholder`,
/// `value` (initial text), `ok`, `stem` (select the name before the last dot).
pub fn prompt(title: &str) -> Value {
    json!({"title": title})
}

/// One entry of a `menu` node, a split button or a row's context menu.
/// Options: `checked`, `disabled`, `danger`, `separated`, `confirm`, `prompt`.
pub fn item(label: &str, action: &str) -> Value {
    json!({"label": label, "action": action})
}

/// A section label inside a menu; it cannot be picked.
pub fn menu_header(label: &str) -> Value {
    json!({"label": label, "header": true})
}

/// A button with a chevron part that opens `items`.
pub fn split_button(label: &str, action: &str, items: Vec<Value>) -> Value {
    button(label, action).with("menu", items)
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

/// A tree section header: a click sends `expand`/`collapse`, the keyboard
/// passes it by. Options: `count`, `checked`.
pub fn tree_header(id: &str, label: &str, expanded: bool) -> Value {
    json!({"id": id, "label": label, "kind": "header", "expanded": expanded})
}

/// Value of a row checkbox: `true`, `false` or `"mixed"` when only some of
/// `total` are checked.
pub fn check_state(checked: usize, total: usize) -> Value {
    match checked {
        0 => json!(false),
        _ if checked >= total => json!(true),
        _ => json!("mixed"),
    }
}

/// Added and deleted line counts for a tree row's `stats`.
pub fn stats(added: u64, deleted: u64) -> Value {
    json!({"added": added, "deleted": deleted})
}

/// A list with a lane column, such as a commit history; rows come from
/// [`lane_row`].
pub fn lanes(id: &str, rows: Vec<Value>, action: &str) -> Value {
    json!({"type": "lanes", "id": id, "rows": rows, "action": action})
}

/// A row of [`lanes`] with its dot on `lane`. Options: `up`, `down`
/// (lists of [`lane_segment`]), `refs`, `detail`, `time`, `selected`,
/// `menu`, `hint`.
pub fn lane_row(id: &str, label: &str, lane: usize, color: usize) -> Value {
    json!({"id": id, "label": label, "lane": lane, "color": color})
}

/// A line of a lanes row from lane `from` at its start edge to lane `to`.
pub fn lane_segment(from: usize, to: usize, color: usize) -> Value {
    json!({"from": from, "to": to, "color": color})
}

/// A context window map: segments of square cells with a legend.
pub fn cells(segments: Vec<Value>) -> Value {
    json!({"type": "cells", "segments": segments})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_check_states_and_split_buttons() {
        assert_eq!(
            (check_state(0, 3), check_state(1, 3), check_state(3, 3)),
            (json!(false), json!("mixed"), json!(true))
        );
        let button = split_button("Commit", "commit", vec![menu_header("Mode")]);
        assert_eq!(button["menu"][0]["header"], json!(true));
        let row = lane_row("c1", "Fix", 0, 2).with("down", vec![lane_segment(0, 1, 3)]);
        assert_eq!(row["down"][0]["to"], json!(1));
    }
}
