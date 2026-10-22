"""Small widget helpers of the architecture page on top of the SDK."""
from proteus_extension import stack


def heading(value):
    return {"type":"text", "text":str(value), "size":"large"}


def small(value, tone="muted"):
    return {"type":"text", "text":str(value), "tone":tone, "size":"small"}


def badge(label, tone="muted"):
    return {"type":"badge", "label":str(label), "tone":tone}


def card(title, *children):
    node = {"type":"card", "children":[child for child in children if child]}
    if title: node["title"] = str(title)
    return node


def row(*children):
    return stack(*[child for child in children if child], direction="row")


def padded(node):
    """A fill root draws cards edge to edge; a stack gets the usual inset."""
    return stack(node)
