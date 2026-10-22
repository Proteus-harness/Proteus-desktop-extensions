"""Context popup: window map, provider cache, history, tools, compaction."""

import window
from widgets import badge, card, kv, label, metric, note, row


def render(snapshot):
    used, maximum, _, usage = window.context(snapshot)
    cache = window.cache(usage)
    cache_detail, cache_tone = window.CACHE[cache["status"]]
    percent = lambda value: "n/a" if value is None else f"{value}%"
    segments = window.cells(snapshot)
    return [
        row(label(window.usage_title(usage), size="large"), badge(window.source_label(usage))),
        row(metric("Занято", window.exact(used), f"{int(used / maximum * 100 + .5)}% окна" if maximum else "размер окна неизвестен"),
            metric("Свободно", window.exact(max(maximum - used, 0)) if maximum else "n/a",
                   f"из {window.exact(maximum)}" if maximum else "max_input_tokens не задан"),
            metric("Кэш-хиты", percent(cache["hit"]), "входной кэш провайдера"),
            metric("Кэш", cache["status"], cache_detail, cache_tone), gap="medium"),
        {"type": "cells", "segments": segments} if segments else note("Для этой сессии ещё нет замеров использования"),
        row(history(snapshot), provider_cache(cache, percent), gap="medium"),
        row(tools(snapshot), compaction(snapshot), gap="medium"),
        *diagnostics(snapshot),
    ]


def history(snapshot):
    history = snapshot["history"]
    rows = [["Сообщений", history.get("messages", 0)], ["От меня", history.get("user_messages", 0)],
            ["От агента", history.get("assistant_messages", 0)], ["Результаты инструментов", history.get("tool_results", 0)],
            ["Оценка", f"{window.exact(history['estimated_tokens'])} токенов"]]
    latest = snapshot.get("latest_context")
    if latest:
        rows += [["Чанков в ходе", latest["chunks"]], ["Токенов хода", window.exact(latest.get("token_estimate"))]]
    return card("История", kv(rows))


def provider_cache(cache, percent):
    value = lambda tokens: "n/a" if tokens is None else window.exact(tokens)
    return card("Кэш провайдера", row(badge(cache["status"], window.CACHE[cache["status"]][1])), kv([
        ["Вход", value(cache["input"])], ["Из кэша", value(cache["cached"])],
        ["Записано", value(cache["written"])], ["Кэш-хиты", percent(cache["hit"])]]))


def tools(snapshot):
    summary = snapshot.get("tools") or {}
    names = summary.get("names") or []
    return card("Инструменты", kv([["Запущено", summary.get("requested", 0)], ["Завершено", summary.get("finished", 0)],
                                   ["С ошибкой", summary.get("failed", 0)]]),
                row(*[badge(name) for name in names]) if names else note("Вызовов инструментов не было"))


def compaction(snapshot):
    latest = snapshot.get("latest_compaction")
    if not latest:
        return card("Компакция", note("Сжатий не было"))
    rows = [["Статус", latest["status"]], ["Сводка", "сохранена, текст скрыт" if latest.get("summary_present") else "нет текста"]]
    result = latest.get("report")
    if result:
        before, after = result.get("original_token_estimate"), result.get("output_token_estimate")
        rows += [["Изменена", "да" if result["changed"] else "нет"],
                 ["Сообщения", f"{result['input_messages']} → {result['output_messages']}"],
                 ["Токены", f"{window.exact(before)} → {window.exact(after)}" if before is not None and after is not None
                  else window.exact(before) if before is not None else "n/a"]]
    return card("Компакция", kv(rows))


def diagnostics(snapshot):
    items = snapshot.get("diagnostics") or []
    return [card("Диагностика", *[label(item, "warning") for item in items])] if items else []
