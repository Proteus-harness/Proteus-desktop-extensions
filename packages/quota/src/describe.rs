//! Badge lines of a quota snapshot (`GET /model/quota`).

use proteus_extension::{Value, ValueExt, local_time};

/// One decimal without a trailing zero, with a decimal comma: `7,5`, `80`.
pub fn number(value: f64) -> String {
    format!("{value:.1}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .replace('.', ",")
}

pub fn remaining(used: f64) -> f64 {
    (100. - used).clamp(0., 100.)
}

pub fn duration(seconds: Option<i64>, fallback: &str) -> String {
    let Some(seconds) = seconds else {
        return match fallback {
            "monthly" => "Месяц".to_owned(),
            "weekly" => "7 дн.".to_owned(),
            "rolling" => "Скользящее окно".to_owned(),
            "budget" => "Бюджет".to_owned(),
            other => other.to_owned(),
        };
    };
    for (size, label) in [(86400, "дн."), (3600, "ч"), (60, "мин")] {
        if seconds > 0 && seconds % size == 0 {
            return format!("{} {label}", seconds / size);
        }
    }
    format!("{seconds} с")
}

pub fn reset_label(timestamp: Option<f64>, now: f64) -> String {
    match timestamp {
        None => "Время сброса не указано".to_owned(),
        Some(timestamp) if timestamp <= now => {
            "Время сброса прошло · ожидаем новые данные".to_owned()
        }
        Some(timestamp) => {
            let minutes = ((timestamp - now) / 60.).ceil().max(1.) as u64;
            let relative = if minutes >= 1440 {
                format!("{} дн. {} ч", minutes / 1440, minutes % 1440 / 60)
            } else if minutes >= 60 {
                format!("{} ч {} мин", minutes / 60, minutes % 60)
            } else {
                format!("{minutes} мин")
            };
            format!("Сброс {} · через {relative}", local_time(timestamp))
        }
    }
}

fn display(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

/// The smallest reported remainder across all quota windows, and the lines.
pub fn describe(snapshot: &Value, now: f64) -> (Option<f64>, Vec<String>) {
    let mut lines = Vec::new();
    let mut limiting: Option<f64> = None;
    let plan = snapshot.at("plan");
    if !plan.is_null() {
        lines.push(format!("План: {}", display(plan)));
    }
    for bucket in snapshot.list("buckets") {
        let name = bucket
            .opt_str("name")
            .filter(|name| !name.is_empty())
            .unwrap_or(bucket.str("id"));
        if bucket.at("limit_reached").as_bool() == Some(true) {
            lines.push(format!("{name}: лимит исчерпан"));
        } else if bucket.at("allowed").as_bool() == Some(false) {
            lines.push(format!("{name}: использование недоступно"));
        }
        for window in bucket.list("windows") {
            let Some(used) = window.f64("used_percent") else {
                continue;
            };
            let left = remaining(used);
            let seconds = window.i64("duration_seconds");
            if !bucket.bool("informational") {
                limiting = Some(limiting.map_or(left, |known| known.min(left)));
            }
            let label = duration(seconds, window.str("id"));
            let reset = reset_label(window.f64("resets_at"), now);
            lines.push(format!(
                "{name} · {label}: осталось {}% · {reset}",
                number(left)
            ));
        }
    }
    for notice in snapshot.list("notices").iter().filter_map(Value::as_str) {
        lines.push(notice.to_owned());
    }
    let credits = snapshot.at("credits");
    if !credits.is_null() {
        let value = if credits.bool("unlimited") {
            "без ограничений".to_owned()
        } else if !credits.at("balance").is_null() {
            display(credits.at("balance"))
        } else if credits.bool("available") {
            "доступны".to_owned()
        } else {
            "нет доступных".to_owned()
        };
        lines.push(format!("Кредиты: {value}"));
    }
    lines.push(format!(
        "Данные на {}",
        local_time(snapshot.f64("observed_at").unwrap_or(0.))
    ));
    (limiting, lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_extension::json;

    #[test]
    fn window_formatting_and_missing_provider() {
        assert_eq!(duration(Some(604_800), "w"), "7 дн.");
        assert_eq!(duration(Some(18_000), "w"), "5 ч");
        assert_eq!(duration(Some(90), "w"), "90 с");
        assert_eq!(duration(None, "primary"), "primary");
        assert_eq!(number(remaining(92.5)), "7,5");
        assert_eq!(number(remaining(20.)), "80");
        assert_eq!(number(0.), "0");
        assert_eq!(remaining(130.), 0.);
        assert_eq!(reset_label(None, 10.), "Время сброса не указано");
        assert!(reset_label(Some(5.), 10.).contains("прошло"));
    }

    #[test]
    fn describes_buckets_and_credits() {
        let snapshot = json!({"observed_at": 1_700_000_000, "plan": "pro", "credits": {"unlimited": false, "balance": 12.5},
            "buckets": [{"id": "main", "limit_reached": true, "windows": [
                {"id": "weekly", "duration_seconds": 604_800, "used_percent": 92.5},
                {"id": "other", "duration_seconds": 604_800, "used_percent": 80},
                {"id": "burst", "used_percent": 10, "resets_at": 5}]},
                {"id": "spare", "name": "Spare", "allowed": false, "windows": []}]});
        let (left, lines) = describe(&snapshot, 10.);
        assert_eq!(left, Some(7.5));
        assert_eq!(
            lines[..5],
            [
                "План: pro",
                "main: лимит исчерпан",
                "main · 7 дн.: осталось 7,5% · Время сброса не указано",
                "main · 7 дн.: осталось 20% · Время сброса не указано",
                "main · burst: осталось 90% · Время сброса прошло · ожидаем новые данные",
            ]
        );
        assert_eq!(
            lines[5..7],
            ["Spare: использование недоступно", "Кредиты: 12.5"]
        );
        assert!(lines[7].starts_with("Данные на "));
        let (left, lines) = describe(
            &json!({"observed_at": 0, "buckets": [], "credits": {"unlimited": true}}),
            10.,
        );
        assert_eq!(
            (left, lines[0].as_str()),
            (None, "Кредиты: без ограничений")
        );
    }
    #[test]
    fn all_enforced_windows_limit_badge_but_informational_budgets_do_not() {
        let snapshot = json!({"observed_at":1700000000,"buckets":[
            {"id":"shared","windows":[{"id":"rolling","used_percent":96},{"id":"weekly","used_percent":21},{"id":"unknown"}]},
            {"id":"budget","informational":true,"windows":[{"id":"budget","used_percent":99}]}]});
        let (left, lines) = describe(&snapshot, 1700000000.);
        assert_eq!(left, Some(4.));
        assert!(!lines.iter().any(|line| line.contains("unknown")));
        assert!(lines.iter().any(|line| line.contains("осталось 1%")));
        assert!(reset_label(Some(1700003600.), 1700000000.).contains("через 1 ч 0 мин"));
    }
}
