//! Workspace-relative paths (`""` is the session folder) and syntax names.

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

/// Ancestors from the nearest one up to the session folder: `a/b/c` → `a/b`, `a`, `""`.
pub fn parents(path: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut current = path.trim_end_matches('/');
    while !current.is_empty() {
        current = parent(current);
        out.push(current);
    }
    out
}

pub fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |index| &path[..index])
}

pub fn name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

/// `path` itself or a path below it.
pub fn within(path: &str, folder: &str) -> bool {
    folder.is_empty()
        || path == folder
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// A name or `sub/name` typed by the user below `folder`; `None` when it
/// leaves the folder or is empty.
pub fn join(folder: &str, typed: &str) -> Option<String> {
    let typed = typed.trim().trim_matches('/');
    let parts: Vec<&str> = typed.split('/').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() || parts.iter().any(|part| matches!(*part, "." | "..")) {
        return None;
    }
    let typed = parts.join("/");
    Some(if folder.is_empty() {
        typed
    } else {
        format!("{folder}/{typed}")
    })
}

/// Absolute path of a workspace entry in the session folder `cwd`.
pub fn absolute(cwd: &str, path: &str) -> String {
    let cwd = match cwd.trim_end_matches('/') {
        "" => "/",
        cwd => cwd,
    };
    match (cwd, path) {
        (cwd, "") => cwd.to_owned(),
        ("/", path) => format!("/{path}"),
        (cwd, path) => format!("{cwd}/{path}"),
    }
}

/// `path` moved from `from` to `to` when it lies below `from`.
pub fn moved(path: &str, from: &str, to: &str) -> Option<String> {
    (!from.is_empty() && within(path, from)).then(|| format!("{to}{}", &path[from.len()..]))
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

    #[test]
    fn parents_end_at_the_session_folder() {
        assert_eq!(parents("a/b/c"), ["a/b", "a", ""]);
        assert_eq!(parents("a"), [""]);
        assert!(parents("").is_empty());
        assert_eq!(
            (name("a/b.rs"), parent("a/b.rs"), parent("b")),
            ("b.rs", "a", "")
        );
    }

    #[test]
    fn joins_typed_names_below_a_folder() {
        assert_eq!(join("", " notes.md "), Some("notes.md".to_owned()));
        assert_eq!(join("src", "a//b.rs/"), Some("src/a/b.rs".to_owned()));
        assert_eq!(join("src", "../x"), None);
        assert_eq!(join("src", " / "), None);
        assert_eq!(absolute("/w/p/", "src/a.rs"), "/w/p/src/a.rs");
        assert_eq!(absolute("/w/p", ""), "/w/p");
        assert_eq!(absolute("/", "a"), "/a");
    }

    #[test]
    fn moves_paths_below_a_renamed_folder() {
        assert!(within("src/a.rs", "src") && within("src", "src") && within("x", ""));
        assert!(!within("srcs/a.rs", "src"));
        assert_eq!(moved("src/a.rs", "src", "lib"), Some("lib/a.rs".to_owned()));
        assert_eq!(moved("srcs", "src", "lib"), None);
    }

    #[test]
    fn languages_follow_names_and_suffixes() {
        assert_eq!(language("Cargo.lock"), "toml");
        assert_eq!(language("src/X.HPP"), "cpp");
        assert_eq!(language("LICENSE"), "text");
    }
}
