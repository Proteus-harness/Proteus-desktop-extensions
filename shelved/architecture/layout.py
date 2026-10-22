"""Column layout of the map, ported from the former native page: «Сборка»
groups slots by category, «Модули» and «Инструменты» grid three per row."""

GAP_X, GAP_Y = 224, 80
MARGIN_X, MARGIN_Y = 32, 32
SCOPES = [("assembly", "Сборка"), ("modules", "Модули"), ("tools", "Инструменты")]


def column(item):
    """Assembly column: orchestration, pipeline, tools hub, backends."""
    if item["kind"] == "config": return 0
    if item["kind"] == "tools": return 2
    if item["category"] == "orchestrator": return 0
    if item["category"] in ("backend", "post_turn"): return 3
    return 1


def layout(nodes, edges, scope, inactive):
    """Returns `(node, x, y)` triples and the edges whose both ends are placed."""
    placed = []

    def place(item, col, row):
        placed.append((item, MARGIN_X + col * GAP_X, MARGIN_Y + row * GAP_Y))

    def of(kind):
        return [item for item in nodes if item["kind"] == kind]

    def grid(items, top):
        for offset, item in enumerate(items):
            place(item, 1 + offset % 3, top + offset // 3)

    if scope == "modules":
        slots = of("slot")
        order = {item["slot"]: index for index, item in enumerate(slots)}
        names = sorted({item["slot"] for item in slots + of("module")})
        names.sort(key=lambda name: order.get(name, len(order)))
        row = 0
        for name in names:
            modules = [item for item in of("module") if item["slot"] == name and (inactive or item["active"])]
            if not modules and not inactive: continue
            slot = next((item for item in slots if item["slot"] == name), None)
            if slot: place(slot, 0, row)
            grid(modules, row)
            row += max(1, -(-len(modules) // 3)) + 0.35
    elif scope == "tools":
        for hub in of("tools")[:1]: place(hub, 0, 0)
        grid([item for item in of("tool") if inactive or item["active"]], 0)
    else:
        columns = [[], [], [], []]
        for item in nodes:
            if item["kind"] not in ("module", "tool"): columns[column(item)].append(item)
        rows = max(map(len, columns))
        for col, items in enumerate(columns):
            shift = (rows - len(items)) / 2
            for row, item in enumerate(items): place(item, col, row + shift)
    visible = {item["id"] for item, _, _ in placed}
    return placed, [edge for edge in edges if edge["from"] in visible and edge["to"] in visible]
