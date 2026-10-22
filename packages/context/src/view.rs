//! The popup: window map, provider cache, history, tools and compaction.

use proteus_extension::ui::{Node, badge, card, cells, kv, metric, note, row, text};
use proteus_extension::{Value, ValueExt, exact};

use crate::window::{self, Cache};

pub fn render(snapshot: &Value) -> Vec<Value> {
    let window::Window {
        used,
        maximum,
        usage,
    } = window::window(snapshot);
    let cache = window::cache(usage);
    let (cache_detail, cache_tone) = window::cache_label(cache.status);
    let segments = window::cells(snapshot);
    let whole = |value: f64| exact(Some(value.round() as i64));
    let mut children = vec![
        row(vec![
            text(window::usage_title(usage)).with("size", "large"),
            badge(window::source_label(usage)),
        ]),
        row(vec![
            metric("Занято", whole(used)).with(
                "detail",
                match maximum {
                    Some(maximum) => format!("{}% окна", (used / maximum * 100. + 0.5).floor()),
                    None => "размер окна неизвестен".to_owned(),
                },
            ),
            metric(
                "Свободно",
                maximum
                    .map(|maximum| whole((maximum - used).max(0.)))
                    .unwrap_or_else(|| "n/a".to_owned()),
            )
            .with(
                "detail",
                maximum
                    .map(|maximum| format!("из {}", whole(maximum)))
                    .unwrap_or_else(|| "max_input_tokens не задан".to_owned()),
            ),
            metric("Кэш-хиты", percent(cache.hit)).with("detail", "входной кэш провайдера"),
            metric("Кэш", cache.status)
                .with("detail", cache_detail)
                .with("tone", cache_tone),
        ])
        .with("gap", "medium"),
        if segments.is_empty() {
            note("Для этой сессии ещё нет замеров использования")
        } else {
            cells(segments)
        },
        row(vec![history(snapshot), provider_cache(&cache)]).with("gap", "medium"),
        row(vec![tools(snapshot), compaction(snapshot)]).with("gap", "medium"),
    ];
    let diagnostics: Vec<Value> = snapshot
        .list("diagnostics")
        .iter()
        .filter_map(Value::as_str)
        .map(|item| text(item).with("tone", "warning"))
        .collect();
    if !diagnostics.is_empty() {
        children.push(card(Some("Диагностика"), diagnostics));
    }
    children
}

fn percent(value: Option<i64>) -> String {
    value
        .map(|value| format!("{value}%"))
        .unwrap_or_else(|| "n/a".to_owned())
}

fn count(value: &Value, key: &str) -> String {
    value.i64(key).unwrap_or(0).to_string()
}

fn history(snapshot: &Value) -> Value {
    let history = snapshot.at("history");
    let mut rows = vec![
        ("Сообщений", count(history, "messages")),
        ("От меня", count(history, "user_messages")),
        ("От агента", count(history, "assistant_messages")),
        ("Результаты инструментов", count(history, "tool_results")),
        (
            "Оценка",
            format!("{} токенов", exact(history.i64("estimated_tokens"))),
        ),
    ];
    let latest = snapshot.at("latest_context");
    if latest.is_object() {
        rows.push(("Чанков в ходе", count(latest, "chunks")));
        rows.push(("Токенов хода", exact(latest.i64("token_estimate"))));
    }
    card(Some("История"), vec![kv(rows)])
}

fn provider_cache(cache: &Cache) -> Value {
    let value = |tokens: Option<i64>| {
        tokens
            .map(|tokens| exact(Some(tokens)))
            .unwrap_or_else(|| "n/a".to_owned())
    };
    card(
        Some("Кэш провайдера"),
        vec![
            row(vec![
                badge(cache.status).with("tone", window::cache_label(cache.status).1),
            ]),
            kv(vec![
                ("Вход", value(cache.input)),
                ("Из кэша", value(cache.cached)),
                ("Записано", value(cache.written)),
                ("Кэш-хиты", percent(cache.hit)),
            ]),
        ],
    )
}

fn tools(snapshot: &Value) -> Value {
    let summary = snapshot.at("tools");
    let names: Vec<Value> = summary
        .list("names")
        .iter()
        .filter_map(Value::as_str)
        .map(badge)
        .collect();
    card(
        Some("Инструменты"),
        vec![
            kv(vec![
                ("Запущено", count(summary, "requested")),
                ("Завершено", count(summary, "finished")),
                ("С ошибкой", count(summary, "failed")),
            ]),
            if names.is_empty() {
                note("Вызовов инструментов не было")
            } else {
                row(names)
            },
        ],
    )
}

fn compaction(snapshot: &Value) -> Value {
    let latest = snapshot.at("latest_compaction");
    if !latest.is_object() {
        return card(Some("Компакция"), vec![note("Сжатий не было")]);
    }
    let summary = if latest.bool("summary_present") {
        "сохранена, текст скрыт"
    } else {
        "нет текста"
    };
    let mut rows = vec![
        ("Статус", latest.str("status").to_owned()),
        ("Сводка", summary.to_owned()),
    ];
    let report = latest.at("report");
    if report.is_object() {
        let (before, after) = (
            report.i64("original_token_estimate"),
            report.i64("output_token_estimate"),
        );
        rows.push((
            "Изменена",
            if report.bool("changed") {
                "да"
            } else {
                "нет"
            }
            .to_owned(),
        ));
        rows.push((
            "Сообщения",
            format!(
                "{} → {}",
                count(report, "input_messages"),
                count(report, "output_messages")
            ),
        ));
        rows.push((
            "Токены",
            match (before, after) {
                (Some(_), Some(_)) => format!("{} → {}", exact(before), exact(after)),
                (Some(_), None) => exact(before),
                _ => "n/a".to_owned(),
            },
        ));
    }
    card(Some("Компакция"), vec![kv(rows)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_extension::json;

    #[test]
    fn bare_snapshot_reads_as_unknown() {
        let bare = json!({"history": {"messages": 0, "estimated_tokens": 0}, "tools": {"names": []}, "diagnostics": []});
        let text = serde_json::to_string(&render(&bare)).unwrap();
        for fragment in [
            "размер окна неизвестен",
            "Оценка по истории",
            "Сжатий не было",
            "n/a",
        ] {
            assert!(text.contains(fragment), "{fragment}");
        }
        assert!(!text.contains("Диагностика"));
    }
}
