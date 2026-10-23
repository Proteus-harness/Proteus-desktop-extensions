//! Turn list and turn summary of the analysis page.

use proteus_extension::ui::Node;
use proteus_extension::{Value, ValueExt, json};

use crate::facts;
use crate::nodes::{Open, badge, card, column, disclosure, ghost, kv, label, row, small};
use crate::text::{plain, preview};

const RECENT: usize = 8;
const LONGEST: usize = 200;
const TASK: usize = 600;

/// Newest first; the recent ones unless expanded, always with the selected turn.
pub fn turn_rows<'a>(turns: &'a [Value], selected: &Value, show_all: bool) -> Vec<&'a Value> {
    let ordered: Vec<&Value> = turns.iter().rev().collect();
    let shown = if show_all { ordered.len() } else { RECENT };
    let mut rows: Vec<&Value> = ordered.iter().copied().take(shown.min(LONGEST)).collect();
    if selected.truthy() && rows.iter().all(|turn| turn.at("turn_id") != selected) {
        rows.extend(
            ordered
                .iter()
                .copied()
                .filter(|turn| turn.at("turn_id") == selected),
        );
    }
    rows
}

pub fn turn_list(turns: &[Value], selected: &Value, show_all: bool) -> Value {
    let mut nodes = Vec::new();
    for turn in turn_rows(turns, selected, show_all) {
        let status = turn.at("status");
        let status_text = status
            .as_str()
            .and_then(facts::turn_status)
            .map(str::to_owned)
            .unwrap_or_else(|| plain(status));
        let prompt = preview(&plain(turn.at("prompt_preview")), 180);
        let prompt = if prompt.is_empty() {
            "Пустая задача".to_owned()
        } else {
            prompt
        };
        nodes.push(
            column(vec![
                row(vec![
                    ghost(
                        format!("Ход {}", plain(turn.at("number"))),
                        "turn",
                        turn.at("turn_id").clone(),
                        turn.at("turn_id") == selected,
                    ),
                    badge(status_text, facts::turn_tone(status.as_str())),
                    small(facts::clock(turn.at("started_at_ms")), "muted"),
                ]),
                small(prompt, "default").with("single_line", true),
            ])
            .with("gap", "none"),
        );
    }
    if turns.len() > RECENT {
        let hidden = if show_all {
            "Только последние".to_owned()
        } else {
            format!("Все ходы · {}", turns.len())
        };
        let mut footer = vec![ghost(hidden, "all-turns", Value::Null, false)];
        if show_all && turns.len() > LONGEST {
            footer.push(small(format!("Показаны последние {LONGEST}."), "muted"));
        }
        nodes.push(row(footer));
    }
    card(format!("Ходы · {}", turns.len()), nodes)
}

fn metric(name: &str, value: impl ToString, tone: &str) -> Value {
    json!({"type": "metric", "label": name, "value": value.to_string(), "tone": tone})
}

pub fn turn_summary(turn: &Value, number: &Value, status: &Value, open: &Open) -> Value {
    let steps = turn.list("steps");
    let summary = facts::metrics(steps);
    let task = turn.at("task").str("text");
    let turn_id = plain(turn.at("turn_id"));
    let key = |name: &str| format!("turn:{turn_id}:{name}");
    let status_text = match status.as_str().and_then(facts::turn_status) {
        Some(text) => text.to_owned(),
        None if status.truthy() => plain(status),
        None => "—".to_owned(),
    };
    let short = preview(task, TASK);
    let mut nodes = vec![
        row(vec![
            badge(status_text, facts::turn_tone(status.as_str())),
            small(
                format!("Длительность шагов: {}", facts::turn_duration(steps)),
                "muted",
            ),
        ]),
        label(short, "default").with("single_line", true),
    ];
    if task.chars().count() > TASK {
        nodes.push(disclosure(
            &key("task"),
            "Полный текст задачи",
            &task.into(),
            open,
        ));
    }
    let cwd = turn.at("task").at("cwd");
    if cwd.truthy() {
        nodes.push(kv("Каталог", plain(cwd), "default"));
    }
    nodes.push(
        row(vec![
            json!({"type": "metric", "label": "Запросы модели", "value": summary.models.to_string()}),
            json!({"type": "metric", "label": "Инструменты", "value": summary.tools.to_string()}),
            metric("Ошибки шагов", summary.errors, if summary.errors > 0 { "danger" } else { "default" }),
            metric("Без результата", summary.missing, if summary.missing > 0 { "warning" } else { "default" }),
            json!({"type": "metric", "label": "Токены: вход / выход", "value": facts::usage_text(&summary),
                   "detail": format!("usage у {} из {} запросов", summary.with_usage, summary.models)}),
        ])
        .with("gap", "medium"),
    );
    if turn.at("error").truthy() {
        nodes.push(label(
            format!("Причина завершения: {}", plain(turn.at("error"))),
            "danger",
        ));
    }
    let output = turn.at("output").at("text");
    let output = if output.truthy() {
        output.clone()
    } else {
        "Итоговый ответ не записан.".into()
    };
    nodes.push(disclosure(&key("output"), "Итоговый ответ", &output, open));
    let config = format!(
        "Конфигурация при запуске · epoch {}",
        plain(turn.at("module_epoch"))
    );
    nodes.push(disclosure(
        &key("config"),
        &config,
        turn.at("config_snapshot"),
        open,
    ));
    let title = if number.truthy() {
        format!("Ход {}", plain(number))
    } else {
        "Ход".to_owned()
    };
    card(title, nodes)
}
