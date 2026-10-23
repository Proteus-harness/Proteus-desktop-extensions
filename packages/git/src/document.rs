//! Document tabs: a working tree diff, all changes, a commit and the commit graph.
//!
//! Arguments double as tab titles (`document.label` is `{filename}`):
//! `src/a.rs · diff`, `Все изменения`, `Коммит abc1234`, `Граф коммитов`.

use proteus_extension::ui::{
    Node, button, confirm, diff, fill, muted, note, row, spacer, stack, text,
};
use proteus_extension::{Host, Value, ValueExt, json, local_time};

use crate::model::{self, Staging};
use crate::{Git, history};

const DIFF: &str = " · diff";
const CHANGES: &str = "Все изменения";
const COMMIT: &str = "Коммит ";
const GRAPH: &str = "Граф коммитов";
/// Files whose diffs the changes document reads at most.
const CHANGES_LIMIT: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Document {
    Diff(String),
    Changes,
    Commit(String),
    Graph,
}

impl Document {
    pub fn parse(argument: &str) -> Self {
        if let Some(path) = argument.strip_suffix(DIFF) {
            return Self::Diff(path.to_owned());
        }
        if let Some(hash) = argument.strip_prefix(COMMIT)
            && (4..=64).contains(&hash.len())
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Self::Commit(hash.to_owned());
        }
        match argument {
            CHANGES => Self::Changes,
            GRAPH => Self::Graph,
            path => Self::Diff(path.to_owned()),
        }
    }

    pub fn argument(&self) -> String {
        match self {
            Self::Diff(path) => format!("{path}{DIFF}"),
            Self::Changes => CHANGES.to_owned(),
            Self::Commit(hash) => format!("{COMMIT}{hash}"),
            Self::Graph => GRAPH.to_owned(),
        }
    }
}

pub fn danger(value: impl ToString) -> Value {
    text(value).with("tone", "danger")
}

pub(crate) fn icon_button(icon: &str, action: &str, value: impl Into<Value>, hint: &str) -> Value {
    button("", action)
        .with("value", value.into())
        .with("variant", "ghost")
        .with("icon", icon)
        .with("hint", hint)
}

fn diff_info(data: &Value, entry: Option<&Value>) -> Value {
    let message = match data.str("kind") {
        "binary" => Some("Двоичный файл"),
        "too_large" => Some("Diff превышает предел 512 КБ"),
        "unavailable" => Some("Diff недоступен"),
        _ => None,
    };
    json!({"path": data.str("path"), "status": entry.map(|entry| entry.str("status")).unwrap_or(""),
           "message": message})
}

fn patch_view(id: &str, data: &Value, entry: Option<&Value>) -> Value {
    diff(id, data.str("patch")).with("files", json!([diff_info(data, entry)]))
}

impl Git {
    pub fn commit_argument(&self, hash: &str) -> String {
        Document::Commit(self.short_of(hash)).argument()
    }

    pub fn document(&mut self, host: &Host, document: &Document) -> Value {
        let mut children = Vec::new();
        children.extend(self.error.as_ref().map(danger));
        let body = match document {
            Document::Diff(path) => self.diff_document(host, path),
            Document::Changes => self.changes_document(host),
            Document::Commit(hash) => self.commit_document(host, hash),
            Document::Graph => self.graph_document(host),
        };
        match body {
            Ok(body) => children.extend(body),
            Err(error) => children.push(danger(error)),
        }
        fill(children)
    }

    fn working_diff(&mut self, host: &Host, path: &str) -> proteus_extension::Result<Value> {
        if let Some(diff) = self.diffs.get(path) {
            return Ok(diff.clone());
        }
        let diff = host.api("/workspace/diff", &[("path", path)])?;
        self.diffs.insert(path.to_owned(), diff.clone());
        Ok(diff)
    }

    fn diff_document(&mut self, host: &Host, path: &str) -> proteus_extension::Result<Vec<Value>> {
        let entry = self
            .changes()
            .list("entries")
            .iter()
            .find(|entry| entry.str("path") == path)
            .cloned();
        let mut header = Vec::new();
        if let Some(entry) = &entry {
            header.push(spacer());
            match model::staging(entry) {
                Staging::Staged => header.push(
                    button("Unstage", "unstage")
                        .with("value", json!([path]))
                        .with("icon", "minus"),
                ),
                _ => header.push(
                    button("Stage", "stage")
                        .with("value", json!([path]))
                        .with("icon", "plus"),
                ),
            }
            let (label, action, title) = if model::untracked(entry) {
                ("Удалить", "trash", "Удалить")
            } else {
                ("Отменить", "discard", "Отменить изменения в")
            };
            let ask = confirm(&format!("{title} `{}`?", model::name(path)))
                .with("message", "Это нельзя отменить.")
                .with("ok", label)
                .with("danger", true);
            header.push(
                button(label, action)
                    .with("value", json!([path]))
                    .with("variant", "ghost")
                    .with("icon", "undo-2")
                    .with("confirm", ask),
            );
        } else {
            header.push(spacer());
        }
        header.push(icon_button("copy", "copy", path, "Копировать путь"));
        let diff = self.working_diff(host, path)?;
        Ok(vec![row(header), patch_view("diff", &diff, entry.as_ref())])
    }

    fn changes_document(&mut self, host: &Host) -> proteus_extension::Result<Vec<Value>> {
        let entries = self.changes().list("entries").to_vec();
        let (added, deleted) = (
            self.changes().i64("added").unwrap_or(0),
            self.changes().i64("deleted").unwrap_or(0),
        );
        let header = row(vec![
            text(format!("Изменения: {}", entries.len())),
            note(format!("+{added} −{deleted}")),
            spacer(),
            icon_button("refresh-cw", "diff_all", Value::Null, "Обновить"),
        ]);
        if entries.is_empty() {
            return Ok(vec![header, muted("Нет изменений для коммита")]);
        }
        let mut patch = String::new();
        let mut files = Vec::new();
        for entry in entries.iter().take(CHANGES_LIMIT) {
            let data = self.working_diff(host, entry.str("path"))?;
            files.push(diff_info(&data, Some(entry)));
            if let Some(text) = data.opt_str("patch") {
                patch.push_str(text);
                if !patch.ends_with('\n') {
                    patch.push('\n');
                }
            }
        }
        let mut children = vec![header];
        if entries.len() > CHANGES_LIMIT {
            children.push(note(format!(
                "Показаны первые {CHANGES_LIMIT} файлов из {}",
                entries.len()
            )));
        }
        children.push(diff("changes", &patch).with("files", files));
        Ok(children)
    }

    fn commit_document(
        &mut self,
        host: &Host,
        hash: &str,
    ) -> proteus_extension::Result<Vec<Value>> {
        let commit = self.commit_details(host, hash)?;
        let full = commit.str("hash").to_owned();
        let mut meta = vec![
            note(format!(
                "{} <{}>  ·  {}",
                commit.str("author"),
                commit.str("email"),
                local_time(commit.f64("time").unwrap_or(0.))
            )),
            spacer(),
            text(commit.str("short")).with("mono", true),
            icon_button("copy", "copy", full.as_str(), "Копировать хеш"),
        ];
        for parent in commit.list("parents").iter().filter_map(Value::as_str) {
            let short: String = parent.chars().take(7).collect();
            meta.push(
                button(&short, "open_commit")
                    .with("value", parent)
                    .with("variant", "ghost")
                    .with("icon", "git-commit-horizontal")
                    .with("hint", "Открыть родительский коммит"),
            );
        }
        let mut header = vec![text(history::summary(&commit)).with("size", "large")];
        if !commit.str("body").is_empty() {
            header.push(muted(commit.str("body")));
        }
        let picked = self.commit_files.get(hash).cloned();
        let files = commit.list("files");
        if picked.is_some() {
            meta.push(
                button("All Files", "all_commit_files")
                    .with("variant", "ghost")
                    .with("hint", "Показать изменения всех файлов этого коммита"),
            );
        }
        header.push(row(meta));
        let patch = match picked {
            Some(path) => patch_view("commit-patch", &self.commit_diff(host, &full, &path)?, files.iter().find(|file| file.str("path") == path)),
            None => diff("commit-patch", commit.str("patch")).with("files", files.iter().map(|file| {
                let message = if commit.bool("truncated") {
                    Some("Выберите файл в дереве графа коммитов")
                } else if file.at("added").is_null() { Some("Двоичный файл") } else { None };
                json!({"path":file.str("path"), "status":file.str("status"), "message":message})
            }).collect::<Vec<_>>()),
        };
        let mut right = vec![];
        if commit.bool("truncated") {
            right.push(row(vec![
                note("Общий diff обрезан: выберите файл в графе"),
                button("Commit Graph", "graph").with("variant", "ghost"),
            ]));
        }
        right.push(patch);
        let mut children = vec![stack(header)];
        children.extend(right);
        Ok(children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_round_trip_and_read_as_titles() {
        for document in [
            Document::Diff("src/a.rs".into()),
            Document::Changes,
            Document::Commit("abc1234".into()),
            Document::Graph,
        ] {
            assert_eq!(Document::parse(&document.argument()), document);
        }
        assert_eq!(
            Document::parse("sample.py"),
            Document::Diff("sample.py".into())
        );
        assert_eq!(
            Document::parse("Коммит xyz"),
            Document::Diff("Коммит xyz".into())
        );
    }
}
