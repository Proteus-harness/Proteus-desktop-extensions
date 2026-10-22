"""«Контекст и инструменты»: window map, provider cache, history, tools, compaction."""

import report
from widgets import badge, card, kv, label, metric, note, row


def render(snapshot):
    used, maximum, _, usage = report.context(snapshot)
    cache = report.cache(usage)
    cache_detail, cache_tone = report.CACHE[cache["status"]]
    percent = lambda value: "n/a" if value is None else f"{value}%"
    segments = report.cells(snapshot)
    return [
        note("Последнее состояние сессии. Выбор хода в отчёте расхода не меняет этот снимок."),
        row(metric("Занято", report.exact(used), f"{int(used / maximum * 100 + .5)}% окна" if maximum else "размер окна неизвестен"),
            metric("Свободно", report.exact(max(maximum - used, 0)) if maximum else "n/a",
                   f"из {report.exact(maximum)}" if maximum else "max_input_tokens не задан"),
            metric("Кэш-хиты", percent(cache["hit"]), "входной кэш провайдера"),
            metric("Кэш", cache["status"], cache_detail, cache_tone), gap="medium"),
        card(report.usage_title(usage), row(badge(report.source_label(usage))),
             {"type": "cells", "segments": segments} if segments else note("Для этой сессии ещё нет замеров использования")),
        row(session(snapshot), history(snapshot), provider_cache(cache, percent), gap="medium"),
        row(tools(snapshot), compaction(snapshot), diagnostics(snapshot), gap="medium"),
    ]


def session(snapshot):
    identity = snapshot.get("session_id")
    return card("Сессия", kv([
        ["ID", identity[:8] if identity else "—"],
        ["Каталог", snapshot.get("session_dir") or "—"],
        ["Workspace", snapshot.get("workspace_path") or "—"],
        ["Активность", report.activity_label(snapshot.get("activity"))],
        ["Источник", report.source_label(snapshot.get("latest_usage"))]]))


def history(snapshot):
    history = snapshot["history"]
    rows = [["Сообщений", history.get("messages", 0)], ["От меня", history.get("user_messages", 0)],
            ["От агента", history.get("assistant_messages", 0)], ["Результаты инструментов", history.get("tool_results", 0)],
            ["Оценка", f"{report.exact(history['estimated_tokens'])} токенов"]]
    latest = snapshot.get("latest_context")
    if latest:
        rows += [["Чанков в ходе", latest["chunks"]], ["Токенов хода", report.exact(latest.get("token_estimate"))]]
    return card("История", kv(rows))


def provider_cache(cache, percent):
    value = lambda tokens: "n/a" if tokens is None else report.exact(tokens)
    return card("Кэш провайдера", row(badge(cache["status"], report.CACHE[cache["status"]][1])), kv([
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
                 ["Токены", f"{report.exact(before)} → {report.exact(after)}" if before is not None and after is not None
                  else report.exact(before) if before is not None else "n/a"]]
    return card("Компакция", kv(rows))


def diagnostics(snapshot):
    items = snapshot.get("diagnostics") or []
    return card("Диагностика", *([label(item, "warning") for item in items] or [note("Нет предупреждений")]))
