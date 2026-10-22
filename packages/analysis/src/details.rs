//! Readable details of one analysis step by kind: model, tool, compaction, hooks.

use proteus_extension::{Value, ValueExt};

use crate::facts::{self, number};
use crate::nodes::{Open, badge, card, disclosure, kv, label, row, small};
use crate::text::{self, get, plain};

fn model(data: &Value, key: &dyn Fn(&str) -> String, open: &Open) -> Vec<Value> {
    let (request, response, failure) =
        (data.at("request"), data.at("response"), data.at("failure"));
    let mut nodes = Vec::new();
    if response.truthy() && response.at("usage").truthy() {
        nodes.push(kv(
            "Токены",
            text::tokens_text(response.at("usage")),
            "default",
        ));
    }
    if response.truthy() {
        nodes.push(kv(
            "Причина остановки",
            text::finish_text(response),
            "default",
        ));
        let calls: Vec<String> = response
            .list("tool_calls")
            .iter()
            .map(|call| get(call, "name", "?"))
            .collect();
        if !calls.is_empty() {
            nodes.push(kv("Вызовы инструментов", calls.join(", "), "default"));
        }
    }
    nodes.push(kv("exchange", get(data, "exchange_id", "—"), "default"));
    if failure.truthy() {
        let message = get(failure, "message", "");
        nodes.push(label(
            format!(
                "Ошибка модели: {} — {message}",
                text::tagged(failure.at("kind"))
            ),
            "danger",
        ));
    }
    let instructions = request.list("instructions");
    let blocks: Vec<String> = instructions
        .iter()
        .map(|block| {
            let kind = text::tagged(block.at("kind"));
            format!(
                "[{kind} · приоритет {}]\n{}",
                plain(block.at("priority")),
                get(block, "text", "")
            )
        })
        .collect();
    let title = format!("Инструкции · {}", instructions.len());
    nodes.push(disclosure(
        &key("instructions"),
        &title,
        &blocks.join("\n\n").into(),
        open,
    ));
    let messages = request.list("messages");
    nodes.push(small(
        format!("Сообщения запроса · {}", messages.len()),
        "muted",
    ));
    for (index, message) in messages.iter().enumerate() {
        let body = text::message_text(message);
        let title = format!("{} · {}", text::role(message), text::preview(&body, 90));
        nodes.push(disclosure(
            &key(&format!("message-{index}")),
            &title,
            &body.into(),
            open,
        ));
    }
    let tools: Vec<String> = request
        .list("tools")
        .iter()
        .map(|tool| get(tool, "name", "?"))
        .collect();
    let title = format!("Доступные инструменты · {}", tools.len());
    nodes.push(disclosure(
        &key("tools"),
        &title,
        &tools.join("\n").into(),
        open,
    ));
    let answer = if response.truthy() {
        response.list("messages")
    } else if data.at("messages").truthy() {
        data.list("messages")
    } else {
        failure.list("completed_messages")
    };
    let answer: Vec<String> = answer.iter().map(text::message_text).collect();
    let answer = answer.join("\n\n");
    let answer = if answer.is_empty() {
        "Ответ не записан.".to_owned()
    } else {
        answer
    };
    nodes.push(disclosure(
        &key("answer"),
        "Записанный ответ",
        &answer.into(),
        open,
    ));
    nodes
}

fn tool(data: &Value, key: &dyn Fn(&str) -> String, open: &Open) -> Vec<Value> {
    let (call, result, resolution) = (data.at("call"), data.at("result"), data.at("resolution"));
    let mut nodes = vec![
        kv("call", get(call, "id", "—"), "default"),
        disclosure(&key("args"), "Аргументы", call.at("args"), open),
    ];
    if data.at("approval_reason").truthy() {
        nodes.push(kv(
            "Запрос подтверждения",
            plain(data.at("approval_reason")),
            "default",
        ));
    }
    if resolution.truthy() {
        let allowed = matches!(resolution.str("decision"), "allowed" | "approved");
        nodes.push(kv(
            "Решение",
            text::resolution_text(resolution),
            if allowed { "default" } else { "danger" },
        ));
    }
    if result.is_null() {
        nodes.push(label(
            "Результат не записан. Это не значит, что внешнего эффекта не было.",
            "warning",
        ));
        return nodes;
    }
    if result.at("error").truthy() {
        nodes.push(label(plain(result.at("error")), "danger"));
    }
    let output = result.get("output").cloned().unwrap_or_else(|| "".into());
    nodes.push(disclosure(&key("output"), "Результат", &output, open));
    nodes
}

fn compaction(data: &Value, key: &dyn Fn(&str) -> String, open: &Open) -> Vec<Value> {
    let report = data.at("report");
    let messages = format!(
        "{} → {} · версия истории {}",
        plain(report.at("input_messages")),
        plain(report.at("output_messages")),
        plain(data.at("history_revision"))
    );
    let mut nodes = vec![
        kv("Сообщения", messages, "default"),
        kv("Токены", text::compaction_tokens(report), "default"),
    ];
    if !report.at("trigger_tokens").is_null() {
        nodes.push(kv("Порог", number(report.at("trigger_tokens")), "default"));
    }
    if report.at("reason").truthy() {
        nodes.push(kv("Причина", plain(report.at("reason")), "default"));
    }
    if report.at("skipped_reason").truthy() {
        nodes.push(kv(
            "Пропущено",
            plain(report.at("skipped_reason")),
            "warning",
        ));
    }
    if report.at("summary").truthy() {
        nodes.push(disclosure(
            &key("summary"),
            "Сводка",
            report.at("summary"),
            open,
        ));
    }
    nodes
}

fn hook(data: &Value, key: &dyn Fn(&str) -> String, open: &Open) -> Vec<Value> {
    let trace = data.at("trace");
    let mut nodes = vec![small("Обработчики", "muted")];
    if trace.list("steps").is_empty() {
        nodes.push(label("Обработчиков нет.", "muted"));
    }
    for item in trace.list("steps") {
        let failed = item.at("outcome").str("status") == "failed";
        let outcome = if failed {
            badge("ошибка", "danger")
        } else {
            badge("принят", "success")
        };
        nodes.push(row(vec![
            label(get(item, "module_id", "?"), "default"),
            outcome,
        ]));
        if failed {
            nodes.push(label(get(item.at("outcome"), "message", ""), "danger"));
        }
    }
    nodes.push(disclosure(
        &key("input"),
        "Входные данные",
        trace.at("input"),
        open,
    ));
    nodes.push(disclosure(
        &key("chain"),
        "Результат цепочки",
        trace.at("output"),
        open,
    ));
    nodes
}

pub fn step_card(step: &Value, open: &Open) -> Value {
    let sequence = plain(step.at("sequence"));
    let key = |name: &str| format!("{sequence}:{name}");
    let data = step.at("data");
    let state = facts::status(step);
    let timing = format!(
        "{} · {}",
        facts::duration(step.at("started_at_ms"), step.at("finished_at_ms")),
        facts::clock(step.at("started_at_ms"))
    );
    let mut nodes = vec![row(vec![
        badge(state, facts::tone(state)),
        small(timing, "muted"),
    ])];
    nodes.extend(match facts::kind(step) {
        "model" => model(data, &key, open),
        "tool" => tool(data, &key, open),
        "compaction" => compaction(data, &key, open),
        _ => hook(data, &key, open),
    });
    nodes.push(disclosure(&key("raw"), "Полная запись", step, open));
    let or_dash = |field: &str| {
        Some(step.at(field))
            .filter(|value| value.truthy())
            .map(plain)
            .unwrap_or_else(|| "—".to_owned())
    };
    nodes.push(small(
        format!(
            "Запись {sequence} · execution {} · thread {}",
            or_dash("execution_id"),
            or_dash("thread_id")
        ),
        "muted",
    ));
    card(
        format!(
            "{} · {}",
            facts::kind_name(facts::kind(step)),
            facts::title(step)
        ),
        nodes,
    )
}
