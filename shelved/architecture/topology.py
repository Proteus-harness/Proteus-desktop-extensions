"""Presentation model of a topology snapshot: graph nodes with facts, Russian
labels and search. Edges always come from the server; nothing is inferred."""

KINDS = {"config":"ПРОФИЛЬ", "tools":"ИНСТРУМЕНТЫ", "slot":"СЛОТ", "module":"МОДУЛЬ",
         "tool":"ИНСТРУМЕНТ", "other":"УЗЕЛ"}
# The map has no "info" tone, so modules use the neutral strip to stay
# distinct from the accent profile node.
TONES = {"config":"accent", "tools":"success", "tool":"success", "module":"default",
         "slot":"warning", "other":"muted"}
CATEGORIES = {"orchestrator":"Оркестратор", "pipeline":"Конвейер", "registry":"Реестр",
              "backend":"Бэкенд", "post_turn":"После хода", "custom":"Пользовательский"}
SOURCES = {"builtin":"встроенный", "process":"процесс", "config":"конфигурация", "unknown":"неизвестно"}
SAFETY = {"readonly":("Только чтение","muted"), "writesfiles":("Запись файлов","warning"),
          "runscommands":("Команды","warning"), "network":("Сеть","warning"),
          "dangerous":("Опасный","danger")}
EDGE_KINDS = {"selects":"Выбор", "active_module":"Выбран", "available_module":"Доступен",
              "runtime":"Используется", "registered_tool":"Зарегистрирован",
              "unregistered_tool":"Не зарегистрирован", "enables":"Включает", "uses":"Использует",
              "ordered_hook":"Hook", "configures":"Настраивает", "contains":"Содержит"}
EDGE_LABELS = {"active":"Выбран", "available":"Доступен", "enabled":"Включён",
               "registered":"Зарегистрирован", "enabled but not registered":"Включён, но не зарегистрирован",
               "provided but disabled":"Предоставлен, но отключён", "builds context":"Собирает контекст",
               "typed contributions":"Типизированные вклады", "model call":"Вызов модели",
               "approval gate":"Проверка одобрения", "tool selection":"Выбор инструментов",
               "execution policy":"Политика выполнения"}


def yes(value):
    return "Да" if value else "Нет"


def source(module):
    kind = (module.get("source") or {}).get("kind", "unknown")
    return SOURCES.get(kind, kind)


def safety(tool):
    """Russian label and tone; the server reports names like `ReadOnly`."""
    raw = tool.get("safety") or ""
    return SAFETY.get(raw.replace("_", "").lower(), (raw or "Не указано", "muted"))


def read_only(tool):
    return (tool.get("safety") or "").replace("_", "").lower() == "readonly"


def tool_state(tool):
    if not tool["registered"]: return "Не зарегистрирован"
    return "Включён" if tool["enabled"] else "Отключён"


def node(identity, kind, label, **fields):
    base = {"id":identity, "kind":kind, "label":label, "subtitle":"", "description":"", "facts":[],
            "category":"", "slot":"", "active":True, "missing":False, "schema":None}
    base.update(fields)
    return base


def nodes(snapshot):
    model = snapshot.get("model")
    config = [("Каталог", snapshot["cwd"]), ("Файл", snapshot.get("config_path") or "Автоматический выбор"),
              ("Ревизия", str(snapshot["module_epoch"])), ("Доступ", snapshot["permission_mode"]),
              ("Модель", f"{model['provider']}/{model['name']}" if model else "Не выбрана")]
    config += [("Источник", path) for path in snapshot.get("config_files", [])]
    tools = snapshot["tools"]
    result = [node("config", "config", snapshot["profile"] or "Профиль", subtitle="Конфигурация",
                   description="Текущая сборка агента", facts=config),
              node("tools", "tools", "Инструменты", subtitle=f"{sum(tool['registered'] for tool in tools)} зарегистрировано",
                   description="Инструменты текущей сборки")]
    for slot in sorted(snapshot["slots"], key=lambda slot: (slot["order"], slot["id"])):
        active = slot.get("active_module")
        chosen = active or "Модуль не выбран"
        result.append(node("slot:" + slot["id"], "slot", slot["title"] or slot["id"], subtitle=chosen,
                           description=slot.get("responsibility") or "", category=slot["category"], slot=slot["id"],
                           facts=[("Слот", slot["id"]), ("Категория", CATEGORIES.get(slot["category"], slot["category"])),
                                  ("Обязательный", yes(slot["required"])), ("Выбранный модуль", chosen)],
                           active=active is not None, missing=slot["required"] and active is None))
    for module in snapshot["modules"]:
        capabilities = ", ".join(module.get("capabilities") or []) or "Не указаны"
        result.append(node(f"module:{module['slot']}:{module['id']}", "module", module["id"],
                           subtitle=module["slot"] + (" · выбран" if module["active"] else " · доступен"),
                           description=module.get("description") or "", slot=module["slot"], active=module["active"],
                           facts=[("Слот", module["slot"]), ("Источник", source(module)), ("Версия", module["version"]),
                                  ("API", module["api_version"]), ("Возможности", capabilities)]))
    for tool in tools:
        owner = tool.get("owner")
        facts = [("Источник", tool["source"]), ("Безопасность", safety(tool)[0]),
                 ("Регистрация", yes(tool["registered"])), ("Включён", yes(tool["enabled"]))]
        if owner: facts.insert(1, ("Владелец", f"{owner['component_id']}/{owner['module_id']}"))
        if tool.get("runtime_managed"): facts.append(("Управляется рантаймом", "Да"))
        result.append(node("tool:" + tool["name"], "tool", tool["name"], subtitle=tool_state(tool),
                           description=tool.get("description") or "", facts=facts,
                           active=tool["registered"] and tool["enabled"], schema=tool.get("input_schema")))
    known = {item["id"] for item in result}
    for edge in snapshot["edges"]:
        for identity in (edge["from"], edge["to"]):
            if identity not in known:
                known.add(identity)
                result.append(node(identity, "other", identity, subtitle="Связь из snapshot",
                                   description="Узел без подробного описания в текущем snapshot."))
    return result


def matches(item, query):
    query = query.strip().lower()
    return query in " ".join((item["label"], item["subtitle"], item["id"], item["description"])).lower()


def related(edges, selected):
    result = {selected}
    for edge in edges:
        if edge["from"] == selected: result.add(edge["to"])
        if edge["to"] == selected: result.add(edge["from"])
    return result


def edge_label(edge):
    label, kind = edge.get("label"), edge["kind"]
    if label in EDGE_LABELS: return EDGE_LABELS[label]
    title = EDGE_KINDS.get(kind, kind)
    if label and kind == "ordered_hook": return f"Hook · порядок {label}"
    return f"{title} · {label}" if label else title


def slot_views(snapshot):
    """Slots in display order with the active module and sorted alternatives."""
    views = []
    for slot in sorted(snapshot["slots"], key=lambda slot: (slot["order"], slot["id"])):
        active = next((module for module in snapshot["modules"]
                       if module["slot"] == slot["id"] and module["id"] == slot.get("active_module")), None)
        alternatives = sorted((module for module in snapshot["modules"] if module["slot"] == slot["id"] and not module["active"]),
                              key=lambda module: module["id"])
        views.append((slot, active, alternatives))
    return views
