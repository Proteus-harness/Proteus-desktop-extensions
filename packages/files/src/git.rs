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
    /// A changed path remains present even when its server status has no color.
    files: HashMap<String, Option<Status>>,
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
            let status = Status::parse(entry.str("status"));
            git.files.insert(path.to_owned(), status);
            let Some(status) = status else {
                continue;
            };
            for parent in paths::parents(path) {
                match git.folders.get_mut(parent) {
                    // Ancestors already carry at least the status of this folder.
                    Some(strongest) if *strongest >= status => break,
                    Some(strongest) => *strongest = status,
                    None => {
                        git.folders.insert(parent.to_owned(), status);
                    }
                }
            }
        }
        git
    }

    /// Returns the row color, presence of a diff, and restore availability.
    pub fn row(&self, path: &str, directory: bool) -> (Option<Status>, bool, bool) {
        if directory {
            return (self.folders.get(path).copied(), false, false);
        }
        let entry = self.files.get(path).copied();
        let status = entry.flatten();
        let restorable = matches!(status, Some(Status::Modified | Status::Deleted));
        (status, entry.is_some(), restorable)
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
        assert_eq!(git.row("src/a", true).0, Some(Status::Added));
        assert_eq!(git.row("src", true).0, Some(Status::Modified));
        assert_eq!(git.row("lib", true).0, Some(Status::Conflict));
        assert_eq!(git.row("", true).0, Some(Status::Conflict));
        assert_eq!(git.row("docs", true).0, None);
        assert_eq!(git.row("lib/gone.c", false).0, Some(Status::Deleted));
        assert_eq!(git.row("../outside.rs", false), (None, false, false));
        assert!(git.row("src/b.rs", false).2 && !git.row("src/a/x.py", false).2);
    }

    #[test]
    fn folder_rollup_matches_full_walk_for_shared_and_lexical_parents() {
        let rows = [
            ("src//папка/a.rs", "modified"),
            ("src//папка/b.rs", "added"),
            ("src/other/c.rs", "deleted"),
            ("src//папка/a.rs", "untracked"),
            ("src//папка/b.rs", "conflict"),
            ("src//папка/c.rs/", "renamed"),
            ("src/other/c.rs", "ignored"),
            ("src/other/no-status.rs", "unknown"),
            ("src/flat.rs", ""),
            ("root.rs", "untracked"),
            ("../skipped.rs", "conflict"),
            ("", "deleted"),
            ("///", "deleted"),
            ("/src/file.rs", "deleted"),
        ];
        for offset in 0..rows.len() {
            let mut expected_files = HashMap::new();
            let mut expected_folders = HashMap::new();
            let entries: Vec<_> = rows
                .iter()
                .cycle()
                .skip(offset)
                .take(rows.len())
                .map(|&(path, raw)| {
                    if !path.is_empty() && !path.starts_with("../") {
                        expected_files.insert(path.to_owned(), raw.to_owned());
                        if let Some(status) = Status::parse(raw) {
                            let mut parent = path.trim_end_matches('/');
                            while !parent.is_empty() {
                                parent = paths::parent(parent);
                                let strongest =
                                    expected_folders.entry(parent.to_owned()).or_insert(status);
                                *strongest = (*strongest).max(status);
                            }
                        }
                    }
                    json!({"path": path, "status": raw})
                })
                .collect();
            let git = Git::parse(&json!({"repository": true, "entries": entries}));
            assert_eq!(git.files.len(), expected_files.len(), "offset {offset}");
            for (path, raw) in expected_files {
                assert_eq!(
                    git.row(&path, false),
                    (
                        Status::parse(&raw),
                        true,
                        matches!(raw.as_str(), "modified" | "renamed" | "deleted"),
                    ),
                    "offset {offset}, path {path}"
                );
            }
            assert_eq!(git.folders, expected_folders, "offset {offset}");
        }
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

    #[test]
    fn changed_membership_survives_unknown_status_and_folder_rollup_sees_all_rows() {
        let git = Git::parse(&json!({"repository":true,"entries":[
            {"path":"edir/a","status":"conflict"},
            {"path":"edir/a","status":"unknown"},
            {"path":"edir/b","status":"ignored"},
            {"path":"edir/b","status":"deleted"},
            {"path":"edir/c","status":"modified"},
            {"path":"edir/c","status":false},
            {"path":"edir/d","status":"renamed"},
            {"path":"edir/e","status":""},
            {"path":"edir/f"},
            {"path":"other/g","status":"added"},
            {"path":"other/h","status":"untracked"},
            {"path":"other/i","status":"modified"},
            {"path":"other/j","status":"conflict"},
            {"path":"../out","status":"modified"},
            {"path":"","status":"deleted"}
        ]}));
        for path in ["edir/a", "edir/c", "edir/e", "edir/f"] {
            assert_eq!(git.row(path, false), (None, true, false));
        }
        for path in ["edir/z", "../out", ""] {
            assert_eq!(git.row(path, false), (None, false, false));
        }
        assert_eq!(
            git.row("edir/b", false),
            (Some(Status::Deleted), true, true)
        );
        assert_eq!(
            git.row("edir/d", false),
            (Some(Status::Modified), true, true)
        );
        assert_eq!(
            git.row("other/g", false),
            (Some(Status::Added), true, false)
        );
        assert_eq!(
            git.row("other/h", false),
            (Some(Status::Added), true, false)
        );
        assert_eq!(
            git.row("other/i", false),
            (Some(Status::Modified), true, true)
        );
        assert_eq!(
            git.row("other/j", false),
            (Some(Status::Conflict), true, false)
        );
        assert_eq!(
            git.row("edir", true),
            (Some(Status::Conflict), false, false)
        );
        assert_eq!(git.row("", true), (Some(Status::Conflict), false, false));
    }
}
