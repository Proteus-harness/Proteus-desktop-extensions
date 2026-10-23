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

/// Native self-contained color SVG. Dimensions are logical pixels.
pub fn svg(source: impl Into<String>, width: f64, height: f64) -> Value {
    json!({"type":"svg", "source":source.into(), "width":width, "height":height})
}

pub fn divider() -> Value {
    json!({"type": "divider"})
}

/// Empty room in a row that pushes the following items to its end.
pub fn spacer() -> Value {
    json!({"type": "spacer"})
}

/// A compact strip at an edge of a panel with a line on top: what comes
/// before a [`spacer`] sits at the start, the rest at the end. Option:
/// `border` (false — no line, as a footer right under a list).
pub fn bar(children: Vec<Value>) -> Value {
    json!({"type": "bar", "children": children})
}

/// The editor area at the bottom of a panel: a line on top and the editor
/// background behind a flush [`input`] and the [`bar`] under it.
pub fn panel_editor(children: Vec<Value>) -> Value {
    json!({"type": "panel_editor", "children": children})
}

/// Shared delayed spinner; no custom animation is needed in a package.
pub fn loading(text: impl ToString) -> Value {
    json!({"type":"loading", "text":text.to_string()})
}

/// A muted message centered in the room left, for an empty list.
pub fn empty(text: impl ToString) -> Value {
    json!({"type": "empty", "text": text.to_string()})
}

/// A small Lucide icon; option `tone` (muted by default).
pub fn icon(name: &str) -> Value {
    json!({"type": "icon", "name": name})
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

/// A text field. Options: `lines`, `submit`, `flush` (no frame and no
/// background, for a [`panel_editor`]), `mono`.
pub fn input(id: &str, value: &str, placeholder: &str, action: &str) -> Value {
    json!({"type": "input", "id": id, "value": value, "placeholder": placeholder, "action": action})
}

/// A compact toolbar search. The native client owns its trigger, focus,
/// Ctrl+F/Esc and reveal; the process receives the ordinary input string.
pub fn search(id: &str, value: &str, placeholder: &str, action: &str) -> Value {
    input(id, value, placeholder, action).with("search", true)
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

/// Native search, virtualized rows and keyboard selection; value is sent directly.
pub fn picker(id: &str, options: &[(Value, String)], selected: &Value, action: &str) -> Value {
    let options: Vec<_> = options
        .iter()
        .map(|(value, label)| json!({"value":value,"label":label,"selected":value==selected}))
        .collect();
    json!({"type":"picker","id":id,"options":options,"action":action})
}

pub fn code(id: &str, text: &str, language: &str) -> Value {
    json!({"type": "code", "id": id, "text": text, "language": language})
}

/// A native diff: the original Git unified or apply_patch envelope. Options:
/// `files` — supplemental `{path, status?, message?}` metadata, including
/// binary/oversized files; `height` — fixed height instead of filling a view.
/// Requires `widgets.diff.v1`.
pub fn diff(id: &str, patch: &str) -> Value {
    json!({"type": "diff", "id": id, "patch": patch})
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
pub fn lanes(id: &str, columns: Vec<Value>, rows: Vec<Value>, action: &str) -> Value {
    json!({"type": "lanes", "id": id, "columns":columns, "rows": rows, "action": action})
}

/// A row of [`lanes`] with its dot on `lane`. Options: `up`, `down`
/// (lists of [`lane_segment`]), `refs`, `cells` (one string per column), `selected`,
/// `menu`, `hint`.
pub fn lane_row(id: &str, label: &str, lane: usize, color: usize) -> Value {
    json!({"id": id, "label": label, "lane": lane, "color": color})
}

/// A line of a lanes row from lane `from` at its start edge to lane `to`.
pub fn lane_segment(from: usize, to: usize, color: usize) -> Value {
    json!({"from": from, "to": to, "color": color})
}

/// A disclosure animated and remembered by the native renderer.
pub fn disclosure(id: &str, label: &str, children: Vec<Value>) -> Value {
    json!({"type":"disclosure", "id":id, "label":label, "children":children})
}

/// A centered local avatar with the person's name and secondary line.
pub fn person(name: &str, detail: &str) -> Value {
    json!({"type":"person", "name":name, "detail":detail})
}

/// A list of two-line rows from [`list_row`], such as a commit history:
/// a click or Enter sends `{id, event: "open"}`.
pub fn list(id: &str, rows: Vec<Value>, action: &str) -> Value {
    json!({"type": "list", "id": id, "rows": rows, "action": action})
}

/// A row of [`list`]. Options: `chips`, `mark` (an icon in a small frame
/// at the end of the first line), `avatar`, `meta` (parts of the second
/// line), `hint`, `hint_detail`, `menu`.
pub fn list_row(id: &str, label: &str) -> Value {
    json!({"id": id, "label": label})
}

/// Native whole-row hold-and-drag order. A confirmed drop sends
/// `{id, before: string|null}`; `null` means the end, cancellation sends nothing.
pub fn sortable(id: &str, items: Vec<Value>, action: &str) -> Value {
    json!({"type":"sortable","id":id,"items":items,"action":action})
}

/// A stable sortable identity, full-width drag label and arbitrary native
/// children. Nested controls keep their own ordinary input/keyboard behavior.
pub fn sortable_item(id: &str, label: &str, children: Vec<Value>) -> Value {
    json!({"id":id,"label":label,"children":children})
}

/// A context window map: segments of square cells with a legend.
pub fn cells(segments: Vec<Value>) -> Value {
    json!({"type": "cells", "segments": segments})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sortable_preserves_nested_control_contracts() {
        let root = sortable(
            "hooks",
            vec![sortable_item(
                "first",
                "First",
                vec![button("Configure", "configure").with("value", "first")],
            )],
            "move",
        );
        assert_eq!(root["items"][0]["id"], "first");
        assert_eq!(root["items"][0]["children"][0]["action"], "configure");
        assert_eq!(root["action"], "move");
    }

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
        let strip = bar(vec![icon("git-branch"), spacer()]).with("border", false);
        assert_eq!(strip["children"][0]["name"], json!("git-branch"));
        let history = list(
            "log",
            vec![list_row("c1", "Fix").with("meta", vec!["Ann"])],
            "log",
        );
        assert_eq!(history["rows"][0]["meta"][0], json!("Ann"));
    }
}
