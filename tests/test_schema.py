"""The Python schema mirror accepts what the client reads and rejects the rest."""

import unittest

import schema


def tree(rows, **options):
    return {"type": "tree", "id": "t", "action": "t", "rows": rows, **options}


def lane(**fields):
    return {"id": "c", "lane": 0, "color": 0, "label": "Fix", "cells":[""], **fields}


class SchemaTest(unittest.TestCase):
    def rejects(self, node):
        with self.assertRaises(AssertionError):
            schema.check(self, node)

    def test_setting_row_keeps_child_validation_and_tree_budgets(self):
        row = {"type": "setting_row", "id": "r", "label": "Радиус",
               "description": "Enter сохраняет", "below": False,
               "control": {"type": "input", "id": "i", "value": "8", "placeholder": "0", "action": "i"}}
        schema.check(self, row)
        self.assertEqual([node["type"] for node in schema.walk(row)], ["setting_row", "input"])
        self.rejects({**row, "extra": True})
        self.rejects({**row, "control": {**row["control"], "extra": True}})
        deep = row
        for index in range(32):
            deep = {**row, "id": str(index), "control": deep}
        self.rejects(deep)
        self.rejects({**row, "control": {"type": "stack", "children": [{"type": "divider"}] * 5_000}})

    def test_setting_reset_is_closed_and_utf8_bounded(self):
        row = {"type": "setting_row", "id": "r", "label": "Radius",
               "control": {"type": "divider"}}
        reset = {"action": "reset-radius", "hint": "Default: 8 px", "value": {"radius": 8}}
        schema.check(self, {**row, "reset": reset})
        schema.check(self, {**row, "reset": {"action": "я" * 128, "hint": "я" * 2048}})
        for invalid in ["reset", {}, {"action": ""}, {"action": " \n"},
                        {"action": "я" * 129}, {"action": "reset", "hint": "я" * 2049},
                        {"action": "reset", "hint": 8}, {"action": "reset", "extra": True}]:
            with self.subTest(reset=invalid):
                self.rejects({**row, "reset": invalid})

    def test_accepts_native_rich_text_and_nested_content(self):
        schema.check(self, {"type":"rich_text", "id":"p", "html":"<h1>Привет</h1>"})
        schema.check(self, {"type":"content", "id":"c", "language":"figure", "source":"x"})
        schema.check(self, {"type":"code", "id":"c", "language":"rust", "text":"x", "embedded":True})
        self.rejects({"type":"rich_text", "id":"p", "html":"x", "markdown":True})
        self.rejects({"type":"content", "id":"c", "language":"figure"})

    def test_current_inline_typography_picker_and_transition_contracts(self):
        rich = {"type":"rich_text", "id":"p", "html":"<span data-content='m'></span>",
                "inlines":[{"id":"m", "language":"latex", "source":"x"}]}
        schema.check(self, rich)
        self.rejects({**rich, "inlines": rich["inlines"] * 2})
        schema.check(self, {"type":"stack", "transition":"detail", "children":[]})
        schema.check(self, {"type":"picker", "id":"p", "action":"choose", "options":[],
                            "title":"Models", "back_action":"back", "search":False})
        svg = {"type":"svg", "source":"<svg/>", "width":24, "height":20, "baseline":16, "em_size":16}
        schema.check(self, svg)
        self.rejects({**svg, "baseline":21})
        self.rejects({**svg, "em_size":0})

    def test_accepts_panel_primitives(self):
        schema.check(self, {"type": "fill", "children": [
            {"type": "tabs", "action": "tab", "tabs": [{"label": "Changes 2", "children": [
                {"type": "stack", "direction": "row", "children": [
                    {"type": "text", "text": "main"}, {"type": "spacer"},
                    {"type": "button", "label": "Commit", "action": "commit", "disabled": True,
                     "hint": "Nothing staged", "menu": [
                         {"label": "Mode", "header": True},
                         {"label": "Amend", "action": "amend", "checked": False},
                         {"label": "Rename", "action": "rename", "value": "a.rs",
                          "prompt": {"title": "Name", "value": "a.rs", "stem": True}}]}]},
                tree([
                    {"id": "s", "label": "Tracked", "kind": "header", "expanded": True,
                     "checked": "mixed", "count": 2},
                    {"id": "a", "label": "a.rs", "detail": "src", "checked": True,
                     "stats": {"added": 3, "deleted": 0}, "hint": "/w/src/a.rs"}],
                    indent=20, chevrons=False, guides=True)]},
                {"label": "History", "children": [
                    {"type": "lanes", "columns": [{"label":"Graph", "kind":"graph", "weight":1}], "id": "log", "action": "log", "rows": [
                        lane(up=[{"from": 1, "to": 0, "color": 7}], down=[{"from": 0, "to": 0, "color": 0}],
                             refs=[{"label": "main", "tone": "accent"}],
                             selected=True, hint="abc", menu=[{"label": "Copy", "action": "copy"}])]}]}]}]})

    def test_accepts_panel_strips_and_lists(self):
        schema.check(self, {"type": "fill", "children": [
            {"type": "tabs", "action": "tab", "tabs": [
                {"label": "Changes", "count": 2, "children": [
                    {"type": "empty", "text": "No changes"},
                    {"type": "bar", "border": False, "children": [
                        {"type": "icon", "name": "git-branch"},
                        {"type": "menu", "label": "main", "items": [{"label": "main", "action": "checkout"}]},
                        {"type": "spacer"}, {"type": "button", "label": "Fetch", "action": "fetch"}]},
                    {"type": "panel_editor", "children": [
                        {"type": "input", "id": "m", "value": "", "placeholder": "", "action": "m",
                         "lines": 6, "flush": True, "mono": True},
                        {"type": "bar", "children": [{"type": "spacer"}]}]}]},
                {"label": "History", "children": [
                    {"type": "list", "id": "log", "action": "log", "rows": [
                        {"id": "c", "label": "Fix", "chips": ["v1"], "mark": "arrow-up", "avatar": True,
                         "meta": ["Ann", "2 ч", "abc1234"], "hint": "Open", "hint_detail": "abc1234",
                         "menu": [{"label": "Copy", "action": "copy"}]}]}]}]}]})

    def test_rejects_bad_new_fields(self):
        for node in (
            {"type": "diff", "id": "d", "patch": "", "height": 0},
            {"type": "diff", "id": "d", "patch": "", "files": [{"path": "a", "colour": "red"}]},
            tree([{"id": "a", "label": "a", "checked": "half"}]),
            tree([{"id": "a", "label": "a", "kind": "footer"}]),
            tree([{"id": "a", "label": "a", "stats": {"added": 1}}]),
            tree([], indent=100),
            {"type": "menu", "items": [{"label": "No action"}]},
            {"type": "menu", "items": [{"label": "x", "action": "x", "confirm": {"title": "a"}, "prompt": {"title": "b"}}]},
            {"type": "button", "label": "x", "action": "x", "prompt": {"title": "b", "select": "all"}},
            {"type": "lanes", "columns": [{"label":"Graph", "kind":"graph", "weight":1}], "id": "l", "action": "l", "rows": [lane(lane=64)]},
            {"type": "lanes", "columns": [{"label":"Graph", "kind":"graph", "weight":1}], "id": "l", "action": "l", "rows": [lane(down=[{"from": 0, "to": 1, "color": 8}])]},
            {"type": "spacer", "size": 4},
            {"type": "bar", "children": [], "gap": 2},
            {"type": "icon", "name": "Git Branch"},
            {"type": "list", "id": "l", "action": "l", "rows": [{"id": "a", "label": "a", "time": "1"}]},
            {"type": "list", "items": [{"title": "old"}]},
            {"type": "tabs", "tabs": [{"label": "a", "count": -1}]},
        ):
            self.rejects(node)

    def test_diff_keeps_unavailable_files_and_single_line_previews(self):
        schema.check(self, {"type": "fill", "children": [
            {"type": "text", "text": "Long preview", "single_line": True},
            {"type": "diff", "id": "d", "patch": "", "files": [
                {"path": "image.png", "status": "added", "message": "Двоичный файл"}]}]})


if __name__ == "__main__":
    unittest.main()
