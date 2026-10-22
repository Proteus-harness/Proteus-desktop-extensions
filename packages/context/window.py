"""Context window snapshot: map segments, provider cache and labels."""

from proteus_extension import tokens


def exact(value):
    return "—" if value is None else f"{value:,}".replace(",", "\u202f")


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
