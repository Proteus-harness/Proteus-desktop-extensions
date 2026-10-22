//! Presentation facts of the /analysis contract: titles, statuses, metrics, formatting.

use proteus_extension::{Value, ValueExt, exact};

pub const ERROR: &str = "Ошибка";
pub const MISSING: &str = "Нет результата";
pub const UNCHANGED: &str = "Без изменений";
pub const DONE: &str = "Готово";

/// Step kinds in display order with their names.
pub const KINDS: [(&str, &str); 4] = [
    ("model", "Модель"),
    ("tool", "Инструмент"),
    ("compaction", "Контекст"),
    ("hook", "Хуки"),
];

pub fn kind_name(kind: &str) -> &str {
    KINDS
        .iter()
        .find(|(known, _)| *known == kind)
        .map_or(kind, |(_, name)| name)
}

pub fn turn_status(status: &str) -> Option<&'static str> {
    Some(match status {
        "unsettled" => "Нет завершения",
        "success" => "Завершён",
        "error" => "Ошибка",
        "canceled" => "Отменён",
        "timeout" => "Таймаут",
        _ => return None,
    })
}

fn hook_name(event: &str) -> Option<&'static str> {
    Some(match event {
        "turn_started" => "Начало хода",
        "before_tool_selection" => "Отбор инструментов",
        "before_context" => "Подготовка контекста",
        "before_model" => "Перед запросом модели",
        "before_tool" => "Перед инструментом",
        "after_tool" => "После инструмента",
        "before_stop" => "Проверка завершения",
        "turn_settled" => "Завершение хода",
        _ => return None,
    })
}

pub fn turn_tone(status: Option<&str>) -> &'static str {
    match status {
        Some("success") => "success",
        Some("unsettled") => "warning",
        _ => "danger",
    }
}

pub fn kind(step: &Value) -> &str {
    step.at("data").str("kind")
}

pub fn title(step: &Value) -> String {
    let data = step.at("data");
    match kind(step) {
        "model" => {
            let origin = if data.str("origin") == "auxiliary" {
                "вспомогательный"
            } else {
                "основной"
            };
            format!(
                "{} · {origin} запрос",
                data.at("request").at("model").str("model")
            )
        }
        "tool" => data.at("call").str("name").to_owned(),
        "compaction" => "Сжатие истории".to_owned(),
        _ => {
            let event = data.at("trace").at("input").at("event").str("event");
            hook_name(event).unwrap_or(event).to_owned()
        }
    }
}

pub fn has_error(step: &Value) -> bool {
    let data = step.at("data");
    match kind(step) {
        "model" => {
            data.at("failure").truthy() || data.at("response").str("finish_reason") == "Error"
        }
        "tool" => !data.at("result").is_null() && !data.at("result").at("ok").truthy(),
        "hook" => data
            .at("trace")
            .list("steps")
            .iter()
            .any(|item| item.at("outcome").str("status") == "failed"),
        _ => false,
    }
}

pub fn status(step: &Value) -> &'static str {
    let data = step.at("data");
    if has_error(step) {
        return ERROR;
    }
    match kind(step) {
        "model" if data.at("response").is_null() => MISSING,
        "tool" if data.at("result").is_null() => MISSING,
        "compaction" if !data.at("report").at("changed").truthy() => UNCHANGED,
        _ => DONE,
    }
}

pub fn tone(state: &str) -> &'static str {
    match state {
        ERROR => "danger",
        MISSING => "warning",
        _ => "muted",
    }
}

pub fn problem(step: &Value) -> bool {
    matches!(status(step), ERROR | MISSING)
}

pub fn matches(step: &Value, kind_filter: &str, problems: bool, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    (kind_filter == "all" || kind(step) == kind_filter)
        && (!problems || problem(step))
        && (query.is_empty() || title(step).to_lowercase().contains(&query))
}

/// The step shown after loading a turn: the first failure, else the first step.
pub fn default_step(steps: &[Value]) -> Option<i64> {
    steps
        .iter()
        .find(|step| has_error(step))
        .or(steps.first())
        .and_then(|step| step.i64("sequence"))
}

#[derive(Debug, Default, PartialEq)]
pub struct Metrics {
    pub models: usize,
    pub tools: usize,
    pub errors: usize,
    pub missing: usize,
    pub with_usage: usize,
    pub input: i64,
    pub output: i64,
}

pub fn metrics(steps: &[Value]) -> Metrics {
    let mut result = Metrics::default();
    for step in steps {
        let data = step.at("data");
        result.errors += has_error(step) as usize;
        match kind(step) {
            "model" => {
                result.models += 1;
                result.missing +=
                    (data.at("response").is_null() && !data.at("failure").truthy()) as usize;
                let usage = data.at("response").at("usage");
                if usage.truthy() {
                    result.with_usage += 1;
                    result.input += usage.i64("input_tokens").unwrap_or(0);
                    result.output += usage.i64("output_tokens").unwrap_or(0);
                }
            }
            "tool" => {
                result.tools += 1;
                result.missing += data.at("result").is_null() as usize;
            }
            _ => {}
        }
    }
    result
}

pub fn usage_text(summary: &Metrics) -> String {
    if summary.with_usage == 0 {
        return "нет данных".to_owned();
    }
    format!(
        "{} / {}",
        exact(Some(summary.input)),
        exact(Some(summary.output))
    )
}

/// Groups thousands with a narrow space: `12 345`.
pub fn number(value: &Value) -> String {
    match value {
        Value::Null => "—".to_owned(),
        value => value
            .as_i64()
            .map(|value| exact(Some(value)))
            .unwrap_or_else(|| value.to_string()),
    }
}

pub fn duration(start: &Value, end: &Value) -> String {
    match (start.as_f64(), end.as_f64()) {
        (Some(start), Some(end)) => format!("{:.2} с", (end - start).max(0.) / 1000.),
        _ => "—".to_owned(),
    }
}

pub fn turn_duration(steps: &[Value]) -> String {
    match (steps.first(), steps.last()) {
        (Some(first), Some(last)) => duration(first.at("started_at_ms"), last.at("finished_at_ms")),
        _ => "—".to_owned(),
    }
}

/// `дд.мм ЧЧ:ММ:СС` in local time for Unix milliseconds; `—` when unknown or out of range.
pub fn clock(ms: &Value) -> String {
    let time = ms
        .as_f64()
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
