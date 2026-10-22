//! Views of the panel, the branch picker and the diff document.

use std::time::{SystemTime, UNIX_EPOCH};

use proteus_extension::ui::{
    Node, button, checkbox, code, confirm, fill, input, item, menu, muted, row, stack, text, tree,
};
use proteus_extension::{Host, Result, Value, ValueExt, json};

use crate::Git;
use crate::changes;

fn danger(value: impl ToString) -> Value {
    text(value).with("tone", "danger")
}

fn plural(count: usize) -> &'static str {
    if count % 10 == 1 && count % 100 != 11 {
        "файл"
    } else if matches!(count % 10, 2..=4) && !matches!(count % 100, 12..=14) {
        "файла"
    } else {
        "файлов"
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.)
}

fn copy_button(path: &str) -> Value {
    button("", "copy")
        .with("value", path)
        .with("variant", "ghost")
        .with("icon", "copy")
        .with("hint", "Копировать путь")
}

impl Git {
    fn notice(&self) -> Option<Value> {
        self.error.as_ref().map(danger)
    }

    pub fn branch_picker(&mut self, host: &Host, current: &str) -> Result<Value> {
        let choices: Vec<Value> = self
            .branch_list(host)?
            .iter()
            .map(|entry| {
                json!({"type": "choice", "label": entry.at("name"), "selected": entry.str("name") == current,
                       "action": "checkout", "value": entry.at("name")})
            })
            .collect();
        let mut children = vec![muted("Переключить ветку")];
        children.extend(self.notice());
        if choices.is_empty() {
            children.push(muted("Веток нет."));
        }
        children.extend(choices);
        Ok(stack(children))
    }

    pub fn panel(&mut self, host: &Host, branch: &str) -> Result<Value> {
        let changes = self.changes.clone().unwrap_or_default();
        if !changes.bool("repository") {
            return Ok(muted("Папка не является git-репозиторием."));
        }
        let entries = changes.list("entries");
        let staged = entries
            .iter()
            .filter(|entry| entry.at("staged").truthy())
            .count();
        let mut branches: Vec<Value> = self
            .branch_list(host)?
            .iter()
            .take(40)
            .map(|entry| {
                let name = entry.str("name");
                item(name, "checkout")
                    .with("value", entry.at("name").clone())
                    .maybe("icon", (name == branch).then_some("check"))
            })
            .collect();
        if branches.is_empty() {
            branches.push(item("Веток нет", "noop"));
        }
        let ask = confirm("Отменить все изменения?")
            .with(
                "message",
                "Изменённые файлы вернутся к HEAD, новые будут удалены. Это нельзя отменить.",
            )
            .with("ok", "Отменить всё")
            .with("danger", true);
        let actions = menu(vec![
            item("Добавить всё в индекс", "stage_all").with("icon", "plus"),
            item("Убрать всё из индекса", "unstage_all").with("icon", "minus"),
            item("Отменить все изменения", "discard_all")
                .with("icon", "undo-2")
                .with("danger", true)
                .with("separated", true)
                .with("confirm", ask),
        ])
        .with("hint", "Действия");
        let header = row(vec![
            menu(branches)
                .with("label", branch)
                .with("icon", "git-branch")
                .with("hint", "Переключить ветку"),
            muted(self.sync_lines().join("  ")),
            actions,
        ]);
        let mut rows = changes::change_rows(entries, &self.collapsed, self.selected.as_deref());
        if self.history && self.log.is_none() {
            self.log = Some(
                host.api("/workspace/log", &[("limit", "30")])?
                    .list("entries")
                    .to_vec(),
            );
        }
        rows.extend(changes::history_rows(
            self.log.as_deref(),
            self.history,
            now(),
        ));
        let mut children = vec![header];
        children.extend(self.notice());
        if entries.is_empty() {
            children.push(muted("Изменений нет."));
        }
        let label = if staged > 0 {
            format!("Закоммитить {staged} {}", plural(staged))
        } else {
            "Закоммитить".to_owned()
        };
        let commit = stack(vec![
            input(
                "message",
                &self.message,
                "Сообщение коммита (Ctrl+Enter)",
                "message",
            )
            .with("lines", 4)
            .with("submit", "commit"),
            row(vec![
                checkbox("Изменить последний коммит", self.amend, "amend"),
                button(&label, "commit")
                    .with("variant", "primary")
                    .with("icon", "git-commit-horizontal"),
            ]),
        ]);
        children.push(tree("changes", rows, "tree"));
        children.push(commit);
        Ok(fill(children))
    }

    pub fn document(&mut self, host: &Host, path: &str) -> Value {
        let entry = self.changes.as_ref().and_then(|changes| {
            changes
                .list("entries")
                .iter()
                .find(|entry| entry.str("path") == path)
                .cloned()
        });
        let body = self.diff(host, path).unwrap_or_else(danger);
        let mut header = vec![text(path)];
        if let Some(entry) = &entry {
            header.push(muted(changes::label(entry.str("status"))));
            if entry.at("unstaged").truthy() {
                header.push(
                    button("В индекс", "stage")
                        .with("value", json!([path]))
                        .with("icon", "plus"),
                );
            }
            if entry.at("staged").truthy() {
                header.push(
                    button("Из индекса", "unstage")
                        .with("value", json!([path]))
                        .with("icon", "minus"),
                );
            }
            let ask = confirm(&format!("Отменить изменения в {}?", changes::name(path)))
                .with("message", "Это нельзя отменить.")
                .with("ok", "Отменить")
                .with("danger", true);
            header.push(
                button("Отменить", "discard")
                    .with("value", json!([path]))
                    .with("variant", "ghost")
                    .with("icon", "undo-2")
                    .with("confirm", ask),
            );
        }
        header.push(copy_button(path));
        let mut children = vec![row(header)];
        children.extend(self.notice());
        children.push(body);
        fill(children)
    }

    fn diff(&mut self, host: &Host, path: &str) -> Result<Value> {
        if !self.documents.contains_key(path) {
            let diff = host.api("/workspace/diff", &[("path", path)])?;
            self.documents.insert(path.to_owned(), diff);
        }
        let diff = &self.documents[path];
        Ok(match diff.opt_str("patch") {
            Some(patch) => code("diff", patch, "diff"),
            None => muted(match diff.str("kind") {
                "binary" => "Двоичный файл",
                "too_large" => "Diff превышает предел 512 КБ",
                _ => "Изменений нет",
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::plural;

    #[test]
    fn plural_follows_russian_rules() {
        let words: Vec<&str> = [1, 2, 5, 11, 12, 21, 22, 25, 111]
            .into_iter()
            .map(plural)
            .collect();
        assert_eq!(
            words,
            [
                "файл",
                "файла",
                "файлов",
                "файлов",
                "файлов",
                "файл",
                "файла",
                "файлов",
                "файлов"
            ]
        );
    }
}
