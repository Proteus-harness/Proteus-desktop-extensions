//! Quota details reuse the standard extension cards and progress primitives.

use proteus_extension::ui::{Node, card, kv, note, progress, text};
use proteus_extension::{Value, ValueExt, local_time};

use crate::describe::{duration, number, remaining, reset_label};

pub fn render(snapshot: &Value, now: f64) -> Vec<Value> {
    let mut children = Vec::new();
    if let Some(plan) = snapshot.opt_str("plan") {
        children.push(text(plan).with("tone", "muted"));
    }
    for bucket in snapshot.list("buckets") {
        let name = bucket.opt_str("name").unwrap_or(bucket.str("id"));
        let mut windows = Vec::new();
        if bucket.at("limit_reached").as_bool() == Some(true) {
            windows.push(note("Лимит исчерпан").with("tone", "danger"));
        } else if bucket.at("allowed").as_bool() == Some(false) {
            windows.push(note("Использование недоступно").with("tone", "warning"));
        }
        for window in bucket.list("windows") {
            let Some(used) = window.f64("used_percent") else {
                windows.push(note("Провайдер не сообщил расход для этого окна"));
                continue;
            };
            let left = remaining(used);
            let label = duration(window.i64("duration_seconds"), window.str("id"));
            windows.push(
                progress(used.clamp(0., 100.), 100.)
                    .with("label", format!("{label} · использовано")),
            );
            if let Some(amount) = amount(window.at("amount")) {
                windows.push(kv(amount));
            }
            windows.push(
                text(format!(
                    "Осталось {}% · {}",
                    number(left),
                    reset_label(window.f64("resets_at"), now)
                ))
                .with("tone", "muted"),
            );
        }
        if windows.is_empty() {
            windows.push(note("Провайдер не сообщил окна лимита"));
        }
        children.push(card(Some(name), windows));
    }
    for notice in snapshot.list("notices").iter().filter_map(Value::as_str) {
        children.push(note(notice).with("tone", "warning"));
    }
    let credits = snapshot.at("credits");
    if credits.is_object() {
        let value = if credits.bool("unlimited") {
            "без ограничений".to_owned()
        } else if let Some(balance) = credits.opt_str("balance") {
            balance.to_owned()
        } else if credits.bool("available") {
            "доступны".to_owned()
        } else {
            "нет доступных".to_owned()
        };
        children.push(card(Some("Кредиты"), vec![text(value)]));
    }
    if let Some(timestamp) = snapshot.f64("observed_at") {
        children.push(text(format!("Данные на {}", local_time(timestamp))).with("tone", "muted"));
    }
    children
}

fn amount(value: &Value) -> Option<Vec<(&'static str, String)>> {
    let used = value.f64("used")?;
    let limit = value.f64("limit")?;
    let unit = value.opt_str("unit")?;
    let format = |n: f64| format!("{n:.2} {unit}").replace('.', ",");
    Some(vec![
        ("Потрачено", format(used)),
        ("Лимит", format(limit)),
        ("Осталось", format((limit - used).max(0.))),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_extension::json;

    #[test]
    fn reports_amounts_notices_and_unknown_reset_without_inventing_values() {
        let snapshot = json!({"observed_at":1700000000,"notices":["Budgets Read required"],"buckets":[
            {"id":"shared","name":"Общий лимит","windows":[{"id":"monthly","used_percent":20}]},
            {"id":"budget","windows":[{"id":"budget","used_percent":25,"amount":{"used":12.5,"limit":50,"unit":"USD"}}]}]});
        let rendered = serde_json::to_string(&render(&snapshot, 1700000000.)).unwrap();
        for text in [
            "80%",
            "Месяц",
            "Время сброса не указано",
            "12,50 USD",
            "37,50 USD",
            "Budgets Read required",
        ] {
            assert!(rendered.contains(text), "missing {text}");
        }
        assert!(!rendered.contains("Сброс 1970"));
        assert_eq!(amount(&Value::Null), None);
    }
}
