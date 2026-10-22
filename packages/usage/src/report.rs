//! Usage calculations moved out of the client; the pinned tariff table is kept.

use std::cmp::Reverse;

use proteus_extension::{Result, Value, ValueExt, bail};

pub use proteus_extension::exact;

const RATES: [(&str, [f64; 4]); 5] = [
    ("gpt-6-astra", [10., 1., 12.5, 50.]),
    ("gpt-5.6-sol", [4., 0.4, 5., 20.]),
    ("gpt-5.6", [4., 0.4, 5., 20.]),
    ("gpt-5.6-terra", [2., 0.2, 2.5, 12.]),
    ("gpt-5.6-luna", [0.2, 0.02, 0.25, 1.2]),
];
pub const PRICE_DATE: &str = "2026-09-10";
pub const PRICE_SOURCE: &str = "https://developers.openai.com/api/docs/pricing";

/// Label and tone of a request status.
pub fn status(status: &str) -> Option<(&'static str, &'static str)> {
    Some(match status {
        "completed" => ("Готово", "success"),
        "error" => ("Ошибка", "danger"),
        "canceled" => ("Отменён", "danger"),
        "timeout" => ("Таймаут", "danger"),
        "unfinished" => ("Нет результата", "warning"),
        _ => return None,
    })
}

pub fn finish(reason: &str) -> Option<&'static str> {
    Some(match reason {
        "Stop" => "Остановка (Stop)",
        "ToolCalls" => "Вызов инструментов (ToolCalls)",
        "Length" => "Лимит длины (Length)",
        "ContentFilter" => "Фильтр контента (ContentFilter)",
        "Error" => "Ошибка (Error)",
        "Unknown" => "Неизвестно (Unknown)",
        _ => return None,
    })
}

/// Price in USD of a request's input and output; `long` marks the long-context tariff.
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub long: bool,
}

/// The price, or the reason the request is not priced.
pub fn price(request: &Value) -> std::result::Result<Price, &'static str> {
    let usage = request.at("usage");
    if usage.is_null() {
        return Err("Нет данных о токенах");
    }
    let model = request.at("model").str("model");
    let Some((_, rate)) = RATES.iter().find(|(name, _)| *name == model) else {
        return Err("Тариф не задан");
    };
    let input = usage.i64("input_tokens").unwrap_or(0);
    let cached = usage.i64("cached_input_tokens").unwrap_or(0);
    let written = usage.i64("cache_creation_input_tokens").unwrap_or(0);
    if cached + written > input {
        return Err("Некорректная разбивка токенов");
    }
    let fresh = input - cached - written;
    let long = input > 272000;
    let (input_factor, output_factor) = if long { (2., 1.5) } else { (1., 1.) };
    let input = (fresh as f64 * rate[0] + cached as f64 * rate[1] + written as f64 * rate[2])
        * input_factor
        / 1e6;
    let output = usage.i64("output_tokens").unwrap_or(0) as f64 * rate[3] * output_factor / 1e6;
    Ok(Price {
        input,
        output,
        long,
    })
}

pub fn cost(request: &Value) -> Option<f64> {
    price(request).ok().map(|price| price.input + price.output)
}

/// US dollars with two to six fraction digits.
pub fn money(value: f64) -> String {
    let mut text = format!("{value:.6}");
    let point = text.find('.').unwrap_or(text.len());
    while text.ends_with('0') && text.len() > point + 3 {
        text.pop();
    }
    format!("${text}")
}

/// `дд.мм ЧЧ:ММ:СС` in local time for Unix milliseconds; `—` when unknown or out of range.
pub fn clock(ms: Option<f64>) -> String {
    let time = ms
        .map(|ms| (ms / 1000.).floor())
        .filter(|seconds| seconds.abs() < 1e15)
        .map(proteus_extension::local)
        .filter(|time| (1..=9999).contains(&time.year));
    match time {
        Some(time) => format!(
            "{:02}.{:02} {:02}:{:02}:{:02}",
            time.day, time.month, time.hour, time.minute, time.second
        ),
        None => "—".to_owned(),
    }
}

pub fn model_key(request: &Value) -> String {
    let model = request.at("model");
    format!("{}/{}", model.str("provider"), model.str("model"))
}

pub fn duration(request: &Value) -> i64 {
    let start = request.i64("started_at_ms").unwrap_or(0);
    request
        .i64("finished_at_ms")
        .map(|end| (end - start).max(0))
        .unwrap_or(0)
}

pub fn seconds(request: &Value) -> String {
    if request.at("finished_at_ms").is_null() {
        return "—".to_owned();
    }
    format!("{:.1} с", duration(request) as f64 / 1000.).replace('.', ",")
}

/// Input plus output tokens; `None` when the provider reported nothing.
pub fn used(request: &Value) -> Option<i64> {
    let usage = request.at("usage");
    let reported = usage.as_object().is_some_and(|usage| !usage.is_empty());
    reported
        .then(|| usage.i64("input_tokens").unwrap_or(0) + usage.i64("output_tokens").unwrap_or(0))
}

pub fn turn_ids(snapshot: &Value) -> Vec<String> {
    let mut turns: Vec<String> = Vec::new();
    for request in snapshot.list("requests") {
        let turn = request.str("turn_id");
        if !turn.is_empty() && !turns.iter().any(|known| known == turn) {
            turns.push(turn.to_owned());
        }
    }
    turns
}

pub fn scoped<'a>(snapshot: &'a Value, period: &str, model: Option<&str>) -> Vec<&'a Value> {
    let turn = match period {
        "all" => None,
        "latest" => Some(snapshot.at("latest_turn_id").clone()),
        turn => Some(Value::from(turn)),
    };
    snapshot
        .list("requests")
        .iter()
        .filter(|request| {
            turn.as_ref()
                .is_none_or(|turn| request.at("turn_id") == turn)
        })
        .filter(|request| model.is_none_or(|model| model_key(request) == model))
        .collect()
}

/// Every whitespace-separated word occurs in the ids or the model.
pub fn matches(request: &Value, query: &str) -> bool {
    let model = request.at("model");
    let haystack = [
        request.str("exchange_id"),
        request.str("turn_id"),
        model.str("provider"),
        model.str("model"),
    ]
    .join(" ")
    .to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|term| haystack.contains(term))
}

pub fn filtered<'a>(
    requests: &[&'a Value],
    status: &str,
    query: &str,
    order: &str,
) -> Result<Vec<&'a Value>> {
    let rows: Vec<&Value> = requests
        .iter()
        .copied()
        .filter(|request| status != "problems" || request.str("status") != "completed")
        .filter(|request| status != "missing" || request.at("usage").is_null())
        .filter(|request| matches(request, query))
        .collect();
    let key: fn(&Value) -> Option<f64> = match order {
        "oldest" => return Ok(rows),
        "newest" => |request| request.f64("started_at_ms"),
        "longest" => |request| {
            request
                .at("finished_at_ms")
                .as_f64()
                .map(|_| duration(request) as f64)
        },
        "tokens" => |request| used(request).map(|tokens| tokens as f64),
        "cost" => cost,
        other => bail!("'{other}'"),
    };
    let (mut known, unknown): (Vec<&Value>, Vec<&Value>) =
        rows.into_iter().partition(|request| key(request).is_some());
    known.sort_by(|a, b| key(b).unwrap_or(0.).total_cmp(&key(a).unwrap_or(0.)));
    known.extend(unknown);
    Ok(known)
}

#[derive(Debug, Default)]
pub struct Totals {
    pub input: i64,
    pub output: i64,
    pub cached: Option<i64>,
    pub written: Option<i64>,
    pub reasoning: Option<i64>,
    pub requests: usize,
    pub reported: usize,
    pub cost: f64,
    pub priced: usize,
    pub auxiliary: usize,
    pub unfinished: usize,
    pub errors: usize,
}

pub fn totals(requests: &[&Value]) -> Totals {
    let usages: Vec<&Value> = requests
        .iter()
        .map(|request| request.at("usage"))
        .filter(|usage| !usage.is_null())
        .collect();
    let prices: Vec<f64> = requests
        .iter()
        .filter_map(|request| cost(request))
        .collect();
    let sum = |field: &str| {
        usages
            .iter()
            .map(|usage| usage.i64(field).unwrap_or(0))
            .sum::<i64>()
    };
    let optional = |field: &str| {
        usages
            .iter()
            .any(|usage| !usage.at(field).is_null())
            .then(|| sum(field))
    };
    let count = |test: fn(&str) -> bool| {
        requests
            .iter()
            .filter(|request| test(request.str("status")))
            .count()
    };
    Totals {
        input: sum("input_tokens"),
        output: sum("output_tokens"),
        cached: optional("cached_input_tokens"),
        written: optional("cache_creation_input_tokens"),
        reasoning: optional("reasoning_output_tokens"),
        requests: requests.len(),
        reported: usages.len(),
        cost: prices.iter().fold(0., |total, price| total + price),
        priced: prices.len(),
        auxiliary: requests
            .iter()
            .filter(|request| request.str("origin") == "auxiliary")
            .count(),
        unfinished: count(|status| status == "unfinished"),
        errors: count(|status| matches!(status, "error" | "canceled" | "timeout")),
    }
}

/// Totals per model, largest token use first.
pub fn by_model(requests: &[&Value]) -> Vec<(String, Totals)> {
    let mut models: Vec<String> = Vec::new();
    for request in requests {
        let model = model_key(request);
        if !models.contains(&model) {
            models.push(model);
        }
    }
    let mut groups: Vec<(String, Totals)> = models
        .into_iter()
        .map(|model| {
            let part: Vec<&Value> = requests
                .iter()
                .copied()
                .filter(|request| model_key(request) == model)
                .collect();
            (model, totals(&part))
        })
        .collect();
    groups.sort_by_key(|(_, part)| Reverse(part.input + part.output));
    groups
}
