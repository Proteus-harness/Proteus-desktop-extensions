"""«Карта связей»: toolbar, graph widget and details of the selected node."""
from proteus_extension import button, code, segments, text
from blocks import badge, card, heading, padded, row, small
from layout import SCOPES, layout
from topology import KINDS, TONES, edge_label, matches, related


def mark(item, selected, linked, query):
    """Graph node fields. `dimmed` carries one meaning at a time: unrelated
    to the selection, else not matching the search, else inactive."""
    if linked is not None: dimmed = item["id"] not in linked
    elif query.strip(): dimmed = not matches(item, query)
    else: dimmed = not item["active"]
    return {"tone":"danger" if item["missing"] else TONES[item["kind"]],
            "selected":item["id"] == selected, "dimmed":dimmed}


def graph(nodes, edges, scope, inactive, selected, query):
    placed, visible = layout(nodes, edges, scope, inactive)
    linked = related(edges, selected) if selected else None
    widget_nodes = [{"id":item["id"], "label":item["label"], "subtitle":item["subtitle"], "x":x, "y":y,
                     **mark(item, selected, linked, query)} for item, x, y in placed]
    pairs = list(dict.fromkeys((edge["from"], edge["to"]) for edge in visible))
    found = [item for item, _, _ in placed if matches(item, query)] if query.strip() else None
    widget = {"type":"graph", "id":f"topology-{scope}-{int(inactive)}", "nodes":widget_nodes,
              "edges":[{"from":start, "to":end} for start, end in pairs], "action":"select"}
    return widget, len(placed), len(pairs), found


def links(edges, nodes, selected):
    labels = {item["id"]: item["label"] for item in nodes}
    items = []
    for edge in edges:
        if edge["from"] == selected: arrow, other = "→", edge["to"]
        elif edge["to"] == selected: arrow, other = "←", edge["from"]
        else: continue
        items.append({"title":f"{arrow} {labels.get(other, other)} · {edge_label(edge)}", "detail":other,
                      "action":"goto", "value":other})
    return items


def details(nodes, edges, selected):
    item = next((item for item in nodes if item["id"] == selected), None)
    if item is None: return None
    about = card(KINDS[item["kind"]], heading(item["label"]), small(item["subtitle"]),
                 text(item["description"]) if item["description"] else None,
                 *[text(f"{label}: {value}") for label, value in item["facts"]],
                 badge("Обязательный слот без модуля", "danger") if item["missing"] else None)
    items = links(edges, nodes, selected)
    relations = card(f"Связи · {len(items)}", {"type":"list", "items":items} if items else small("Связей нет"))
    parts = [about, relations]
    if item["schema"] is not None:
        parts.append(card("Параметры инструмента", code("map-schema-" + item["id"], item["schema"], 180)))
    return {"type":"stack", "gap":"medium", "children":parts}


def toolbar(scope, inactive, query, found):
    controls = [segments("scope", SCOPES, scope)]
    if scope != "assembly":
        controls.append(button("Неактивные", "inactive", not inactive, variant="ghost", selected=inactive,
                               hint="Показать недоступные модули и отключённые инструменты"))
    status = None
    if found is not None:
        status = text(f"Найдено: {len(found)}" if found else "Ничего не найдено", "muted")
    return [row(*controls),
            {"type":"input", "id":"query", "value":query, "placeholder":"Найти на карте…", "action":"query"},
            status]


def view(snapshot, nodes, scope, inactive, selected, query):
    widget, count, links_count, found = graph(nodes, snapshot["edges"], scope, inactive, selected, query)
    hint = ("Нажмите узел ещё раз, чтобы снять выбор." if selected else
            f"Выберите объект · {count} объектов · {links_count} связей · колесо — масштаб, перетаскивание — сдвиг")
    body = [part for part in toolbar(scope, inactive, query, found) if part]
    canvas = {"type":"fill", "children":[widget, padded(small(hint))]}
    panel = details(nodes, snapshot["edges"], selected) if selected else None
    if panel is None: return body + canvas["children"]
    # Details stand to the right of the map, as in the native page.
    return body + [{"type":"split", "widths":[None, 320], "children":[canvas, panel]}]
