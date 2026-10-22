"""Readable details of one analysis step by kind: model, tool, compaction, hooks."""
import facts
from nodes import badge, card, disclosure, kv, label, row


def model(step, data, key, is_open):
    request, response, failure = data["request"], data.get("response"), data.get("failure")
    nodes = []
    if response and response.get("usage"):
        nodes.append(kv("Токены", facts.tokens_text(response["usage"])))
    if response:
        nodes.append(kv("Причина остановки", facts.finish_text(response)))
        calls = [call.get("name", "?") for call in response.get("tool_calls", [])]
        if calls: nodes.append(kv("Вызовы инструментов", ", ".join(calls)))
    nodes.append(kv("exchange", data.get("exchange_id", "—")))
    if failure:
        nodes.append(label(f"Ошибка модели: {facts.tagged(failure.get('kind'))} — {failure.get('message', '')}", "danger"))
    instructions = request.get("instructions", [])
    nodes.append(disclosure(key("instructions"), f"Инструкции · {len(instructions)}",
                            "\n\n".join(f"[{facts.tagged(block.get('kind'))} · приоритет {block.get('priority')}]\n{block.get('text', '')}"
                                        for block in instructions), is_open))
    messages = request.get("messages", [])
    nodes.append(label(f"Сообщения запроса · {len(messages)}", "muted", "small"))
    for index, message in enumerate(messages):
        text = facts.message_text(message)
        nodes.append(disclosure(key(f"message-{index}"), f"{facts.role(message)} · {facts.preview(text)}", text, is_open))
    tools = request.get("tools", [])
    nodes.append(disclosure(key("tools"), f"Доступные инструменты · {len(tools)}",
                            "\n".join(tool.get("name", "?") for tool in tools), is_open))
    answer = response.get("messages", []) if response else data.get("messages") or (failure or {}).get("completed_messages") or []
    text = "\n\n".join(facts.message_text(message) for message in answer)
    nodes.append(disclosure(key("answer"), "Записанный ответ", text or "Ответ не записан.", is_open))
    return nodes


def tool(step, data, key, is_open):
    call, result, resolution = data["call"], data.get("result"), data.get("resolution")
    nodes = [kv("call", call.get("id", "—")), disclosure(key("args"), "Аргументы", call.get("args"), is_open)]
    if data.get("approval_reason"):
        nodes.append(kv("Запрос подтверждения", data["approval_reason"]))
    if resolution:
        allowed = resolution.get("decision") in ("allowed", "approved")
        nodes.append(kv("Решение", facts.resolution_text(resolution), "default" if allowed else "danger"))
    if result is None:
        nodes.append(label("Результат не записан. Это не значит, что внешнего эффекта не было.", "warning"))
        return nodes
    if result.get("error"):
        nodes.append(label(result["error"], "danger"))
    nodes.append(disclosure(key("output"), "Результат", result.get("output", ""), is_open))
    return nodes


def compaction(step, data, key, is_open):
    report = data["report"]
    nodes = [kv("Сообщения", f"{report.get('input_messages')} → {report.get('output_messages')} · версия истории {data.get('history_revision')}"),
             kv("Токены", facts.compaction_tokens(report))]
    if report.get("trigger_tokens") is not None: nodes.append(kv("Порог", facts.number(report["trigger_tokens"])))
    if report.get("reason"): nodes.append(kv("Причина", report["reason"]))
    if report.get("skipped_reason"): nodes.append(kv("Пропущено", report["skipped_reason"], "warning"))
    if report.get("summary"): nodes.append(disclosure(key("summary"), "Сводка", report["summary"], is_open))
    return nodes


def hook(step, data, key, is_open):
    trace = data["trace"]
    nodes = [label("Обработчики", "muted", "small")]
    if not trace["steps"]: nodes.append(label("Обработчиков нет.", "muted"))
    for item in trace["steps"]:
        failed = item["outcome"]["status"] == "failed"
        nodes.append(row(label(item.get("module_id", "?")), badge("ошибка" if failed else "принят", "danger" if failed else "success")))
        if failed: nodes.append(label(item["outcome"].get("message", ""), "danger"))
    nodes += [disclosure(key("input"), "Входные данные", trace.get("input"), is_open),
              disclosure(key("chain"), "Результат цепочки", trace.get("output"), is_open)]
    return nodes


KINDS = {"model":model, "tool":tool, "compaction":compaction, "hook":hook}


def step_card(step, is_open):
    """`is_open(key)` tells whether a disclosure with that key is revealed."""
    sequence = step["sequence"]
    key = lambda name: f"{sequence}:{name}"
    data = step["data"]
    state = facts.status(step)
    nodes = [row(badge(state, facts.tone(state)),
                 label(f"{facts.duration(step['started_at_ms'], step.get('finished_at_ms'))} · {facts.clock(step['started_at_ms'])}", "muted", "small"))]
    nodes += KINDS[data["kind"]](step, data, key, is_open)
    nodes.append(disclosure(key("raw"), "Полная запись", step, is_open))
    nodes.append(label(f"Запись {sequence} · execution {step.get('execution_id') or '—'} · thread {step.get('thread_id') or '—'}", "muted", "small"))
    return card(f"{facts.KINDS[data['kind']]} · {facts.title(step)}", *nodes)
