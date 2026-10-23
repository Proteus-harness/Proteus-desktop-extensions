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
use std::collections::BTreeMap;

fn files_in(
    files: &[&Value],
    prefix: &str,
    depth: usize,
    hash: &str,
    closed: &std::collections::HashSet<String>,
    picked: Option<&str>,
    rows: &mut Vec<Value>,
) {
    let mut groups = BTreeMap::<&str, Vec<&Value>>::new();
    for file in files {
        let relative = file.str("path").strip_prefix(prefix).unwrap_or("");
        groups
            .entry(relative.split('/').next().unwrap_or(""))
            .or_default()
            .push(file);
    }
    for (name, items) in groups {
        let path = format!("{prefix}{name}");
        if items.iter().any(|file| file.str("path") != path) {
            let expanded = !closed.contains(&format!("{hash}:{path}"));
            rows.push(json!({"id":format!("dir:{path}"),"label":name,"depth":depth,"expanded":expanded,"icon":"folder"}));
            if expanded {
                files_in(
                    &items,
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
        let log = self.history(host, true)?;
        if log.entries.is_empty() {
            return Ok(vec![empty("Коммитов нет")]);
        }
        let matches = self.graph_matches(&log.entries);
        let current = matches
            .iter()
            .position(|hash| Some(hash.as_str()) == self.commit.as_deref())
            .map(|i| i + 1)
            .unwrap_or(0);
        let mut left = vec![bar(vec![
            search(
                "graph-search",
                &self.graph_query,
                "Поиск по коммитам…",
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
            .with("disabled", matches.is_empty()),
            icon_button(
                "chevron-right",
                "graph_next",
                Value::Null,
                "Следующее совпадение",
            )
            .with("disabled", matches.is_empty()),
            note(format!("{current}/{}", matches.len())).with("single_line", true),
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
            &files.iter().collect::<Vec<_>>(),
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
            tree("commit-detail-files", rows, "detail_file"),
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

    pub(crate) fn graph_matches(&self, entries: &[Value]) -> Vec<String> {
        if self.graph_query.is_empty() {
            return Vec::new();
        }
        let query = if self.graph_case {
            self.graph_query.clone()
        } else {
            self.graph_query.to_lowercase()
        };
        entries
            .iter()
            .filter(|e| {
                let text = format!(
                    "{} {} {} {}",
                    e.str("hash"),
                    history::summary(e),
                    e.str("author"),
                    e.str("email")
                );
                (if self.graph_case {
                    text
                } else {
                    text.to_lowercase()
                })
                .contains(&query)
            })
            .map(|e| e.str("hash").to_owned())
            .collect()
    }

    /// A changed query starts a new result set. Advancing belongs only to
    /// the explicit previous/next actions, as in Zed's GitGraph::search.
    pub(crate) fn graph_search(&mut self) {
        let entries = self
            .graph
            .as_ref()
            .map(|g| g.entries.as_slice())
            .unwrap_or(&[]);
        if let Some(first) = self.graph_matches(entries).into_iter().next() {
            self.commit = Some(first);
        }
    }

    pub(crate) fn graph_find(&mut self, previous: bool) {
        let entries = self
            .graph
            .as_ref()
            .map(|g| g.entries.as_slice())
            .unwrap_or(&[]);
        let matches = self.graph_matches(entries);
        if matches.is_empty() {
            return;
        }
        let current = matches
            .iter()
            .position(|hash| Some(hash.as_str()) == self.commit.as_deref());
        let next = match current {
            Some(i) if previous => (i + matches.len() - 1) % matches.len(),
            Some(i) => (i + 1) % matches.len(),
            None if previous => matches.len() - 1,
            None => 0,
        };
        self.commit = Some(matches[next].clone());
    }
}
