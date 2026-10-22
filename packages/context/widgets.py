"""Small widget builders of the context popup."""

from proteus_extension import stack


def label(value, tone="default", size="normal", mono=False):
    node = {"type": "text", "text": str(value), "tone": tone}
    if size != "normal": node["size"] = size
    if mono: node["mono"] = True
    return node


def note(value):
    return label(value, "muted", "small")


def row(*children, gap="small"):
    node = stack(*children, direction="row")
    if gap != "small": node["gap"] = gap
    return node


def card(title, *children):
    node = {"type": "card", "children": list(children)}
    if title is not None: node["title"] = title
    return node


def badge(value, tone="muted"):
    return {"type": "badge", "label": str(value), "tone": tone}


def metric(name, value, detail=None, tone="default"):
    node = {"type": "metric", "label": name, "value": str(value), "detail": detail}
    if tone != "default": node["tone"] = tone
    return node


def table(columns, rows):
    return {"type": "table", "columns": list(columns), "rows": [[str(cell) for cell in line] for line in rows]}


def kv(rows):
    """Key/value pairs as a two-column table."""
    return table(["Параметр", "Значение"], rows)
