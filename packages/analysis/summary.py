"""Turn list and turn summary of the analysis page."""
import facts
from nodes import badge, card, column, disclosure, ghost, kv, label, row

RECENT = 8
LONGEST = 200
TASK = 600


def turn_rows(turns, selected, show_all):
    """Newest first; the recent ones unless expanded, always with the selected turn."""
    ordered = list(reversed(turns))
    shown = ordered if show_all else ordered[:RECENT]
    shown = shown[:LONGEST]
    if selected and all(turn["turn_id"] != selected for turn in shown):
        shown += [turn for turn in ordered if turn["turn_id"] == selected]
    return shown


def turn_list(turns, selected, show_all):
    nodes = []
    for turn in turn_rows(turns, selected, show_all):
        nodes.append(column(
            row(ghost(f"Ход {turn['number']}", "turn", turn["turn_id"], turn["turn_id"] == selected),
                badge(facts.TURN_STATUS.get(turn["status"], turn["status"]), facts.turn_tone(turn["status"])),
                label(facts.clock(turn["started_at_ms"]), "muted", "small")),
            label(facts.preview(turn["prompt_preview"], 180) or "Пустая задача", "default", "small"), gap="none"))
    if len(turns) > RECENT:
        hidden = "Только последние" if show_all else f"Все ходы · {len(turns)}"
        footer = [ghost(hidden, "all-turns")]
        if show_all and len(turns) > LONGEST: footer.append(label(f"Показаны последние {LONGEST}.", "muted", "small"))
        nodes.append(row(*footer))
    return card(f"Ходы · {len(turns)}", *nodes)


def turn_summary(turn, number, status, is_open):
    steps = turn["steps"]
    summary = facts.metrics(steps)
    task = turn.get("task", {}).get("text", "")
    key = lambda name: f"turn:{turn['turn_id']}:{name}"
    nodes = [row(badge(facts.TURN_STATUS.get(status, status or "—"), facts.turn_tone(status)),
                 label(f"Длительность шагов: {facts.turn_duration(steps)}", "muted", "small")),
             label(task if len(task) <= TASK else task[:TASK].rstrip() + "…")]
    if len(task) > TASK: nodes.append(disclosure(key("task"), "Полный текст задачи", task, is_open))
    if turn.get("task", {}).get("cwd"): nodes.append(kv("Каталог", turn["task"]["cwd"]))
    nodes.append(row(
        {"type":"metric", "label":"Запросы модели", "value":str(summary["models"])},
        {"type":"metric", "label":"Инструменты", "value":str(summary["tools"])},
        {"type":"metric", "label":"Ошибки шагов", "value":str(summary["errors"]), "tone":"danger" if summary["errors"] else "default"},
        {"type":"metric", "label":"Без результата", "value":str(summary["missing"]), "tone":"warning" if summary["missing"] else "default"},
        {"type":"metric", "label":"Токены: вход / выход", "value":facts.usage_text(summary),
         "detail":f"usage у {summary['with_usage']} из {summary['models']} запросов"}, gap="medium"))
    if turn.get("error"): nodes.append(label(f"Причина завершения: {turn['error']}", "danger"))
    output = (turn.get("output") or {}).get("text")
    nodes.append(disclosure(key("output"), "Итоговый ответ", output or "Итоговый ответ не записан.", is_open))
    nodes.append(disclosure(key("config"), f"Конфигурация при запуске · epoch {turn.get('module_epoch')}", turn.get("config_snapshot"), is_open))
    return card(f"Ход {number}" if number else "Ход", *nodes)
