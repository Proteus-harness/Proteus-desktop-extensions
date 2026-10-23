"""The Python schema mirror accepts what the client reads and rejects the rest."""

import unittest

import schema


def tree(rows, **options):
    return {"type": "tree", "id": "t", "action": "t", "rows": rows, **options}


def lane(**fields):
    return {"id": "c", "lane": 0, "color": 0, "label": "Fix", **fields}


class SchemaTest(unittest.TestCase):
    def rejects(self, node):
        with self.assertRaises(AssertionError):
            schema.check(self, node)

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
                    {"type": "lanes", "id": "log", "action": "log", "rows": [
                        lane(up=[{"from": 1, "to": 0, "color": 7}], down=[{"from": 0, "to": 0, "color": 0}],
                             refs=[{"label": "main", "tone": "accent"}], detail="Ann", time="2 h",
                             selected=True, hint="abc", menu=[{"label": "Copy", "action": "copy"}])]}]}]}]})

    def test_rejects_bad_new_fields(self):
        for node in (
            tree([{"id": "a", "label": "a", "checked": "half"}]),
            tree([{"id": "a", "label": "a", "kind": "footer"}]),
            tree([{"id": "a", "label": "a", "stats": {"added": 1}}]),
            tree([], indent=100),
            {"type": "menu", "items": [{"label": "No action"}]},
            {"type": "menu", "items": [{"label": "x", "action": "x", "confirm": {"title": "a"}, "prompt": {"title": "b"}}]},
            {"type": "button", "label": "x", "action": "x", "prompt": {"title": "b", "select": "all"}},
            {"type": "lanes", "id": "l", "action": "l", "rows": [lane(lane=64)]},
            {"type": "lanes", "id": "l", "action": "l", "rows": [lane(down=[{"from": 0, "to": 1, "color": 8}])]},
            {"type": "spacer", "size": 4},
        ):
            self.rejects(node)


if __name__ == "__main__":
    unittest.main()
