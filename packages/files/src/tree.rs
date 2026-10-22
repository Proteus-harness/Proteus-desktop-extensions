//! Workspace search tree, git colours and syntax names; independent of the client.

use std::collections::{BTreeSet, HashMap};

use proteus_extension::{Value, ValueExt};

const FOLDER: [&str; 3] = ["danger", "warning", "success"];

const LANGUAGES: &[(&str, &str)] = &[
    ("rust", "rs"),
    ("python", "py pyi"),
    ("javascript", "js mjs cjs jsx"),
    ("typescript", "ts mts cts"),
    ("tsx", "tsx"),
    ("json", "json jsonc jsonl"),
    ("toml", "toml"),
    ("yaml", "yaml yml"),
    ("bash", "sh bash zsh"),
    ("c", "c h"),
    ("cpp", "cc cpp cxx hpp hh hxx"),
    ("go", "go"),
    ("css", "css scss"),
    ("html", "html htm"),
    ("sql", "sql"),
    ("diff", "diff patch"),
];

/// Git status colours the tree like Zed: changed files and the folders above them.
pub fn tone(status: &str) -> Option<&'static str> {
    Some(match status {
        "conflict" | "deleted" => "danger",
        "untracked" | "added" => "success",
        "renamed" | "modified" => "warning",
        _ => return None,
    })
}

/// Parents like `PurePosixPath.parents`: `a/b/c` → `a/b`, `a`, `.`.
pub fn parents(path: &str) -> Vec<&str> {
    let mut current = path.trim_end_matches('/');
    let mut out = Vec::new();
    if current.is_empty() || current == "." {
        return out;
    }
    loop {
        match current.rfind('/') {
            Some(0) => {
                out.push("/");
                break;
            }
            Some(index) if &current[..index] != "." => {
                current = &current[..index];
                out.push(current);
            }
            _ => {
                out.push(".");
                break;
            }
        }
    }
    out
}

pub fn name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

/// Statuses of changed files and the strongest tone of each folder above them.
pub fn colours(entries: &[Value]) -> (HashMap<String, String>, HashMap<String, &'static str>) {
    let statuses: HashMap<String, String> = entries
        .iter()
        .map(|entry| (entry.str("path").to_owned(), entry.str("status").to_owned()))
        .collect();
    let rank = |tone: &str| FOLDER.iter().position(|folder| *folder == tone);
    let mut folders: HashMap<String, &'static str> = HashMap::new();
    for (path, status) in &statuses {
        let tone = match tone(status) {
            Some("danger") if status != "conflict" => "warning",
            Some(tone) => tone,
            None => "warning",
        };
        for parent in parents(path) {
            if parent == "." {
                break;
            }
            folders
                .entry(parent.to_owned())
                .and_modify(|current| {
                    if rank(tone) < rank(current) {
                        *current = tone;
                    }
                })
                .or_insert(tone);
        }
    }
    (statuses, folders)
}

/// Rows of `/workspace/search` results with their ancestors, folders first.
pub fn search_rows(result: &Value, row: impl Fn(&str, &str, usize, bool) -> Value) -> Vec<Value> {
    let mut kinds: HashMap<String, bool> = HashMap::new();
    let mut children: HashMap<String, BTreeSet<String>> = HashMap::new();
    for entry in result.list("entries") {
        let path = entry.str("path");
        kinds.insert(path.to_owned(), entry.str("kind") == "directory");
        let mut child = path.to_owned();
        for parent in parents(path) {
            let parent = if parent == "." { "" } else { parent };
            children.entry(parent.to_owned()).or_default().insert(child);
            if !parent.is_empty() {
                kinds.entry(parent.to_owned()).or_insert(true);
            }
            child = parent.to_owned();
        }
    }
    let mut rows = Vec::new();
    visit("", 0, &kinds, &children, &row, &mut rows);
    rows
}

fn visit(
    parent: &str,
    depth: usize,
    kinds: &HashMap<String, bool>,
    children: &HashMap<String, BTreeSet<String>>,
    row: &impl Fn(&str, &str, usize, bool) -> Value,
    rows: &mut Vec<Value>,
) {
    let Some(paths) = children.get(parent) else {
        return;
    };
    let directory = |path: &str| kinds.get(path).copied().unwrap_or(false);
    let mut paths: Vec<&String> = paths.iter().filter(|path| *path != parent).collect();
    paths.sort_by_key(|path| (!directory(path), name(path).to_lowercase()));
    for path in paths {
        let mut item = row(path, name(path), depth, directory(path));
        if directory(path) {
            item["expanded"] = children.contains_key(path.as_str()).into();
        }
        rows.push(item);
        visit(path, depth + 1, kinds, children, row, rows);
    }
}

pub fn language(path: &str) -> &'static str {
    let name = name(path);
    if matches!(name, "Cargo.lock" | "Pipfile") {
        return "toml";
    }
    if matches!(
        name,
        "Makefile" | "Dockerfile" | ".bashrc" | ".zshrc" | ".profile"
    ) {
        return "bash";
    }
    let suffix = match name.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < name.len() => name[dot + 1..].to_lowercase(),
        _ => return "text",
    };
    LANGUAGES
        .iter()
        .find(|(_, extensions)| extensions.split(' ').any(|extension| extension == suffix))
        .map(|(language, _)| *language)
        .unwrap_or("text")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_extension::json;

    #[test]
    fn search_retains_ancestors_and_directory_order() {
        let result = json!({"entries": [{"path": "src/z.rs", "kind": "file"}, {"path": "src/a/b.py", "kind": "file"},
                                        {"path": "Cargo.lock", "kind": "file"}]});
        let rows = search_rows(
            &result,
            |path, _, depth, directory| json!({"id": path, "depth": depth, "expanded": if directory { json!(false) } else { Value::Null }}),
        );
        let order: Vec<(&str, i64)> = rows
            .iter()
            .map(|row| (row.str("id"), row.i64("depth").unwrap()))
            .collect();
        assert_eq!(
            order,
            [
                ("src", 0),
                ("src/a", 1),
                ("src/a/b.py", 2),
                ("src/z.rs", 1),
                ("Cargo.lock", 0)
            ]
        );
        assert!(rows[0].bool("expanded"));
        assert_eq!(language("Cargo.lock"), "toml");
        assert_eq!(language("src/X.HPP"), "cpp");
        assert_eq!(language("LICENSE"), "text");
    }

    #[test]
    fn parents_follow_posix_paths() {
        assert_eq!(parents("a/b/c"), ["a/b", "a", "."]);
        assert_eq!(parents("a"), ["."]);
        assert_eq!(parents("/x/y"), ["/x", "/"]);
        assert!(parents("").is_empty());
    }

    #[test]
    fn folders_take_the_strongest_status() {
        let (statuses, folders) = colours(&[
            json!({"path": "src/a/x.py", "status": "untracked"}),
            json!({"path": "src/b.rs", "status": "modified"}),
            json!({"path": "lib/gone.c", "status": "deleted"}),
        ]);
        assert_eq!(statuses["lib/gone.c"], "deleted");
        let expected: HashMap<String, &str> =
            [("src/a", "success"), ("src", "warning"), ("lib", "warning")]
                .map(|(path, tone)| (path.to_owned(), tone))
                .into();
        assert_eq!(folders, expected);
    }
}
