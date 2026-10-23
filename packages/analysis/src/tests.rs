use proteus_extension::{Host, Value, ValueExt, json};

use crate::facts::{self, default_step, metrics, usage_text};
use crate::summary::turn_rows;
use crate::text;
use crate::{Analysis, Extension};

/// The closed widget schema of the client (tests/schema.py), only what this page uses.
fn check(node: &Value) {
    let allowed: &[&str] = match node.str("type") {
        "fill" => &["children"],
        "split" => &["children", "widths"],
        "stack" => &["direction", "gap", "children"],
        "card" => &["title", "children"],
        "disclosure" => &["id", "label", "open", "children"],
        "text" => &["text", "tone", "size", "mono", "single_line"],
        "metric" => &["label", "value", "detail", "tone"],
        "button" => &[
            "label", "action", "value", "variant", "icon", "selected", "hint", "confirm",
        ],
        "badge" => &["label", "tone"],
        "input" => &["id", "value", "placeholder", "action", "lines", "submit"],
        "select" => &["id", "label", "options", "action"],
        "code" => &["id", "text", "language", "height", "wrap"],
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
    let shown = match node.str("type") {
        "text" => "text",
        "metric" => "value",
        "badge" => "label",
        _ => "type",
    };
    assert!(node.at(shown).is_string(), "{node}");
    node.list("children").iter().for_each(check);
}

fn message(role: &str, text: &str, part: Option<Value>) -> Value {
    json!({"id": format!("message-{role}"), "role": role, "phase": null, "name": null, "tool_call_id": null, "metadata": null,
           "parts": [{"part_id": "part-1", "provenance": "model", "scope": "conversation",
                      "payload": part.unwrap_or_else(|| json!({"Text": {"text": text}}))}]})
}

/// The /analysis contract of tests/wire.py: turns plus every step kind of the selected one.
fn analysis() -> Value {
    let usage = json!({"input_tokens": 10000, "output_tokens": 1000, "cached_input_tokens": 2000,
                       "cache_creation_input_tokens": null, "reasoning_output_tokens": 400});
    let call = json!({"id": "call-1", "name": "probe", "args": {"path": "sample.py"}, "surface": "function", "raw_arguments": null});
    let model = json!({"kind": "model", "exchange_id": "exchange-1", "origin": "direct", "messages": [], "failure": null,
        "request": {"model": {"provider": "fixture", "model": "gpt-6-astra"}, "instructions": [{"kind": "system", "text": "Be brief.", "priority": 0}],
                    "messages": [message("User", "Synthetic task", None)], "tools": [{"name": "probe"}]},
        "response": {"messages": [message("Assistant", "", Some(json!({"ToolCall": {"call": call}})))], "tool_calls": [call],
                     "finish_reason": "ToolCalls", "usage": usage, "end_turn": null, "provider_metadata": null}});
    let hook = json!({"kind": "hook", "trace": {"owner": null, "input": {"event": {"event": "before_tool", "call": call, "spec": null, "blocked": null}},
        "steps": [{"invocation_id": "hook-1", "module_id": "guard", "outcome": {"status": "failed", "message": "Synthetic hook failure", "model_failure": null}}],
        "output": null}});
    let tool = json!({"kind": "tool", "call": call, "approval_reason": "Synthetic approval", "resolution": {"decision": "approved"},
        "result": {"call_id": "call-1", "ok": true, "output": "ok\n".repeat(3), "content": [], "error": null, "metadata": null}});
    let compaction = json!({"kind": "compaction", "history_revision": 2, "report": {"changed": true, "user_message_replacements": [],
        "reason": "threshold", "input_messages": 12, "output_messages": 3, "original_token_estimate": 150000, "output_token_estimate": 9000,
        "trigger_tokens": 140000, "summary_source": null, "skipped_reason": null, "summary": "Synthetic summary", "metadata": null}});
    let steps: Vec<Value> = [model, hook, tool, compaction]
        .into_iter()
        .zip(1..)
        .map(|(data, sequence): (Value, i64)| json!({"sequence": sequence, "execution_id": "execution-1", "thread_id": null,
            "started_at_ms": 1000 * sequence, "finished_at_ms": 1000 * sequence + 100, "data": data}))
        .collect();
    let turns: Vec<Value> = (1..=10)
        .map(|number| json!({"turn_id": format!("turn-{number}"), "number": number, "prompt_preview": "Synthetic task ".repeat(20),
            "status": if number % 3 != 0 { "success" } else { "error" }, "started_at_ms": 1000 * number, "finished_at_ms": null}))
        .collect();
    json!({"session_id": "session-fixture", "revision": 7, "turns": turns,
           "selected": {"turn_id": "turn-10", "task": {"text": "Synthetic task", "cwd": "/workspace"}, "module_epoch": 1,
                        "config_snapshot": {"fixture": true}, "steps": steps, "output": {"text": "Synthetic answer", "metadata": null},
                        "error": "Synthetic turn error"}})
}

fn steps() -> Vec<Value> {
    analysis()["selected"]["steps"].as_array().unwrap().clone()
}

fn render(worker: &mut Analysis) -> String {
    let picker = json!({"type": "select", "id": "session", "label": "Synthetic · 2 сообщ.", "options": [], "action": "session"});
    let root = worker.page(&analysis(), picker);
    check(&root);
    serde_json::to_string(&root).unwrap()
}

fn act(worker: &mut Analysis, action: &str, value: Value) {
    worker
        .action(&mut Host::default(), "page", action, &value)
        .unwrap();
}

#[test]
fn hook_failures_follow_public_contract() {
    let mut step = json!({"data": {"kind": "hook", "trace": {"input": {"event": {"event": "before_tool"}},
        "steps": [{"outcome": {"status": "failed", "message": "fixture", "model_failure": null}}]}}});
    assert_eq!(facts::title(&step), "Перед инструментом");
    assert_eq!(facts::status(&step), "Ошибка");
    step["data"]["trace"]["input"]["event"]["event"] = json!("future_event");
    assert_eq!(facts::title(&step), "future_event");
    assert_eq!(
        facts::status(&json!({"data": {"kind": "model", "response": {"finish_reason": "Error"}}})),
        "Ошибка"
    );
}

#[test]
fn turn_metrics_and_problem_filter() {
    let steps = steps();
    let missing = json!({"sequence": 9, "started_at_ms": 0, "finished_at_ms": null, "data": {"kind": "tool", "call": {"name": "late"}, "result": null}});
    let all: Vec<Value> = steps.iter().cloned().chain([missing]).collect();
    let summary = metrics(&all);
    assert_eq!(
        (
            summary.models,
            summary.tools,
            summary.errors,
            summary.missing,
            summary.with_usage
        ),
        (1, 2, 1, 1, 1)
    );
    assert_eq!(usage_text(&summary), "10\u{202f}000 / 1\u{202f}000");
    assert_eq!(usage_text(&metrics(&steps[1..])), "нет данных");
    let sequences = |steps: &[Value], kind: &str, problems: bool, query: &str| -> Vec<i64> {
        steps
            .iter()
            .filter(|step| facts::matches(step, kind, problems, query))
            .filter_map(|step| step.i64("sequence"))
            .collect()
    };
    assert_eq!(sequences(&all, "all", true, ""), [2, 9]);
    assert_eq!(sequences(&steps, "tool", false, " PRO"), [3]);
    assert_eq!(default_step(&steps), Some(2));
    assert_eq!(default_step(&steps[2..]), Some(3));
    assert_eq!(default_step(&[]), None);
    assert_eq!(facts::turn_duration(&steps), "3.10 с");
}

#[test]
fn step_texts_are_readable() {
    let message = json!({"role": "Assistant", "parts": [{"payload": {"Reasoning": {"text": "hidden", "signature": null}}},
        {"payload": {"Text": {"text": " hi "}}}, {"payload": {"ToolCall": {"call": {"name": "probe", "args": {"a": 1}}}}}]});
    assert_eq!(text::message_text(&message), "hi\nВызов probe: {\"a\": 1}");
    assert!(
        text::message_text(
            &json!({"role": "User", "parts": [{"payload": {"Patch": {"patch": {}}}}]})
        )
        .contains("\"Patch\"")
    );
    assert_eq!(text::role(&message), "Ассистент");
    assert_eq!(text::preview(&"a\n b ".repeat(40), 12), "a b a b a b");
    assert_eq!(
        text::finish_text(&json!({"finish_reason": "ToolCalls", "end_turn": false})),
        "вызов инструментов · конец хода: нет"
    );
    let usage = json!({"input_tokens": 1500, "output_tokens": 2, "cached_input_tokens": 0, "cache_creation_input_tokens": null});
    assert_eq!(
        text::tokens_text(&usage),
        "вход 1\u{202f}500 · выход 2 · из кэша 0"
    );
    assert_eq!(
        text::compaction_tokens(
            &json!({"original_token_estimate": 150000, "output_token_estimate": null})
        ),
        "150\u{202f}000"
    );
    assert_eq!(text::compaction_tokens(&json!({})), "нет данных");
    assert_eq!(
        text::resolution_text(&json!({"decision": "hook_blocked", "reason": "guard"})),
        "Заблокирован хуком: guard"
    );
    assert_eq!(
        text::tagged(&json!({"retryable": {"retry_delay_ms": 5}})),
        "retryable"
    );
    assert_eq!(facts::duration(&json!(1000), &Value::Null), "—");
}

#[test]
fn turns_are_newest_first_and_keep_the_selection() {
    let data = analysis();
    let turns = data.list("turns");
    let numbers = |selected: &str, all: bool| -> Vec<i64> {
        turn_rows(turns, &json!(selected), all)
            .iter()
            .filter_map(|turn| turn.i64("number"))
            .collect()
    };
    assert_eq!(numbers("turn-10", false), [10, 9, 8, 7, 6, 5, 4, 3]);
    assert_eq!(numbers("turn-1", false)[7..], [3, 1]);
    assert_eq!(numbers("turn-1", true).len(), 10);
}

#[test]
fn analysis_page_matches_the_closed_widget_schema() {
    let mut worker = Analysis::default();
    let page = render(&mut worker);
    for expected in [
        "Перед инструментом",
        "ошибка",
        "Synthetic hook failure",
        "Причина завершения: Synthetic turn error",
        "версия журнала 7",
        "usage у 1 из 1 запросов",
        "Ошибки и без результата · 1",
        "Полная запись",
        "Запись 2 · execution execution-1",
    ] {
        assert!(page.contains(expected), "{expected}");
    }
    for (sequence, expected) in [
        (1, "из кэша 2\u{202f}000"),
        (3, "Подтверждён пользователем"),
        (4, "12 → 3 · версия истории 2"),
    ] {
        act(&mut worker, "step", json!(sequence));
        assert!(render(&mut worker).contains(expected), "{expected}");
    }
    act(&mut worker, "step", json!(1));
    assert!(render(&mut worker).contains("\"id\":\"analysis-1:raw\""));
    act(&mut worker, "step", json!(1));
    assert!(!render(&mut worker).contains("analysis-1:raw"));
    act(&mut worker, "problems", json!(true));
    assert!(render(&mut worker).contains("Шаги · 1 из 4"));
    act(&mut worker, "step", json!({"id": "3"}));
    assert_eq!(worker.step, Some(3));
}

#[test]
fn switching_chats_drops_the_old_turn() {
    let mut worker = Analysis {
        turn: Some("turn-1".to_owned()),
        step: Some(3),
        ..Analysis::default()
    };
    worker.session_changed(&mut Host::default());
    assert_eq!((worker.turn, worker.step), (None, None));
}

#[test]
fn formats_local_time_or_a_dash() {
    let text = facts::clock(&json!(1_700_000_000_000.));
    assert_eq!(text.len(), 14, "{text}");
    assert_eq!(&text[2..3], ".");
    assert_eq!(facts::clock(&Value::Null), "—");
    assert_eq!(facts::clock(&json!(1e300)), "—");
}
