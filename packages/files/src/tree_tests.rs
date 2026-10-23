use super::*;
use proteus_extension::json;

/// A fake workspace: folder → entries `(name, is_directory)`.
fn workspace(folders: &[(&str, &[(&str, bool)])]) -> HashMap<String, Listing> {
    folders
        .iter()
        .map(|(folder, entries)| {
            let entries: Vec<Value> = entries
                .iter()
                .map(|(name, directory)| {
                    let path = paths::join(folder, name).unwrap();
                    json!({"name": name, "path": path, "kind": if *directory { "directory" } else { "file" }})
                })
                .collect();
            (folder.to_string(), Listing::parse(&json!({"entries": entries})))
        })
        .collect()
}

fn loader(folders: &HashMap<String, Listing>) -> impl FnMut(&str) -> Result<Listing> + '_ {
    |path| {
        folders
            .get(path)
            .cloned()
            .ok_or_else(|| proteus_extension::anyhow!("gone"))
    }
}

fn shown(lines: &[Line]) -> Vec<(String, usize, Option<bool>)> {
    lines
        .iter()
        .map(|line| (line.label.clone(), line.depth, line.expanded))
        .collect()
}

#[test]
fn folds_single_folder_chains_into_one_row() {
    let folders = workspace(&[
        ("", &[("a", true), (".git", true), ("top.rs", false)]),
        ("a", &[("b", true)]),
        ("a/b", &[("c", true)]),
        ("a/b/c", &[("x.rs", false), ("y.rs", false)]),
    ]);
    let mut project = Project::default();
    let mut load = loader(&folders);
    let lines = project.lines("proj", &mut load).unwrap();
    assert_eq!(
        shown(&lines),
        [
            ("proj".to_owned(), 0, Some(true)),
            ("a/b/c".to_owned(), 1, Some(false)),
            ("top.rs".to_owned(), 1, None),
        ]
    );
    assert_eq!(lines[1].path, "a/b/c");

    project.expand("a/b/c", &mut load);
    let lines = project.lines("proj", &mut load).unwrap();
    let labels: Vec<&str> = lines.iter().map(|line| line.label.as_str()).collect();
    assert_eq!(labels, ["proj", "a/b/c", "x.rs", "y.rs", "top.rs"]);
    assert_eq!(lines[2].depth, 2);
    assert_eq!(project.kind("a/b/c/x.rs"), Some(Kind::File));
    assert!(project.is_expanded("a") && project.is_expanded("a/b"));

    project.collapse("a/b/c");
    assert_eq!(project.expanded, HashSet::from([String::new()]));
    let lines = project.lines("proj", &mut load).unwrap();
    assert_eq!(lines[1].label, "a/b/c");
}

#[test]
fn a_folder_with_files_does_not_fold() {
    let folders = workspace(&[
        ("", &[("src", true)]),
        ("src", &[("lib", true), ("main.rs", false)]),
        ("src/lib", &[("mod.rs", false)]),
    ]);
    let mut project = Project::default();
    let mut load = loader(&folders);
    project.expand("src", &mut load);
    let labels: Vec<String> = project
        .lines("p", &mut load)
        .unwrap()
        .into_iter()
        .map(|line| line.label)
        .collect();
    assert_eq!(labels, ["p", "src", "lib", "main.rs"]);
}

#[test]
fn expand_all_and_collapse_all_stay_below_the_folder() {
    let folders = workspace(&[
        ("", &[("a", true), ("z", true)]),
        ("a", &[("b", true), ("f", false)]),
        ("a/b", &[("g", false)]),
        ("z", &[("h", false)]),
    ]);
    let mut project = Project::default();
    let mut load = loader(&folders);
    project.expand_all("a", &mut load);
    assert!(project.expanded.contains("a/b") && !project.expanded.contains("z"));
    project.expanded.insert("z".to_owned());
    project.collapse_all("a");
    assert!(!project.expanded.contains("a") && !project.expanded.contains("a/b"));
    assert!(project.expanded.contains("z"));
    project.collapse_all("");
    assert_eq!(project.expanded, HashSet::from([String::new()]));
}

#[test]
fn refresh_drops_vanished_folders_and_keeps_renamed_ones() {
    let before = workspace(&[("", &[("a", true), ("b", true)]), ("a", &[]), ("b", &[])]);
    let mut project = Project::default();
    project.expanded.extend(["a".to_owned(), "b".to_owned()]);
    project.lines("p", &mut loader(&before)).unwrap();
    let after = workspace(&[("", &[("b", true)]), ("b", &[])]);
    project.invalidate(true);
    project.lines("p", &mut loader(&after)).unwrap();
    assert!(!project.expanded.contains("a") && project.expanded.contains("b"));
    assert!(!project.exists("a") && project.exists("b"));

    project.moved("b", "c");
    assert!(project.expanded.contains("c") && !project.expanded.contains("b"));
    project.removed("c");
    assert!(!project.expanded.contains("c"));
}

#[test]
fn an_unlistable_root_is_an_error() {
    let folders = workspace(&[]);
    assert!(
        Project::default()
            .lines("p", &mut loader(&folders))
            .is_err()
    );
}
