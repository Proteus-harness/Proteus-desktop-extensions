//! Context window snapshot (`GET /context`): map segments, provider cache and labels.

use proteus_extension::{Value, ValueExt, json, tokens};

pub struct Window<'a> {
    pub used: f64,
    pub maximum: Option<f64>,
    pub usage: &'a Value,
}

pub fn window(snapshot: &Value) -> Window<'_> {
    let usage = snapshot.at("latest_usage");
    let actual = usage.at("actual");
    let used = if actual.is_object() {
        actual.f64("input_tokens").unwrap_or(0.)
    } else {
        usage
            .f64("estimated_input_tokens")
            .or_else(|| snapshot.at("history").f64("estimated_tokens"))
            .unwrap_or(0.)
    };
    let maximum = usage
        .f64("max_input_tokens")
        .filter(|maximum| *maximum > 0.);
    Window {
        used,
        maximum,
        usage,
    }
}

pub struct Cache {
    pub status: &'static str,
    pub input: Option<i64>,
    pub cached: Option<i64>,
    pub written: Option<i64>,
    pub hit: Option<i64>,
}

/// Status line and tone of each cache status.
pub fn cache_label(status: &str) -> (&'static str, &'static str) {
    match status {
        "cold" => ("кэш не читался", "muted"),
        "warming" => ("кэш прогревается", "warning"),
        "hot" => ("кэш активно читается", "success"),
        _ => ("нет данных провайдера", "muted"),
    }
}

/// Provider input cache of the latest request.
pub fn cache(usage: &Value) -> Cache {
    let actual = usage.at("actual");
    let input = actual.i64("input_tokens");
    let (cached, written) = (
        actual.i64("cached_input_tokens"),
        actual.i64("cache_creation_input_tokens"),
    );
    let hit = match (input, cached) {
        (Some(input), Some(cached)) if input > 0 => Some(
            (cached as f64 / input as f64 * 100. + 0.5)
                .floor()
                .min(100.) as i64,
        ),
        _ => None,
    };
    let status = match input {
        None | Some(0) => "n/a",
        _ if cached.unwrap_or(0) == 0 && written.unwrap_or(0) == 0 => "cold",
        _ if hit.is_some_and(|hit| hit >= 50) => "hot",
        _ => "warming",
    };
    Cache {
        status,
        input,
        cached,
        written,
        hit,
    }
}

pub fn source_label(usage: &Value) -> String {
    match usage.opt_str("source") {
        None if usage.is_null() => "оценка по истории".to_owned(),
        Some("mixed") => "данные провайдера + локальная оценка".to_owned(),
        Some("provider") => "данные провайдера".to_owned(),
        Some("estimated") => "локальная оценка".to_owned(),
        Some(other) => other.to_owned(),
        None => "оценка по истории".to_owned(),
    }
}

pub fn usage_title(usage: &Value) -> String {
    if usage.is_null() {
        return "Оценка по истории".to_owned();
    }
    let phase = usage.opt_str("phase").unwrap_or("request");
    format!(
        "{}/{} · {phase}",
        usage.str("model_provider"),
        usage.str("model_name")
    )
}

const CELLS: usize = 200;
const PALETTE: [&str; 6] = [
    "#6b9eff", "#5cc784", "#d8a21e", "#c08cf0", "#5cc0d8", "#ef8fb7",
];
const FREE: &str = "#2e2e2e";
const RESERVE: &str = "#54383d";

fn category(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "instructions" => ("инструкции", "#6b9eff"),
        "messages" => ("история сообщений", "#5cc784"),
        "context" => ("эфемерный контекст", "#c08cf0"),
        "tool_calls" => ("вызовы инструментов", "#e0975c"),
        "tool_results" => ("результаты инструментов", "#d8a21e"),
        "tool_schemas" => ("схемы инструментов", "#5cc0d8"),
        "files" => ("файлы", "#7fd4b2"),
        "patches" => ("патчи", "#ef8fb7"),
        _ => return None,
    })
}

/// Largest remainder; a nonzero value keeps at least one cell.
pub fn allocate(values: &[f64], total: usize) -> Vec<usize> {
    let whole: f64 = values.iter().sum();
    if whole <= 0. {
        return vec![0; values.len()];
    }
    let exact: Vec<f64> = values
        .iter()
        .map(|value| value / whole * total as f64)
        .collect();
    let mut counts: Vec<usize> = exact.iter().map(|value| *value as usize).collect();
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|a, b| {
        (exact[*b] - counts[*b] as f64).total_cmp(&(exact[*a] - counts[*a] as f64))
    });
    let missing = total.saturating_sub(counts.iter().sum());
    for index in order.into_iter().take(missing) {
        counts[index] += 1;
    }
    for index in 0..values.len() {
        if values[index] > 0. && counts[index] == 0 {
            let donor = (0..counts.len())
                .max_by_key(|donor| (counts[*donor], usize::MAX - donor))
                .unwrap_or(0);
            if counts[donor] > 1 {
                counts[donor] -= 1;
                counts[index] = 1;
            }
        }
    }
    counts
}

/// Segments of the window: categories scaled to the used size, then free space and reserve.
pub fn cells(snapshot: &Value) -> Vec<Value> {
    let Window {
        used,
        maximum,
        usage,
    } = window(snapshot);
    let categories: Vec<&Value> = usage
        .list("categories")
        .iter()
        .filter(|category| {
            category.f64("tokens").unwrap_or(0.) > 0.
                && !category.str("name").starts_with("provider_cache")
        })
        .collect();
    let total: f64 = categories
        .iter()
        .map(|category| category.f64("tokens").unwrap_or(0.))
        .sum();
    let mut parts: Vec<(String, String, f64)> = Vec::new();
    if used > 0. {
        for (index, item) in categories.iter().enumerate() {
            let name = item.str("name");
            let (label, color) = category(name)
                .map(|(label, color)| (label.to_owned(), color.to_owned()))
                .unwrap_or_else(|| {
                    (
                        name.replace('_', " "),
                        PALETTE[index % PALETTE.len()].to_owned(),
                    )
                });
            parts.push((
                label,
                color,
                used * item.f64("tokens").unwrap_or(0.) / total,
            ));
        }
        if parts.is_empty() {
            parts.push(("занято".to_owned(), PALETTE[0].to_owned(), used));
        }
    }
    if let Some(maximum) = maximum {
        let trigger = usage
            .f64("compaction_trigger_tokens")
            .filter(|trigger| *trigger > 0.);
        let reserve = trigger
            .filter(|trigger| *trigger < maximum)
            .map(|trigger| maximum - trigger)
            .unwrap_or(0.)
            .min((maximum - used).max(0.));
        let free = (maximum - used - reserve).max(0.);
        for (label, color, value) in [
            ("свободно", FREE, free),
            ("резерв автокомпакта", RESERVE, reserve),
        ] {
            if value > 0. {
                parts.push((label.to_owned(), color.to_owned(), value));
            }
        }
    }
    let basis = maximum
        .unwrap_or_else(|| parts.iter().map(|part| part.2).sum::<f64>())
        .max(1.);
    let counts = allocate(&parts.iter().map(|part| part.2).collect::<Vec<_>>(), CELLS);
    parts
        .into_iter()
        .zip(counts)
        .map(|((label, color, value), count)| {
            json!({"label": label, "cells": count, "color": color, "value": tokens(Some(value.round())),
                   "detail": format!("{:.1}%", value / basis * 100.)})
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_nonzero_categories_keep_a_cell() {
        let segments = cells(
            &json!({"history": {"estimated_tokens": 100000}, "latest_usage": {
            "max_input_tokens": 200000, "compaction_trigger_tokens": 180000,
            "categories": [{"name": "messages", "tokens": 99999}, {"name": "tiny", "tokens": 1},
                           {"name": "provider_cache_read", "tokens": 50000}]}}),
        );
        let labels: Vec<&str> = segments
            .iter()
            .map(|segment| segment.str("label"))
            .collect();
        assert_eq!(
            labels,
            [
                "история сообщений",
                "tiny",
                "свободно",
                "резерв автокомпакта"
            ]
        );
        assert_eq!(
            segments
                .iter()
                .map(|segment| segment.i64("cells").unwrap())
                .sum::<i64>(),
            200
        );
        assert!(
            segments
                .iter()
                .all(|segment| segment.i64("cells").unwrap() >= 1)
        );
        assert_eq!(segments[3].str("detail"), "10.0%");
    }

    #[test]
    fn cache_status_follows_provider_counters() {
        let status = |actual: Value| cache(&json!({"actual": actual})).status;
        assert_eq!(cache(&Value::Null).status, "n/a");
        assert_eq!(status(json!({"input_tokens": 0})), "n/a");
        assert_eq!(
            status(json!({"input_tokens": 100, "cached_input_tokens": 0})),
            "cold"
        );
        assert_eq!(
            status(json!({"input_tokens": 100, "cache_creation_input_tokens": 50})),
            "warming"
        );
        assert_eq!(
            status(json!({"input_tokens": 100, "cached_input_tokens": 49})),
            "warming"
        );
        let hot = cache(&json!({"actual": {"input_tokens": 200, "cached_input_tokens": 101}}));
        assert_eq!((hot.status, hot.hit), ("hot", Some(51)));
    }
}
