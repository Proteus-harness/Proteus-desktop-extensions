mod changes;
mod view;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use proteus_extension::ui::muted;
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, json, run};

const POLL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Git {
    changes: Option<Value>,
    deadline: Option<Instant>,
    branches: Option<Vec<Value>>,
    log: Option<Vec<Value>>,
    /// Branch and HEAD commit the branches and the log were read for.
    head: Option<(Value, Value)>,
    collapsed: HashSet<String>,
    history: bool,
    selected: Option<String>,
    message: String,
    amend: bool,
    error: Option<String>,
    documents: HashMap<String, Value>,
}

impl Extension for Git {
    fn poll(&self) -> Option<Duration> {
        Some(POLL)
    }

    fn session_changed(&mut self, _: &mut Host) {
        self.message.clear();
        self.amend = false;
        self.error = None;
        self.selected = None;
    }

    fn event(&mut self, _: &mut Host, event: &str) {
        if matches!(event, "session_opened" | "run_settled") {
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
            if surface == Surface::Badge {
                host.hide_badge(view, "Git");
            } else {
                host.show(view, muted("Нет активной сессии."));
            }
            return Ok(());
        }
        self.refresh(host)?;
        let changes = self.changes.clone().unwrap_or_default();
        let branch = [changes.str("branch"), changes.at("head").str("short")]
            .into_iter()
            .find(|name| !name.is_empty())
            .unwrap_or("—")
            .to_owned();
        match surface {
            Surface::Badge => {
                let mut lines = vec![format!("Изменений: {}", changes.list("entries").len())];
                lines.extend(self.sync_lines());
                let face = json!({"type": "text", "text": branch, "icon": true, "caret": true, "tone": "muted"});
                host.send_badge(
                    view,
                    &format!("Ветка {branch}"),
                    lines,
                    Some(face),
                    !changes.bool("repository"),
                );
            }
            Surface::Popup => {
                let root = self.branch_picker(host, &branch)?;
                host.show(view, root);
            }
            Surface::Panel => {
                let root = self.panel(host, &branch)?;
                host.show_with_keys(view, root, json!({"ctrl-enter": "commit"}));
            }
            Surface::Document => {
                let root = self.document(host, argument.unwrap_or(""));
                host.show(view, root);
            }
            Surface::Page => {}
        }
        Ok(())
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        let entries = self
            .changes
            .as_ref()
            .map(|changes| changes.list("entries").to_vec())
            .unwrap_or_default();
        match action {
            "message" => self.message = value.as_str().unwrap_or("").to_owned(),
            "amend" => {
                self.amend = value.bool("checked");
                if self.amend && self.message.trim().is_empty() {
                    let head = self
                        .changes
                        .as_ref()
                        .map(|changes| changes.at("head").str("summary"));
                    self.message = head.unwrap_or("").to_owned();
                }
            }
            "commit" => {
                let message = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| self.message.clone());
                let body = json!({"message": message, "amend": self.amend});
                if self.write(host, "/workspace/commit", body).is_some() {
                    self.message.clear();
                    self.amend = false;
                } else {
                    self.message = message;
                }
            }
            "stage" | "unstage" | "discard" => {
                self.write(
                    host,
                    &format!("/workspace/{action}"),
                    json!({"paths": value}),
                );
            }
            "stage_all" | "unstage_all" | "discard_all" => {
                let path = format!(
                    "/workspace/{}",
                    action.strip_suffix("_all").unwrap_or(action)
                );
                self.write(host, &path, json!({"all": true}));
            }
            "checkout" => {
                if self
                    .write(host, "/workspace/checkout", json!({"branch": value}))
                    .is_some()
                    && view == "popup"
                {
                    host.close_popup();
                }
            }
            "copy" => host.clipboard(value.as_str().unwrap_or("")),
            "diff" => {
                let path = value.as_str().unwrap_or("");
                self.selected = Some(path.to_owned());
                host.open_document(path, false, true);
            }
            "refresh" => self.deadline = None,
            "tree" => self.tree(host, value.str("id"), value.str("event"), &entries),
            _ => {}
        }
        Ok(())
    }
}

impl Git {
    fn refresh(&mut self, host: &Host) -> Result {
        if self
            .deadline
            .is_none_or(|deadline| Instant::now() >= deadline)
        {
            let changes = host.api("/workspace/changes", &[])?;
            self.update(changes);
        }
        Ok(())
    }

    fn update(&mut self, changes: Value) {
        if self.changes.as_ref() != Some(&changes) {
            self.documents.clear();
        }
        let head = (
            changes.at("branch").clone(),
            changes.at("head").at("short").clone(),
        );
        if self.head.as_ref() != Some(&head) {
            self.branches = None;
            self.log = None;
            self.head = Some(head);
        }
        self.changes = Some(changes);
        self.deadline = Some(Instant::now() + POLL);
    }

    /// Sends a git operation; a failure is kept for the views instead of an error.
    fn write(&mut self, host: &Host, path: &str, body: Value) -> Option<Value> {
        let mut payload = json!({"session_dir": host.session});
        if let (Some(payload), Value::Object(fields)) = (payload.as_object_mut(), body) {
            payload.extend(fields);
        }
        match host.post_with_timeout(path, payload, Duration::from_secs(120)) {
            Err(error) => {
                self.error = Some(error.to_string());
                None
            }
            Ok(result) => {
                self.error = None;
                let changes = if result.get("commit").is_some() {
                    result.at("changes").clone()
                } else {
                    result.clone()
                };
                self.update(changes);
                Some(result)
            }
        }
    }

    fn branch_list(&mut self, host: &Host) -> Result<Vec<Value>> {
        if self.branches.is_none() {
            self.branches = Some(
                host.api("/workspace/branches", &[])?
                    .list("branches")
                    .to_vec(),
            );
        }
        Ok(self.branches.clone().unwrap_or_default())
    }

    fn sync_lines(&self) -> Vec<String> {
        let changes = self.changes.as_ref().unwrap_or(&Value::Null);
        match changes
            .opt_str("upstream")
            .filter(|upstream| !upstream.is_empty())
        {
            None => vec!["Без upstream".to_owned()],
            Some(upstream) => vec![format!(
                "{upstream}: ↑{} ↓{}",
                changes.at("ahead"),
                changes.at("behind")
            )],
        }
    }

    fn tree(&mut self, host: &Host, identity: &str, event: &str, entries: &[Value]) {
        if identity == "section:history" {
            if matches!(event, "select" | "open" | "expand" | "collapse") {
                self.history = event == "expand" || (event != "collapse" && !self.history);
            }
            return;
        }
        if let Some(key) = identity.strip_prefix("section:") {
            match event {
                "check" => {
                    let groups = changes::groups(entries);
                    let unstage = groups
                        .get(key)
                        .is_none_or(|items| items.iter().all(|entry| entry.at("staged").truthy()));
                    let paths = changes::section_paths(entries, key, unstage);
                    if !paths.is_empty() {
                        let path = if unstage {
                            "/workspace/unstage"
                        } else {
                            "/workspace/stage"
                        };
                        self.write(host, path, json!({"paths": paths}));
                    }
                }
                "select" | "open" => {
                    if !self.collapsed.remove(key) {
                        self.collapsed.insert(key.to_owned());
                    }
                }
                "expand" => {
                    self.collapsed.remove(key);
                }
                "collapse" => {
                    self.collapsed.insert(key.to_owned());
                }
                _ => {}
            }
            return;
        }
        if identity.starts_with("commit:") {
            return;
        }
        let Some(entry) = entries.iter().find(|entry| entry.str("path") == identity) else {
            return;
        };
        self.selected = Some(identity.to_owned());
        if event == "check" {
            let path = if entry.at("staged").truthy() {
                "/workspace/unstage"
            } else {
                "/workspace/stage"
            };
            self.write(host, path, json!({"paths": [identity]}));
        } else if matches!(event, "select" | "preview" | "open") {
            host.open_document(identity, event != "open", event == "open");
        }
    }
}

fn main() {
    run(Git::default());
}
