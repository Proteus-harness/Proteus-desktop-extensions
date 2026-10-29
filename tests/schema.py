"""Python mirror of the client's closed widget schema (src/declarative/widget.rs, control.rs, graph.rs, tree.rs, lanes.rs, list.rs).

The Rust test `shipped_processes_conform_to_the_public_ui_protocol` is authoritative;
this copy lets package tests fail fast without building the client.
"""

import re
import math
import xml.etree.ElementTree as ET

# type: (allowed fields, required fields)
NODES = {
    "stack": ({"direction", "gap", "children"}, set()),
    "fill": ({"children"}, {"children"}),
    "bar": ({"children", "border"}, {"children"}),
    "panel_editor": ({"children"}, {"children"}),
    "empty": ({"text"}, {"text"}),
    "loading": ({"text"}, {"text"}),
    "rich_text": ({"id", "html"}, {"id", "html"}),
    "content": ({"id", "language", "source"}, {"id", "language", "source"}),
    "svg": ({"source", "width", "height"}, {"source", "width", "height"}),
    "picker": ({"id", "options", "action", "placeholder", "empty"}, {"id", "options", "action"}),
    "sortable": ({"id", "items", "action"}, {"id", "items", "action"}),
    "icon": ({"name", "tone"}, {"name"}),
    "split": ({"children", "widths"}, {"children"}),
    "card": ({"title", "children"}, set()),
    "text": ({"text", "tone", "size", "mono", "single_line"}, {"text"}),
    "metric": ({"label", "value", "detail", "tone"}, {"label", "value"}),
    "progress": ({"value", "max", "label"}, {"value"}),
    "table": ({"columns", "rows"}, {"columns", "rows"}),
    "bar_chart": ({"bars", "unit"}, {"bars"}),
    "tabs": ({"tabs", "action"}, {"tabs"}),
    "list": ({"id", "rows", "action"}, {"id", "rows", "action"}),
    "button": ({"label", "action", "value", "variant", "icon", "selected", "hint", "confirm", "prompt", "menu", "disabled"},
               {"label", "action"}),
    "badge": ({"label", "tone"}, {"label"}),
    "divider": (set(), set()),
    "spacer": (set(), set()),
    "input": ({"id", "value", "placeholder", "action", "lines", "submit", "flush", "mono", "search"},
              {"id", "value", "placeholder", "action"}),
    "checkbox": ({"label", "checked", "action", "value"}, {"label", "checked", "action"}),
    "menu": ({"label", "icon", "hint", "items"}, {"items"}),
    "code": ({"id", "text", "language", "height", "wrap", "embedded"}, {"id", "text", "language"}),
    "diff": ({"id", "patch", "files", "height"}, {"id", "patch"}),
    "terminal": ({"id", "cwd"}, {"id", "cwd"}),
    "steps": ({"id", "options", "selected", "action"}, {"id", "options", "selected", "action"}),
    "select": ({"id", "label", "options", "action"}, {"id", "label", "options", "action"}),
    "tree": ({"id", "rows", "action", "indent", "chevrons", "guides"}, {"id", "rows", "action"}),
    "lanes": ({"id", "columns", "rows", "action"}, {"id", "columns", "rows", "action"}),
    "disclosure": ({"id", "label", "open", "children"}, {"id", "label", "children"}),
    "person": ({"name", "detail"}, {"name", "detail"}),
    "graph": ({"id", "nodes", "edges", "action"}, {"id", "nodes", "edges", "action"}),
    "cells": ({"columns", "segments"}, {"segments"}),
}
ENUMS = {"size": {"small", "normal", "large"},
         "direction": {"column", "row"}, "gap": {"none", "small", "medium", "large"},
         "variant": {"primary", "default", "ghost", "danger"}}
TONES = {"default", "muted", "accent", "success", "warning", "danger"}
LIST_ROW = {"id", "label", "chips", "mark", "avatar", "meta", "hint", "hint_detail", "menu"}
TAB = {"label", "count", "children"}
TREE_ROW = {"id", "label", "detail", "depth", "expanded", "tone", "selected",
            "icon", "icon_tone", "checked", "strike", "menu", "kind", "count", "stats", "hint"}
LANE_ROW = {"id", "lane", "color", "up", "down", "label", "refs", "cells", "selected", "menu", "hint"}
LANES, LANE_COLORS = 64, 8
GRAPH_NODE = {"id", "label", "subtitle", "x", "y", "tone", "selected", "dimmed"}
SEGMENT = {"label", "cells", "tone", "color", "value", "detail"}
MENU_ITEM = {"label", "action", "value", "icon", "danger", "confirm", "prompt", "separated",
             "checked", "disabled", "header"}
CONFIRM = {"title", "message", "ok", "danger"}
PROMPT = {"title", "placeholder", "value", "ok", "stem"}
ICON = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
COLOR = re.compile(r"^#[0-9a-fA-F]{6}$")


def tone(test, value):
    test.assertTrue(value in TONES or bool(COLOR.match(str(value))), value)


def icon(test, value):
    if value is not None: test.assertRegex(value, ICON, "icons are Lucide file names, e.g. git-branch")


def menu(test, items):
    for item in items:
        test.assertLessEqual(set(item), MENU_ITEM, item)
        test.assertIn("label", item)
        if not item.get("header", False): test.assertTrue(item.get("action"), item)
        test.assertIn(item.get("checked"), (None, True, False), item)
        icon(test, item.get("icon"))
        ask(test, item)


def ask(test, node):
    """`confirm` and `prompt` of a button or menu item; at most one of them."""
    confirm(test, node.get("confirm"))
    value = node.get("prompt")
    if value is not None:
        test.assertLessEqual(set(value), PROMPT, value)
        test.assertIn("title", value)
        test.assertIsNone(node.get("confirm"), "confirm and prompt cannot be combined")


def confirm(test, value):
    if value is not None:
        test.assertLessEqual(set(value), CONFIRM, value)
        test.assertIn("title", value)


def count(test, value):
    test.assertTrue(isinstance(value, int) and not isinstance(value, bool) and value >= 0, value)


def lanes(test, columns, rows):
    test.assertEqual(sum(c.get("kind") == "graph" for c in columns), 1)
    for c in columns:
        test.assertLessEqual(set(c), {"label", "kind", "weight"})
        test.assertGreater(c["weight"], 0)
        test.assertIn(c.get("kind", "text"), {"graph", "text", "person", "mono"})
    for row in rows:
        test.assertLessEqual(set(row), LANE_ROW, row)
        test.assertLessEqual({"id", "lane", "color", "label", "cells"}, set(row), row)
        test.assertEqual(len(row["cells"]), len(columns))
        segments = row.get("up", []) + row.get("down", [])
        for segment in segments:
            test.assertEqual(set(segment), {"from", "to", "color"}, segment)
        for lane in [row["lane"]] + [segment[end] for segment in segments for end in ("from", "to")]:
            count(test, lane)
            test.assertLess(lane, LANES, row)
        for color in [row["color"]] + [segment["color"] for segment in segments]:
            count(test, color)
            test.assertLess(color, LANE_COLORS, row)
        for chip in row.get("refs", []):
            test.assertLessEqual(set(chip), {"label", "tone"}, chip)
            tone(test, chip.get("tone", "default"))
        menu(test, row.get("menu", []))


def rows(test, rows):
    for row in rows:
        test.assertLessEqual(set(row), LIST_ROW, row)
        test.assertLessEqual({"id", "label"}, set(row), row)
        test.assertTrue(all(isinstance(chip, str) for chip in row.get("chips", [])), row)
        test.assertTrue(all(isinstance(part, str) for part in row.get("meta", [])), row)
        icon(test, row.get("mark"))
        menu(test, row.get("menu", []))


def children(node):
    yield from node.get("children", [])
    for tab in node.get("tabs", []):
        yield from tab["children"]
    for item in node.get("items", []) if node.get("type") == "sortable" else []:
        yield from item["children"]


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
    if kind in ("rich_text", "content"):
        test.assertIsInstance(node["id"], str)
        for field in (("html",) if kind == "rich_text" else ("language", "source")):
            test.assertIsInstance(node[field], str)
    if kind == "svg":
        test.assertLessEqual(len(node["source"].encode()), 1024 * 1024)
        try:
            root = ET.fromstring(node["source"])
        except ET.ParseError as error:
            test.fail(str(error))
        test.assertEqual(root.tag.rsplit("}", 1)[-1], "svg")
        for element in root.iter():
            test.assertNotIn(element.tag.rsplit("}", 1)[-1], ("foreignObject", "script"))
            for key, value in element.attrib.items():
                if key.rsplit("}", 1)[-1] == "href": test.assertTrue(value.lstrip().startswith("#"))
        for field in ("width", "height"):
            test.assertTrue(isinstance(node[field], (int, float)) and math.isfinite(node[field]) and 1 <= node[field] <= 4096)
    if kind == "picker":
        values = [str(option["value"]) for option in node["options"]]
        test.assertEqual(len(values), len(set(values)))
        test.assertLessEqual(sum(option.get("selected", False) for option in node["options"]), 1)
        for option in node["options"]:
            test.assertEqual(set(option), {"label", "value", "selected"})
    if kind == "sortable":
        ids = [item["id"] for item in node["items"]]
        test.assertEqual(len(ids), len(set(ids)))
        for item in node["items"]:
            test.assertEqual(set(item), {"id", "label", "children"})
            test.assertTrue(item["id"])
    if kind == "diff":
        test.assertIsInstance(node["patch"], str)
        if node.get("height") is not None:
            test.assertGreater(node["height"], 0)
        for file in node.get("files", []):
            test.assertLessEqual(set(file), {"path", "status", "message"}, file)
            test.assertIsInstance(file.get("path"), str)
    if kind in ("button", "menu"):
        icon(test, node.get("icon"))
        ask(test, node)
    if kind == "button":
        menu(test, node.get("menu", []))
    if kind == "menu":
        test.assertTrue(node["items"], node)
        menu(test, node["items"])
    if kind == "input":
        test.assertTrue(1 <= node.get("lines", 1) <= 40, node)
        if node.get("search", False): test.assertEqual(node.get("lines", 1), 1)
    if kind in ("text", "metric", "badge"):
        test.assertIsInstance(node.get("text", node.get("value", node.get("label"))), str)
    if kind == "table":
        test.assertTrue(all(len(line) == len(node["columns"]) and all(isinstance(cell, str) for cell in line)
                            for line in node["rows"]), node)
    if kind == "select":
        test.assertTrue(all(set(option) == {"label", "value", "selected"} for option in node["options"]), node)
    if kind == "list":
        rows(test, node["rows"])
    if kind == "icon":
        icon(test, node["name"])
    if kind == "tabs":
        for tab in node["tabs"]:
            test.assertLessEqual(set(tab), TAB, tab)
            if tab.get("count") is not None: count(test, tab["count"])
    if kind == "tree":
        test.assertTrue(0 <= node.get("indent", 16) <= 64, node)
        for row in node["rows"]:
            test.assertLessEqual(set(row), TREE_ROW, row)
            test.assertLessEqual({"id", "label"}, set(row), row)
            for field in ("tone", "icon_tone"):
                if field in row: tone(test, row[field])
            icon(test, row.get("icon"))
            menu(test, row.get("menu", []))
            test.assertIn(row.get("checked"), (None, True, False, "mixed"), row)
            test.assertIn(row.get("kind", "item"), ("item", "header"), row)
            if row.get("count") is not None: count(test, row["count"])
            if row.get("stats") is not None:
                test.assertEqual(set(row["stats"]), {"added", "deleted"}, row)
                for value in row["stats"].values(): count(test, value)
    if kind == "lanes":
        lanes(test, node["columns"], node["rows"])
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
