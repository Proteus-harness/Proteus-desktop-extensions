"""Pure presentation facts of the /analysis contract: titles, statuses, metrics, formatting."""
import datetime
import json

TURN_STATUS = {"unsettled":"Нет завершения", "success":"Завершён", "error":"Ошибка", "canceled":"Отменён", "timeout":"Таймаут"}
KINDS = {"model":"Модель", "tool":"Инструмент", "compaction":"Контекст", "hook":"Хуки"}
HOOKS = {"turn_started":"Начало хода", "before_tool_selection":"Отбор инструментов", "before_context":"Подготовка контекста",
         "before_model":"Перед запросом модели", "before_tool":"Перед инструментом", "after_tool":"После инструмента",
         "before_stop":"Проверка завершения", "turn_settled":"Завершение хода"}
FINISH = {"Stop":"остановка", "ToolCalls":"вызов инструментов", "Length":"лимит длины", "ContentFilter":"фильтр содержимого",
          "Error":"ошибка", "Unknown":"неизвестно"}
ROLES = {"System":"Система", "Developer":"Разработчик", "User":"Пользователь", "Assistant":"Ассистент", "Tool":"Инструмент"}
DECISIONS = {"allowed":"Разрешён", "approved":"Подтверждён пользователем", "approval_denied":"Отклонён пользователем",
             "policy_denied":"Запрещён политикой", "hook_blocked":"Заблокирован хуком", "validation_failed":"Аргументы не прошли проверку",
             "unsupported":"Не поддерживается"}
ERROR, MISSING, UNCHANGED, DONE = "Ошибка", "Нет результата", "Без изменений", "Готово"


def turn_tone(status):
    return "success" if status == "success" else "warning" if status == "unsettled" else "danger"


def kind(step):
    return step["data"]["kind"]


def title(step):
    data = step["data"]
    if data["kind"] == "model":
        return data["request"]["model"]["model"] + (" · вспомогательный запрос" if data.get("origin") == "auxiliary" else " · основной запрос")
    if data["kind"] == "tool": return data["call"]["name"]
    if data["kind"] == "compaction": return "Сжатие истории"
    event = data["trace"]["input"]["event"]["event"]
    return HOOKS.get(event, event)


def has_error(step):
    data = step["data"]
    if data["kind"] == "model":
        return bool(data.get("failure")) or (data.get("response") or {}).get("finish_reason") == "Error"
    if data["kind"] == "tool":
        return data.get("result") is not None and not data["result"]["ok"]
    if data["kind"] == "hook":
        return any(item["outcome"]["status"] == "failed" for item in data["trace"]["steps"])
    return False


def status(step):
    if has_error(step): return ERROR
    data = step["data"]
    if data["kind"] == "model" and data.get("response") is None: return MISSING
    if data["kind"] == "tool" and data.get("result") is None: return MISSING
    if data["kind"] == "compaction" and not data["report"]["changed"]: return UNCHANGED
    return DONE


def tone(state):
    return {ERROR:"danger", MISSING:"warning"}.get(state, "muted")


def problem(step):
    return status(step) in (ERROR, MISSING)


def matches(step, kind_filter="all", problems=False, query=""):
    query = query.strip().lower()
    return ((kind_filter == "all" or kind(step) == kind_filter) and (not problems or problem(step))
            and (not query or query in title(step).lower()))


def default_step(steps):
    """The step shown after loading a turn: the first failure, else the first step."""
    chosen = next((step for step in steps if has_error(step)), steps[0] if steps else None)
    return chosen and chosen["sequence"]


def metrics(steps):
    result = {"models":0, "tools":0, "errors":0, "missing":0, "with_usage":0, "input":0, "output":0}
    for step in steps:
        data = step["data"]
        result["errors"] += has_error(step)
        if data["kind"] == "model":
            result["models"] += 1
            result["missing"] += data.get("response") is None and not data.get("failure")
            usage = (data.get("response") or {}).get("usage")
            if usage:
                result["with_usage"] += 1
                result["input"] += usage["input_tokens"]
                result["output"] += usage["output_tokens"]
        elif data["kind"] == "tool":
            result["tools"] += 1
            result["missing"] += data.get("result") is None
    return result


def usage_text(summary):
    if not summary["with_usage"]: return "нет данных"
    return f"{number(summary['input'])} / {number(summary['output'])}"


def number(value):
    """Groups thousands with a narrow space: `12 345`."""
    return "—" if value is None else f"{value:,}".replace(",", "\u202f")


def duration(start, end):
    return "—" if end is None or start is None else f"{max(0, end - start) / 1000:.2f} с"


def turn_duration(steps):
    return duration(steps[0]["started_at_ms"], steps[-1].get("finished_at_ms")) if steps else "—"


def clock(ms):
    try:
        return datetime.datetime.fromtimestamp(ms / 1000).strftime("%d.%m %H:%M:%S")
    except (TypeError, ValueError, OverflowError, OSError):
        return "—"


def short_id(value):
    return str(value)[:8]


def preview(text, limit=90):
    flat = " ".join(str(text).split())
    return flat if len(flat) <= limit else flat[:limit - 1].rstrip() + "…"


def pretty(value):
    return json.dumps(value, ensure_ascii=False, indent=2)


def tagged(value):
    """Name of an externally tagged serde enum: `"other"` or `{"retryable": {...}}`."""
    return next(iter(value)) if isinstance(value, dict) and len(value) == 1 else str(value)


def part_text(part):
    name, body = (next(iter(part.items())) if isinstance(part, dict) and len(part) == 1 else ("", part))
    body = body if isinstance(body, dict) else {}
    if name == "Text": return body.get("text", "").strip()
    if name in ("Reasoning", "ReasoningSummary"): return ""
    if name == "ToolCall":
        call = body.get("call", {})
        return f"Вызов {call.get('name', '?')}: {json.dumps(call.get('args'), ensure_ascii=False)}"
    if name == "ToolResult":
        result = body.get("result", {})
        return f"Результат {result.get('call_id', '')}:\n{result.get('output') or result.get('error') or ''}".rstrip()
    if name == "FileRef": return f"Файл {body.get('path', '')}" + (f"\n{body['content']}" if body.get("content") else "")
    if name == "Image": return "[изображение]"
    return pretty(part)


def message_text(message):
    """Presentation text of a canonical message without reasoning; raw JSON when it has none."""
    text = "\n".join(filter(None, (part_text(part.get("payload")) for part in message.get("parts", []))))
    return text or pretty(message)


def role(message):
    return ROLES.get(message.get("role"), str(message.get("role")))


def finish_text(response):
    reason = response.get("finish_reason")
    end = {True:"да", False:"нет"}.get(response.get("end_turn"), "не указан")
    return f"{FINISH.get(reason, reason)} · конец хода: {end}"


def tokens_text(usage):
    parts = [f"вход {number(usage['input_tokens'])}", f"выход {number(usage['output_tokens'])}"]
    for key, name in (("cached_input_tokens","из кэша"), ("cache_creation_input_tokens","запись в кэш"), ("reasoning_output_tokens","рассуждения")):
        if usage.get(key) is not None: parts.append(f"{name} {number(usage[key])}")
    return " · ".join(parts)


def compaction_tokens(report):
    before, after = report.get("original_token_estimate"), report.get("output_token_estimate")
    if before is not None and after is not None: return f"{number(before)} → {number(after)}"
    return number(before) if before is not None else "нет данных"


def resolution_text(resolution):
    decision = resolution.get("decision", "")
    text = DECISIONS.get(decision, decision)
    return f"{text}: {resolution['reason']}" if resolution.get("reason") else text
