//! Usage calculations moved out of the client; the pinned tariff table is kept.

use std::borrow::Cow;
use std::cmp::Reverse;

use indexmap::{IndexMap, IndexSet};
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

fn model_parts(request: &Value) -> (&str, &str) {
    let model = request.at("model");
    (model.str("provider"), model.str("model"))
}

pub fn model_key(request: &Value) -> String {
    let (provider, name) = model_parts(request);
    format!("{provider}/{name}")
}

/// Compare the display key exactly, including aliases that contain slashes.
fn same_model_key(key: &str, (provider, name): (&str, &str)) -> bool {
    key.strip_prefix(provider)
        .and_then(|rest| rest.strip_prefix('/'))
        == Some(name)
}

/// Model picker labels in first-seen order. Only new labels need allocation.
pub fn model_options(requests: &[Value]) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for request in requests {
        let parts = model_parts(request);
        if !labels.iter().any(|key| same_model_key(key, parts)) {
            labels.push(model_key(request));
        }
    }
    labels
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

/// The position of each non-empty id is its first-seen turn number minus one.
pub fn turn_ids(snapshot: &Value) -> IndexSet<&str> {
    let mut turns = IndexSet::new();
    for request in snapshot.list("requests") {
        let id = request.str("turn_id");
        if !id.is_empty() {
            turns.insert(id);
        }
    }
    turns
}

pub fn scoped<'a>(snapshot: &'a Value, period: &str, model: Option<&str>) -> Vec<&'a Value> {
    let turn = match period {
        "all" => None,
        "latest" => match snapshot.opt_str("latest_turn_id") {
            Some(id) if !id.is_empty() => Some(id),
            _ => return Vec::new(),
        },
        turn => Some(turn),
    };
    snapshot
        .list("requests")
        .iter()
        .filter(|request| turn.is_none_or(|id| request.opt_str("turn_id") == Some(id)))
        .filter(|request| model.is_none_or(|model| same_model_key(model, model_parts(request))))
        .collect()
}

/// Every whitespace-separated word occurs in the ids or the model.
fn matches(request: &Value, mut terms: std::str::SplitWhitespace<'_>) -> bool {
    let model = request.at("model");
    let fields = [
        request.str("exchange_id"),
        request.str("turn_id"),
        model.str("provider"),
        model.str("model"),
    ];
    if fields.iter().all(|field| {
        field
            .bytes()
            .all(|byte| byte.is_ascii() && !byte.is_ascii_uppercase())
    }) {
        // Search terms are split on whitespace and cannot span a joining space.
        // Already-lowercase ASCII fields need neither a join nor Unicode casing.
        return terms.all(|term| fields.iter().any(|field| field.contains(term)));
    }
    let haystack = fields.join(" ").to_lowercase();
    terms.all(|term| haystack.contains(term))
}

pub fn filtered<'s, 'a>(
    requests: &'s [&'a Value],
    status: &str,
    query: &str,
    order: &str,
) -> Result<Cow<'s, [&'a Value]>> {
    let blank_query = query.trim().is_empty();
    if order == "oldest" && status == "all" && blank_query {
        // The default view only reads the already ordered scope.
        return Ok(Cow::Borrowed(requests));
    }
    let eligible = requests
        .iter()
        .copied()
        .filter(|request| status != "problems" || request.str("status") != "completed")
        .filter(|request| status != "missing" || request.at("usage").is_null());
    // The unfiltered page does not need to assemble any search text.
    let rows: Vec<&Value> = if blank_query {
        eligible.collect()
    } else {
        let mut lowered = None;
        eligible
            .filter(|request| {
                let query = lowered.get_or_insert_with(|| query.to_lowercase());
                matches(request, query.split_whitespace())
            })
            .collect()
    };
    let key: fn(&Value) -> Option<f64> = match order {
        "oldest" => return Ok(Cow::Owned(rows)),
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
    Ok(Cow::Owned(known))
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

impl Totals {
    /// Accumulate in request order, including the original sequence of floating-point additions.
    fn add(&mut self, request: &Value) {
        self.requests += 1;
        self.auxiliary += usize::from(request.str("origin") == "auxiliary");
        match request.str("status") {
            "unfinished" => self.unfinished += 1,
            "error" | "canceled" | "timeout" => self.errors += 1,
            _ => {}
        }
        let usage = request.at("usage");
        if !usage.is_null() {
            self.reported += 1;
            self.input += usage.i64("input_tokens").unwrap_or(0);
            self.output += usage.i64("output_tokens").unwrap_or(0);
            let add_optional = |total: &mut Option<i64>, field: &str| {
                if !usage.at(field).is_null() {
                    *total.get_or_insert(0) += usage.i64(field).unwrap_or(0);
                }
            };
            add_optional(&mut self.cached, "cached_input_tokens");
            add_optional(&mut self.written, "cache_creation_input_tokens");
            add_optional(&mut self.reasoning, "reasoning_output_tokens");
        }
        if let Some(price) = cost(request) {
            self.priced += 1;
            self.cost += price;
        }
    }
}

pub fn totals(requests: &[&Value]) -> Totals {
    let mut total = Totals::default();
    for &request in requests {
        total.add(request);
    }
    total
}

/// Totals per model, largest token use first.
pub fn by_model(requests: &[&Value]) -> Vec<(String, Totals)> {
    // Insertion order is the tie-breaker when two models use the same number of tokens.
    let mut parts: IndexMap<String, Totals> = IndexMap::new();
    for &request in requests {
        parts.entry(model_key(request)).or_default().add(request);
    }
    let mut groups: Vec<_> = parts.into_iter().collect();
    groups.sort_by_key(|(_, part)| Reverse(part.input + part.output));
    groups
}
