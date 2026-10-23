mod details;
mod document;
mod graph;
mod history;
mod list;
mod model;
mod panel;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use proteus_extension::ui::muted;
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, json, run};

use document::Document;
use model::Group;

const POLL: Duration = Duration::from_secs(5);
const REMOTE: Duration = Duration::from_secs(130);
const HOOKS: Duration = Duration::from_secs(120);

/// Loaded pages of `/workspace/log`.
#[derive(Clone, Debug, Default)]
pub struct Log {
    pub entries: Vec<Value>,
    pub more: bool,
}

#[derive(Default)]
pub struct Git {
    changes: Option<Value>,
    deadline: Option<Instant>,
    branches: Option<Vec<Value>>,
    /// History of the current branch and, for the graph document, of every ref.
    log: Option<Log>,
    graph: Option<Log>,
    /// Whether HEAD has a parent to uncommit to, by HEAD's short hash.
    head_parent: Option<(String, bool)>,
    /// Branch and HEAD commit the branches and the logs were read for.
    head: Option<(Value, Value)>,
    collapsed: HashSet<&'static str>,
    group: Group,
    tab: usize,
    selected: Option<String>,
    commit: Option<String>,
    message: String,
    /// Message put into the editor when amend was turned on.
    amend_message: Option<String>,
    amend: bool,
    /// The commit editor is hidden, only its footer shows.
    editor_collapsed: bool,
    signoff: bool,
    no_verify: bool,
    error: Option<String>,
    /// Working tree diffs by path; dropped when the changes differ.
    diffs: HashMap<String, Value>,
    /// Commits by hash and per-file commit diffs by (hash, path): never change.
    commits: HashMap<String, Value>,
    commit_diffs: HashMap<(String, String), Value>,
    /// File picked in a commit document, by commit hash.
    commit_files: HashMap<String, String>,
    detail_folders: HashSet<String>,
    graph_query: String,
    graph_case: bool,
}

impl Extension for Git {
    fn poll(&self) -> Option<Duration> {
        Some(POLL)
    }

    fn session_changed(&mut self, _: &mut Host) {
        *self = Self {
            group: self.group,
            editor_collapsed: self.editor_collapsed,
            collapsed: std::mem::take(&mut self.collapsed),
            ..Self::default()
        };
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
        let changes = self.changes();
        let branch = branch_name(&changes);
        match surface {
            Surface::Badge => {
                let mut lines = vec![format!("Изменений: {}", changes.list("entries").len())];
                lines.extend(self.sync_line());
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
                if !host.is_shown(view) {
                    // A new panel opens on its first tab.
                    self.tab = 0;
                }
                let root = self.panel(host, &branch)?;
                host.show(view, root);
            }
            Surface::Document => {
                let root = self.document(host, &Document::parse(argument.unwrap_or("")));
                host.show(view, root);
            }
            Surface::Page => {}
        }
        Ok(())
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        let entries = self.changes().list("entries").to_vec();
        if action != "message" {
            self.error = None;
        }
        let paths = |value: &Value| -> Vec<String> {
            match value {
                Value::String(path) => vec![path.clone()],
                _ => value
                    .as_array()
                    .map(|paths| {
                        paths
                            .iter()
                            .filter_map(|path| path.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        };
        match action {
            "tab" => self.tab = value.as_u64().unwrap_or(0) as usize,
            "group" => {
                self.group = match value.as_str() {
                    Some("staging") => Group::Staging,
                    _ => Group::Status,
                }
            }
            "message" => self.message = value.as_str().unwrap_or("").to_owned(),
            "amend" => self.toggle_amend(host),
            "toggle_editor" => self.editor_collapsed = !self.editor_collapsed,
            "signoff" => self.signoff = !self.signoff,
            "no_verify" => self.no_verify = !self.no_verify,
            "commit" => self.commit(host, value.as_str(), &entries),
            "stage" | "unstage" | "discard" => {
                let paths = paths(value);
                if !paths.is_empty() {
                    self.write(
                        host,
                        &format!("/workspace/{action}"),
                        json!({"paths": paths}),
                    );
                }
            }
            "stage_all" | "unstage_all" | "discard_all" => {
                let path = format!("/workspace/{}", action.trim_end_matches("_all"));
                self.write(host, &path, json!({"all": true}));
            }
            "discard_tracked" => {
                let paths: Vec<&str> = entries
                    .iter()
                    .filter(|entry| !model::created(entry) && !model::conflict(entry))
                    .map(|entry| entry.str("path"))
                    .collect();
                if !paths.is_empty() {
                    self.write(host, "/workspace/discard", json!({"paths": paths}));
                }
            }
            "trash" | "trash_untracked" => {
                let paths = if action == "trash" {
                    paths(value)
                } else {
                    entries
                        .iter()
                        .filter(|entry| model::untracked(entry))
                        .map(|entry| entry.str("path").to_owned())
                        .collect()
                };
                if !paths.is_empty()
                    && self
                        .send(host, "/workspace/delete", json!({"paths": paths}), POLL)
                        .is_some()
                {
                    self.deadline = None;
                }
            }
            "ignore" | "exclude" => {
                let target = if action == "ignore" {
                    "gitignore"
                } else {
                    "exclude"
                };
                let body = json!({"paths": paths(value), "target": target});
                self.write(host, "/workspace/ignore", body);
            }
            "stash" | "stash_staged" => {
                let body = json!({"staged_only": action == "stash_staged"});
                self.write(host, "/workspace/stash", body);
            }
            "stash_pop" => {
                self.write(host, "/workspace/stash/pop", json!({}));
            }
            "uncommit" => {
                self.write(host, "/workspace/uncommit", json!({}));
            }
            "fetch" | "pull" | "pull_rebase" | "push" | "force_push" | "publish" => {
                let (path, body) = match action {
                    "fetch" => ("/workspace/fetch", json!({})),
                    "pull" => ("/workspace/pull", json!({})),
                    "pull_rebase" => ("/workspace/pull", json!({"rebase": true})),
                    "push" => ("/workspace/push", json!({})),
                    "force_push" => ("/workspace/push", json!({"force": true})),
                    _ => ("/workspace/push", json!({"set_upstream": true})),
                };
                if let Some(changes) = self.send(host, path, body, REMOTE) {
                    self.update(changes);
                    self.graph = None;
                }
            }
            "checkout" | "create_branch" => {
                let (branch, create) = match action {
                    "checkout" => (value.as_str().unwrap_or("").to_owned(), false),
                    _ => (value.str("text").trim().to_owned(), true),
                };
                if !branch.is_empty()
                    && self
                        .write(
                            host,
                            "/workspace/checkout",
                            json!({"branch": branch, "create": create}),
                        )
                        .is_some()
                {
                    self.branches = None;
                    if view == "popup" {
                        host.close_popup();
                    }
                }
            }
            "copy" => host.clipboard(value.as_str().unwrap_or("")),
            "copy_path" => {
                let path = value.as_str().unwrap_or("");
                let full = match host.workspace.as_deref() {
                    Some(root) => format!("{}/{path}", root.trim_end_matches('/')),
                    None => path.to_owned(),
                };
                host.clipboard(&full);
            }
            "diff" => {
                let path = value.as_str().unwrap_or("");
                self.selected = Some(path.to_owned());
                self.diffs.remove(path);
                host.open_document(&Document::Diff(path.to_owned()).argument(), true, true);
            }
            "diff_all" => {
                self.diffs.clear();
                host.open_document(&Document::Changes.argument(), true, true);
            }
            "graph" => host.open_document(&Document::Graph.argument(), false, true),
            "open_commit" | "keep_commit" => {
                let hash = value.as_str().unwrap_or("");
                if !hash.is_empty() {
                    self.commit = Some(hash.to_owned());
                    host.open_document(&self.commit_argument(hash), action != "keep_commit", true);
                }
            }
            "more" | "graph_more" => self.more(host, action == "graph_more")?,
            "refresh" => self.deadline = None,
            "tree" => self.tree(host, value.str("id"), value.str("event"), &entries),
            "log" | "graph_log" => {
                let hash = value.str("id").to_owned();
                self.commit = Some(hash.clone());
                if action == "log" || value.str("event") == "open" {
                    host.browse_document(&self.commit_argument(&hash), value.str("event"));
                }
            }
            "close_commit_details" => self.commit = None,
            "graph_search" => {
                self.graph_query = value.as_str().unwrap_or("").to_owned();
                self.graph_find(false);
            }
            "graph_case" => {
                self.graph_case = !self.graph_case;
                self.graph_find(false);
            }
            "graph_previous" | "graph_next" => self.graph_find(action == "graph_previous"),
            "detail_file" => self.detail_file(host, value.str("id"), value.str("event")),
            "commit_file" => {
                let (hash, path) = (value.at("id").as_str(), view.strip_prefix("document:"));
                if let (Some(file), Some(argument)) = (hash, path)
                    && let Document::Commit(hash) = Document::parse(argument)
                {
                    self.commit_files.insert(hash, file.to_owned());
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn branch_name(changes: &Value) -> String {
    [changes.str("branch"), changes.at("head").str("short")]
        .into_iter()
        .find(|name| !name.is_empty())
        .unwrap_or("—")
        .to_owned()
}

impl Git {
    fn changes(&self) -> Value {
        self.changes.clone().unwrap_or_default()
    }

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
            self.diffs.clear();
        }
        let head = (
            changes.at("branch").clone(),
            changes.at("head").at("short").clone(),
        );
        if self.head.as_ref() != Some(&head) {
            self.branches = None;
            self.log = None;
            self.graph = None;
            self.head = Some(head);
        }
        self.changes = Some(changes);
        self.deadline = Some(Instant::now() + POLL);
    }

    /// Sends a git operation; a failure is kept for the views instead of an error.
    fn send(&mut self, host: &Host, path: &str, body: Value, timeout: Duration) -> Option<Value> {
        let mut payload = json!({"session_dir": host.session});
        if let (Some(payload), Value::Object(fields)) = (payload.as_object_mut(), body) {
            payload.extend(fields);
        }
        match host.post_with_timeout(path, payload, timeout) {
            Err(error) => {
                self.error = Some(error.to_string());
                None
            }
            Ok(result) => {
                self.error = None;
                Some(result)
            }
        }
    }

    /// An operation that answers with the new changes (or a commit and them).
    fn write(&mut self, host: &Host, path: &str, body: Value) -> Option<Value> {
        let result = self.send(host, path, body, HOOKS)?;
        let changes = if result.get("commit").is_some() {
            result.at("changes").clone()
        } else {
            result.clone()
        };
        self.update(changes);
        Some(result)
    }

    fn toggle_amend(&mut self, host: &Host) {
        self.amend = !self.amend;
        if self.amend {
            if self.message.trim().is_empty() {
                let short = self.changes().at("head").str("short").to_owned();
                let message = self
                    .commit_details(host, &short)
                    .map(|commit| {
                        let (summary, body) = (commit.str("summary"), commit.str("body"));
                        if body.is_empty() {
                            summary.to_owned()
                        } else {
                            format!("{summary}\n\n{body}")
                        }
                    })
                    .unwrap_or_else(|_| self.changes().at("head").str("summary").to_owned());
                self.message = message.clone();
                self.amend_message = Some(message);
            }
        } else if self.amend_message.take().as_ref() == Some(&self.message) {
            self.message.clear();
        }
    }

    fn commit(&mut self, host: &Host, typed: Option<&str>, entries: &[Value]) {
        if let Some(typed) = typed {
            self.message = typed.to_owned();
        }
        let message = model::message(&self.message, entries);
        let button =
            model::commit_button(model::Counts::of(entries), self.amend, message.is_some());
        if let Some(reason) = button.blocked {
            self.error = Some(reason.to_owned());
            return;
        }
        let body = json!({"message": message.unwrap_or_default(), "amend": self.amend,
                          "signoff": self.signoff, "no_verify": self.no_verify,
                          "all_tracked": button.all_tracked});
        if self.write(host, "/workspace/commit", body).is_some() {
            self.message.clear();
            self.amend = false;
            self.amend_message = None;
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

    fn sync_line(&self) -> Option<String> {
        let changes = self.changes();
        Some(
            match changes.opt_str("upstream").filter(|up| !up.is_empty()) {
                None => "Ветка не опубликована".to_owned(),
                Some(upstream) => format!(
                    "{upstream}: ↑{} ↓{}",
                    changes.at("ahead"),
                    changes.at("behind")
                ),
            },
        )
    }
}

fn main() {
    run(Git::default());
}
