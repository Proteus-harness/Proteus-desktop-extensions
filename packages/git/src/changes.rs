//! Rows of the git panel tree, built from /workspace/changes and /workspace/log; independent of the client.

use std::collections::{HashMap, HashSet};

use proteus_extension::ui::{Node, confirm, item};
use proteus_extension::{Value, ValueExt, file_icon, json};

const SECTIONS: [(&str, &str); 3] = [
    ("conflicts", "Конфликты"),
    ("tracked", "Отслеживаемые"),
    ("untracked", "Новые"),
];

pub fn tone(status: &str) -> &'static str {
    match status {
        "conflict" | "deleted" => "danger",
        "untracked" | "added" => "success",
        _ => "warning",
    }
}

pub fn label(status: &str) -> &str {
    match status {
        "conflict" => "конфликт",
        "deleted" => "удалён",
        "untracked" => "новый",
        "added" => "добавлен",
        "renamed" => "переименован",
        "modified" => "изменён",
        other => other,
    }
}

pub fn name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parent(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(index) => &path[..index],
        None => ".",
    }
}

fn section(entry: &Value) -> &'static str {
    if entry.str("status") == "conflict" {
        "conflicts"
    } else if entry.str("unstaged") == "untracked" && !entry.at("staged").truthy() {
        "untracked"
    } else {
        "tracked"
    }
}

pub fn groups(entries: &[Value]) -> HashMap<&'static str, Vec<&Value>> {
    let mut result: HashMap<&str, Vec<&Value>> = HashMap::new();
    for entry in entries {
        result.entry(section(entry)).or_default().push(entry);
    }
    result
}

pub fn ago(seconds: f64, now: f64) -> String {
    let delta = ((now - seconds) as i64).max(0);
    for (size, unit) in [
        (86400 * 365, "г"),
        (86400 * 30, "мес"),
        (86400, "дн"),
        (3600, "ч"),
        (60, "мин"),
    ] {
        if delta >= size {
            return format!("{} {unit} назад", delta / size);
        }
    }
    "только что".to_owned()
}

fn file_menu(entry: &Value) -> Vec<Value> {
    let path = entry.str("path");
    let mut menu = vec![
        item("Показать изменения", "diff")
            .with("value", path)
            .with("icon", "file-diff"),
    ];
    if entry.at("unstaged").truthy() {
        menu.push(
            item("Добавить в индекс", "stage")
                .with("value", json!([path]))
                .with("icon", "plus"),
        );
    }
    if entry.at("staged").truthy() {
        menu.push(
            item("Убрать из индекса", "unstage")
                .with("value", json!([path]))
                .with("icon", "minus"),
        );
    }
    menu.push(
        item("Копировать путь", "copy")
            .with("value", path)
            .with("icon", "copy")
            .with("separated", true),
    );
    let ask = confirm(&format!("Отменить изменения в {}?", name(path)))
        .with(
            "message",
            "Новый файл будет удалён, изменённый вернётся к HEAD. Это нельзя отменить.",
        )
        .with("ok", "Отменить изменения")
        .with("danger", true);
    menu.push(
        item("Отменить изменения", "discard")
            .with("value", json!([path]))
            .with("icon", "undo-2")
            .with("danger", true)
            .with("confirm", ask),
    );
    menu
}

pub fn change_rows(
    entries: &[Value],
    collapsed: &HashSet<String>,
    selected: Option<&str>,
) -> Vec<Value> {
    let mut rows = Vec::new();
    let sections = groups(entries);
    for (key, title) in SECTIONS {
        let Some(items) = sections.get(key).filter(|items| !items.is_empty()) else {
            continue;
        };
        let staged = items
            .iter()
            .filter(|entry| entry.at("staged").truthy())
            .count();
        rows.push(json!({"id": format!("section:{key}"), "label": title, "detail": items.len().to_string(), "depth": 0,
                         "expanded": !collapsed.contains(key), "checked": staged == items.len(), "tone": "muted"}));
        if collapsed.contains(key) {
            continue;
        }
        for entry in items {
            let path = entry.str("path");
            let mut parts = Vec::new();
            if parent(path) != "." {
                parts.push(parent(path).to_owned());
            }
            if entry.at("original").truthy() {
                parts.push(format!("← {}", name(entry.str("original"))));
            }
            if entry.at("staged").truthy() && entry.at("unstaged").truthy() {
                parts.push("частично".to_owned());
            }
            let mut row = json!({"id": path, "label": name(path), "detail": parts.join("  "), "depth": 1, "expanded": null,
                                 "tone": tone(entry.str("status")), "selected": Some(path) == selected,
                                 "checked": entry.at("staged").truthy(), "icon": file_icon(path), "menu": file_menu(entry)});
            if entry.str("status") == "deleted" {
                row["strike"] = true.into();
            }
            rows.push(row);
        }
    }
    rows
}

pub fn history_rows(log: Option<&[Value]>, expanded: bool, now: f64) -> Vec<Value> {
    let mut rows = vec![
        json!({"id": "section:history", "label": "История", "detail": "", "depth": 0,
                               "expanded": expanded, "tone": "muted"}),
    ];
    if expanded {
        for entry in log.unwrap_or(&[]) {
            let summary = entry
                .opt_str("summary")
                .filter(|summary| !summary.is_empty())
                .unwrap_or("(без описания)");
            let detail = format!(
                "{}  {}  {}",
                entry.str("short"),
                entry.str("author"),
                ago(entry.f64("time").unwrap_or(0.), now)
            );
            rows.push(json!({"id": format!("commit:{}", entry.str("hash")), "label": summary, "detail": detail, "depth": 1,
                             "expanded": null, "icon": "git-commit-horizontal", "tone": "default",
                             "menu": [item("Копировать хеш", "copy").with("value", entry.str("hash")).with("icon", "copy")]}));
        }
    }
    rows
}

/// Paths of a section to stage (`staged` false) or unstage (`staged` true).
pub fn section_paths(entries: &[Value], key: &str, staged: bool) -> Vec<String> {
    let field = if staged { "staged" } else { "unstaged" };
    groups(entries)
        .get(key)
        .map(|items| {
            items
                .iter()
                .filter(|entry| entry.at(field).truthy())
                .map(|entry| entry.str("path").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ago_names_the_largest_unit() {
        assert_eq!(ago(1000., 1030.), "только что");
        assert_eq!(ago(0., 7200.), "2 ч назад");
        assert_eq!(ago(0., 86400. * 400.), "1 г назад");
        assert_eq!(ago(100., 50.), "только что");
    }

    #[test]
    fn collapsed_history_has_no_commits() {
        let log = [json!({"hash": "a", "short": "a", "summary": "", "author": "X", "time": 0})];
        assert_eq!(history_rows(Some(&log), false, 0.).len(), 1);
        let rows = history_rows(Some(&log), true, 120.);
        assert_eq!(
            (rows[1].str("label"), rows[1].str("detail")),
            ("(без описания)", "a  X  2 мин назад")
        );
    }
}
