"""Usage calculations moved out of the client; pinned tariff table retained."""

import time

from proteus_extension import tokens

RATES = {
    "gpt-6-astra": (10, 1, 12.5, 50),
    "gpt-5.6-sol": (4, .4, 5, 20),
    "gpt-5.6": (4, .4, 5, 20),
    "gpt-5.6-terra": (2, .2, 2.5, 12),
    "gpt-5.6-luna": (.2, .02, .25, 1.2),
}
PRICE_DATE = "2026-09-10"
PRICE_SOURCE = "https://developers.openai.com/api/docs/pricing"


STATUS = {"completed": ("Готово", "success"), "error": ("Ошибка", "danger"), "canceled": ("Отменён", "danger"),
          "timeout": ("Таймаут", "danger"), "unfinished": ("Нет результата", "warning")}
FINISH = {"Stop": "Остановка (Stop)", "ToolCalls": "Вызов инструментов (ToolCalls)", "Length": "Лимит длины (Length)",
          "ContentFilter": "Фильтр контента (ContentFilter)", "Error": "Ошибка (Error)", "Unknown": "Неизвестно (Unknown)"}


def price(request):
    """`(input, output, long)` in USD, or the reason the request is not priced."""
    usage = request.get("usage")
    if usage is None:
        return "Нет данных о токенах"
    rate = RATES.get(request["model"]["model"])
    if rate is None:
        return "Тариф не задан"
    input_tokens = usage["input_tokens"]
    cached = usage.get("cached_input_tokens") or 0
    written = usage.get("cache_creation_input_tokens") or 0
    if cached + written > input_tokens:
        return "Некорректная разбивка токенов"
    fresh = input_tokens - cached - written
    long = input_tokens > 272000
    return ((fresh * rate[0] + cached * rate[1] + written * rate[2]) * (2 if long else 1) / 1000000,
            usage["output_tokens"] * rate[3] * (1.5 if long else 1) / 1000000, long)


def cost(request):
    value = price(request)
    return None if isinstance(value, str) else value[0] + value[1]


def money(value):
    """US dollars with two to six fraction digits."""
    text = f"{value:.6f}"
    while text.endswith("0") and len(text) > text.index(".") + 3:
        text = text[:-1]
    return "$" + text


def clock(ms):
    try:
        return time.strftime("%d.%m %H:%M:%S", time.localtime(ms / 1000))
    except (OverflowError, OSError, ValueError):
        return "—"


def model_key(request):
    model = request["model"]
    return model["provider"] + "/" + model["model"]


def duration(request):
    end = request.get("finished_at_ms")
    return max(0, end - request["started_at_ms"]) if end is not None else 0


def seconds(request):
    if request.get("finished_at_ms") is None:
        return "—"
    return f"{duration(request) / 1000:.1f} с".replace(".", ",")


def exact(value):
    return "—" if value is None else f"{value:,}".replace(",", "\u202f")


def used(request):
    usage = request.get("usage")
    return usage["input_tokens"] + usage["output_tokens"] if usage else None


def turn_ids(snapshot):
    return list(dict.fromkeys(request["turn_id"] for request in snapshot["requests"] if request.get("turn_id")))


def scoped(snapshot, period, model):
    requests = snapshot["requests"]
    if period != "all":
        turn = snapshot.get("latest_turn_id") if period == "latest" else period
        requests = [request for request in requests if request.get("turn_id") == turn]
    if model is not None:
        requests = [request for request in requests if model_key(request) == model]
    return requests


def matches(request, query):
    """Every whitespace-separated word occurs in the ids or the model."""
    haystack = " ".join((request["exchange_id"], str(request.get("turn_id") or ""),
                         request["model"]["provider"], request["model"]["model"])).lower()
    return all(term in haystack for term in query.lower().split())


def filtered(requests, status, query, order):
    rows = [request for request in requests
            if (status != "problems" or request["status"] != "completed")
            and (status != "missing" or request.get("usage") is None)
            and matches(request, query)]
    if order == "oldest":
        return rows
    keys = {
        "newest": lambda request: request["started_at_ms"],
        "longest": lambda request: duration(request) if request.get("finished_at_ms") is not None else None,
        "tokens": used,
        "cost": cost,
    }
    key = keys[order]
    known = sorted((request for request in rows if key(request) is not None), key=key, reverse=True)
    return known + [request for request in rows if key(request) is None]


def totals(requests):
    usages = [request["usage"] for request in requests if request.get("usage") is not None]
    prices = [value for request in requests if (value := cost(request)) is not None]
    result = {field:sum(usage.get(field) or 0 for usage in usages) for field in (
        "input_tokens","output_tokens","cached_input_tokens","cache_creation_input_tokens","reasoning_output_tokens")}
    for field in ("cached_input_tokens","cache_creation_input_tokens","reasoning_output_tokens"):
        if not any(usage.get(field) is not None for usage in usages): result[field]=None
    result.update(requests=len(requests), reported=len(usages), cost=sum(prices), priced=len(prices),
                  auxiliary=sum(request["origin"] == "auxiliary" for request in requests),
                  unfinished=sum(request["status"] == "unfinished" for request in requests),
                  errors=sum(request["status"] in ("error","canceled","timeout") for request in requests))
    return result


def by_model(requests):
    """Totals per model, largest token use first."""
    models = list(dict.fromkeys(model_key(request) for request in requests))
    groups = [(model, totals([request for request in requests if model_key(request) == model])) for model in models]
    return sorted(groups, key=lambda group: -(group[1]["input_tokens"] + group[1]["output_tokens"]))


def context(snapshot):
    usage = snapshot.get("latest_usage") or {}
    actual = usage.get("actual")
    used = actual["input_tokens"] if actual else usage.get("estimated_input_tokens", snapshot["history"]["estimated_tokens"])
    maximum = usage.get("max_input_tokens") or None
    cached = (actual or {}).get("cached_input_tokens")
    return used, maximum, cached, usage


CACHE = {"n/a": ("нет данных провайдера", "muted"), "cold": ("кэш не читался", "muted"),
         "warming": ("кэш прогревается", "warning"), "hot": ("кэш активно читается", "success")}


def cache(usage):
    """Provider input cache of the latest request: status, input, read, written, hit percent."""
    actual = (usage or {}).get("actual") or {}
    input_tokens = actual.get("input_tokens")
    cached, written = actual.get("cached_input_tokens"), actual.get("cache_creation_input_tokens")
    hit = min(100, int(cached / input_tokens * 100 + .5)) if input_tokens and cached is not None else None
    if not input_tokens:
        status = "n/a"
    elif not cached and not written:
        status = "cold"
    else:
        status = "hot" if hit is not None and hit >= 50 else "warming"
    return {"status": status, "input": input_tokens, "cached": cached, "written": written, "hit": hit}


SOURCES = {"mixed": "данные провайдера + локальная оценка", "provider": "данные провайдера", "estimated": "локальная оценка"}


def source_label(usage):
    if not usage:
        return "оценка по истории"
    return SOURCES.get(usage["source"], usage["source"])


def usage_title(usage):
    if not usage:
        return "Оценка по истории"
    return f"{usage['model_provider']}/{usage['model_name']} · {usage.get('phase') or 'request'}"


ACTIVITY = {"idle": "ожидает", "running": "выполняется", "waiting_approval": "ждёт подтверждения", "waiting_input": "ждёт ввода"}


def activity_label(activity):
    if not activity:
        return "неактивна"
    status = ACTIVITY.get(activity["status"], activity["status"])
    if activity.get("running_runs"):
        return f"{status} · запусков: {activity['running_runs']}"
    if activity.get("pending_approvals"):
        return f"{status} · подтверждения"
    if activity.get("pending_user_inputs"):
        return f"{status} · ввод"
    return status


CELLS = 200
PALETTE = ["#6b9eff", "#5cc784", "#d8a21e", "#c08cf0", "#5cc0d8", "#ef8fb7"]
CATEGORIES = {
    "instructions": ("инструкции", "#6b9eff"),
    "messages": ("история сообщений", "#5cc784"),
    "context": ("эфемерный контекст", "#c08cf0"),
    "tool_calls": ("вызовы инструментов", "#e0975c"),
    "tool_results": ("результаты инструментов", "#d8a21e"),
    "tool_schemas": ("схемы инструментов", "#5cc0d8"),
    "files": ("файлы", "#7fd4b2"),
    "patches": ("патчи", "#ef8fb7"),
}
FREE, RESERVE = "#2e2e2e", "#54383d"


def allocate(values, total=CELLS):
    """Largest remainder; a nonzero value keeps at least one cell."""
    whole = sum(values)
    if not whole:
        return [0] * len(values)
    exact = [value / whole * total for value in values]
    counts = [int(value) for value in exact]
    for index in sorted(range(len(values)), key=lambda index: exact[index] - counts[index], reverse=True)[:total - sum(counts)]:
        counts[index] += 1
    for index, value in enumerate(values):
        if value > 0 and counts[index] == 0:
            donor = max(range(len(counts)), key=counts.__getitem__)
            if counts[donor] > 1:
                counts[donor] -= 1
                counts[index] = 1
    return counts


def cells(snapshot):
    """Segments of the context window: categories scaled to the used size, then free space and reserve."""
    used, maximum, _, usage = context(snapshot)
    categories = [category for category in usage.get("categories", [])
                  if category["tokens"] > 0 and not category["name"].startswith("provider_cache")]
    total = sum(category["tokens"] for category in categories)
    parts = [(*CATEGORIES.get(category["name"], (category["name"].replace("_", " "), PALETTE[index % len(PALETTE)])),
              used * category["tokens"] / total) for index, category in enumerate(categories)] if used else []
    if not parts and used:
        parts = [("занято", PALETTE[0], used)]
    if maximum:
        trigger = usage.get("compaction_trigger_tokens")
        reserve = min(maximum - trigger if trigger and trigger < maximum else 0, max(maximum - used, 0))
        parts += [(label, color, value) for label, color, value in (
            ("свободно", FREE, max(maximum - used - reserve, 0)), ("резерв автокомпакта", RESERVE, reserve)) if value > 0]
    basis = maximum or sum(value for _, _, value in parts) or 1
    return [{"label": label, "cells": count, "color": color, "value": tokens(round(value)), "detail": f"{value / basis * 100:.1f}%"}
            for (label, color, value), count in zip(parts, allocate([value for _, _, value in parts]))]
