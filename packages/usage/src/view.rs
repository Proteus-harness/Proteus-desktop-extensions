//! «Запросы и расход»: period totals, per-model split and the request list.

use std::collections::HashMap;

use proteus_extension::ui::{Node, button, card, note, row, select, table};
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

pub fn render(state: &mut Usage, snapshot: &Value) -> Result<Vec<Value>> {
    let requests = snapshot.list("requests");
    let turns = report::turn_ids(snapshot);
    let mut models: Vec<String> = Vec::new();
    for request in requests {
        let model = report::model_key(request);
        if !models.contains(&model) {
            models.push(model);
        }
    }
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
    let numbers: HashMap<&str, usize> = requests
        .iter()
        .enumerate()
        .map(|(index, request)| (request.str("exchange_id"), index + 1))
        .collect();
    let mut body = vec![controls(state, &turns, &models), summary(&scope)];
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
    for request in rows.iter().skip(start).take(PAGE) {
        let id = request.str("exchange_id");
        body.push(entry(
            request,
            numbers[id],
            &turns,
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

fn controls(state: &mut Usage, turns: &[String], models: &[String]) -> Value {
    let mut periods = choices(&[("all", "Весь чат"), ("latest", "Последний ход")]);
    periods.extend(
        turns
            .iter()
            .enumerate()
            .rev()
            .map(|(index, turn)| (Value::from(turn.as_str()), format!("Ход {}", index + 1))),
    );
    let find = |period: &str| {
        periods
            .iter()
            .rev()
            .find(|(value, _)| value == period)
            .map(|(_, label)| label.clone())
    };
    let title = find(&state.period).unwrap_or_else(|| {
        state.period = "all".to_owned();
        find("all").unwrap_or_default()
    });
    let mut children = vec![select(
        "period",
        &title,
        &periods,
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

fn summary(scope: &[&Value]) -> Value {
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
                format!("{} токенов", exact(Some(total.input + total.output))),
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
    let models = report::by_model(scope);
    if models.len() > 1 {
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
        children.push(table(&["Модель", "Запросов", "Токены", "Стоимость"], rows));
    }
    card(Some("Итог выбранного периода"), children)
}
