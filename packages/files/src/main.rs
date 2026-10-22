mod document;
mod tree;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use proteus_extension::ui::{Node, fill, input, item, muted, text};
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, file_icon, json, run};

const POLL: Duration = Duration::from_secs(5);

struct Files {
    query: String,
    expanded: HashSet<String>,
    listings: HashMap<String, Value>,
    selected: Option<String>,
    statuses: HashMap<String, String>,
    folders: HashMap<String, &'static str>,
    deadline: Option<Instant>,
    truncated: bool,
    /// Fetched documents by path and mode (`file` or `diff`).
    documents: HashMap<(String, String), Value>,
    modes: HashMap<String, String>,
    search: Option<(String, Value)>,
}

impl Default for Files {
    fn default() -> Self {
        Self {
            query: String::new(),
            expanded: HashSet::from([String::new()]),
            listings: HashMap::new(),
            selected: None,
            statuses: HashMap::new(),
            folders: HashMap::new(),
            deadline: None,
            truncated: false,
            documents: HashMap::new(),
            modes: HashMap::new(),
            search: None,
        }
    }
}

impl Extension for Files {
    fn poll(&self) -> Option<Duration> {
        Some(POLL)
    }

    fn session_changed(&mut self, _: &mut Host) {
        self.query.clear();
        self.modes.clear();
    }

    fn event(&mut self, host: &mut Host, event: &str) {
        if event == "navigation_changed"
            && let Some(document) = host
                .active_document
                .as_ref()
                .and_then(Value::as_str)
                .filter(|document| !document.is_empty())
        {
            self.selected = Some(document.to_owned());
            self.expand_to(document);
        }
        if event == "session_opened" {
            self.listings.clear();
            self.expanded = HashSet::from([String::new()]);
            self.selected = None;
        }
        if matches!(event, "session_opened" | "run_settled") {
            self.documents.clear();
            self.deadline = None;
        }
    }

    fn render(
        &mut self,
        host: &mut Host,
        view: &str,
        surface: Surface,
        argument: Option<&str>,
    ) -> Result {
        if host.session.as_deref().is_none_or(str::is_empty) {
            host.show(view, muted("Нет активной сессии."));
            return Ok(());
        }
        match surface {
            Surface::Panel => {
                self.refresh(host)?;
                let rows = self.rows(host)?;
                let mut children =
                    vec![input("search", &self.query, "Поиск по проекту…", "search")];
                if self.truncated {
                    children.push(
                        text("Список ограничен сервером; уточните поиск.").with("tone", "warning"),
                    );
                }
                if rows.is_empty() {
                    children.push(muted(if self.query.is_empty() {
                        "Папка пуста."
                    } else {
                        "Ничего не найдено."
                    }));
                }
                children.push(proteus_extension::ui::tree("files", rows, "file"));
                let keys =
                    json!({"ctrl-f": "search-focus", "/": "search-focus", "escape": "escape"});
                host.show_with_keys(view, fill(children), keys);
            }
            Surface::Document => {
                let root = self.document(host, argument.unwrap_or(""));
                host.show(view, root);
            }
            _ => {}
        }
        Ok(())
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        match action {
            "search-focus" => host.focus(view, Some("search")),
            "escape" => {
                let control = if self.query.is_empty() {
                    None
                } else {
                    self.query.clear();
                    Some("files")
                };
                host.focus(view, control);
            }
            "search" => self.query = value.as_str().unwrap_or("").to_owned(),
            "copy" => host.clipboard(value.as_str().unwrap_or("")),
            "document-mode" => {
                if let (Some(path), Some(mode)) = (value[0].as_str(), value[1].as_str()) {
                    self.modes.insert(path.to_owned(), mode.to_owned());
                }
            }
            "open" | "diff" => {
                let path = value.as_str().unwrap_or("");
                let mode = if action == "diff" { "diff" } else { "file" };
                self.selected = Some(path.to_owned());
                self.modes.insert(path.to_owned(), mode.to_owned());
                self.documents.remove(&(path.to_owned(), mode.to_owned()));
                host.open_document(path, false, true);
            }
            "file" => self.file(host, value.str("id"), value.str("event"))?,
            _ => {}
        }
        Ok(())
    }
}

impl Files {
    fn expand_to(&mut self, path: &str) {
        self.expanded.extend(
            tree::parents(path)
                .into_iter()
                .filter(|parent| *parent != ".")
                .map(str::to_owned),
        );
    }

    fn refresh(&mut self, host: &Host) -> Result {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() < deadline)
        {
            return Ok(());
        }
        let changes = host.api("/workspace/changes", &[])?;
        (self.statuses, self.folders) = tree::colours(changes.list("entries"));
        self.listings.clear();
        self.search = None;
        self.deadline = Some(Instant::now() + POLL);
        Ok(())
    }

    fn row(&self, path: &str, label: &str, depth: usize, directory: bool) -> Value {
        let status = self.statuses.get(path).map(String::as_str);
        let tone = if directory {
            self.folders.get(path).copied().unwrap_or("default")
        } else {
            status.and_then(tree::tone).unwrap_or("default")
        };
        let mut menu = Vec::new();
        if !directory {
            menu.push(
                item("Открыть", "open")
                    .with("value", path)
                    .with("icon", "external-link"),
            );
            if status.is_some_and(|status| !status.is_empty()) {
                menu.push(
                    item("Показать изменения", "diff")
                        .with("value", path)
                        .with("icon", "file-diff"),
                );
            }
        }
        let copy = item("Копировать путь", "copy")
            .with("value", path)
            .with("icon", "copy");
        menu.push(if menu.is_empty() {
            copy
        } else {
            copy.with("separated", true)
        });
        let expanded = if directory {
            Value::from(self.expanded.contains(path))
        } else {
            Value::Null
        };
        let mut row = json!({"id": path, "label": label, "depth": depth, "expanded": expanded, "tone": tone,
                             "selected": self.selected.as_deref() == Some(path), "menu": menu});
        if !directory {
            row["icon"] = file_icon(path).into();
        }
        if status == Some("deleted") {
            row["strike"] = true.into();
        }
        row
    }

    fn rows(&mut self, host: &Host) -> Result<Vec<Value>> {
        self.truncated = false;
        if !self.query.is_empty() {
            if self
                .search
                .as_ref()
                .is_none_or(|(query, _)| *query != self.query)
            {
                self.search = Some((
                    self.query.clone(),
                    host.api("/workspace/search", &[("q", &self.query)])?,
                ));
            }
            let (_, result) = self.search.as_ref().expect("fetched above");
            self.truncated = result.bool("truncated");
            return Ok(tree::search_rows(
                result,
                |path, label, depth, directory| self.row(path, label, depth, directory),
            ));
        }
        let mut rows = Vec::new();
        self.visit(host, "", 0, &mut rows)?;
        Ok(rows)
    }

    fn visit(&mut self, host: &Host, path: &str, depth: usize, rows: &mut Vec<Value>) -> Result {
        if !self.listings.contains_key(path) {
            match host.api("/workspace/list", &[("path", path)]) {
                Ok(listing) => {
                    self.listings.insert(path.to_owned(), listing);
                }
                Err(error) if path.is_empty() => return Err(error),
                // The directory vanished, for example the agent removed it.
                Err(_) => {
                    self.expanded.remove(path);
                    return Ok(());
                }
            }
        }
        let listing = self.listings[path].clone();
        self.truncated |= listing.bool("truncated");
        for entry in listing.list("entries") {
            let directory = entry.str("kind") == "directory";
            rows.push(self.row(entry.str("path"), entry.str("name"), depth, directory));
            if directory && self.expanded.contains(entry.str("path")) {
                self.visit(host, entry.str("path"), depth + 1, rows)?;
            }
        }
        Ok(())
    }

    fn file(&mut self, host: &Host, path: &str, event: &str) -> Result {
        let directory = self
            .rows(host)?
            .iter()
            .any(|row| row.str("id") == path && !row.at("expanded").is_null());
        self.selected = Some(path.to_owned());
        if directory {
            let toggle = matches!(event, "select" | "open");
            if !self.query.is_empty() && toggle {
                self.query.clear();
                self.expand_to(path);
            }
            match event {
                "select" | "open" => {
                    if !self.expanded.remove(path) {
                        self.expanded.insert(path.to_owned());
                    }
                }
                "expand" => {
                    self.expanded.insert(path.to_owned());
                }
                "collapse" => {
                    self.expanded.remove(path);
                }
                _ => {}
            }
        } else if matches!(event, "select" | "preview" | "open") {
            self.modes.insert(path.to_owned(), "file".to_owned());
            host.open_document(path, event != "open", event == "open");
        }
        Ok(())
    }
}

fn main() {
    run(Files::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_carry_git_colours_and_menus() {
        let mut files = Files::default();
        (files.statuses, files.folders) =
            tree::colours(&[json!({"path": "lib/gone.c", "status": "deleted"})]);
        let row = files.row("lib/gone.c", "gone.c", 0, false);
        assert!(row.bool("strike"));
        assert_eq!(row.str("icon"), "file-code");
        assert_eq!(row.str("tone"), "danger");
        let actions: Vec<&str> = row
            .list("menu")
            .iter()
            .map(|item| item.str("action"))
            .collect();
        assert_eq!(actions, ["open", "diff", "copy"]);
        let folder = files.row("lib", "lib", 0, true);
        assert_eq!(
            (folder.str("tone"), folder.bool("expanded")),
            ("warning", false)
        );
        assert_eq!(folder.list("menu").len(), 1);
        assert!(folder.list("menu")[0].get("separated").is_none());
    }
}
