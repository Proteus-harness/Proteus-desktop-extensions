// Адаптировано из Zed: crates/editor/src/items.rs (entry_git_aware_label_color)
//! Git status of project entries and the label colour it gives them.

use std::collections::HashMap;

use proteus_extension::{Value, ValueExt};

use crate::paths;

/// Strength of a change; a folder shows the strongest one below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Added,
    Modified,
    Deleted,
    Conflict,
}

impl Status {
    pub fn parse(status: &str) -> Option<Self> {
        Some(match status {
            "conflict" => Self::Conflict,
            "deleted" => Self::Deleted,
            "modified" | "renamed" => Self::Modified,
            "added" | "untracked" => Self::Added,
            _ => return None,
        })
    }
}

/// Like Zed: conflicts and edits are yellow, deletions red, new files green;
/// unchanged entries are muted unless selected.
pub fn tone(status: Option<Status>, selected: bool) -> &'static str {
    match status {
        Some(Status::Conflict | Status::Modified) => "warning",
        Some(Status::Deleted) => "danger",
        Some(Status::Added) => "success",
        None if selected => "default",
        None => "muted",
    }
}

#[derive(Default)]
pub struct Git {
    pub repository: bool,
    /// Server status (`modified`, `untracked`…) of each changed file.
    files: HashMap<String, String>,
    /// Strongest status below each folder; `""` is the session folder.
    folders: HashMap<String, Status>,
}

impl Git {
    pub fn parse(changes: &Value) -> Self {
        let mut git = Self {
            repository: changes.bool("repository"),
            ..Self::default()
        };
        for entry in changes.list("entries") {
            let path = entry.str("path");
            if path.is_empty() || path.starts_with("../") {
                continue;
            }
            git.files
                .insert(path.to_owned(), entry.str("status").to_owned());
            let Some(status) = Status::parse(entry.str("status")) else {
                continue;
            };
            for parent in paths::parents(path) {
                let strongest = git.folders.entry(parent.to_owned()).or_insert(status);
                *strongest = (*strongest).max(status);
            }
        }
        git
    }

    pub fn status(&self, path: &str, directory: bool) -> Option<Status> {
        if directory {
            self.folders.get(path).copied()
        } else {
            self.files
                .get(path)
                .and_then(|status| Status::parse(status))
        }
    }

    /// The server status of a changed file.
    pub fn change(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    /// Tracked edits that `Отменить изменения` restores from HEAD.
    pub fn restorable(&self, path: &str) -> bool {
        matches!(self.change(path), Some("modified" | "renamed" | "deleted"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_extension::json;

    #[test]
    fn folders_take_the_strongest_status_below() {
        let git = Git::parse(&json!({"repository": true, "entries": [
            {"path": "src/a/x.py", "status": "untracked"},
            {"path": "src/b.rs", "status": "modified"},
            {"path": "lib/gone.c", "status": "deleted"},
            {"path": "lib/sub/both.c", "status": "conflict"},
            {"path": "../outside.rs", "status": "modified"},
        ]}));
        assert!(git.repository);
        assert_eq!(git.status("src/a", true), Some(Status::Added));
        assert_eq!(git.status("src", true), Some(Status::Modified));
        assert_eq!(git.status("lib", true), Some(Status::Conflict));
        assert_eq!(git.status("", true), Some(Status::Conflict));
        assert_eq!(git.status("docs", true), None);
        assert_eq!(git.status("lib/gone.c", false), Some(Status::Deleted));
        assert_eq!(git.change("../outside.rs"), None);
        assert!(git.restorable("src/b.rs") && !git.restorable("src/a/x.py"));
    }

    #[test]
    fn tones_follow_zed_priority() {
        assert_eq!(tone(Some(Status::Conflict), false), "warning");
        assert_eq!(tone(Some(Status::Deleted), true), "danger");
        assert_eq!(tone(Some(Status::Modified), false), "warning");
        assert_eq!(tone(Some(Status::Added), false), "success");
        assert_eq!((tone(None, false), tone(None, true)), ("muted", "default"));
        assert_eq!(Status::parse("renamed"), Some(Status::Modified));
        assert_eq!(Status::parse("ignored"), None);
    }
}
