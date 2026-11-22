// Адаптировано из Zed: crates/git_ui/src/git_panel.rs (render_commit_history)
//! Commit history: pages of `/workspace/log` drawn as `lanes` in the graph
//! and as two-line `list` rows in the panel, and commits read once.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use proteus_extension::ui::{Node, item, lane_row, lane_segment, lanes, list, list_row};
use proteus_extension::{Host, Result, Value, ValueExt, json};

use crate::graph::{self, Segment};
use crate::{Git, Log};

pub const PAGE: usize = 100;

/// The top-level log record has a fixed set of fields used by History and Graph.
/// Keep parent/ref elements as received: the HEAD affordance checks raw parents,
/// while graph lanes and ref chips apply their own interpretation.
#[derive(Debug)]
pub struct LogEntry {
    pub hash: String,
    pub short: String,
    pub author: String,
    pub email: String,
    summary: String,
    parents: Vec<Value>,
    refs: Vec<Value>,
    time: Option<f64>,
}

impl From<Value> for LogEntry {
    fn from(mut entry: Value) -> Self {
        fn take(entry: &mut Value, key: &str) -> Value {
            entry.get_mut(key).map(Value::take).unwrap_or_default()
        }

        fn text(entry: &mut Value, key: &str) -> String {
            match take(entry, key) {
                Value::String(text) => text,
                _ => String::new(),
            }
        }

        fn list(entry: &mut Value, key: &str) -> Vec<Value> {
            match take(entry, key) {
                Value::Array(items) => items,
                _ => Vec::new(),
            }
        }

        Self {
            hash: text(&mut entry, "hash"),
            short: text(&mut entry, "short"),
            author: text(&mut entry, "author"),
            email: text(&mut entry, "email"),
            summary: text(&mut entry, "summary"),
            parents: list(&mut entry, "parents"),
            refs: list(&mut entry, "refs"),
            time: entry.f64("time"),
        }
    }
}

impl LogEntry {
    pub fn summary(&self) -> &str {
        summary_or_placeholder(&self.summary)
    }
}

impl Log {
    fn from_page(mut page: Value) -> Self {
        let more = page.bool("has_more");
        let entries = match page.get_mut("entries").map(Value::take) {
            Some(Value::Array(items)) => {
                let mut entries = Vec::with_capacity(items.len());
                entries.extend(items.into_iter().map(LogEntry::from));
                entries
            }
            _ => Vec::new(),
        };
        Self { entries, more }
    }
}

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.)
}

/// Short age like Zed's history: «5 мин», «2 ч», «3 дн».
pub fn ago(seconds: f64, now: f64) -> String {
    let delta = ((now - seconds) as i64).max(0);
    for (size, unit) in [
        (86400 * 365, "г"),
        (86400 * 30, "мес"),
        (86400 * 7, "нед"),
        (86400, "дн"),
        (3600, "ч"),
        (60, "мин"),
    ] {
        if delta >= size {
            return format!("{} {unit}", delta / size);
        }
    }
    "только что".to_owned()
}

fn tone(kind: &str) -> &'static str {
    match kind {
        "head" | "branch" => "accent",
        "tag" => "warning",
        _ => "muted",
    }
}

fn segments(segments: &[Segment]) -> Vec<Value> {
    segments
        .iter()
        .map(|segment| lane_segment(segment.from, segment.to, segment.color))
        .collect()
}

pub fn summary(entry: &Value) -> &str {
    summary_or_placeholder(entry.str("summary"))
}

fn summary_or_placeholder(summary: &str) -> &str {
    if summary.is_empty() {
        "(без описания)"
    } else {
        summary
    }
}

/// Rows of the `lanes` widget for commits newest first.
pub fn rows(entries: &[LogEntry], selected: Option<&str>, _now: f64) -> Vec<Value> {
    let commits: Vec<(&str, Vec<&str>)> = entries
        .iter()
        .map(|entry| {
            let parents = entry.parents.iter().filter_map(Value::as_str);
            (entry.hash.as_str(), parents.collect())
        })
        .collect();
    let layout = graph::layout(&commits);
    entries
        .iter()
        .zip(layout)
        .map(|(entry, lane)| {
            let hash = entry.hash.as_str();
            let refs: Vec<Value> = entry
                .refs
                .iter()
                .map(|chip| json!({"label": chip.str("name"), "tone": tone(chip.str("kind"))}))
                .collect();
            let time = entry.time.unwrap_or(0.);
            let menu = commit_menu(hash);
            lane_row(hash, entry.summary(), lane.lane, lane.color)
                .with("up", segments(&lane.up))
                .with("down", segments(&lane.down))
                .with("refs", refs)
                .with(
                    "cells",
                    vec![
                        String::new(),
                        entry.summary().to_owned(),
                        proteus_extension::local_time(time),
                        entry.author.clone(),
                        entry.short.clone(),
                    ],
                )
                .with("selected", Some(hash) == selected)
                .with("menu", menu)
        })
        .collect()
}

pub fn widget(id: &str, action: &str, rows: Vec<Value>) -> Value {
    let columns = [
        ("Graph", "graph", 30.),
        ("Description", "text", 37.),
        ("Date", "text", 13.),
        ("Author", "person", 11.),
        ("Commit", "mono", 9.),
    ]
    .into_iter()
    .map(|(label, kind, weight)| json!({"label":label,"kind":kind,"weight":weight}))
    .collect();
    lanes(id, columns, rows, action)
}

fn commit_menu(hash: &str) -> Vec<Value> {
    vec![
        item("View Commit", "open_commit")
            .with("value", hash)
            .with("icon", "git-commit-horizontal"),
        item("Keep Commit", "keep_commit")
            .with("value", hash)
            .with("icon", "pin"),
        item("Копировать хеш", "copy")
            .with("value", hash)
            .with("icon", "copy"),
    ]
}

/// Rows of the panel history like Zed's: the subject with its tags and an
/// arrow on the first `ahead` commits, which are not pushed yet, over the
/// author, the age and the short hash.
pub fn panel_rows(entries: &[LogEntry], ahead: usize, now: f64) -> Vec<Value> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let (hash, short) = (entry.hash.as_str(), entry.short.as_str());
            let tags: Vec<&str> = entry
                .refs
                .iter()
                .filter(|chip| chip.str("kind") == "tag")
                .map(|chip| chip.str("name"))
                .collect();
            let unpushed = index < ahead;
            let detail = if unpushed {
                format!("Есть неотправленные изменения — {short}")
            } else {
                short.to_owned()
            };
            let time = entry.time.map(|time| ago(time, now));
            let row = list_row(hash, entry.summary())
                .with("chips", tags)
                .with("avatar", true)
                .with(
                    "meta",
                    vec![
                        entry.author.clone(),
                        time.unwrap_or_default(),
                        short.to_owned(),
                    ],
                )
                .with("hint", "View Commit")
                .with("hint_detail", detail)
                .with("menu", commit_menu(hash));
            if unpushed {
                row.with("mark", "arrow-up")
            } else {
                row
            }
        })
        .collect()
}

pub fn panel_list(rows: Vec<Value>) -> Value {
    list("log", rows, "log")
}

impl Git {
    fn fetch_log(host: &Host, all: bool, skip: usize) -> Result<Log> {
        let (limit, skip) = (PAGE.to_string(), skip.to_string());
        let all = if all { "true" } else { "false" };
        let page = host.api(
            "/workspace/log",
            &[("limit", &limit), ("skip", &skip), ("all", all)],
        )?;
        Ok(Log::from_page(page))
    }

    /// Load history for the current branch or, with `all`, every ref.
    pub fn ensure_history(&mut self, host: &Host, all: bool) -> Result<()> {
        let slot = if all { &mut self.graph } else { &mut self.log };
        if slot.is_none() {
            *slot = Some(Self::fetch_log(host, all, 0)?);
        }
        Ok(())
    }

    pub fn more(&mut self, host: &Host, all: bool) -> Result {
        self.ensure_history(host, all)?;
        let slot = if all { &mut self.graph } else { &mut self.log };
        let log = slot.as_mut().expect("history was loaded");
        let page = Self::fetch_log(host, all, log.entries.len())?;
        log.entries.extend(page.entries);
        log.more = page.more;
        Ok(())
    }

    /// HEAD has a parent: «Отменить коммит» has a commit to return to.
    pub fn head_has_parent(&mut self, host: &Host, short: &str) -> bool {
        if let Some((known, parent)) = &self.head_parent
            && known == short
        {
            return *parent;
        }
        let parent = match self.log.as_ref().and_then(|log| log.entries.first()) {
            Some(entry) if entry.short == short => !entry.parents.is_empty(),
            _ => {
                let reply = host
                    .api(
                        "/workspace/log",
                        &[("limit", "1"), ("skip", "0"), ("all", "false")],
                    )
                    .ok();
                let Some(first) = reply.as_ref().and_then(|log| log.list("entries").first()) else {
                    return false;
                };
                !first.list("parents").is_empty()
            }
        };
        self.head_parent = Some((short.to_owned(), parent));
        parent
    }

    pub fn commit_details(&mut self, host: &Host, hash: &str) -> Result<Arc<Value>> {
        if let Some(commit) = self.history_cache.commit(hash) {
            return Ok(commit);
        }
        let commit = Arc::new(host.api("/workspace/commit", &[("hash", hash)])?);
        self.history_cache.insert_commit(hash, Arc::clone(&commit));
        Ok(commit)
    }

    pub fn commit_diff(&mut self, host: &Host, hash: &str, path: &str) -> Result<Arc<Value>> {
        if let Some(diff) = self.history_cache.diff(hash, path) {
            return Ok(diff);
        }
        let diff = Arc::new(host.api("/workspace/commit/diff", &[("hash", hash), ("path", path)])?);
        self.history_cache
            .insert_diff(hash, path, Arc::clone(&diff));
        Ok(diff)
    }

    pub fn short_of(&self, hash: &str) -> String {
        let logs = [self.log.as_ref(), self.graph.as_ref()];
        logs.into_iter()
            .flatten()
            .flat_map(|log| &log.entries)
            .find(|entry| entry.hash == hash)
            .map(|entry| entry.short.clone())
            .unwrap_or_else(|| hash.chars().take(7).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ago_names_the_largest_unit() {
        assert_eq!(ago(1000., 1030.), "только что");
        assert_eq!(ago(0., 7200.), "2 ч");
        assert_eq!(ago(0., 86400. * 3.), "3 дн");
        assert_eq!(ago(0., 86400. * 400.), "1 г");
        assert_eq!(ago(100., 50.), "только что");
    }

    #[test]
    fn rows_carry_lanes_refs_and_menus() {
        let entries = [
            json!({"hash": "b", "short": "b", "parents": ["a"], "summary": "", "author": "X", "time": 0,
                   "refs": [{"name": "HEAD", "kind": "head"}, {"name": "origin/main", "kind": "remote"}]}),
            json!({"hash": "a", "short": "a", "parents": [], "summary": "Init", "author": "Y", "time": 0}),
        ].map(LogEntry::from);
        let rows = rows(&entries, Some("a"), 120.);
        assert_eq!(rows[0].str("label"), "(без описания)");
        assert_eq!(rows[0]["refs"][1]["tone"], "muted");
        assert_eq!(rows[0]["down"], json!([{"from": 0, "to": 0, "color": 0}]));
        assert_eq!(rows[1]["up"], rows[0]["down"]);
        assert!(rows[1].bool("selected"));
        assert_eq!(rows[1]["cells"][3], "Y");
    }

    #[test]
    fn panel_rows_follow_zed_history() {
        let tag = |name: &str| json!({"name": name, "kind": "tag"});
        let entries = [
            json!({"hash": "b", "short": "bbbbbbb", "parents": ["a"], "summary": "Fix", "author": "X", "time": 0,
                   "refs": [{"name": "main", "kind": "branch"}, tag("v1"), tag("v2"), tag("v3"), tag("v4")]}),
            json!({"hash": "a", "short": "aaaaaaa", "parents": [], "summary": "Init", "author": "Y", "time": 0}),
        ].map(LogEntry::from);
        let rows = panel_rows(&entries, 1, 7200.);
        assert_eq!(rows[0]["chips"], json!(["v1", "v2", "v3", "v4"]));
        assert_eq!(rows[0].str("mark"), "arrow-up");
        assert_eq!(rows[0]["meta"], json!(["X", "2 ч", "bbbbbbb"]));
        assert_eq!(
            rows[0].str("hint_detail"),
            "Есть неотправленные изменения — bbbbbbb"
        );
        assert!(rows[1].get("mark").is_none());
        assert_eq!(rows[1].str("hint_detail"), "aaaaaaa");
    }

    #[test]
    fn page_moves_decoded_fields_and_keeps_malformed_rows_and_nested_order() {
        let raw = json!({"has_more":true,"entries":[
            {"hash":"1234567890-λ","short":"1234567","author":"A","email":"a@example.invalid",
             "summary":"Fix λ","time":2.5,"parents":[null,"base",false],
             "refs":[{"name":"v2","kind":"tag"},null,{"kind":"tag","name":"v1"}],"unused":"drop"},
            null,
            {"hash":42,"short":false,"author":{},"email":[],"summary":"","parents":{},"refs":false,"time":"12"}
        ]});
        let hash = raw["entries"][0]["hash"].as_str().unwrap().as_ptr();
        let parents = raw["entries"][0]["parents"].as_array().unwrap().as_ptr();
        let refs = raw["entries"][0]["refs"].as_array().unwrap().as_ptr();
        let page = Log::from_page(raw);
        assert!(page.more);
        assert_eq!(page.entries.len(), 3);
        let first = &page.entries[0];
        assert_eq!(
            (
                first.hash.as_ptr(),
                first.parents.as_ptr(),
                first.refs.as_ptr()
            ),
            (hash, parents, refs)
        );
        assert_eq!(
            first.parents,
            vec![Value::Null, json!("base"), json!(false)]
        );
        assert_eq!(first.time, Some(2.5));
        for entry in &page.entries[1..] {
            assert_eq!(
                (&*entry.hash, &*entry.short, &*entry.author, &*entry.email),
                ("", "", "", "")
            );
            assert_eq!(entry.summary(), "(без описания)");
            assert!(entry.parents.is_empty() && entry.refs.is_empty() && entry.time.is_none());
        }
        let graph = rows(&page.entries, None, 7200.);
        assert_eq!(
            graph[0]["refs"],
            json!([
                {"label":"v2","tone":"warning"},{"label":"","tone":"muted"},{"label":"v1","tone":"warning"}
            ])
        );
        let panel = panel_rows(&page.entries, 0, 7200.);
        assert_eq!(panel[0]["chips"], json!(["v2", "v1"]));
        assert_eq!(panel[1]["meta"][1], "");
        assert!(
            Log::from_page(json!({"entries":{},"has_more":17}))
                .entries
                .is_empty()
        );
    }
}
