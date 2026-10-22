"""Usage calculations moved out of the client; pinned tariff table retained."""

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


def cost(request):
    usage = request.get("usage")
    rate = RATES.get(request["model"]["model"])
    if usage is None or rate is None:
        return None
    input_tokens = usage["input_tokens"]
    cached = usage.get("cached_input_tokens") or 0
    written = usage.get("cache_creation_input_tokens") or 0
    if cached + written > input_tokens:
        return None
    fresh = input_tokens - cached - written
    long = input_tokens > 272000
    return ((fresh * rate[0] + cached * rate[1] + written * rate[2]) * (2 if long else 1)
            + usage["output_tokens"] * rate[3] * (1.5 if long else 1)) / 1000000


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


def scoped(snapshot, period, model):
    requests = snapshot["requests"]
    if period != "all":
        turn = snapshot.get("latest_turn_id") if period == "latest" else period
        requests = [request for request in requests if request.get("turn_id") == turn]
    if model is not None:
        requests = [request for request in requests if model_key(request) == model]
    return requests


def filtered(requests, status, query, order):
    rows = [request for request in requests
            if (status != "problems" or request["status"] != "completed")
            and (status != "missing" or request.get("usage") is None)
            and query.lower() in (model_key(request) + " " + request["exchange_id"]
                                  + " " + str(request.get("turn_id") or "")).lower()]
    keys = {
        "oldest": lambda request: request["started_at_ms"],
        "newest": lambda request: -request["started_at_ms"],
        "longest": lambda request: -duration(request),
        "tokens": lambda request: -sum((request.get("usage") or {}).get(field, 0) for field in ("input_tokens","output_tokens")),
        "cost": lambda request: -(cost(request) or 0),
    }
    return sorted(rows, key=keys[order])


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


def context(snapshot):
    usage = snapshot.get("latest_usage") or {}
    actual = usage.get("actual")
    used = actual["input_tokens"] if actual else usage.get("estimated_input_tokens", snapshot["history"]["estimated_tokens"])
    maximum = usage.get("max_input_tokens")
    cached = (actual or {}).get("cached_input_tokens")
    return used, maximum, cached, usage


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
