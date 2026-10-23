// Адаптировано из Zed: crates/git_ui/src/git_panel.rs, crates/git_ui/src/git_ui.rs
//! The panel (changes and history tabs) and the branch picker popup.

use proteus_extension::ui::{
    Node, bar, button, confirm, empty, fill, icon, input, item, menu, menu_header, muted, note,
    panel_editor, prompt, row, spacer, split_button, stats, tree,
};
use proteus_extension::{Host, Result, Value, ValueExt, json};

use crate::document::danger;
use crate::model::{self, Counts, Group, Remote, Section, Staging};
use crate::{Git, history};

pub fn status_label(status: &str) -> &str {
    match status {
        "conflict" => "конфликт",
        "deleted" => "удалён",
        "untracked" => "новый",
        "added" => "добавлен",
        "renamed" => "переименован",
        "modified" => "изменён",
        other => other,
    }
}

/// Zed's `GitStatusIcon`: icon and colour of a file status.
pub fn status_icon(status: &str) -> (&'static str, &'static str) {
    match status {
        "conflict" => ("triangle-alert", "warning"),
        "deleted" => ("square-minus", "danger"),
        "added" | "untracked" => ("square-plus", "success"),
        _ => ("square-dot", "warning"),
    }
}

/// Path of a file row; rows of the staging sections are prefixed by section
/// because a partially staged file is listed twice.
pub fn row_path(identity: &str) -> &str {
    match identity.split_once(':') {
        Some(("staged" | "unstaged" | "file", path)) => path,
        _ => identity,
    }
}

fn row_id(section: Section, path: &str) -> String {
    match section {
        Section::Staged | Section::Unstaged => format!("{}:{path}", section.key()),
        _ => format!("file:{path}"),
    }
}

/// Zed's entry context menu.
pub fn entry_menu(entry: &Value) -> Vec<Value> {
    let path = entry.str("path");
    let file = model::name(path);
    let created = model::untracked(entry);
    let mut menu = vec![match model::staging(entry) {
        Staging::Staged => item("Убрать из индекса", "unstage")
            .with("value", json!([path]))
            .with("icon", "minus"),
        _ => item("Добавить в индекс", "stage")
            .with("value", json!([path]))
            .with("icon", "plus"),
    }];
    let (label, action, title) = if created {
        ("Удалить файл", "trash", format!("Удалить `{file}`?"))
    } else if entry.str("status") == "deleted" {
        (
            "Восстановить файл",
            "discard",
            format!("Восстановить `{file}`?"),
        )
    } else {
        (
            "Отменить изменения",
            "discard",
            format!("Отменить изменения в `{file}`?"),
        )
    };
    let ask = confirm(&title)
        .with("message", "Это нельзя отменить.")
        .with("ok", label)
        .with("danger", true);
    menu.push(
        item(label, action)
            .with("value", json!([path]))
            .with("icon", "undo-2")
            .with("danger", true)
            .with("confirm", ask),
    );
    menu.extend([
        item("Копировать путь", "copy_path")
            .with("value", path)
            .with("icon", "copy")
            .with("separated", true),
        item("Копировать относительный путь", "copy").with("value", path),
        item("Добавить в .gitignore", "ignore")
            .with("value", json!([path]))
            .with("disabled", !created)
            .with("separated", true),
        item("Добавить в .git/info/exclude", "exclude")
            .with("value", json!([path]))
            .with("disabled", !created),
        item("Открыть diff", "diff")
            .with("value", path)
            .with("icon", "file-diff")
            .with("separated", true),
    ]);
    menu
}

/// Header and file rows of the changes tree.
pub fn change_rows(
    entries: &[Value],
    group: Group,
    collapsed: &std::collections::HashSet<&'static str>,
    selected: Option<&str>,
) -> Vec<Value> {
    let mut rows = Vec::new();
    for (section, items) in model::sections(entries, group) {
        let open = !collapsed.contains(section.key());
        let checked = match model::section_staging(section, &items) {
            Staging::Staged => json!(true),
            Staging::Partial => json!("mixed"),
            Staging::Unstaged => json!(false),
        };
        rows.push(
            proteus_extension::ui::tree_header(
                &format!("section:{}", section.key()),
                section.title(),
                open,
            )
            .with("depth", 0)
            .with("count", items.len())
            .with("checked", checked),
        );
        if !open {
            continue;
        }
        for entry in items {
            let path = entry.str("path");
            let status = entry.str("status");
            let (icon, tone) = status_icon(status);
            let id = row_id(section, path);
            let checked = match (section, model::staging(entry)) {
                (Section::Staged, _) => json!(true),
                (Section::Unstaged, _) => json!(false),
                (_, Staging::Staged) => json!(true),
                (_, Staging::Partial) => json!("mixed"),
                (_, Staging::Unstaged) => json!(false),
            };
            let mut detail = model::parent(path).to_owned();
            if let Some(original) = entry.opt_str("original").filter(|o| !o.is_empty()) {
                detail = format!("{detail} ← {}", model::name(original))
                    .trim()
                    .to_owned();
            }
            let mut row = json!({"id": id, "label": model::name(path), "detail": detail, "depth": 1,
                                 "expanded": null, "icon": icon, "icon_tone": tone,
                                 "selected": selected == Some(id.as_str()) || selected == Some(path), "checked": checked,
                                 "strike": status == "deleted", "menu": entry_menu(entry),
                                 "hint": format!("{path} — {}", status_label(status))});
            if let (Some(added), Some(deleted)) =
                (entry.at("added").as_u64(), entry.at("deleted").as_u64())
            {
                row["stats"] = stats(added, deleted);
            }
            rows.push(row);
        }
    }
    rows
}

fn ask(title: &str, message: &str, ok: &str) -> Value {
    confirm(title)
        .with("message", message)
        .with("ok", ok)
        .with("danger", true)
}

/// Zed's panel context menu, opened by the chevron of «Добавить всё».
fn panel_menu(counts: Counts) -> Vec<Value> {
    let mut items = vec![
        item("Добавить всё", "stage_all")
            .with("icon", "plus")
            .with("disabled", counts.unstaged == 0),
        item("Убрать всё из индекса", "unstage_all")
            .with("icon", "minus")
            .with("disabled", counts.staged == 0),
        item("Отменить все изменения", "discard_all")
            .with("icon", "undo-2")
            .with("danger", true)
            .with("disabled", counts.total == 0)
            .with(
                "confirm",
                ask(
                    "Отменить все изменения?",
                    "Изменённые файлы вернутся к HEAD, новые будут удалены. Это нельзя отменить.",
                    "Отменить всё",
                ),
            ),
        item("Спрятать всё", "stash")
            .with("icon", "archive")
            .with("separated", true)
            .with("disabled", counts.total == 0),
    ];
    items.extend([
        item("Спрятать индекс", "stash_staged").with("disabled", counts.staged == 0),
        item("Вернуть спрятанное", "stash_pop").with("icon", "archive-restore"),
        item("Отменить изменения отслеживаемых", "discard_tracked")
            .with("separated", true)
            .with("danger", true)
            .with("disabled", counts.tracked == 0)
            .with(
                "confirm",
                ask(
                    "Отменить изменения отслеживаемых файлов?",
                    "Отслеживаемые файлы вернутся к HEAD. Это нельзя отменить.",
                    "Отменить",
                ),
            ),
        item("Удалить новые файлы", "trash_untracked")
            .with("danger", true)
            .with("disabled", counts.untracked == 0)
            .with(
                "confirm",
                ask(
                    "Удалить новые файлы?",
                    "Неотслеживаемые файлы будут удалены. Это нельзя отменить.",
                    "Удалить",
                ),
            ),
    ]);
    items
}

fn view_options(group: Group) -> Value {
    menu(vec![
        menu_header("Группировать"),
        item("По статусу", "group")
            .with("value", "status")
            .with("checked", group == Group::Status),
        item("В индексе и не в индексе", "group")
            .with("value", "staging")
            .with("checked", group == Group::Staging),
    ])
    .with("icon", "list-filter")
    .with("hint", "Вид")
}

fn remote_button(changes: &Value) -> Value {
    let (label, icon, action, hint) = Remote::of(changes).button();
    let force = ask(
        "Принудительно отправить?",
        "Коммиты на удалённом сервере, которых нет локально, будут потеряны.",
        "Отправить",
    );
    split_button(
        &label,
        action,
        vec![
            item("Получить", "fetch").with("icon", "refresh-cw"),
            item("Подтянуть", "pull").with("icon", "arrow-down"),
            item("Подтянуть (rebase)", "pull_rebase"),
            item("Отправить", "push")
                .with("icon", "arrow-up")
                .with("separated", true),
            item("Принудительно отправить", "force_push")
                .with("danger", true)
                .with("confirm", force),
        ],
    )
    .with("icon", icon)
    .with("variant", "default")
    .with("hint", hint)
}

impl Git {
    pub fn branch_menu(&mut self, host: &Host, current: &str) -> Result<Vec<Value>> {
        let mut items = vec![menu_header("Ветки")];
        let branches = self.branch_list(host)?;
        if branches.is_empty() {
            items.push(item("Веток нет", "noop").with("disabled", true));
        }
        for entry in branches.iter().take(60) {
            let name = entry.str("name");
            items.push(
                item(name, "checkout")
                    .with("value", name)
                    .with("checked", name == current),
            );
        }
        let ask = prompt("Новая ветка")
            .with("placeholder", "Имя ветки")
            .with("ok", "Создать");
        items.push(
            item("Новая ветка…", "create_branch")
                .with("icon", "git-branch-plus")
                .with("separated", true)
                .with("prompt", ask),
        );
        Ok(items)
    }

    pub fn branch_picker(&mut self, host: &Host, current: &str) -> Result<Value> {
        let mut children = vec![note("Переключить ветку")];
        children.extend(self.error.as_ref().map(danger));
        let branches = self.branch_list(host)?;
        if branches.is_empty() {
            children.push(muted("Веток нет."));
        }
        for entry in &branches {
            let name = entry.str("name");
            children.push(
                json!({"type": "choice", "label": name, "selected": name == current,
                                 "action": "checkout", "value": name}),
            );
        }
        let ask = prompt("Новая ветка")
            .with("placeholder", "Имя ветки")
            .with("ok", "Создать");
        children.push(
            button("Новая ветка…", "create_branch")
                .with("variant", "ghost")
                .with("icon", "git-branch-plus")
                .with("prompt", ask),
        );
        Ok(proteus_extension::ui::stack(children))
    }

    pub fn panel(&mut self, host: &Host, branch: &str) -> Result<Value> {
        let changes = self.changes();
        if !changes.bool("repository") {
            return Ok(fill(vec![empty("Нет git-репозитория")]));
        }
        let entries = changes.list("entries").to_vec();
        let mut changes_tab = self.changes_tab(&changes, &entries);
        changes_tab.extend(self.error.as_ref().map(danger));
        changes_tab.push(self.footer(host, &changes, branch)?);
        changes_tab.push(self.commit_editor(&changes, &entries));
        changes_tab.extend(self.last_commit(host, &changes));
        let history_tab = self.history_tab(host, &changes);
        let tabs = json!({"type": "tabs", "action": "tab", "tabs": [
            {"label": "Изменения", "count": entries.len(), "children": changes_tab},
            {"label": "История", "children": history_tab},
        ]});
        Ok(fill(vec![tabs]))
    }

    fn changes_tab(&self, changes: &Value, entries: &[Value]) -> Vec<Value> {
        let counts = Counts::of(entries);
        let stages = counts.total == 0 || counts.unstaged > 0;
        let (label, action) = if stages {
            ("Добавить всё", "stage_all")
        } else {
            ("Убрать всё", "unstage_all")
        };
        let mut header = vec![
            button("Показать diff", "diff_all")
                .with("variant", "ghost")
                .with("icon", "file-diff")
                .with("disabled", counts.total == 0),
        ];
        let (added, deleted) = (
            changes.i64("added").unwrap_or(0),
            changes.i64("deleted").unwrap_or(0),
        );
        if added + deleted > 0 {
            header.push(note(format!("+{added} −{deleted}")));
        }
        header.extend([
            spacer(),
            view_options(self.group),
            split_button(label, action, panel_menu(counts)).with("disabled", counts.total == 0),
        ]);
        let mut children = vec![row(header)];
        if entries.is_empty() {
            children.push(empty("Нет изменений для коммита"));
            return children;
        }
        if changes.bool("truncated") {
            children.push(note("Список изменений обрезан"));
        }
        let rows = change_rows(
            entries,
            self.group,
            &self.collapsed,
            self.selected.as_deref(),
        );
        children.push(
            tree("changes", rows, "tree")
                .with("chevrons", false)
                .with("indent", 16),
        );
        children
    }

    /// Zed's history tab: placeholders while there is nothing to list.
    fn history_tab(&mut self, host: &Host, changes: &Value) -> Vec<Value> {
        if self.tab != 1 {
            return vec![empty("Загрузка истории…")];
        }
        let Ok(log) = self.history(host, false) else {
            return vec![empty("Не удалось загрузить историю коммитов")];
        };
        if log.entries.is_empty() {
            return vec![empty("Коммитов пока нет")];
        }
        let ahead = match changes.opt_str("upstream") {
            Some(upstream) if !upstream.is_empty() => changes.i64("ahead").unwrap_or(0).max(0),
            _ => 0,
        } as usize;
        let rows = history::panel_rows(&log.entries, ahead, history::now());
        let mut children = vec![history::panel_list(rows)];
        if log.more {
            children.push(bar(vec![
                button("Загрузить ещё", "more").with("variant", "ghost"),
            ]));
        }
        children
    }

    /// Zed's `PanelRepoFooter`: the branch picker and the remote button.
    fn footer(&mut self, host: &Host, changes: &Value, branch: &str) -> Result<Value> {
        let branches = menu(self.branch_menu(host, branch)?)
            .with("label", branch)
            .with("hint", "Переключить ветку");
        Ok(bar(vec![
            icon("git-branch"),
            branches,
            spacer(),
            remote_button(changes),
        ])
        .with("border", false))
    }

    /// Zed's commit editor and the commit footer under it.
    fn commit_editor(&self, changes: &Value, entries: &[Value]) -> Value {
        let counts = Counts::of(entries);
        let message = model::message(&self.message, entries);
        let state = model::commit_button(counts, self.amend, message.is_some());
        let placeholder =
            model::suggestion(entries).unwrap_or_else(|| "Введите сообщение коммита".to_owned());
        let mut options = Vec::new();
        if changes
            .at("head")
            .opt_str("short")
            .is_some_and(|short| !short.is_empty())
        {
            options.push(item("Исправить последний коммит", "amend").with("checked", self.amend));
        }
        options.extend([
            item("Подпись (signoff)", "signoff").with("checked", self.signoff),
            item("Без хуков", "no_verify").with("checked", self.no_verify),
        ]);
        let flags: String = [
            (self.amend, " --amend"),
            (self.signoff, " --signoff"),
            (self.no_verify, " --no-verify"),
        ]
        .into_iter()
        .filter_map(|(on, flag)| on.then_some(flag))
        .collect();
        let hint = match state.blocked {
            Some(reason) => reason.to_owned(),
            None => format!("{} · git commit{flags}", state.label),
        };
        let commit = split_button(state.label, "commit", options)
            .with("variant", "default")
            .with("disabled", state.blocked.is_some())
            .with("hint", hint);
        let (toggle, label) = if self.editor_collapsed {
            ("chevron-up", "Показать редактор коммита")
        } else {
            ("chevron-down", "Скрыть редактор коммита")
        };
        let toggle = button("", "toggle_editor")
            .with("variant", "ghost")
            .with("icon", toggle)
            .with("hint", label);
        let mut children = Vec::new();
        if !self.editor_collapsed {
            children.push(
                input("message", &self.message, &placeholder, "message")
                    .with("lines", model::EDITOR_LINES)
                    .with("submit", "commit")
                    .with("flush", true)
                    .with("mono", true),
            );
        }
        children.push(bar(vec![toggle, spacer(), commit]));
        panel_editor(children)
    }

    /// Zed's `render_previous_commit`.
    fn last_commit(&mut self, host: &Host, changes: &Value) -> Option<Value> {
        let head = changes.at("head");
        let short = head.opt_str("short").filter(|short| !short.is_empty())?;
        let summary = head.str("summary");
        let mut items = vec![
            button(
                if summary.is_empty() { short } else { summary },
                "open_commit",
            )
            .with("value", short)
            .with("variant", "ghost")
            .with("hint", format!("Открыть коммит {short}")),
            spacer(),
        ];
        if self.head_has_parent(host, short) {
            items.push(
                button("", "uncommit")
                    .with("variant", "ghost")
                    .with("icon", "undo-2")
                    .with("hint", "Отменить коммит"),
            );
        }
        items.push(
            button("", "graph")
                .with("variant", "ghost")
                .with("icon", "git-graph")
                .with("hint", "Открыть граф коммитов"),
        );
        Some(bar(items))
    }
}
