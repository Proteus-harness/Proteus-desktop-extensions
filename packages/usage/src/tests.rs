use std::collections::HashSet;

use proteus_extension::{Host, Value, ValueExt, json};

use crate::report::{self, cost, filtered, money, price, totals};
use crate::{Extension, Usage};

/// The closed widget schema of the client (tests/schema.py), only what this page uses.
fn check(node: &Value) {
    let allowed: &[&str] = match node.str("type") {
        "stack" => &["direction", "gap", "children"],
        "card" => &["title", "children"],
        "text" => &["text", "tone", "size", "mono"],
        "metric" => &["label", "value", "detail", "tone"],
        "table" => &["columns", "rows"],
        "button" => &[
            "label", "action", "value", "variant", "icon", "selected", "hint", "confirm",
        ],
        "badge" => &["label", "tone"],
        "input" => &["id", "value", "placeholder", "action", "lines", "submit"],
        "select" => &["id", "label", "options", "action"],
        other => panic!("unknown node {other}: {node}"),
    };
    for key in node
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| *key != "type")
    {
        assert!(allowed.contains(&key.as_str()), "{key} in {node}");
    }
    if node.str("type") == "table" {
        let columns = node.list("columns").len();
        assert!(
            node.list("rows")
                .iter()
                .all(|row| row.as_array().unwrap().len() == columns
                    && row.as_array().unwrap().iter().all(Value::is_string))
        );
    }
    node.list("children").iter().for_each(check);
}

fn request(number: i64) -> Value {
    json!({"exchange_id": format!("request-{number}"), "turn_id": format!("turn-{}", number % 2),
           "model": {"provider": "fixture", "model": "gpt-6-astra"}, "origin": "direct",
           "started_at_ms": number * 1000, "finished_at_ms": number * 1000 + 500, "status": "completed",
           "finish_reason": "Stop", "usage": {"input_tokens": 1000 * number, "output_tokens": 10, "cached_input_tokens": null,
                                              "cache_creation_input_tokens": null, "reasoning_output_tokens": null},
           "message_count": 2, "tool_count": 1, "reasoning_effort": null, "max_output_tokens": null})
}

fn with(mut request: Value, fields: Value) -> Value {
    for (key, value) in fields.as_object().unwrap() {
        request[key] = value.clone();
    }
    request
}

fn view(usage: &mut Usage, snapshot: &Value) -> String {
    let root = usage.page(json!({"type": "select", "id": "session", "label": "s", "options": [], "action": "session"}), snapshot).unwrap();
    check(&root);
    serde_json::to_string(&root).unwrap()
}

fn ids(rows: &[&Value]) -> Vec<String> {
    rows.iter()
        .map(|row| row.str("exchange_id").to_owned())
        .collect()
}

#[test]
fn unpriced_requests_explain_why() {
    let base = request(1);
    assert_eq!(
        price(&with(base.clone(), json!({"usage": null}))).err(),
        Some("Нет данных о токенах")
    );
    assert_eq!(
        price(&with(
            base.clone(),
            json!({"model": {"provider": "x", "model": "local"}})
        ))
        .err(),
        Some("Тариф не задан")
    );
    let broken = json!({"input_tokens": 1000, "output_tokens": 10, "cached_input_tokens": 900, "cache_creation_input_tokens": 200});
    assert_eq!(
        price(&with(base.clone(), json!({"usage": broken}))).err(),
        Some("Некорректная разбивка токенов")
    );
    let long = with(
        base,
        json!({"model": {"provider": "x", "model": "gpt-5.6"},
                                 "usage": {"input_tokens": 1000000, "output_tokens": 1000, "cached_input_tokens": 500000}}),
    );
    let value = price(&long).unwrap();
    assert!(value.long);
    assert!((value.input - (500000. * 4. + 500000. * 0.4) * 2. / 1e6).abs() < 1e-9);
    assert!((value.output - 1000. * 20. * 1.5 / 1e6).abs() < 1e-9);
    assert!((cost(&long).unwrap() - value.input - value.output).abs() < 1e-9);
}

#[test]
fn money_keeps_two_to_six_decimals() {
    assert_eq!(money(0.5), "$0.50");
    assert_eq!(money(0.001234), "$0.001234");
    assert_eq!(money(12.), "$12.00");
    assert_eq!(money(0.1234567), "$0.123457");
}

#[test]
fn search_requires_every_word_and_unknowns_sort_last() {
    let rows = [
        request(1),
        with(
            request(2),
            json!({"model": {"provider": "fixture", "model": "local"}, "usage": null}),
        ),
        with(
            request(3),
            json!({"status": "unfinished", "finished_at_ms": null}),
        ),
    ];
    let rows: Vec<&Value> = rows.iter().collect();
    let order = |query: &str, order: &str| ids(&filtered(&rows, "all", query, order).unwrap());
    assert_eq!(order("FIXTURE local", "oldest"), ["request-2"]);
    assert!(order("fixture nothing", "oldest").is_empty());
    assert_eq!(order(" request-3  turn-1 ", "oldest"), ["request-3"]);
    assert_eq!(order("", "tokens"), ["request-3", "request-1", "request-2"]);
    assert_eq!(order("", "longest").last().unwrap(), "request-3");
    assert_eq!(order("", "cost").last().unwrap(), "request-2");
}

#[test]
fn requests_tab_renders_cards_summary_and_pages() {
    let mut requests: Vec<Value> = (1..22).map(request).collect();
    let local = json!({"provider": "fixture", "model": "local"});
    requests.push(with(request(22), json!({"model": local})));
    requests.push(with(request(23), json!({"model": local, "usage": null, "origin": "auxiliary", "status": "error", "turn_id": null})));
    let snapshot =
        json!({"session_id": "s", "revision": 1, "latest_turn_id": "turn-1", "requests": requests});
    let mut usage = Usage {
        expanded: HashSet::from(["request-23".to_owned()]),
        order: "newest".to_owned(),
        ..Usage::default()
    };
    let text = view(&mut usage, &snapshot);
    for fragment in [
        "Итог выбранного периода",
        "Токены известны для 22 из 23",
        "Стоимость учтена для 21 из 23",
        "Вспомогательный",
        "Вне хода",
        "Ошибка",
        "Нет данных о токенах",
        "Запросы: 23 из 23",
        "Все модели",
        "Дальше",
        "входят в выход",
        "#23 · local",
    ] {
        assert!(text.contains(fragment), "{fragment}");
    }
    assert!(!text.contains("Назад"));
    let root: Value = serde_json::from_str(&text).unwrap();
    let controls = &root.list("children")[1];
    let periods: Vec<&str> = controls.list("children")[0]
        .list("options")
        .iter()
        .map(|option| option.str("label"))
        .collect();
    assert_eq!(periods[2..4], ["Ход 2", "Ход 1"]);
    usage
        .action(&mut Host::default(), "page", "page", &json!(1))
        .unwrap();
    let text = view(&mut usage, &snapshot);
    assert!(text.contains("Назад"));
    assert!(!text.contains("Дальше"));
}

#[test]
fn requests_tab_empty_states_and_single_model() {
    let empty = json!({"session_id": "s", "revision": 1, "latest_turn_id": null, "requests": []});
    assert!(
        view(&mut Usage::default(), &empty).contains("В этой сессии ещё нет записанных запросов.")
    );
    let single =
        json!({"session_id": "s", "revision": 1, "latest_turn_id": null, "requests": [request(1)]});
    let text = view(
        &mut Usage {
            query: "absent".to_owned(),
            ..Usage::default()
        },
        &single,
    );
    assert!(text.contains("По этим фильтрам запросов нет."));
    assert!(!text.contains("Все модели"));
    assert!(!text.contains("Дальше"));
    assert!(view(&mut Usage::default(), &Value::Null).contains("Журнал расхода недоступен"));
}

#[test]
fn optional_usage_and_cache_cost() {
    let mut request = json!({"model": {"provider": "fixture", "model": "gpt-6-astra"}, "origin": "direct", "status": "completed",
                             "usage": {"input_tokens": 10000, "output_tokens": 1000, "cached_input_tokens": 2000, "reasoning_output_tokens": 400}});
    assert!((cost(&request).unwrap() - 0.132).abs() < 1e-9);
    let total = totals(&[&request]);
    assert_eq!(total.reasoning, Some(400));
    assert_eq!(total.written, None);
    request["usage"]["cached_input_tokens"] = json!(10001);
    assert_eq!(cost(&request), None);
    request["usage"] = Value::Null;
    let total = totals(&[&request]);
    assert_eq!(
        (total.reported, total.priced, total.reasoning),
        (0, 0, None)
    );
}

#[test]
fn switching_sessions_drops_filters() {
    let mut usage = Usage {
        period: "turn-1".to_owned(),
        page: 2,
        ..Usage::default()
    };
    usage.session_changed(&mut Host::default());
    assert_eq!((usage.period.as_str(), usage.page), ("all", 0));
    assert_eq!(report::seconds(&request(1)), "0,5 с");
}

#[test]
fn formats_local_time_or_a_dash() {
    let text = report::clock(Some(1_700_000_000_000.));
    assert_eq!(text.len(), 14, "{text}");
    assert_eq!(&text[2..3], ".");
    assert_eq!(report::clock(None), "—");
    assert_eq!(report::clock(Some(1e300)), "—");
}
