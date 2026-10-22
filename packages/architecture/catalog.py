"""«Каталог сборки»: slot cards with alternatives, then filterable tools."""
from proteus_extension import button, code, segments, text
from blocks import badge, card, heading, row, small
from topology import read_only, safety, slot_views, source

FILTERS = [("all", "Все"), ("enabled", "Включены"), ("disabled", "Отключены"),
           ("read", "Чтение"), ("mutating", "Изменение"), ("process", "Процессные")]


def keeps(kind, tool):
    working = tool["enabled"] and tool["registered"]
    if kind == "enabled": return working
    if kind == "disabled": return not working
    if kind == "read": return read_only(tool)
    if kind == "mutating": return not read_only(tool)
    if kind == "process": return bool(tool.get("owner")) or tool["source"].startswith("process")
    return True


def tool_matches(tool, query):
    query = query.strip().lower()
    return query in " ".join((tool["name"], tool.get("description") or "", tool["source"])).lower()


def slot_card(slot, active, alternatives):
    if active: status = badge("Активен", "success")
    elif slot["required"]: status = badge("Не выбран", "danger")
    else: status = badge("Отключён", "muted")
    description = ((active or {}).get("description") or "").strip() or slot.get("responsibility") or ""
    parts = [row(text(active["id"] if active else "Модуль не выбран"), badge(source(active)) if active else None),
             small(description) if description else None,
             row(status, badge("Обязательный" if slot["required"] else "Необязательный"))]
    if alternatives:
        parts += [small(f"Другие реализации: {len(alternatives)}"),
                  row(*[badge(f"{module['id']} · {source(module)}") for module in alternatives])]
    return card(f"{slot['title'] or slot['id']} · {slot['id']}", *parts)


def tool_card(tool, open_schema):
    label, tone = safety(tool)
    badges = [badge(label, tone), badge("Зарегистрирован" if tool["registered"] else "Предоставлен"),
              badge("Включён", "success") if tool["enabled"] else badge("Отключён", "warning")]
    if tool.get("runtime_managed"): badges.append(badge("Управляется рантаймом"))
    opened = open_schema == tool["name"]
    toggle = button("Скрыть схему параметров" if opened else "Схема параметров", "schema", tool["name"], variant="ghost")
    return card(tool["source"], text(tool["name"]),
                small(tool["description"]) if tool.get("description") else None,
                row(*badges), row(toggle),
                code("tool-schema-" + tool["name"], tool.get("input_schema"), 220) if opened else None)


def view(snapshot, tool_filter, query, open_schema):
    tools = [tool for tool in snapshot["tools"] if keeps(tool_filter, tool) and tool_matches(tool, query)]
    slots = [slot_card(*item) for item in slot_views(snapshot)]
    return [heading("Слоты сборки"),
            row(*slots) if slots else small("Слотов нет"),
            heading("Инструменты"),
            segments("tool-filter", FILTERS, tool_filter),
            {"type":"input", "id":"tool-query", "value":query, "placeholder":"Найти инструмент", "action":"tool-query"},
            small(f"Показано: {len(tools)} из {len(snapshot['tools'])}"),
            *([tool_card(tool, open_schema) for tool in tools] or [small("Нет инструментов под фильтр")])]
