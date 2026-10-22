"""Widget helpers of the analysis page on top of the closed public schema."""
import json


def label(value, tone="default", size=None, mono=False):
    node = {"type":"text", "text":str(value), "tone":tone}
    if size: node["size"] = size
    if mono: node["mono"] = True
    return node


def row(*children, gap=None):
    node = {"type":"stack", "direction":"row", "children":[child for child in children if child]}
    if gap: node["gap"] = gap
    return node


def column(*children, gap=None):
    node = {"type":"stack", "direction":"column", "children":[child for child in children if child]}
    if gap: node["gap"] = gap
    return node


def card(title, *children):
    return {"type":"card", "title":title, "children":[child for child in children if child]}


def badge(value, tone="default"):
    return {"type":"badge", "label":str(value), "tone":tone}


def kv(key, value, tone="default"):
    return row(label(key, "muted"), label(value, tone))


def ghost(title, action, value=None, selected=False):
    node = {"type":"button", "label":title, "action":action, "value":value, "variant":"ghost"}
    if selected: node["selected"] = True
    return node


def height(body, wrap):
    lines = body.splitlines() or [""]
    count = sum(max(1, -(-len(line) // 110)) for line in lines) if wrap else len(lines)
    return max(48, min(360, count * 18 + 16))


def disclosure(key, title, body, is_open):
    """A heading that reveals a read-only block; JSON values are pretty-printed, text soft-wraps."""
    toggle = row(ghost(("▾ " if is_open(key) else "▸ ") + title, "open", key))
    if not is_open(key): return toggle
    wrap = isinstance(body, str)
    text = body if wrap else json.dumps(body, ensure_ascii=False, indent=2)
    text = text or "—"
    block = {"type":"code", "id":"analysis-" + key, "text":text, "language":"text" if wrap else "json",
             "height":height(text, wrap)}
    if wrap: block["wrap"] = True
    return column(toggle, block, gap="none")
