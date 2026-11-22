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
    if !value.is_array() && !value.is_object() {
        return value.to_string();
    }

    let mut out = Vec::new();
    dump_into(&mut out, value);
    String::from_utf8(out).expect("JSON is UTF-8")
}

fn dump_into(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i != 0 {
                    out.extend_from_slice(b", ");
                }
                dump_into(out, item);
            }
            out.push(b']');
        }
        Value::Object(map) => {
            out.push(b'{');
            for (i, (key, value)) in map.iter().enumerate() {
                if i != 0 {
                    out.extend_from_slice(b", ");
                }
                serde_json::to_writer(&mut *out, key).expect("serializing a string to Vec");
                out.extend_from_slice(b": ");
                dump_into(out, value);
            }
            out.push(b'}');
        }
        other => serde_json::to_writer(out, other).expect("serializing a JSON scalar to Vec"),
    }
}

#[cfg(test)]
#[test]
fn dumps_preserves_order_and_spaces_outside_escaped_strings() {
    let value: Value = serde_json::from_str(
        r#"{"z":[-0.0,{"🧪\t":"a:b,[]{}\"\\\n\u0000"}],"":{},"é":[true,null,[]]}"#,
    )
    .unwrap();
    assert_eq!(
        dumps(&value),
        r#"{"z": [-0.0, {"🧪\t": "a:b,[]{}\"\\\n\u0000"}], "": {}, "é": [true, null, []]}"#
    );
}

pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

pub fn short_id(value: &Value) -> String {
    plain(value).chars().take(8).collect()
}

pub fn preview(text: &str, limit: usize) -> String {
    let mut out = String::new();
    let mut remaining = limit;
    for ch in text.chars() {
        if remaining == 0 {
            break;
        }
        if ch.is_whitespace() {
            if out.is_empty() || out.ends_with(' ') {
                continue;
            }
            out.push(' ');
        } else {
            out.push(ch);
        }
        remaining -= 1;
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
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
    let parts = message.list("parts");
    if let [part] = parts
        && let Some(payload) = part.at("payload").as_object()
        && payload.len() == 1
        && let Some(text) = payload.get("Text").and_then(|body| body.opt_str("text"))
    {
        let text = text.trim();
        return if text.is_empty() {
            pretty(message)
        } else {
            text.to_owned()
        };
    }
    let parts: Vec<String> = parts
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

#[cfg(test)]
#[test]
fn single_text_part_preserves_trim_and_empty_fallback() {
    let mut message =
        serde_json::json!({"parts": [{"payload": {"Text": {"text": "\u{2003}🧪 e\u{0301}\r\n"}}}]});
    assert_eq!(message_text(&message), "🧪 e\u{0301}");
    message["parts"][0]["payload"]["Text"]["text"] = serde_json::json!("\u{2003}\t");
    assert_eq!(message_text(&message), pretty(&message));
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
