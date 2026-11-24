//! «Запросы и расход»: period totals, per-model split and the request list.

use std::collections::HashMap;
use std::fmt::Write as _;

use indexmap::IndexSet;
use proteus_extension::ui::{Node, button, card, note, row, select, select_owned, table};
use proteus_extension::{Result, Value, ValueExt, bail, json};

use crate::Usage;
use crate::entry::entry;
use crate::report::{self, exact, money};
use crate::widgets::{label, metric, sized};

const PAGE: usize = 20;
const ORDERS: [(&str, &str); 5] = [
    ("oldest", "По порядку"),
    ("newest", "Сначала последние"),
    ("longest", "Сначала долгие"),
    ("tokens", "Больше токенов"),
    ("cost", "Сначала дороже"),
];
const STATUSES: [(&str, &str); 3] = [
    ("all", "Все запросы"),
    ("problems", "С проблемами"),
    ("missing", "Без данных о токенах"),
];

fn choices(pairs: &[(&str, &str)]) -> Vec<(Value, String)> {
    pairs
        .iter()
        .map(|(value, label)| (Value::from(*value), label.to_string()))
        .collect()
}

fn named<'a>(pairs: &'a [(&str, &str)], value: &str) -> Result<&'a str> {
    match pairs.iter().find(|(known, _)| *known == value) {
        Some((_, label)) => Ok(label),
        None => bail!("'{value}'"),
    }
}

/// The displayed number is the last occurrence in the original (unfiltered) snapshot.
fn page_numbers<'a>(requests: &[Value], page: &[&'a Value]) -> HashMap<&'a str, usize> {
    let mut numbers: HashMap<_, _> = page
        .iter()
        .map(|request| (request.str("exchange_id"), 0))
        .collect();
    let mut remaining = numbers.len();
    if remaining > 0 {
        for (index, request) in requests.iter().enumerate().rev() {
            if let Some(number) = numbers.get_mut(request.str("exchange_id"))
                && *number == 0
            {
                *number = index + 1;
                remaining -= 1;
                if remaining == 0 {
                    break;
                }
            }
        }
    }
    numbers
}

pub fn render(state: &mut Usage, snapshot: &Value) -> Result<Vec<Value>> {
    let requests = snapshot.list("requests");
    let turns = report::turn_ids(snapshot);
    let models = report::model_options(requests);
    if state
        .model
        .as_ref()
        .is_some_and(|model| !models.contains(model))
    {
        state.model = None;
    }
    let scope = report::scoped(snapshot, &state.period, state.model.as_deref());
    let rows = report::filtered(&scope, &state.status, &state.query, &state.order)?;
    let pages = rows.len().div_ceil(PAGE).max(1) as i64;
    state.page = state.page.min(pages - 1);
    // The scope can only contain picker keys, and an explicit model fixes that key.
    let may_show_model_table = models.len() > 1 && state.model.is_none() && scope.len() > 1;
    let mut body = vec![
        controls(state, &turns, &models),
        summary(&scope, may_show_model_table),
    ];
    body.push(json!({"type": "input", "id": "query", "value": state.query,
                     "placeholder": "ID запроса, хода или модель", "action": "query"}));
    body.push(row(vec![
        select(
            "status",
            named(&STATUSES, &state.status)?,
            &choices(&STATUSES),
            &state.status.as_str().into(),
            "status",
        ),
        select(
            "order",
            named(&ORDERS, &state.order)?,
            &choices(&ORDERS),
            &state.order.as_str().into(),
            "order",
        ),
        note(format!("Запросы: {} из {}", rows.len(), scope.len())),
    ]));
    if rows.is_empty() {
        let text = if requests.is_empty() {
            "В этой сессии ещё нет записанных запросов."
        } else {
            "По этим фильтрам запросов нет."
        };
        body.push(label(text, "muted"));
    }
    let start = state.page.max(0) as usize * PAGE;
    let page = &rows[start..(start + PAGE).min(rows.len())];
    let numbers = page_numbers(requests, page);
    for request in page {
        let id = request.str("exchange_id");
        body.push(entry(
            request,
            numbers[id],
            turns.get_index_of(request.str("turn_id")).map(|i| i + 1),
            state.expanded.contains(id),
        ));
    }
    if pages > 1 {
        let mut pager = Vec::new();
        if state.page > 0 {
            pager.push(
                button("Назад", "page")
                    .with("value", state.page - 1)
                    .with("variant", "ghost"),
            );
        }
        pager.push(note(format!("{} / {pages}", state.page + 1)));
        if state.page + 1 < pages {
            pager.push(
                button("Дальше", "page")
                    .with("value", state.page + 1)
                    .with("variant", "ghost"),
            );
        }
        body.push(row(pager));
    }
    body.push(note(format!(
        "Оценка по стандартным ценам API ({}, {}), а не списание. Неизвестные модели не оцениваются.",
        report::PRICE_SOURCE,
        report::PRICE_DATE
    )));
    Ok(body)
}

fn period_title(number: usize) -> String {
    // The old JSON clone held only the used bytes. Do not transfer capacity
    // from format!'s growth estimate into thousands of cached labels.
    let mut title = String::with_capacity("Ход ".len() + number.ilog10() as usize + 1);
    write!(&mut title, "Ход {number}").expect("writing into a String");
    title
}

fn controls(state: &mut Usage, turns: &IndexSet<&str>, models: &[String]) -> Value {
    const PERIODS: [(&str, &str); 2] = [("all", "Весь чат"), ("latest", "Последний ход")];
    // The old reverse search preferred a matching turn over the built-in key.
    let title_for = |period: &str| {
        turns
            .get_index_of(period)
            .map(|index| period_title(index + 1))
            .or_else(|| {
                PERIODS
                    .iter()
                    .find(|(key, _)| *key == period)
                    .map(|(_, title)| (*title).to_owned())
            })
    };
    let title = title_for(&state.period).unwrap_or_else(|| {
        state.period = "all".to_owned();
        title_for("all").expect("built-in period")
    });
    let periods = PERIODS
        .into_iter()
        .map(|(key, title)| (Value::from(key), title.to_owned()))
        .chain(
            turns
                .iter()
                .enumerate()
                .rev()
                .map(|(index, turn)| (Value::from(*turn), period_title(index + 1))),
        );
    let mut children = vec![select_owned(
        "period",
        &title,
        periods,
        &state.period.as_str().into(),
        "period",
    )];
    if models.len() > 1 {
        let mut options = vec![(Value::Null, "Все модели".to_owned())];
        options.extend(
            models
                .iter()
                .map(|model| (Value::from(model.as_str()), model.clone())),
        );
        let selected = state
            .model
            .as_deref()
            .map(Value::from)
            .unwrap_or(Value::Null);
        children.push(select(
            "model",
            state.model.as_deref().unwrap_or("Все модели"),
            &options,
            &selected,
            "model",
        ));
    }
    row(children)
}

fn summary(scope: &[&Value], may_show_model_table: bool) -> Value {
    let total = report::totals(scope);
    let parts: Vec<String> = [
        (total.auxiliary, "вспомогательные"),
        (total.errors, "ошибки"),
        (total.unfinished, "без результата"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, name)| format!("{name}: {count}"))
    .collect();
    let parts = parts.join(" · ");
    let price = if total.priced > 0 {
        sized(format!("≈ {}", money(total.cost)), "accent", "large")
    } else {
        label("стоимость неизвестна", "muted")
    };
    let mut children = vec![
        row(vec![
            sized(
                format!("Токены: {}", exact(Some(total.input + total.output))),
                "default",
                "large",
            ),
            price,
        ]),
        row(vec![
            metric("Вход", exact(Some(total.input)), None),
            metric("Выход", exact(Some(total.output)), None),
            metric("Из кэша", exact(total.cached), Some("входит во вход")),
            metric("Запись кэша", exact(total.written), Some("входит во вход")),
            metric(
                "Рассуждения",
                exact(total.reasoning),
                Some("входят в выход"),
            ),
            metric(
                "Запросы",
                total.requests,
                Some(parts.as_str()).filter(|parts| !parts.is_empty()),
            ),
        ])
        .with("gap", "medium"),
    ];
    if total.reported < total.requests {
        children.push(sized(
            format!(
                "Токены известны для {} из {} запросов. Остальной расход неизвестен.",
                total.reported, total.requests
            ),
            "warning",
            "small",
        ));
    }
    if total.priced < total.reported {
        children.push(sized(
            format!(
                "Стоимость учтена для {} из {} запросов; показана известная часть.",
                total.priced, total.requests
            ),
            "warning",
            "small",
        ));
    }
    let models = may_show_model_table
        .then(|| report::by_model(scope))
        .filter(|models| models.len() > 1);
    if let Some(models) = models {
        let rows = models
            .iter()
            .map(|(model, part)| {
                let cost = if part.priced > 0 {
                    format!("≈ {}", money(part.cost))
                } else {
                    "—".to_owned()
                };
                vec![
                    model.clone(),
                    part.requests.to_string(),
                    exact(Some(part.input + part.output)),
                    cost,
                ]
            })
            .collect();
        children.push(table(&["Model", "Запросов", "Tokens", "Стоимость"], rows));
    }
    card(Some("Итог выбранного периода"), children)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_label_uses_exact_utf8_capacity_across_digit_boundaries() {
        for n in [1, 9, 10, 99, 100, 1_000, 16_384, usize::MAX] {
            let title = period_title(n);
            assert_eq!(title, format!("Ход {n}"));
            assert_eq!(title.capacity(), title.len());
        }
    }

    #[test]
    fn period_picker_keeps_reverse_turn_priority_when_a_reserved_value_collides() {
        let turns: IndexSet<_> = ["a", "all", "latest", "b"].into_iter().collect();
        let mut usage = Usage {
            period: "not present".to_owned(),
            ..Usage::default()
        };
        let picker = controls(&mut usage, &turns, &[]);
        let menu = &picker["children"][0];
        assert_eq!(usage.period, "all");
        assert_eq!(menu["label"], "Ход 2");
        let options = menu["options"].as_array().unwrap();
        let ids: Vec<_> = options.iter().map(|v| v.str("value")).collect();
        assert_eq!(ids, ["all", "latest", "b", "latest", "all", "a"]);
        assert!(options[0].bool("selected") && options[4].bool("selected"));
        assert_eq!(options.iter().filter(|v| v.bool("selected")).count(), 2);

        usage.period = "latest".to_owned();
        let picker = controls(&mut usage, &turns, &[]);
        assert_eq!(picker["children"][0]["label"], "Ход 3");
    }

    #[test]
    fn breakdown_uses_scope_and_exact_picker_key_after_reset() {
        let snapshot = json!({"requests": [
            {"exchange_id":"one", "model":{"provider":"a/b","model":"gpt-6-astra"},
             "status":"error", "usage":{"input_tokens":1000,"output_tokens":10}},
            {"exchange_id":"two", "model":{"provider":"a","model":"b/gpt-6-astra"},
             "status":"completed", "usage":{"input_tokens":1000,"output_tokens":10}},
            {"exchange_id":"three", "status":"completed", "model":42}
        ]});
        let mut usage = Usage {
            model: Some("a/b/gpt-6-astra".to_owned()),
            status: "problems".to_owned(),
            ..Usage::default()
        };
        let summary = |usage: &mut Usage| render(usage, &snapshot).unwrap().remove(1);
        let selected = summary(&mut usage);
        let has_table = |node: &Value| {
            node.list("children")
                .iter()
                .any(|v| v.str("type") == "table")
        };
        assert!(!has_table(&selected));
        let text = selected.to_string();
        assert!(text.contains("≈ $0.0105"));
        assert!(text.contains("Стоимость учтена для 1 из 2 запросов"));
        usage.model = Some("missing".to_owned());
        let mixed = summary(&mut usage);
        assert!(usage.model.is_none() && has_table(&mixed));
        let rows = mixed
            .list("children")
            .iter()
            .find(|v| v.str("type") == "table")
            .unwrap();
        assert_eq!(rows["rows"][0][0], "a/b/gpt-6-astra");
        assert_eq!(rows["rows"][0][1], "2");
        assert_eq!(rows["rows"][1][0], "/");
    }
}
