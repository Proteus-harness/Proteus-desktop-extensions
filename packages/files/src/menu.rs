// Адаптировано из Zed: crates/project_panel/src/project_panel.rs (deploy_context_menu)
//! Context menu of a project entry in Zed's order.

use proteus_extension::ui::{Node, confirm, item, prompt};
use proteus_extension::{Value, json};

use crate::paths;

/// What the menu of one row may offer.
pub struct Target<'a> {
    pub path: &'a str,
    pub directory: bool,
    /// Absolute path, or the relative one when the session folder is unknown.
    pub absolute: &'a str,
    pub repository: bool,
    /// The file has changes to show.
    pub changed: bool,
    /// Tracked edits that can be restored from HEAD.
    pub restorable: bool,
}

pub fn build(target: &Target) -> Vec<Value> {
    let path = target.path;
    let root = path.is_empty();
    let name = paths::name(path);
    let folder = if target.directory {
        path
    } else {
        paths::parent(path)
    };
    let mut groups = vec![
        vec![
            item("Новый файл", "new-file").with("value", folder).with(
                "prompt",
                ask("Новый файл", "Создать").with("placeholder", "имя или папка/имя"),
            ),
            item("Новая папка", "new-folder")
                .with("value", folder)
                .with(
                    "prompt",
                    ask("Новая папка", "Создать").with("placeholder", "имя или папка/имя"),
                ),
        ],
        vec![item("Копировать путь", "copy").with("value", target.absolute)],
    ];
    if root {
        groups.push(vec![
            item("Свернуть всё", "collapse-all").with("value", path),
        ]);
        return separated(groups);
    }
    groups[1].push(item("Копировать относительный путь", "copy").with("value", path));
    if target.repository {
        let mut git = Vec::new();
        if !target.directory && target.restorable {
            git.push(
                item("Отменить изменения", "restore")
                    .with("value", path)
                    .with(
                        "confirm",
                        confirm(&format!("Отменить изменения в `{name}`?"))
                            .with(
                                "message",
                                "Несохранённые в git изменения файла будут потеряны.",
                            )
                            .with("ok", "Отменить изменения")
                            .with("danger", true),
                    ),
            );
        }
        git.push(item("Добавить в .gitignore", "ignore").with("value", json!([path, "gitignore"])));
        git.push(
            item("Добавить в .git/info/exclude", "ignore").with("value", json!([path, "exclude"])),
        );
        if !target.directory && target.changed {
            git.push(item("Показать изменения", "diff").with("value", path));
        }
        groups.push(git);
    }
    groups.push(vec![
        item("Переименовать", "rename").with("value", path).with(
            "prompt",
            ask("Переименовать", "Переименовать")
                .with("value", name)
                .with("stem", !target.directory),
        ),
        item("Удалить", "delete")
            .with("value", path)
            .with("danger", true)
            .with("confirm", {
                let question = confirm(&format!("Удалить `{name}` безвозвратно?"))
                    .with("ok", "Удалить")
                    .with("danger", true);
                if target.directory {
                    question.with("message", "Папка будет удалена со всем содержимым.")
                } else {
                    question
                }
            }),
    ]);
    if target.directory {
        groups.push(vec![
            item("Развернуть всё", "expand-all").with("value", path),
            item("Свернуть всё", "collapse-all").with("value", path),
        ]);
    }
    separated(groups)
}

fn ask(title: &str, ok: &str) -> Value {
    prompt(title).with("ok", ok)
}

/// Joins non-empty groups with separators like Zed's `ContextMenu::separator`.
fn separated(groups: Vec<Vec<Value>>) -> Vec<Value> {
    let mut items = Vec::new();
    for group in groups.into_iter().filter(|group| !group.is_empty()) {
        let first = !items.is_empty();
        for (index, entry) in group.into_iter().enumerate() {
            items.push(if first && index == 0 {
                entry.with("separated", true)
            } else {
                entry
            });
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_extension::ValueExt;

    fn labels(items: &[Value]) -> Vec<String> {
        items
            .iter()
            .map(|item| {
                let mark = if item.bool("separated") { "| " } else { "" };
                format!("{mark}{}", item.str("label"))
            })
            .collect()
    }

    fn target(path: &str, directory: bool) -> Target<'_> {
        Target {
            path,
            directory,
            absolute: "/w/x",
            repository: true,
            changed: true,
            restorable: true,
        }
    }

    #[test]
    fn a_changed_file_gets_git_entries_in_zed_order() {
        let items = build(&target("src/a.rs", false));
        assert_eq!(
            labels(&items),
            [
                "Новый файл",
                "Новая папка",
                "| Копировать путь",
                "Копировать относительный путь",
                "| Отменить изменения",
                "Добавить в .gitignore",
                "Добавить в .git/info/exclude",
                "Показать изменения",
                "| Переименовать",
                "Удалить",
            ]
        );
        assert_eq!(items[0].str("value"), "src");
        assert_eq!(items[2].str("value"), "/w/x");
        assert_eq!(items[3].str("value"), "src/a.rs");
        assert_eq!(items[5].at("value"), &json!(["src/a.rs", "gitignore"]));
        let rename = &items[8]["prompt"];
        assert_eq!((rename.str("value"), rename.bool("stem")), ("a.rs", true));
        assert_eq!(
            items[9]["confirm"].str("title"),
            "Удалить `a.rs` безвозвратно?"
        );
        assert!(items[4]["confirm"].bool("danger"));
    }

    #[test]
    fn folders_and_the_root_get_their_own_entries() {
        let mut folder = target("src", true);
        folder.repository = false;
        let items = build(&folder);
        assert_eq!(
            labels(&items),
            [
                "Новый файл",
                "Новая папка",
                "| Копировать путь",
                "Копировать относительный путь",
                "| Переименовать",
                "Удалить",
                "| Развернуть всё",
                "Свернуть всё",
            ]
        );
        assert_eq!(items[0].str("value"), "src");
        assert!(!items[4]["prompt"].bool("stem"));
        assert!(items[5]["confirm"].get("message").is_some());

        let root = build(&target("", true));
        assert_eq!(
            labels(&root),
            [
                "Новый файл",
                "Новая папка",
                "| Копировать путь",
                "| Свернуть всё"
            ]
        );
    }

    #[test]
    fn unchanged_files_offer_only_ignore_entries() {
        let mut file = target("a.rs", false);
        (file.changed, file.restorable) = (false, false);
        let items = build(&file);
        let actions: Vec<&str> = items.iter().map(|item| item.str("action")).collect();
        assert_eq!(
            actions,
            [
                "new-file",
                "new-folder",
                "copy",
                "copy",
                "ignore",
                "ignore",
                "rename",
                "delete"
            ]
        );
    }
}
