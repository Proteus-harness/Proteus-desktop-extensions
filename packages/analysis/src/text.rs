//! Readable texts of messages, responses and decisions.

use proteus_extension::{Value, ValueExt};

use crate::facts::number;

/// A value as Python's `str` shows it; containers as compact JSON.
pub fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        other => dumps(other),
    }
}

/// `field` when present, else `default`, as Python's `dict.get(field, default)`.
pub fn get(value: &Value, field: &str, default: &str) -> String {
    value
        .get(field)
        .map(plain)
        .unwrap_or_else(|| default.to_owned())
}

/// Compact JSON with Python's `", "` and `": "` separators.
pub fn dumps(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(dumps).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(map) => {
            let fields: Vec<String> = map
                .iter()
                .map(|(key, value)| format!("{}: {}", Value::from(key.as_str()), dumps(value)))
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
        other => other.to_string(),
    }
}

pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

pub fn short_id(value: &Value) -> String {
    plain(value).chars().take(8).collect()
}

pub fn preview(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars()
        .take(limit)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Name of an externally tagged serde enum: `"other"` or `{"retryable": {...}}`.
pub fn tagged(value: &Value) -> String {
    match value.as_object() {
        Some(map) if map.len() == 1 => map.keys().next().cloned().unwrap_or_default(),
        _ => plain(value),
    }
}

fn part_text(part: &Value) -> String {
    let (name, body) = match part.as_object() {
        Some(map) if map.len() == 1 => map
            .iter()
            .next()
            .map(|(name, body)| (name.as_str(), body))
            .unwrap_or(("", part)),
        _ => ("", part),
    };
    let body = if body.is_object() { body } else { &Value::Null };
    match name {
        "Text" => body.str("text").trim().to_owned(),
        "Reasoning" | "ReasoningSummary" => String::new(),
        "ToolCall" => {
            let call = body.at("call");
            format!(
                "Вызов {}: {}",
                get(call, "name", "?"),
                dumps(call.at("args"))
            )
        }
        "ToolResult" => {
            let result = body.at("result");
            let output = [result.at("output"), result.at("error")]
                .into_iter()
                .find(|value| value.truthy());
            let output = output.map(plain).unwrap_or_default();
            format!("Результат {}:\n{output}", get(result, "call_id", ""))
                .trim_end()
                .to_owned()
        }
        "FileRef" => {
            let content = body.at("content");
            let content = if content.truthy() {
                format!("\n{}", plain(content))
            } else {
                String::new()
            };
            format!("Файл {}{content}", get(body, "path", ""))
        }
        "Image" => "[изображение]".to_owned(),
        _ => pretty(part),
    }
}

/// Presentation text of a canonical message without reasoning; raw JSON when it has none.
pub fn message_text(message: &Value) -> String {
    let parts: Vec<String> = message
        .list("parts")
        .iter()
        .map(|part| part_text(part.at("payload")))
        .filter(|text| !text.is_empty())
        .collect();
    if parts.is_empty() {
        pretty(message)
    } else {
        parts.join("\n")
    }
}

pub fn role(message: &Value) -> String {
    let role = message.at("role");
    let name = match role.as_str() {
        Some("System") => "Система",
        Some("Developer") => "Разработчик",
        Some("User") => "Пользователь",
        Some("Assistant") => "Ассистент",
        Some("Tool") => "Инструмент",
        _ => return plain(role),
    };
    name.to_owned()
}

pub fn finish_text(response: &Value) -> String {
    let reason = response.at("finish_reason");
    let name = match reason.as_str() {
        Some("Stop") => "остановка".to_owned(),
        Some("ToolCalls") => "вызов инструментов".to_owned(),
        Some("Length") => "лимит длины".to_owned(),
        Some("ContentFilter") => "фильтр содержимого".to_owned(),
        Some("Error") => "ошибка".to_owned(),
        Some("Unknown") => "неизвестно".to_owned(),
        _ => plain(reason),
    };
    let end = match response.at("end_turn").as_bool() {
        Some(true) => "да",
        Some(false) => "нет",
        None => "не указан",
    };
    format!("{name} · конец хода: {end}")
}

pub fn tokens_text(usage: &Value) -> String {
    let mut parts = vec![
        format!("вход {}", number(usage.at("input_tokens"))),
        format!("выход {}", number(usage.at("output_tokens"))),
    ];
    for (key, name) in [
        ("cached_input_tokens", "из кэша"),
        ("cache_creation_input_tokens", "запись в кэш"),
        ("reasoning_output_tokens", "рассуждения"),
    ] {
        if !usage.at(key).is_null() {
            parts.push(format!("{name} {}", number(usage.at(key))));
        }
    }
    parts.join(" · ")
}

pub fn compaction_tokens(report: &Value) -> String {
    let (before, after) = (
        report.at("original_token_estimate"),
        report.at("output_token_estimate"),
    );
    match (before.is_null(), after.is_null()) {
        (false, false) => format!("{} → {}", number(before), number(after)),
        (false, true) => number(before),
        _ => "нет данных".to_owned(),
    }
}

pub fn resolution_text(resolution: &Value) -> String {
    let decision = get(resolution, "decision", "");
    let text = match decision.as_str() {
        "allowed" => "Разрешён",
        "approved" => "Подтверждён пользователем",
        "approval_denied" => "Отклонён пользователем",
        "policy_denied" => "Запрещён политикой",
        "hook_blocked" => "Заблокирован хуком",
        "validation_failed" => "Аргументы не прошли проверку",
        "unsupported" => "Не поддерживается",
        other => other,
    };
    let reason = resolution.at("reason");
    if reason.truthy() {
        format!("{text}: {}", plain(reason))
    } else {
        text.to_owned()
    }
}
