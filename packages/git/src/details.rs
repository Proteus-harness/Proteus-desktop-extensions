// Адаптировано из Zed: crates/git_ui/src/git_graph.rs (render_commit_detail_panel, render_search_bar)
//! Graph document and its selection inspector. Native controls own columns,
//! disclosures, tree navigation, scrolling and document preview behavior.
use crate::document::icon_button;
use crate::{Git, history, panel};
use proteus_extension::ui::{
    Node, bar, button, disclosure, divider, empty, fill, note, person, search, spacer, split,
    stats, text, tree,
};
use proteus_extension::{Host, Result, Value, ValueExt, json, local_time};
use std::collections::{BTreeMap, btree_map::Entry};

// Most leaf groups contain only one file. Keep that reference inline.
enum FileGroup<'a> {
    One(&'a Value),
    Many(Vec<&'a Value>),
}

fn files_in<'a>(
    files: impl IntoIterator<Item = &'a Value>,
    prefix: &str,
    depth: usize,
    hash: &str,
    closed: &std::collections::HashSet<String>,
    picked: Option<&str>,
    rows: &mut Vec<Value>,
) {
    let mut groups = BTreeMap::<&str, FileGroup<'_>>::new();
    for file in files {
        let relative = file.str("path").strip_prefix(prefix).unwrap_or("");
        match groups.entry(relative.split('/').next().unwrap_or("")) {
            Entry::Vacant(entry) => {
                entry.insert(FileGroup::One(file));
            }
            Entry::Occupied(mut entry) => {
                let group = entry.get_mut();
                match group {
                    FileGroup::One(first) => {
                        let mut files = Vec::with_capacity(4);
                        files.extend([*first, file]);
                        *group = FileGroup::Many(files);
                    }
                    FileGroup::Many(files) => files.push(file),
                }
            }
        }
    }
    for (name, group) in groups {
        let items = match &group {
            FileGroup::One(file) => std::slice::from_ref(file),
            FileGroup::Many(files) => files.as_slice(),
        };
        let path = format!("{prefix}{name}");
        if items.iter().any(|file| file.str("path") != path) {
            let expanded = !closed.contains(&format!("{hash}:{path}"));
            rows.push(
                json!({"id":format!("dir:{path}"),"label":name,"depth":depth,"expanded":expanded}),
            );
            if expanded {
                files_in(
                    items.iter().copied(),
                    &format!("{path}/"),
                    depth + 1,
                    hash,
                    closed,
                    picked,
                    rows,
                );
            }
        } else if let Some(file) = items.first() {
            let status = file.str("status");
            let (icon, tone) = panel::status_icon(status);
            let mut entry = json!({"id":path,"label":name,"depth":depth,"icon":icon,"icon_tone":tone,"selected":picked == Some(path.as_str()),"strike":status=="deleted","hint":path});
            if let (Some(a), Some(d)) = (file.at("added").as_u64(), file.at("deleted").as_u64()) {
                entry["stats"] = stats(a, d);
            }
            rows.push(entry);
        }
    }
}

impl Git {
    pub(crate) fn graph_document(&mut self, host: &Host) -> Result<Vec<Value>> {
        self.ensure_history(host, true)?;
        let log = self.graph.as_ref().expect("graph history was loaded");
        if log.entries.is_empty() {
            return Ok(vec![empty("Коммитов нет")]);
        }
        let (count, current) =
            match_position(self.graph_matches(&log.entries), self.commit.as_deref());
        let mut left = vec![bar(vec![
            search(
                "graph-search",
                &self.graph_query,
                "Поиск по коммитам",
                "graph_search",
            ),
            spacer(),
            button("Aa", "graph_case")
                .with("variant", "ghost")
                .with("selected", self.graph_case)
                .with("hint", "Учитывать регистр"),
            icon_button(
                "chevron-left",
                "graph_previous",
                Value::Null,
                "Предыдущее совпадение",
            )
            .with("disabled", count == 0),
            icon_button(
                "chevron-right",
                "graph_next",
                Value::Null,
                "Следующее совпадение",
            )
            .with("disabled", count == 0),
            note(format!("{current}/{count}")).with("single_line", true),
        ])];
        let rows = history::rows(&log.entries, self.commit.as_deref(), history::now());
        left.push(history::widget("graph", "graph_log", rows));
        if log.more {
            left.push(bar(vec![
                note(format!("{} коммитов", log.entries.len())),
                spacer(),
                button("Загрузить ещё", "graph_more").with("variant", "ghost"),
            ]));
        }
        let Some(hash) = self.commit.clone() else {
            return Ok(vec![fill(left)]);
        };
        let right = match self.commit_details(host, &hash) {
            Ok(commit) => self.commit_inspector(&commit),
            Err(error) => fill(vec![
                bar(vec![
                    spacer(),
                    icon_button("x", "close_commit_details", Value::Null, "Закрыть сведения"),
                ]),
                text(error).with("tone", "danger"),
            ]),
        };
        Ok(vec![split(vec![fill(left), right], vec![None, Some(320.)])])
    }

    fn commit_inspector(&self, commit: &Value) -> Value {
        let hash = commit.str("hash");
        let files = commit.list("files");
        let mut rows = Vec::new();
        let short = self.short_of(hash);
        files_in(
            files,
            "",
            0,
            hash,
            &self.detail_folders,
            self.commit_files.get(&short).map(String::as_str),
            &mut rows,
        );
        let added: u64 = files.iter().filter_map(|f| f.at("added").as_u64()).sum();
        let deleted: u64 = files.iter().filter_map(|f| f.at("deleted").as_u64()).sum();
        let mut message = vec![text(history::summary(commit))];
        if !commit.str("body").is_empty() {
            message.push(text(commit.str("body")));
        }
        fill(vec![
            bar(vec![
                note("Commit Details"),
                spacer(),
                icon_button("x", "close_commit_details", Value::Null, "Закрыть сведения"),
            ])
            .with("border", false),
            person(
                commit.str("author"),
                &local_time(commit.f64("time").unwrap_or(0.)),
            ),
            bar(vec![
                icon_button("mail", "copy", commit.str("email"), "Копировать email")
                    .with("label", commit.str("email")),
            ])
            .with("border", false),
            bar(vec![
                icon_button("hash", "copy", hash, "Копировать полный хеш").with("label", hash),
            ])
            .with("border", false),
            divider(),
            disclosure("commit-message", "Message", message),
            divider(),
            bar(vec![
                note(format!("{} изменённых файлов", files.len())),
                spacer(),
                json!({"type":"text","text":format!("+{added}"),"tone":"success"}),
                json!({"type":"text","text":format!("−{deleted}"),"tone":"danger"}),
            ])
            .with("border", false),
            tree("commit-detail-files", rows, "detail_file").with("chevrons", false),
            bar(vec![
                button("View Commit", "open_commit")
                    .with("value", hash)
                    .with("icon", "git-commit-horizontal")
                    .with("variant", "ghost"),
                spacer(),
                icon_button("pin", "keep_commit", hash, "Открыть и закрепить коммит"),
            ]),
        ])
    }

    pub(crate) fn detail_file(&mut self, host: &Host, id: &str, event: &str) {
        let Some(hash) = self.commit.clone() else {
            return;
        };
        if let Some(path) = id.strip_prefix("dir:") {
            let key = format!("{hash}:{path}");
            let open = match event {
                "expand" => true,
                "collapse" => false,
                "select" => self.detail_folders.contains(&key),
                _ => return,
            };
            if open {
                self.detail_folders.remove(&key);
            } else {
                self.detail_folders.insert(key);
            }
        } else if matches!(event, "select" | "preview" | "open") {
            self.commit_files
                .insert(self.short_of(&hash), id.to_owned());
            host.browse_document(&self.commit_argument(&hash), event);
        }
    }

    fn graph_matches<'a>(
        &'a self,
        entries: &'a [history::LogEntry],
    ) -> impl Iterator<Item = &'a str> {
        let query = if self.graph_case {
            self.graph_query.clone()
        } else {
            self.graph_query.to_lowercase()
        };
        entries
            .iter()
            .take(if query.is_empty() { 0 } else { entries.len() })
            .filter(move |e| {
                let text = format!("{} {} {} {}", e.hash, e.summary(), e.author, e.email);
                (if self.graph_case {
                    text
                } else {
                    text.to_lowercase()
                })
                .contains(&query)
            })
            .map(|e| e.hash.as_str())
    }

    /// A changed query starts a new result set. Advancing belongs only to
    /// the explicit previous/next actions, as in Zed's GitGraph::search.
    pub(crate) fn graph_search(&mut self) {
        let entries = self
            .graph
            .as_ref()
            .map(|g| g.entries.as_slice())
            .unwrap_or(&[]);
        let first = self.graph_matches(entries).next().map(str::to_owned);
        if let Some(first) = first {
            self.commit = Some(first);
        }
    }

    pub(crate) fn graph_find(&mut self, previous: bool) {
        let entries = self
            .graph
            .as_ref()
            .map(|g| g.entries.as_slice())
            .unwrap_or(&[]);
        let next = adjacent_match(
            self.graph_matches(entries),
            self.commit.as_deref(),
            previous,
        )
        .map(str::to_owned);
        if let Some(next) = next {
            self.commit = Some(next);
        }
    }
}

fn match_position<'a>(
    matches: impl Iterator<Item = &'a str>,
    selected: Option<&str>,
) -> (usize, usize) {
    let mut count = 0;
    let mut current = 0;
    for hash in matches {
        count += 1;
        if current == 0 && Some(hash) == selected {
            current = count;
        }
    }
    (count, current)
}

fn adjacent_match<'a>(
    mut matches: impl Iterator<Item = &'a str>,
    selected: Option<&str>,
    previous: bool,
) -> Option<&'a str> {
    let first = matches.next()?;
    if previous {
        // Before the first occurrence, or the last hit when wrapping or absent.
        let wrap = Some(first) == selected;
        let mut target = first;
        for hash in matches {
            if !wrap && Some(hash) == selected {
                break;
            }
            target = hash;
        }
        Some(target)
    } else {
        let is_current = |hash| Some(hash) == selected;
        let found = selected.is_some() && (is_current(first) || matches.by_ref().any(is_current));
        Some(if found {
            matches.next().unwrap_or(first)
        } else {
            first
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_file_groups_preserve_paths_and_selection_when_expanded() {
        let files = [
            json!({"path":"src/util/b.rs","status":"modified"}),
            json!({"path":"README.md","status":"added"}),
            json!({"path":"src/a.rs","status":"deleted"}),
        ];
        let mut rows = Vec::new();
        files_in(
            &files,
            "",
            0,
            "commit",
            &Default::default(),
            Some("src/util/b.rs"),
            &mut rows,
        );
        let ids: Vec<_> = rows.iter().map(|row| row.str("id")).collect();
        assert_eq!(
            ids,
            [
                "README.md",
                "dir:src",
                "src/a.rs",
                "dir:src/util",
                "src/util/b.rs"
            ]
        );
        assert_eq!(rows[1]["expanded"], true);
        assert_eq!(rows[2]["strike"], true);
        assert_eq!(rows[4]["selected"], true);
    }

    #[test]
    fn graph_match_navigation_uses_first_duplicate_and_keeps_wrap() {
        let hits = ["one", "two", "one", "", "last"];
        let p = |selected| match_position(hits.into_iter(), selected);
        let n = |selected, previous| adjacent_match(hits.into_iter(), selected, previous);
        assert_eq!(p(Some("one")), (5, 1));
        assert_eq!(p(Some("")), (5, 4));
        assert_eq!(p(Some("missing")), (5, 0));
        assert_eq!(p(None), (5, 0));
        assert_eq!(n(Some("one"), false), Some("two"));
        assert_eq!(n(Some("one"), true), Some("last"));
        assert_eq!(n(Some("two"), true), Some("one"));
        assert_eq!(n(Some(""), false), Some("last"));
        assert_eq!(n(Some("last"), false), Some("one"));
        assert_eq!(n(Some("missing"), true), Some("last"));
        assert_eq!(n(Some("missing"), false), Some("one"));
        assert_eq!(n(None, true), Some("last"));
        assert_eq!(n(None, false), Some("one"));
        assert_eq!(
            adjacent_match(["d", "d", "f"].into_iter(), Some("d"), false),
            Some("d")
        );
        assert_eq!(
            adjacent_match(["d", "a", "d"].into_iter(), Some("d"), true),
            Some("d")
        );
        assert_eq!(n(Some(""), true), Some("one"));
        for dir in [true, false] {
            assert_eq!(adjacent_match([].into_iter(), None, dir), None);
            assert_eq!(adjacent_match(["x"].into_iter(), Some("x"), dir), Some("x"));
        }
    }
}
