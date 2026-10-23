//! One request card of the list and its expanded details.

use proteus_extension::ui::{Node, button, card, kv, note, row, table};
use proteus_extension::{Value, ValueExt};

use crate::report::{self, exact, money};
use crate::widgets::{badge, label, plain, sized};

const FIELDS: [&str; 5] = [
    "input_tokens",
    "cached_input_tokens",
    "cache_creation_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
];

pub fn entry(request: &Value, number: usize, turns: &[String], open: bool) -> Value {
    let usage = request.at("usage");
    let reported = usage.as_object().is_some_and(|usage| !usage.is_empty());
    let status = request.str("status");
    let (status, tone) = report::status(status).unwrap_or((status, "muted"));
    let turn = request.str("turn_id");
    let place = match turns.iter().position(|known| known == turn) {
        Some(index) if !turn.is_empty() => format!("Ход {}", index + 1),
        _ => "Вне хода".to_owned(),
    };
    let marker = if open { "▾ " } else { "▸ " };
    let mut head = vec![
        button(
            &format!("{marker}#{number} · {}", request.at("model").str("model")),
            "request",
        )
        .with("value", request.at("exchange_id").clone())
        .with("variant", "ghost")
        .maybe("selected", open.then_some(true))
        .with(
            "hint",
            if open {
                "Свернуть"
            } else {
                "Подробнее"
            },
        ),
        badge(status, tone),
    ];
    if request.str("origin") == "auxiliary" {
        head.push(badge("Вспомогательный", "accent"));
    }
    head.push(note(format!(
        "{place} · {} · {}",
        report::seconds(request),
        report::clock(request.f64("started_at_ms"))
    )));
    head.push(label(
        if reported {
            format!("{} токенов", exact(report::used(request)))
        } else {
            "токены неизвестны".to_owned()
        },
        "default",
    ));
    head.push(note(match report::cost(request) {
        Some(price) => format!("≈ {}", money(price)),
        None => "—".to_owned(),
    }));
    let cells = FIELDS.iter().map(|field| exact(usage.i64(field))).collect();
    let mut children = vec![
        row(head),
        table(&["Вход", "Кэш", "Запись", "Выход", "Мысли"], vec![cells]),
    ];
    if open {
        children.extend(details(request));
    }
    card(None, children)
}

fn details(request: &Value) -> Vec<Value> {
    let auxiliary = request.str("origin") == "auxiliary";
    let mut rows = vec![
        ("Provider", request.at("model").str("provider").to_owned()),
        (
            "Тип",
            if auxiliary {
                "Вспомогательный запрос"
            } else {
                "Запрос модели"
            }
            .to_owned(),
        ),
        ("Начало", report::clock(request.f64("started_at_ms"))),
        ("Длительность", report::seconds(request)),
        ("Сообщений на входе", plain(request.at("message_count"))),
        ("Доступных инструментов", plain(request.at("tool_count"))),
    ];
    let effort = request.at("reasoning_effort");
    if effort.truthy() {
        rows.push(("Уровень рассуждения", plain(effort)));
    }
    if !request.at("max_output_tokens").is_null() {
        rows.push(("Лимит выхода", exact(request.i64("max_output_tokens"))));
    }
    let reason = request.at("finish_reason");
    if !reason.is_null() {
        let reason = plain(reason);
        rows.push((
            "Причина завершения",
            report::finish(&reason).map(str::to_owned).unwrap_or(reason),
        ));
    }
    match report::price(request) {
        Err(reason) => rows.push(("Стоимость", reason.to_owned())),
        Ok(price) => {
            rows.push(("Стоимость входа", money(price.input)));
            rows.push(("Стоимость выхода", money(price.output)));
            if price.long {
                rows.push(("Тариф", "Применён тариф длинного контекста".to_owned()));
            }
        }
    }
    rows.push(("ID запроса", plain(request.at("exchange_id"))));
    let turn = request.at("turn_id");
    if turn.truthy() {
        rows.push(("ID хода", plain(turn)));
    }
    let mut result = vec![kv(rows)];
    if request.at("usage").is_null() {
        result.push(sized(
            "Провайдер не вернул данные о токенах. Нулевой расход не предполагается.",
            "warning",
            "small",
        ));
    }
    result
}
