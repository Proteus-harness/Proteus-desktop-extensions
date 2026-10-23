// Адаптировано из Zed: crates/git_ui/src/git_panel.rs (render_commit_history)
//! Commit history: pages of `/workspace/log` drawn as `lanes` in the graph
//! and as two-line `list` rows in the panel, and commits read once.

use std::time::{SystemTime, UNIX_EPOCH};

use proteus_extension::ui::{Node, item, lane_row, lane_segment, lanes, list, list_row};
use proteus_extension::{Host, Result, Value, ValueExt, json};

use crate::graph::{self, Segment};
use crate::{Git, Log};

pub const PAGE: usize = 100;

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
    entry
        .opt_str("summary")
        .filter(|summary| !summary.is_empty())
        .unwrap_or("(без описания)")
}

/// Rows of the `lanes` widget for commits newest first.
pub fn rows(entries: &[Value], selected: Option<&str>, _now: f64) -> Vec<Value> {
    let commits: Vec<(&str, Vec<&str>)> = entries
        .iter()
        .map(|entry| {
            let parents = entry.list("parents").iter().filter_map(Value::as_str);
            (entry.str("hash"), parents.collect())
        })
        .collect();
    let layout = graph::layout(&commits);
    entries
        .iter()
        .zip(layout)
        .map(|(entry, lane)| {
            let hash = entry.str("hash");
            let refs: Vec<Value> = entry
                .list("refs")
                .iter()
                .map(|chip| json!({"label": chip.str("name"), "tone": tone(chip.str("kind"))}))
                .collect();
            let time = entry.f64("time").unwrap_or(0.);
            let menu = commit_menu(hash);
            lane_row(hash, summary(entry), lane.lane, lane.color)
                .with("up", segments(&lane.up))
                .with("down", segments(&lane.down))
                .with("refs", refs)
                .with(
                    "cells",
                    vec![
                        String::new(),
                        summary(entry).to_owned(),
                        proteus_extension::local_time(time),
                        entry.str("author").to_owned(),
                        entry.str("short").to_owned(),
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
pub fn panel_rows(entries: &[Value], ahead: usize, now: f64) -> Vec<Value> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let (hash, short) = (entry.str("hash"), entry.str("short"));
            let tags: Vec<&str> = entry
                .list("refs")
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
            let time = entry.f64("time").map(|time| ago(time, now));
            let row = list_row(hash, summary(entry))
                .with("chips", tags)
                .with("avatar", true)
                .with(
                    "meta",
                    vec![
                        entry.str("author").to_owned(),
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
        Ok(Log {
            entries: page.list("entries").to_vec(),
            more: page.bool("has_more"),
        })
    }

    /// History of the current branch, or of every ref with `all`.
    pub fn history(&mut self, host: &Host, all: bool) -> Result<Log> {
        let slot = if all { &self.graph } else { &self.log };
        if let Some(log) = slot {
            return Ok(log.clone());
        }
        let log = Self::fetch_log(host, all, 0)?;
        *(if all { &mut self.graph } else { &mut self.log }) = Some(log.clone());
        Ok(log)
    }

    pub fn more(&mut self, host: &Host, all: bool) -> Result {
        let mut log = self.history(host, all)?;
        let page = Self::fetch_log(host, all, log.entries.len())?;
        log.entries.extend(page.entries);
        log.more = page.more;
        *(if all { &mut self.graph } else { &mut self.log }) = Some(log);
        Ok(())
    }

    /// HEAD has a parent: «Отменить коммит» has a commit to return to.
    pub fn head_has_parent(&mut self, host: &Host, short: &str) -> bool {
        if let Some((known, parent)) = &self.head_parent
            && known == short
        {
            return *parent;
        }
        let first = match self.log.as_ref().and_then(|log| log.entries.first()) {
            Some(entry) if entry.str("short") == short => Some(entry.clone()),
            _ => host
                .api(
                    "/workspace/log",
                    &[("limit", "1"), ("skip", "0"), ("all", "false")],
                )
                .ok()
                .and_then(|log| log.list("entries").first().cloned()),
        };
        let Some(first) = first else {
            return false;
        };
        let parent = !first.list("parents").is_empty();
        self.head_parent = Some((short.to_owned(), parent));
        parent
    }

    pub fn commit_details(&mut self, host: &Host, hash: &str) -> Result<Value> {
        if let Some(commit) = self.commits.get(hash) {
            return Ok(commit.clone());
        }
        let commit = host.api("/workspace/commit", &[("hash", hash)])?;
        self.commits.insert(hash.to_owned(), commit.clone());
        Ok(commit)
    }

    pub fn commit_diff(&mut self, host: &Host, hash: &str, path: &str) -> Result<Value> {
        let key = (hash.to_owned(), path.to_owned());
        if let Some(diff) = self.commit_diffs.get(&key) {
            return Ok(diff.clone());
        }
        let diff = host.api("/workspace/commit/diff", &[("hash", hash), ("path", path)])?;
        self.commit_diffs.insert(key, diff.clone());
        Ok(diff)
    }

    pub fn short_of(&self, hash: &str) -> String {
        let logs = [self.log.as_ref(), self.graph.as_ref()];
        logs.into_iter()
            .flatten()
            .flat_map(|log| &log.entries)
            .find(|entry| entry.str("hash") == hash)
            .map(|entry| entry.str("short").to_owned())
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
        ];
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
        ];
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
}
