//! Quota details reuse the standard extension cards and progress primitives.

use proteus_extension::ui::{Node, card, kv, note, progress, text};
use proteus_extension::{Value, ValueExt, local_time};

use crate::describe::{credit_value, credits, duration, number, overage, remaining, reset_label};

pub fn render(
    snapshot: &Value,
    model: Option<&str>,
    now: f64,
    hide_zero_credits: bool,
) -> Vec<Value> {
    let mut children = Vec::new();
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
            // A labeled Progress adds its own rounded integer; show the exact
            // remainder once, together with the window and reset time.
            windows.push(note(format!(
                "{label} · осталось {}%{} · {}",
                number(left),
                overage(used),
                reset_label(window.f64("resets_at"), now)
            )));
            windows.push(progress(left, 100.));
            if let Some(amount) = amount(window.at("amount")) {
                windows.push(kv(amount));
            }
        }
        if windows.is_empty() {
            windows.push(note("Провайдер не сообщил окна лимита"));
        }
        // Keep the scope on the existing title: one more widget per bucket
        // would shrink the set of snapshots that fit the client's node budget.
        let scoped = bucket
            .bool("informational")
            .then(|| format!("{name} · {}", crate::describe::INFORMATIONAL_SCOPE));
        children.push(card(Some(scoped.as_deref().unwrap_or(name)), windows));
    }
    for notice in snapshot.list("notices").iter().filter_map(Value::as_str) {
        children.push(note(notice).with("tone", "warning"));
    }
    if let Some(credits) =
        credits(snapshot, hide_zero_credits).filter(|credits| credits.is_object())
    {
        children.push(card(Some("Кредиты"), vec![text(credit_value(credits))]));
    }
    children.extend(metadata(snapshot, model));
    children
}

pub fn metadata(snapshot: &Value, model: Option<&str>) -> Option<Value> {
    let mut parts = Vec::with_capacity(3);
    if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
        parts.push(model.to_owned());
    }
    if let Some(plan) = snapshot
        .opt_str("plan")
        .filter(|plan| !plan.trim().is_empty())
    {
        parts.push(format!("Тариф {plan}"));
    }
    if let Some(timestamp) = snapshot.f64("observed_at") {
        parts.push(format!("Данные на {}", local_time(timestamp)));
    }
    (!parts.is_empty()).then(|| note(parts.join(" · ")))
}

fn amount(value: &Value) -> Option<Vec<(&'static str, String)>> {
    let used = value.f64("used")?;
    let limit = value.f64("limit")?;
    let unit = value.opt_str("unit")?;
    let format = |n: f64| format!("{n:.2} {unit}").replacen('.', ",", 1);
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
    fn informational_bucket_explains_scope_without_hiding_reported_status() {
        use crate::describe::{INFORMATIONAL_SCOPE, describe};
        let sample = |informational| {
            json!({"observed_at": 1, "buckets": [
                {"id":"primary", "windows":[{"id":"daily","used_percent":25}]},
                {"id":"shared","informational":informational,"limit_reached":true,"windows":[]}
            ]})
        };
        for informational in [false, true] {
            let snapshot = sample(informational);
            let (remaining, lines) = describe(&snapshot, 1., true);
            assert_eq!(remaining, Some(75.));
            assert_eq!(
                lines.contains(&format!("shared: {INFORMATIONAL_SCOPE}")),
                informational
            );
            assert!(lines.contains(&"shared: лимит исчерпан".to_owned()));
            let cards = render(&snapshot, None, 1., true);
            let shared = &cards[1]["children"];
            let expected = if informational {
                format!("shared · {INFORMATIONAL_SCOPE}")
            } else {
                "shared".to_owned()
            };
            assert_eq!(cards[1]["title"], expected);
            assert_eq!(shared.as_array().unwrap().len(), 1);
            assert!(
                shared
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|node| node["text"] == "Лимит исчерпан")
            );
        }
    }

    #[test]
    fn reports_amounts_notices_and_unknown_reset_without_inventing_values() {
        let snapshot = json!({"observed_at":1700000000,"notices":["Budgets Read required"],"buckets":[
            {"id":"shared","name":"Общий лимит","windows":[{"id":"monthly","used_percent":20}]},
            {"id":"budget","windows":[{"id":"budget","used_percent":25,"amount":{"used":12.5,"limit":50,"unit":"USD"}}]}]});
        let nodes = render(&snapshot, None, 1700000000., true);
        let row = &nodes[0]["children"];
        assert_eq!(
            row[0]["text"],
            "Месяц · осталось 80% · Время сброса не указано"
        );
        let bar = &row[1];
        assert!(bar.get("label").is_none());
        assert_eq!(bar["value"].as_f64(), Some(80.));
        let rendered = serde_json::to_string(&nodes).unwrap();
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

    #[test]
    fn valid_percentage_overage_remains_visible_in_both_quota_surfaces() {
        let snapshot = |used: f64| {
            json!({"observed_at": 1_700_000_000,
        "buckets": [{"id":"shared","windows":[{"id":"weekly","used_percent": used}]}]})
        };
        for (used, expected) in [
            (99.0, "1%"),
            (92.5, "7,5%"),
            (100.0, "0%"),
            (100.04, "0% · сверх лимита <0,1%"),
            (100.1, "0% · сверх лимита 0,1%"),
            (125.0, "0% · сверх лимита 25%"),
            (127.5, "0% · сверх лимита 27,5%"),
        ] {
            let data = snapshot(used);
            let (left, lines) = crate::describe::describe(&data, 1_700_000_000., true);
            assert_eq!(left, Some((100.0_f64 - used).max(0.0)));
            assert_eq!(
                lines[0],
                format!("shared · 7 дн.: осталось {expected} · Время сброса не указано")
            );
            let nodes = render(&data, None, 1_700_000_000., true);
            let children = &nodes[0]["children"];
            assert_eq!(children[1]["type"], "progress");
            assert!(children[1].get("label").is_none());
            assert_eq!(children[1]["value"].as_f64(), left);
            assert_eq!(children[1]["max"], 100.0);
            assert_eq!(
                children[0]["text"],
                format!("7 дн. · осталось {expected} · Время сброса не указано")
            );
        }
    }

    #[test]
    fn amount_localizes_only_the_number_not_the_unit() {
        for unit in ["USD", "руб.", "усл. ед.", "USD.h"] {
            let value = json!({"used":12.5,"limit":50.0,"unit":unit});
            let rows = amount(&value).unwrap();
            assert_eq!(
                rows,
                vec![
                    ("Потрачено", format!("12,50 {unit}")),
                    ("Лимит", format!("50,00 {unit}")),
                    ("Осталось", format!("37,50 {unit}"))
                ]
            );
        }
    }
}
