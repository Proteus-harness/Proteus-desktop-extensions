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
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, json, run};

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
    documents: HashMap<(String, String), Value>,
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
        if host.session.as_deref().is_none_or(str::is_empty) {
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
                let root = self.document(host, argument.unwrap_or(""));
                host.show(view, root);
            }
            _ => {}
        }
        Ok(())
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        match action {
            "escape" => host.focus(view, None),
            "files" => self.file(host, value.str("id"), value.str("event")),
            _ => self.operation(host, action, value),
        }
        Ok(())
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
                .map(|listing| tree::Listing::parse(&listing))
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
        let rows = lines.iter().map(|line| self.row(line)).collect();
        children.push(
            proteus_extension::ui::tree("files", rows, "files")
                .with("indent", 20)
                .with("chevrons", false)
                .with("guides", true),
        );
        Ok(fill(children))
    }

    fn row(&self, line: &Line) -> Value {
        let path = line.path.as_str();
        let directory = line.kind == Kind::Directory;
        let selected = self.selected.as_deref() == Some(path);
        let status = self.git.status(path, directory);
        let absolute = self.absolute(path);
        let change = (!directory).then(|| self.git.change(path)).flatten();
        let items = menu::build(&menu::Target {
            path,
            directory,
            absolute: &absolute,
            repository: self.git.repository,
            changed: change.is_some(),
            restorable: self.git.restorable(path),
        });
        let icon = match line.expanded {
            Some(open) => icons::folder(open),
            None => icons::file(path, line.kind == Kind::Symlink),
        };
        json!({
            "id": if path.is_empty() { ROOT } else { path },
            "label": line.label,
            "depth": line.depth,
            "expanded": line.expanded,
            "tone": git::tone(status, selected),
            "selected": selected,
            "icon": icon,
            "icon_tone": "muted",
            "hint": absolute,
            "menu": items,
        })
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
                    .map(|listing| tree::Listing::parse(&listing))
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
        let row = files.row(&line("src/main.rs", "main.rs", Kind::File, None));
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

        let folder = files.row(&line("docs", "docs", Kind::Directory, Some(false)));
        assert_eq!(
            (folder.str("tone"), folder.str("icon")),
            ("success", "folder")
        );

        files.selected = Some("README".to_owned());
        let plain = files.row(&line("README", "README", Kind::File, None));
        assert_eq!(plain.str("tone"), "default");
        let other = files.row(&line("LICENSE", "LICENSE", Kind::File, None));
        assert_eq!(other.str("tone"), "muted");

        let root = files.row(&Line {
            depth: 0,
            ..line("", "proj", Kind::Directory, Some(true))
        });
        assert_eq!((root.str("id"), root.str("icon")), (ROOT, "folder-open"));
        assert_eq!(root.str("hint"), "/w/proj");
        assert_eq!(root.list("menu").len(), 4);
    }
}
