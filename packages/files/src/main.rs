mod document;
mod git;
mod icons;
mod menu;
mod operations;
mod paths;
mod tree;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use proteus_extension::ui::{Node, fill, muted, text};
use proteus_extension::{Extension, Host, Map, Result, Surface, Value, ValueExt, json, run};

use git::Git;
use tree::{Kind, Line, Project};

const POLL: Duration = Duration::from_secs(5);
/// Tree id of the session folder row; entry paths never equal it.
const ROOT: &str = ".";

#[derive(Default)]
struct Files {
    project: Project,
    git: Git,
    /// Absolute folder of the addressed session from `/sessions/current`.
    cwd: Option<String>,
    selected: Option<String>,
    deadline: Option<Instant>,
    /// The next refresh lists every folder again, not only the open ones.
    stale: bool,
    /// Failure of the last file operation, shown above the tree.
    error: Option<String>,
    /// Fetched documents by path and mode (`file` or `diff`).
    documents: HashMap<(String, String), document::SavedDocument>,
    /// Live document views in this session, shared across its windows.
    document_views: HashMap<String, String>,
    modes: HashMap<String, String>,
}

impl Extension for Files {
    fn poll(&self) -> Option<Duration> {
        Some(POLL)
    }

    fn session_changed(&mut self, _: &mut Host) {
        // An addressed render can switch sessions before session_opened.
        *self = Self::default();
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
            self.project.reveal(document);
        }
        if matches!(event, "session_opened" | "run_settled") {
            self.documents.clear();
            self.changed();
        }
    }

    fn render(
        &mut self,
        host: &mut Host,
        view: &str,
        surface: Surface,
        argument: Option<&str>,
    ) -> Result {
        if surface == Surface::Preview {
            let mut load = |path: &str| {
                Ok(tree::Listing::parse(match path {
                    "" => {
                        json!({"entries":[{"name":"src","path":"src","kind":"directory"},{"name":"README.md","path":"README.md","kind":"file"}]})
                    }
                    "src" => {
                        json!({"entries":[{"name":"main.rs","path":"src/main.rs","kind":"file"},{"name":"ui.rs","path":"src/ui.rs","kind":"file"}]})
                    }
                    _ => json!({"entries":[]}),
                }))
            };
            self.project.expanded.insert("src".into());
            let lines = self.project.lines("Пример проекта", &mut load)?;
            let rows = lines.into_iter().map(|line| self.row(line)).collect();
            host.show(
                view,
                proteus_extension::ui::tree("files-preview", rows, "files")
                    .with("indent", 20)
                    .with("chevrons", false)
                    .with("guides", true),
            );
            return Ok(());
        }
        let has_session = host
            .session
            .as_deref()
            .is_some_and(|session| !session.is_empty());
        self.track_document_view(
            view,
            (has_session && surface == Surface::Document).then_some(argument.unwrap_or("")),
        );
        if !has_session {
            host.show(view, muted("Нет активной сессии."));
            return Ok(());
        }
        match surface {
            Surface::Panel => {
                self.refresh(host);
                let root = self.panel(host)?;
                host.show_with_keys(view, root, json!({"escape": "escape"}));
            }
            Surface::Document => {
                let children = self.document(host, argument.unwrap_or(""));
                host.show_shared_fill(view, children);
            }
            _ => {}
        }
        Ok(())
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        if host.surface(view) == Some(Surface::Preview) {
            if action == "files" {
                self.selected = value.opt_str("id").map(str::to_owned);
            }
            return Ok(());
        }
        match action {
            "escape" => host.focus(view, None),
            "files" => self.file(host, value.str("id"), value.str("event")),
            _ => self.operation(host, action, value),
        }
        Ok(())
    }

    fn closed(&mut self, _: &mut Host, view: &str) {
        self.track_document_view(view, None);
    }
}

impl Files {
    /// Workspace changed outside the panel: list everything again.
    fn changed(&mut self) {
        self.deadline = None;
        self.stale = true;
    }

    fn refresh(&mut self, host: &Host) {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() < deadline)
        {
            return;
        }
        if self.cwd.is_none() {
            // The full config awaits the model catalogue. Session metadata
            // already carries the addressed workspace without that dependency.
            self.cwd = host
                .api("/sessions/current", &[])
                .ok()
                .and_then(|sessions| {
                    sessions
                        .as_array()?
                        .iter()
                        .find(|summary| Some(summary.str("session_dir")) == host.session.as_deref())
                        .and_then(|summary| summary.opt_str("workspace_path"))
                        .map(str::to_owned)
                })
                .filter(|cwd| !cwd.is_empty());
        }
        // Without Git (or when status fails) the tree simply stays uncoloured.
        self.git = host
            .api("/workspace/changes", &[])
            .map(|changes| Git::parse(&changes))
            .unwrap_or_default();
        self.project.invalidate(std::mem::take(&mut self.stale));
        self.deadline = Some(Instant::now() + POLL);
    }

    fn root_label(&self, host: &Host) -> String {
        let folder = self
            .cwd
            .as_deref()
            .or(host.workspace.as_deref())
            .unwrap_or("");
        match paths::name(folder) {
            "" => "Проект".to_owned(),
            name => name.to_owned(),
        }
    }

    fn absolute(&self, path: &str) -> String {
        match &self.cwd {
            Some(cwd) => paths::absolute(cwd, path),
            None if path.is_empty() => ".".to_owned(),
            None => path.to_owned(),
        }
    }

    fn lines(&mut self, host: &Host) -> Result<Vec<Line>> {
        let label = self.root_label(host);
        let mut load = |path: &str| {
            host.api("/workspace/list", &[("path", path)])
                .map(tree::Listing::parse)
        };
        self.project.lines(&label, &mut load)
    }

    fn panel(&mut self, host: &Host) -> Result<Value> {
        let lines = self.lines(host)?;
        if self
            .selected
            .as_deref()
            .is_some_and(|selected| !self.project.exists(selected))
        {
            self.selected = None;
        }
        let mut children = Vec::new();
        if let Some(error) = &self.error {
            children.push(text(error).with("tone", "danger"));
        }
        if self.project.truncated {
            children.push(muted(
                "Часть папок показана не полностью: слишком много файлов.",
            ));
        }
        if lines.len() == 1 && lines[0].expanded == Some(true) {
            children.push(muted("Папка пуста."));
        }
        let rows = lines.into_iter().map(|line| self.row(line)).collect();
        children.push(
            proteus_extension::ui::tree("files", rows, "files")
                .with("indent", 20)
                .with("chevrons", false)
                .with("guides", true),
        );
        Ok(fill(children))
    }

    fn row(&self, line: Line) -> Value {
        let path = line.path.as_str();
        let directory = line.kind == Kind::Directory;
        let selected = self.selected.as_deref() == Some(path);
        let (status, changed, restorable) = self.git.row(path, directory);
        let absolute = self.absolute(path);
        let items = menu::build(&menu::Target {
            path,
            directory,
            absolute: &absolute,
            repository: self.git.repository,
            changed,
            restorable,
        });
        let icon = match line.expanded {
            Some(open) => icons::folder(open),
            None => icons::file(path, line.kind == Kind::Symlink),
        };
        // These strings already belong to the row. The menu needs the same
        // compact nested containers as the former JSON conversion, but its
        // keys and payloads do not need to be copied to achieve that.
        let mut row = json!({
            "id": null,
            "label": null,
            "depth": line.depth,
            "expanded": line.expanded,
            "tone": git::tone(status),
            "selected": selected,
            "icon": icon,
            "icon_tone": "muted",
            "hint": null,
            "menu": null,
        });
        row["id"] = if line.path.is_empty() {
            Value::from(ROOT)
        } else {
            tight_text(line.path)
        };
        row["label"] = tight_text(line.label);
        row["hint"] = tight_text(absolute);
        row["menu"] = compact_menu(Value::Array(items));
        row
    }

    fn file(&mut self, host: &Host, id: &str, event: &str) {
        let path = if id == ROOT { "" } else { id };
        self.selected = Some(path.to_owned());
        if matches!(event, "select" | "open") {
            self.error = None;
        }
        if self.project.kind(path) == Some(Kind::Directory) {
            let mut load = |folder: &str| {
                host.api("/workspace/list", &[("path", folder)])
                    .map(tree::Listing::parse)
            };
            let open = self.project.is_expanded(path);
            match event {
                // Zed toggles a folder on click and on Enter.
                "select" | "open" if open => self.project.collapse(path),
                "select" | "open" | "expand" => self.project.expand(path, &mut load),
                "collapse" => self.project.collapse(path),
                _ => {}
            }
        } else if matches!(event, "select" | "preview" | "open") && !path.is_empty() {
            self.modes.insert(path.to_owned(), "file".to_owned());
            host.browse_document(path, event);
        }
    }
}

fn tight_text(value: String) -> Value {
    Value::String(String::from(value.into_boxed_str()))
}

fn compact_menu(value: Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut compact = Map::with_capacity(fields.len());
            for (key, value) in fields {
                compact.insert(key, compact_menu(value));
            }
            Value::Object(compact)
        }
        Value::Array(items) => {
            let mut items = items.into_boxed_slice();
            for value in &mut *items {
                *value = compact_menu(value.take());
            }
            Value::Array(items.into_vec())
        }
        Value::String(text) => tight_text(text),
        other => other,
    }
}

fn main() {
    run(Files::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(path: &str, label: &str, kind: Kind, expanded: Option<bool>) -> Line {
        Line {
            path: path.to_owned(),
            label: label.to_owned(),
            depth: 1,
            kind,
            expanded,
        }
    }

    #[test]
    fn rows_carry_zed_colours_icons_and_hints() {
        let mut files = Files {
            cwd: Some("/w/proj".to_owned()),
            git: Git::parse(&json!({"repository": true, "entries": [
                {"path": "src/main.rs", "status": "modified"},
                {"path": "docs/new.md", "status": "untracked"},
            ]})),
            ..Files::default()
        };
        let mut edited = line("src/main.rs", "main.rs", Kind::File, None);
        edited.path.reserve(256);
        edited.label.reserve(256);
        let row = files.row(edited);
        for field in ["id", "label", "hint"] {
            let Value::String(text) = &row[field] else {
                panic!("expected string");
            };
            assert_eq!(text.capacity(), text.len());
        }
        let menu = row["menu"].as_array().unwrap();
        assert_eq!(menu.capacity(), menu.len());
        assert_eq!(
            (row.str("tone"), row.str("icon"), row.str("icon_tone")),
            ("warning", "file-code", "muted")
        );
        assert_eq!(row.str("hint"), "/w/proj/src/main.rs");
        assert!(row.get("strike").is_none() && row.get("checked").is_none());
        let actions: Vec<&str> = row
            .list("menu")
            .iter()
            .map(|item| item.str("action"))
            .collect();
        assert!(actions.contains(&"restore") && actions.contains(&"diff"));

        let folder = files.row(line("docs", "docs", Kind::Directory, Some(false)));
        assert_eq!(
            (folder.str("tone"), folder.str("icon")),
            ("success", "folder")
        );

        for (path, name, tone) in [
            ("README", "README", "muted"),
            ("src/main.rs", "main.rs", "warning"),
        ] {
            files.selected = None;
            let mut expected = files.row(line(path, name, Kind::File, None));
            assert_eq!(expected.str("tone"), tone);
            assert!(!expected.bool("selected"));
            expected["selected"] = json!(true);
            files.selected = Some(path.to_owned());
            assert_eq!(files.row(line(path, name, Kind::File, None)), expected);
        }

        let root = files.row(Line {
            depth: 0,
            ..line("", "proj", Kind::Directory, Some(true))
        });
        assert_eq!((root.str("id"), root.str("icon")), (ROOT, "folder-open"));
        assert_eq!(root.str("hint"), "/w/proj");
        assert_eq!(root.list("menu").len(), 4);
    }
}
