// Адаптировано из Zed: crates/git_ui/src/git_panel.rs
//! What the panel knows about `/workspace/changes`: sections, counts, the
//! commit message suggestion and the state of the commit and remote buttons.

use proteus_extension::{Value, ValueExt};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Group {
    /// Конфликты / Отслеживаемые / Новые.
    #[default]
    Status,
    /// Конфликты / В индексе / Не в индексе.
    Staging,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Conflicts,
    Tracked,
    New,
    Staged,
    Unstaged,
}

impl Section {
    pub const ALL: [Section; 5] = [
        Self::Conflicts,
        Self::Tracked,
        Self::New,
        Self::Staged,
        Self::Unstaged,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Conflicts => "conflicts",
            Self::Tracked => "tracked",
            Self::New => "new",
            Self::Staged => "staged",
            Self::Unstaged => "unstaged",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.key() == key)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Conflicts => "Конфликты",
            Self::Tracked => "Отслеживаемые",
            Self::New => "Новые",
            Self::Staged => "В индексе",
            Self::Unstaged => "Не в индексе",
        }
    }
}

/// How much of a file is in the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Staging {
    Unstaged,
    Partial,
    Staged,
}

pub fn staging(entry: &Value) -> Staging {
    match (entry.at("staged").truthy(), entry.at("unstaged").truthy()) {
        (true, false) => Staging::Staged,
        (true, true) => Staging::Partial,
        _ => Staging::Unstaged,
    }
}

pub fn conflict(entry: &Value) -> bool {
    entry.str("status") == "conflict"
}

/// A new file: untracked or added to the index.
pub fn created(entry: &Value) -> bool {
    matches!(entry.str("status"), "untracked" | "added")
}

pub fn untracked(entry: &Value) -> bool {
    entry.str("status") == "untracked" && !entry.at("staged").truthy()
}

pub fn name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Folder of a path, empty at the top.
pub fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |index| &path[..index])
}

/// Sections with their files in display order; a partially staged file is
/// listed in both staging sections.
pub fn sections(entries: &[Value], group: Group) -> Vec<(Section, Vec<&Value>)> {
    let order = match group {
        Group::Status => [Section::Conflicts, Section::Tracked, Section::New],
        Group::Staging => [Section::Conflicts, Section::Staged, Section::Unstaged],
    };
    order
        .into_iter()
        .map(|section| {
            let items = entries
                .iter()
                .filter(|entry| belongs(entry, section))
                .collect();
            (section, items)
        })
        .filter(|(_, items): &(Section, Vec<&Value>)| !items.is_empty())
        .collect()
}

fn belongs(entry: &Value, section: Section) -> bool {
    match section {
        Section::Conflicts => conflict(entry),
        _ if conflict(entry) => false,
        Section::Tracked => !created(entry),
        Section::New => created(entry),
        Section::Staged => entry.at("staged").truthy(),
        Section::Unstaged => entry.at("unstaged").truthy(),
    }
}

/// How much of a section is in the index, for its header checkbox: a
/// partially staged file makes the section partial.
pub fn section_staging(section: Section, items: &[&Value]) -> Staging {
    let (full, some) = match section {
        Section::Staged => (true, true),
        Section::Unstaged => (false, false),
        _ => (
            items.iter().all(|entry| staging(entry) == Staging::Staged),
            items
                .iter()
                .any(|entry| staging(entry) != Staging::Unstaged),
        ),
    };
    match (full, some) {
        (true, _) => Staging::Staged,
        (false, true) => Staging::Partial,
        _ => Staging::Unstaged,
    }
}

/// Counts behind the panel's buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub total: usize,
    pub staged: usize,
    pub unstaged: usize,
    pub tracked: usize,
    pub staged_tracked: usize,
    pub untracked: usize,
    pub unstaged_conflicts: usize,
}

impl Counts {
    pub fn of(entries: &[Value]) -> Self {
        let mut counts = Self {
            total: entries.len(),
            ..Self::default()
        };
        for entry in entries {
            let staged = entry.at("staged").truthy();
            counts.staged += staged as usize;
            counts.unstaged += entry.at("unstaged").truthy() as usize;
            if conflict(entry) {
                counts.unstaged_conflicts += entry.at("unstaged").truthy() as usize;
            } else if !created(entry) {
                counts.tracked += 1;
                counts.staged_tracked += staged as usize;
            }
            counts.untracked += untracked(entry) as usize;
        }
        counts
    }
}

/// Zed's message for a commit of one file: «Обновить a.rs», «Создать …», «Удалить …».
pub fn suggestion(entries: &[Value]) -> Option<String> {
    let staged: Vec<&Value> = entries
        .iter()
        .filter(|entry| entry.at("staged").truthy())
        .collect();
    let tracked: Vec<&Value> = entries
        .iter()
        .filter(|entry| !conflict(entry) && !created(entry))
        .collect();
    let entry = match (staged.as_slice(), tracked.as_slice()) {
        ([single], _) => *single,
        ([], [single]) => *single,
        _ => return None,
    };
    if entries.iter().any(conflict) {
        return None;
    }
    let verb = match entry.str("status") {
        "deleted" => "Удалить",
        "added" | "untracked" => "Создать",
        "modified" | "renamed" => "Обновить",
        _ => return None,
    };
    Some(format!("{verb} {}", name(entry.str("path"))))
}

/// The message a commit uses: the typed text or, when nothing is typed, the suggestion.
pub fn message(typed: &str, entries: &[Value]) -> Option<String> {
    if !typed.trim().is_empty() {
        Some(typed.to_owned())
    } else if typed.is_empty() {
        suggestion(entries)
    } else {
        None
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommitButton {
    pub label: &'static str,
    /// Why the button is disabled.
    pub blocked: Option<&'static str>,
    /// Stage tracked changes first: nothing is in the index.
    pub all_tracked: bool,
}

pub fn commit_button(counts: Counts, amend: bool, has_message: bool) -> CommitButton {
    let label = match (amend, counts.staged > 0, counts.tracked > 0) {
        (true, false, true) => "Исправить отслеживаемые",
        (true, ..) => "Исправить",
        (false, true, _) => "Коммит",
        (false, false, _) => "Коммит отслеживаемых",
    };
    let blocked = if counts.unstaged_conflicts > 0 {
        Some("Сначала разрешите конфликты")
    } else if counts.staged == 0 && counts.tracked == 0 && !amend {
        Some("Нет изменений для коммита")
    } else if !has_message {
        Some("Нет сообщения коммита")
    } else {
        None
    };
    CommitButton {
        label,
        blocked,
        all_tracked: counts.staged == 0 && counts.tracked > 0,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Remote {
    Publish,
    Fetch,
    Push(u64),
    Pull { ahead: u64, behind: u64 },
}

impl Remote {
    pub fn of(changes: &Value) -> Self {
        let (ahead, behind) = (
            changes.i64("ahead").unwrap_or(0).max(0) as u64,
            changes.i64("behind").unwrap_or(0).max(0) as u64,
        );
        if changes.opt_str("upstream").is_none_or(str::is_empty) {
            Self::Publish
        } else if behind > 0 {
            Self::Pull { ahead, behind }
        } else if ahead > 0 {
            Self::Push(ahead)
        } else {
            Self::Fetch
        }
    }

    /// Label, icon, action and hint of the main part of the button.
    pub fn button(&self) -> (String, &'static str, &'static str, &'static str) {
        match *self {
            Self::Publish => (
                "Опубликовать".into(),
                "cloud-upload",
                "publish",
                "Опубликовать ветку на удалённом сервере",
            ),
            Self::Fetch => (
                "Получить".into(),
                "refresh-cw",
                "fetch",
                "Получить изменения с удалённого сервера",
            ),
            Self::Push(ahead) => (
                format!("Отправить ↑{ahead}"),
                "arrow-up",
                "push",
                "Отправить коммиты на удалённый сервер",
            ),
            Self::Pull { ahead, behind } => {
                let label = if ahead > 0 {
                    format!("Подтянуть ↓{behind} ↑{ahead}")
                } else {
                    format!("Подтянуть ↓{behind}")
                };
                (
                    label,
                    "arrow-down",
                    "pull",
                    "Подтянуть изменения с удалённого сервера",
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_extension::json;

    fn entry(path: &str, status: &str, staged: Option<&str>, unstaged: Option<&str>) -> Value {
        json!({"path": path, "status": status, "staged": staged, "unstaged": unstaged})
    }

    fn sample() -> Vec<Value> {
        vec![
            entry("src/a.rs", "modified", Some("modified"), Some("modified")),
            entry("b.py", "deleted", None, Some("deleted")),
            entry("new.md", "untracked", None, Some("untracked")),
            entry("both.txt", "conflict", Some("conflict"), Some("conflict")),
        ]
    }

    fn keys(entries: &[Value], group: Group) -> Vec<(&'static str, Vec<&str>)> {
        sections(entries, group)
            .into_iter()
            .map(|(section, items)| {
                (
                    section.key(),
                    items.iter().map(|entry| entry.str("path")).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn sections_group_by_status_or_staging() {
        let entries = sample();
        assert_eq!(
            keys(&entries, Group::Status),
            [
                ("conflicts", vec!["both.txt"]),
                ("tracked", vec!["src/a.rs", "b.py"]),
                ("new", vec!["new.md"])
            ]
        );
        assert_eq!(
            keys(&entries, Group::Staging),
            [
                ("conflicts", vec!["both.txt"]),
                ("staged", vec!["src/a.rs"]),
                ("unstaged", vec!["src/a.rs", "b.py", "new.md"])
            ]
        );
        assert_eq!(staging(&entries[0]), Staging::Partial);
        let status = sections(&entries, Group::Status);
        assert_eq!(section_staging(status[1].0, &status[1].1), Staging::Partial);
        assert_eq!(
            section_staging(status[2].0, &status[2].1),
            Staging::Unstaged
        );
        let counts = Counts::of(&entries);
        assert_eq!((counts.staged, counts.tracked, counts.untracked), (2, 2, 1));
        assert_eq!(counts.unstaged_conflicts, 1);
    }

    #[test]
    fn suggestion_names_a_single_file() {
        let staged = [
            entry("src/a.rs", "modified", Some("modified"), None),
            entry("b.py", "modified", None, Some("modified")),
        ];
        assert_eq!(suggestion(&staged).as_deref(), Some("Обновить a.rs"));
        let tracked = [
            entry("gone.txt", "deleted", None, Some("deleted")),
            entry("new.md", "untracked", None, Some("untracked")),
        ];
        assert_eq!(suggestion(&tracked).as_deref(), Some("Удалить gone.txt"));
        let created = [entry("n.rs", "added", Some("added"), None)];
        assert_eq!(suggestion(&created).as_deref(), Some("Создать n.rs"));
        let many = [
            entry("a", "modified", None, Some("modified")),
            entry("b", "modified", None, Some("modified")),
        ];
        assert_eq!(suggestion(&many), None);
        assert_eq!(message("", &created).as_deref(), Some("Создать n.rs"));
        assert_eq!(message("  ", &created), None);
        assert_eq!(message("Fix", &many).as_deref(), Some("Fix"));
    }

    #[test]
    fn commit_button_explains_why_it_is_disabled() {
        let counts = Counts::of(&sample());
        assert_eq!(
            commit_button(counts, false, true).blocked,
            Some("Сначала разрешите конфликты")
        );
        let tracked = Counts::of(&[entry("a", "modified", None, Some("modified"))]);
        let button = commit_button(tracked, false, true);
        assert_eq!(
            (button.label, button.blocked, button.all_tracked),
            ("Коммит отслеживаемых", None, true)
        );
        assert_eq!(
            commit_button(tracked, false, false).blocked,
            Some("Нет сообщения коммита")
        );
        assert_eq!(
            commit_button(tracked, true, true).label,
            "Исправить отслеживаемые"
        );
        let empty = Counts::default();
        assert_eq!(
            commit_button(empty, false, true).blocked,
            Some("Нет изменений для коммита")
        );
        assert_eq!(commit_button(empty, true, true).blocked, None);
        let staged = Counts::of(&[entry("a", "modified", Some("modified"), None)]);
        assert_eq!(commit_button(staged, false, true).label, "Коммит");
    }

    #[test]
    fn remote_button_follows_the_upstream() {
        let state = |upstream: Option<&str>, ahead: u64, behind: u64| {
            Remote::of(&json!({"upstream": upstream, "ahead": ahead, "behind": behind}))
        };
        assert_eq!(state(None, 3, 0), Remote::Publish);
        assert_eq!(state(Some("origin/main"), 0, 0), Remote::Fetch);
        assert_eq!(state(Some("origin/main"), 2, 0).button().0, "Отправить ↑2");
        assert_eq!(
            state(Some("origin/main"), 1, 4).button().0,
            "Подтянуть ↓4 ↑1"
        );
    }
}
