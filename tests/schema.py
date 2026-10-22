"""Python mirror of the client's closed widget schema (src/declarative/widget.rs, graph.rs, tree.rs).

The Rust test `shipped_processes_conform_to_the_public_ui_protocol` is authoritative;
this copy lets package tests fail fast without building the client.
"""

import re

# type: (allowed fields, required fields)
NODES = {
    "stack": ({"direction", "gap", "children"}, set()),
    "fill": ({"children"}, {"children"}),
    "split": ({"children", "widths"}, {"children"}),
    "card": ({"title", "children"}, set()),
    "text": ({"text", "tone", "size", "mono"}, {"text"}),
    "metric": ({"label", "value", "detail", "tone"}, {"label", "value"}),
    "progress": ({"value", "max", "label"}, {"value"}),
    "table": ({"columns", "rows"}, {"columns", "rows"}),
    "bar_chart": ({"bars", "unit"}, {"bars"}),
    "tabs": ({"tabs"}, {"tabs"}),
    "list": ({"items"}, {"items"}),
    "button": ({"label", "action", "value", "variant", "icon", "selected", "hint", "confirm"}, {"label", "action"}),
    "badge": ({"label", "tone"}, {"label"}),
    "divider": (set(), set()),
    "input": ({"id", "value", "placeholder", "action", "lines", "submit"}, {"id", "value", "placeholder", "action"}),
    "checkbox": ({"label", "checked", "action", "value"}, {"label", "checked", "action"}),
    "menu": ({"label", "icon", "hint", "items"}, {"items"}),
    "code": ({"id", "text", "language", "height", "wrap"}, {"id", "text", "language"}),
    "terminal": ({"id", "cwd"}, {"id", "cwd"}),
    "choice": ({"label", "selected", "action", "value"}, {"label", "selected", "action", "value"}),
    "steps": ({"id", "options", "selected", "action"}, {"id", "options", "selected", "action"}),
    "select": ({"id", "label", "options", "action"}, {"id", "label", "options", "action"}),
    "tree": ({"id", "rows", "action"}, {"id", "rows", "action"}),
    "graph": ({"id", "nodes", "edges", "action"}, {"id", "nodes", "edges", "action"}),
    "cells": ({"columns", "segments"}, {"segments"}),
}
ENUMS = {"size": {"small", "normal", "large"},
         "direction": {"column", "row"}, "gap": {"none", "small", "medium", "large"},
         "variant": {"primary", "default", "ghost", "danger"}}
TONES = {"default", "muted", "accent", "success", "warning", "danger"}
LIST_ITEM = {"title", "detail", "action", "value"}
TREE_ROW = {"id", "label", "detail", "depth", "expanded", "tone", "selected",
            "icon", "icon_tone", "checked", "strike", "menu"}
GRAPH_NODE = {"id", "label", "subtitle", "x", "y", "tone", "selected", "dimmed"}
SEGMENT = {"label", "cells", "tone", "color", "value", "detail"}
MENU_ITEM = {"label", "action", "value", "icon", "danger", "confirm", "separated"}
CONFIRM = {"title", "message", "ok", "danger"}
ICON = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
COLOR = re.compile(r"^#[0-9a-fA-F]{6}$")


def tone(test, value):
    test.assertTrue(value in TONES or bool(COLOR.match(str(value))), value)


def icon(test, value):
    if value is not None: test.assertRegex(value, ICON, "icons are Lucide file names, e.g. git-branch")


def menu(test, items):
    for item in items:
        test.assertLessEqual(set(item), MENU_ITEM, item)
        test.assertLessEqual({"label", "action"}, set(item), item)
        icon(test, item.get("icon"))
        confirm(test, item.get("confirm"))


def confirm(test, value):
    if value is not None:
        test.assertLessEqual(set(value), CONFIRM, value)
        test.assertIn("title", value)


def children(node):
    yield from node.get("children", [])
    for tab in node.get("tabs", []):
        yield from tab["children"]


def walk(node):
    yield node
    for child in children(node):
        yield from walk(child)


def check(test, node, depth=0):
    test.assertLessEqual(depth, 32)
    test.assertIn(node.get("type"), NODES, node)
    allowed, required = NODES[node["type"]]
    fields = set(node) - {"type"}
    test.assertLessEqual(fields, allowed, node)
    test.assertLessEqual(required, fields, node)
    for field, values in ENUMS.items():
        if field in node: test.assertIn(node[field], values, node)
    if "tone" in node: tone(test, node["tone"])
    kind = node["type"]
    if kind in ("button", "menu"):
        icon(test, node.get("icon"))
        confirm(test, node.get("confirm"))
    if kind == "menu":
        test.assertTrue(node["items"], node)
        menu(test, node["items"])
    if kind == "input":
        test.assertTrue(1 <= node.get("lines", 1) <= 40, node)
    if kind in ("text", "metric", "badge"):
        test.assertIsInstance(node.get("text", node.get("value", node.get("label"))), str)
    if kind == "table":
        test.assertTrue(all(len(line) == len(node["columns"]) and all(isinstance(cell, str) for cell in line)
                            for line in node["rows"]), node)
    if kind == "select":
        test.assertTrue(all(set(option) == {"label", "value", "selected"} for option in node["options"]), node)
    if kind == "list":
        test.assertTrue(all(set(item) <= LIST_ITEM and "title" in item for item in node["items"]), node)
    if kind == "tree":
        for row in node["rows"]:
            test.assertLessEqual(set(row), TREE_ROW, row)
            test.assertLessEqual({"id", "label"}, set(row), row)
            for field in ("tone", "icon_tone"):
                if field in row: tone(test, row[field])
            icon(test, row.get("icon"))
            menu(test, row.get("menu", []))
    if kind == "cells":
        test.assertTrue(all(set(segment) <= SEGMENT for segment in node["segments"]), node)
    if kind == "split":
        widths = node.get("widths", [])
        test.assertLessEqual(len(widths), len(node["children"]))
        test.assertTrue(all(width is None or width > 0 for width in widths), node)
    if kind == "graph":
        ids = {item["id"] for item in node["nodes"]}
        test.assertEqual(len(ids), len(node["nodes"]))
        for item in node["nodes"]:
            test.assertLessEqual(set(item), GRAPH_NODE, item)
            tone(test, item.get("tone", "default"))
        test.assertTrue(all(set(edge) == {"from", "to"} and {edge["from"], edge["to"]} <= ids for edge in node["edges"]))
    for child in children(node):
        check(test, child, depth + 1)
