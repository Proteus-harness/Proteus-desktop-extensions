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
            (folder.to_string(), Listing::parse(json!({"entries": entries})))
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
fn folded_expand_all_refreshes_intermediate_folders_when_the_shape_changes() {
    for full in [false, true] {
        let before = workspace(&[
            ("", &[("src", true), ("other", true)]),
            ("src", &[("lib", true)]),
            ("src/lib", &[("deep", true)]),
            ("src/lib/deep", &[("existing.rs", false)]),
            ("other", &[]),
        ]);
        let mut project = Project::default();
        let rows = project.lines("p", &mut loader(&before)).unwrap();
        assert!(rows.iter().any(|row| row.path == "src/lib/deep"));
        project.expand_all("src/lib/deep", &mut loader(&before));
        for path in ["src", "src/lib", "src/lib/deep"] {
            assert!(project.is_expanded(path));
        }
        assert!(!project.is_expanded("other"));

        let after = workspace(&[
            ("", &[("src", true), ("other", true)]),
            ("src", &[("lib", true), ("outer.rs", false)]),
            ("src/lib", &[("deep", true), ("inner.rs", false)]),
            ("src/lib/deep", &[("existing.rs", false)]),
            ("other", &[]),
        ]);
        project.invalidate(full);
        let mut reads = Vec::new();
        let rows = project
            .lines("p", &mut |path| {
                reads.push(path.to_owned());
                loader(&after)(path)
            })
            .unwrap();
        for path in [
            "src/outer.rs",
            "src/lib/inner.rs",
            "src/lib/deep/existing.rs",
        ] {
            assert!(
                rows.iter().any(|row| row.path == path),
                "{path}, full={full}"
            );
        }
        for path in ["src", "src/lib", "src/lib/deep"] {
            assert_eq!(reads.iter().filter(|read| read.as_str() == path).count(), 1);
        }
    }
}

#[test]
fn folded_expand_all_agrees_with_ordinary_expand_and_collapse_intent() {
    let folders = workspace(&[
        ("", &[("a", true)]),
        ("a", &[("b", true)]),
        ("a/b", &[("file.rs", false)]),
    ]);
    let mut one = Project::default();
    let mut all = Project::default();
    one.lines("p", &mut loader(&folders)).unwrap();
    all.lines("p", &mut loader(&folders)).unwrap();
    one.expand("a/b", &mut loader(&folders));
    all.expand_all("a/b", &mut loader(&folders));
    assert_eq!(one.expanded, all.expanded);
    one.collapse("a/b");
    all.collapse("a/b");
    assert_eq!(one.expanded, all.expanded);
    assert_eq!(all.expanded, HashSet::from([String::new()]));

    one.expand("a/b", &mut loader(&folders));
    all.expand_all("a/b", &mut loader(&folders));
    one.collapse_all("a/b");
    all.collapse_all("a/b");
    assert_eq!(one.expanded, all.expanded);
    assert!(all.is_expanded("a")); // Existing collapse_all scope is unchanged.
    assert!(!all.is_expanded("a/b"));
}

#[test]
fn collapse_removes_only_the_path_prefixes_inside_its_folded_head() {
    let keys = [
        "",
        "a",
        "a/",
        "a/b",
        "a//b",
        "a//b/",
        "a/b/child",
        "a/side",
        "ab",
        "аб",
        "аб/в",
        "аб/в/г",
        "unrelated",
    ];
    for (path, head) in [
        ("", None),
        ("a/b", None),
        ("a/b", Some("a")),
        ("a/b", Some("a/")),
        ("a/b", Some("")),
        ("a//b/", Some("a/")),
        ("аб/в", Some("аб")),
        ("a/b", Some("unrelated")),
    ] {
        let mut project = Project {
            expanded: keys.into_iter().map(str::to_owned).collect(),
            ..Project::default()
        };
        if let Some(head) = head {
            project.heads.insert(path.into(), head.into());
        }
        let head = head.unwrap_or(path);
        let expected = keys
            .into_iter()
            .filter(|open| !(paths::within(path, open) && paths::within(open, head)))
            .map(str::to_owned)
            .collect();
        project.collapse(path);
        assert_eq!(project.expanded, expected, "path={path:?}, head={head:?}");
        project.collapse(path);
        assert_eq!(
            project.expanded, expected,
            "repeating collapse changed a sibling"
        );
    }
}

#[test]
fn expand_all_without_a_known_folded_head_does_not_reveal_other_ancestors() {
    let folders = workspace(&[
        ("", &[("outer", true)]),
        ("outer", &[("target", true)]),
        ("outer/target", &[]),
    ]);
    let mut plain = Project::default();
    plain.expand_all("outer/target", &mut loader(&folders));
    assert_eq!(
        plain.expanded,
        HashSet::from([String::new(), "outer/target".into()])
    );
    let mut root = Project::default();
    root.expand_all("", &mut loader(&folders));
    assert_eq!(
        root.expanded,
        HashSet::from([String::new(), "outer".into(), "outer/target".into(),])
    );
}

#[test]
fn recorded_folded_head_needs_no_extra_reads_after_cache_invalidation() {
    let before = workspace(&[
        ("", &[("outer", true)]),
        ("outer", &[("a", true), ("other.rs", false)]),
        ("outer/a", &[("b", true)]),
        ("outer/a/b", &[]),
    ]);
    let mut project = Project::default();
    project.expand("outer", &mut loader(&before));
    project.lines("p", &mut loader(&before)).unwrap();
    assert_eq!(project.heads["outer/a/b"], "outer/a");
    project.invalidate(true); // The last rendered heads are deliberately retained.
    let mut reads = Vec::new();
    project.expand_all("outer/a/b", &mut |path| {
        reads.push(path.to_owned());
        Ok(Listing::default())
    });
    assert_eq!(reads, ["outer/a/b"]);
    assert!(project.is_expanded("outer/a"));
    // Successful parent evidence, not the aged folded mapping, decides absence.
    let gone = workspace(&[("", &[("outer", true)]), ("outer", &[("other.rs", false)])]);
    project.invalidate(false);
    project.lines("p", &mut loader(&gone)).unwrap();
    assert!(!project.is_expanded("outer/a"));
}

#[test]
fn folded_ancestors_do_not_spend_or_extend_expand_all_listing_budget() {
    let path = "outer/a/b";
    let mut project = Project::default();
    project.heads.insert(path.into(), "outer/a".into());
    let mut reads = Vec::new();
    project.expand_all(path, &mut |folder| {
        reads.push(folder.to_owned());
        Ok(Listing {
            entries: if folder == path {
                (0..EXPAND_ALL)
                    .map(|i| Entry::new(&format!("d{i}"), format!("{path}/d{i}"), Kind::Directory))
                    .collect()
            } else {
                Vec::new()
            },
            truncated: false,
        })
    });
    assert_eq!(reads.len(), EXPAND_ALL);
    assert!(reads.iter().all(|folder| paths::within(folder, path)));
    assert!(project.is_expanded("outer/a"));
    assert!(!project.is_expanded("outer"));
    assert!(!project.is_expanded(&format!("{path}/d{}", EXPAND_ALL - 1)));
}

#[test]
fn folded_ancestors_do_not_spend_or_extend_expand_all_descendant_depth() {
    let path = "a/b";
    let mut project = Project::default();
    project.heads.insert(path.into(), "a".into());
    let mut reads = Vec::new();
    project.expand_all(path, &mut |folder| {
        reads.push(folder.to_owned());
        Ok(Listing {
            entries: vec![Entry::new(
                "child",
                format!("{folder}/child"),
                Kind::Directory,
            )],
            truncated: false,
        })
    });
    assert_eq!(reads.len(), CHAIN + 1); // Root at depth zero, as before.
    assert!(reads.iter().all(|folder| paths::within(folder, path)));
    assert!(project.is_expanded("a"));
    assert!(!project.is_expanded(&format!("{}/child", reads.last().unwrap())));
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

#[test]
fn failed_expanded_listing_keeps_intent_and_retries_on_partial_refresh() {
    let folders = workspace(&[
        ("", &[("a", true)]),
        ("a", &[("b", true), ("visible.rs", false)]),
        ("a/b", &[("nested.rs", false)]),
    ]);
    let mut project = Project::default();
    project.expanded.extend(["a".into(), "a/b".into()]);
    project.lines("p", &mut loader(&folders)).unwrap();
    project.invalidate(false);

    let attempts = std::cell::Cell::new(0);
    let mut unavailable = |path: &str| {
        if path == "a" {
            attempts.set(attempts.get() + 1);
            Err(proteus_extension::anyhow!("temporarily unavailable"))
        } else {
            loader(&folders)(path)
        }
    };
    project.lines("p", &mut unavailable).unwrap();
    assert_eq!(attempts.get(), 1, "fold and visit share one failed attempt");
    assert!(project.is_expanded("a") && project.is_expanded("a/b"));
    assert!(project.exists("a/visible.rs") && project.exists("a/b"));
    // Another render before refresh does not busy-retry the failed listing.
    project.lines("p", &mut unavailable).unwrap();
    assert_eq!(attempts.get(), 1);

    project.invalidate(false);
    let lines = project.lines("p", &mut loader(&folders)).unwrap();
    assert!(project.is_expanded("a") && project.is_expanded("a/b"));
    assert!(lines.iter().any(|line| line.path == "a/visible.rs"));
    assert!(lines.iter().any(|line| line.path == "a/b/nested.rs"));
}

#[test]
fn collapsed_failure_expires_but_successful_lookahead_stays_cached() {
    let folders = workspace(&[
        ("", &[("a", true), ("z", true)]),
        ("a", &[("b", true)]),
        ("a/b", &[("file.rs", false)]),
        ("z", &[("file.rs", false)]),
    ]);
    let calls = std::cell::RefCell::new(HashMap::<String, usize>::new());
    let fail = std::cell::Cell::new(true);
    let mut load = |path: &str| {
        *calls.borrow_mut().entry(path.to_owned()).or_default() += 1;
        if path == "a" && fail.get() {
            Err(proteus_extension::anyhow!("temporarily unavailable"))
        } else {
            loader(&folders)(path)
        }
    };
    let mut project = Project::default();
    project.lines("p", &mut load).unwrap();
    fail.set(false);
    project.lines("p", &mut load).unwrap();
    assert_eq!(calls.borrow().get("a"), Some(&1));
    assert_eq!(calls.borrow().get("z"), Some(&1));

    project.invalidate(false);
    let lines = project.lines("p", &mut load).unwrap();
    assert_eq!(calls.borrow().get("a"), Some(&2));
    assert_eq!(calls.borrow().get("z"), Some(&1));
    assert!(
        lines
            .iter()
            .any(|line| line.path == "a/b" && line.expanded == Some(false))
    );
    assert_eq!(project.expanded, HashSet::from([String::new()]));
}

#[test]
fn only_complete_parent_absence_prunes_unknown_descendants() {
    let folders = workspace(&[("", &[("a", true)]), ("a", &[("b", true)]), ("a/b", &[])]);
    let mut project = Project::default();
    project.expanded.extend(["a".into(), "a/b".into()]);
    project
        .listings
        .insert(String::new(), Some(Rc::new(folders[""].clone())));
    project.listings.insert("a".into(), None);
    project.prune();
    assert!(project.is_expanded("a/b") && project.exists("a/b"));

    project.listings.insert(
        "a".into(),
        Some(Rc::new(Listing {
            entries: Vec::new(),
            truncated: true,
        })),
    );
    project.prune();
    assert!(project.is_expanded("a/b") && project.exists("a/b"));

    project
        .listings
        .insert("a".into(), Some(Rc::new(Listing::default())));
    project.prune();
    assert!(!project.is_expanded("a/b") && !project.exists("a/b"));
    assert!(project.is_expanded("a") && project.exists("a"));
}

#[test]
fn recovered_listing_does_not_reverse_explicit_collapse() {
    let folders = workspace(&[("", &[("a", true)]), ("a", &[("file.rs", false)])]);
    let mut project = Project::default();
    project.expanded.insert("a".into());
    project.lines("p", &mut loader(&folders)).unwrap();
    project.invalidate(false);
    let mut unavailable = |path: &str| {
        if path == "a" {
            Err(proteus_extension::anyhow!("unavailable"))
        } else {
            loader(&folders)(path)
        }
    };
    project.lines("p", &mut unavailable).unwrap();
    project.collapse("a");
    project.invalidate(false);
    let lines = project.lines("p", &mut loader(&folders)).unwrap();
    assert!(!project.is_expanded("a"));
    assert!(
        lines
            .iter()
            .any(|line| line.path == "a" && line.expanded == Some(false))
    );
    assert!(!lines.iter().any(|line| line.path == "a/file.rs"));
}

#[test]
fn failed_collapsed_lookahead_still_obeys_per_render_budget() {
    let root = Listing {
        entries: (0..LOOKAHEAD + 8)
            .map(|index| {
                let path = format!("folder-{index}");
                Entry::new(&path, path.clone(), Kind::Directory)
            })
            .collect(),
        truncated: false,
    };
    let attempts = std::cell::Cell::new(0);
    let mut load = |path: &str| {
        if path.is_empty() {
            return Ok(root.clone());
        }
        attempts.set(attempts.get() + 1);
        Err(proteus_extension::anyhow!("unavailable"))
    };
    let mut project = Project::default();
    project.lines("p", &mut load).unwrap();
    assert_eq!(attempts.get(), LOOKAHEAD);
    project.invalidate(false);
    project.lines("p", &mut load).unwrap();
    assert_eq!(attempts.get(), LOOKAHEAD * 2);
    assert_eq!(project.expanded, HashSet::from([String::new()]));
}

#[test]
fn failed_full_refresh_also_preserves_expanded_intent() {
    let folders = workspace(&[("", &[("a", true)]), ("a", &[("file.rs", false)])]);
    let mut project = Project::default();
    project.expanded.insert("a".into());
    project.lines("p", &mut loader(&folders)).unwrap();
    project.invalidate(true);
    let mut unavailable = |path: &str| {
        if path == "a" {
            Err(proteus_extension::anyhow!("unavailable"))
        } else {
            loader(&folders)(path)
        }
    };
    project.lines("p", &mut unavailable).unwrap();
    assert!(project.is_expanded("a") && project.exists("a/file.rs"));
    project.invalidate(false);
    assert!(
        project
            .lines("p", &mut loader(&folders))
            .unwrap()
            .iter()
            .any(|line| line.path == "a/file.rs")
    );
}

#[test]
fn lookahead_and_recursion_keep_rows_ordered_and_do_not_pin_refreshed_listings() {
    let before = workspace(&[
        ("", &[("first.rs", false), ("a", true), ("last.rs", false)]),
        ("a", &[("β", true)]),
        ("a/β", &[("visible.rs", false), ("sub", true)]),
        ("a/β/sub", &[]),
    ]);
    let mut project = Project::default();
    project.reveal("a/β/visible.rs");
    let mut reads = Vec::new();
    let rows = project
        .lines("p", &mut |path| {
            reads.push(path.to_owned());
            loader(&before)(path)
        })
        .unwrap();
    assert_eq!(reads, ["", "a", "a/β", "a/β/sub"]);
    assert_eq!(
        rows.iter()
            .map(|line| line.label.as_str())
            .collect::<Vec<_>>(),
        ["p", "first.rs", "a/β", "visible.rs", "sub", "last.rs"]
    );
    let warm = project
        .lines("p", &mut |_| panic!("warm render should not list folders"))
        .unwrap();
    assert_eq!(rows, warm);

    let old = Rc::downgrade(project.listings["a/β"].as_ref().unwrap());
    project.invalidate(false);
    assert!(old.upgrade().is_none(), "rows must not retain a listing");
    let after = workspace(&[
        ("", &[("first.rs", false), ("a", true), ("last.rs", false)]),
        ("a", &[("β", true)]),
        ("a/β", &[("new.rs", false), ("sub", true)]),
    ]);
    let fresh = project.lines("p", &mut loader(&after)).unwrap();
    assert_eq!(rows[3].label, "visible.rs");
    assert_eq!(fresh[3].label, "new.rs");
}

#[test]
fn listing_names_share_an_exact_path_suffix_and_keep_original_labels() {
    let source = json!({
        "entries": [
            {"name":"папка", "path":"././prefix/папка", "kind":"directory"},
            {"name":"日本語.rs", "path":"a\\日本語.rs", "kind":"file"},
            {"name":"literal\\name", "path":"a/literal\\name", "kind":"file"},
            {"name":"other", "path":"a/path", "kind":"symlink"},
            {"name":"tail", "path":"not-a-tail", "kind":"unknown"},
            {"name":"", "path":"a/", "kind":null},
            {"name":42, "path":false},
            {"name":"without-path"},
            {"name":".git", "path":"kept-by-path"},
            {"name":"visible", "path":"./.git", "kind":"file"}
        ],
        "truncated": true
    });
    let listed = Listing::parse(source);
    assert!(listed.truncated);
    assert_eq!(
        listed
            .entries
            .iter()
            .map(|entry| {
                let shares = match &entry.name {
                    EntryName::PathSuffix(start) => {
                        assert_eq!(entry.name().as_ptr(), entry.path[*start..].as_ptr());
                        true
                    }
                    EntryName::Override(_) => false,
                };
                (entry.name(), entry.path.as_str(), entry.kind, shares)
            })
            .collect::<Vec<_>>(),
        [
            ("папка", "prefix/папка", Kind::Directory, true),
            ("日本語.rs", "a\\日本語.rs", Kind::File, true),
            ("literal\\name", "a/literal\\name", Kind::File, true),
            ("other", "a/path", Kind::Symlink, false),
            ("tail", "not-a-tail", Kind::Other, false),
            ("", "a/", Kind::Other, true),
            ("", "", Kind::Other, true),
            ("without-path", "", Kind::Other, false),
            ("visible", ".git", Kind::File, false),
        ]
    );
    assert!(Listing::parse(json!({"entries":false})).entries.is_empty());
}

#[test]
fn listing_moves_only_tight_raw_paths_and_preserves_defaults() {
    let raw = r#"{"entries":[{"name":"same","path":"ordinary/имя","kind":"file"},
                           {"name":"alias","path":"././dir/имя","kind":"directory"},
                           {"name":"wide","path":"replace","kind":"file"},
                           {"name":"wrong","path":7},
                           {"name":"missing"},
                           {"name":".git","path":"skip"}],"truncated":true}"#;
    let mut source: Value = serde_json::from_str(raw).unwrap();
    let mut wide = String::with_capacity(128);
    wide.push_str("dir/loose");
    source["entries"][2]["path"] = Value::String(wide);
    let ordinary = source["entries"][0]["path"].as_str().unwrap().as_ptr();
    let listed = Listing::parse(source);
    assert!(listed.truncated);
    assert_eq!(listed.entries[0].path.as_ptr(), ordinary);
    assert_eq!(
        listed
            .entries
            .iter()
            .map(|e| e.path.as_str())
            .collect::<Vec<_>>(),
        ["ordinary/имя", "dir/имя", "dir/loose", "", ""]
    );
    assert!(
        listed
            .entries
            .iter()
            .all(|e| e.path.capacity() == e.path.len())
    );
    assert!(Listing::parse(json!({"entries":17})).entries.is_empty());
}
