"""Checks of the shelved Python architecture package (not shipped)."""
import importlib.util
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "shelved"))
sys.path.insert(0, str(ROOT / "tests"))
import schema


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def check_tree(test, root):
    schema.check(test, root)


sys.path.insert(0, str(ROOT / "shelved/architecture"))
architecture = load("architecture_main", "shelved/architecture/main.py")
import catalog as arch_catalog
import layout as arch_layout
import mapview as arch_map
import topology as arch_topology

def arch_snapshot():
    slot = lambda identity, category, order, active, required=True: {"id":identity, "title":identity.title(), "responsibility":"r " + identity,
        "active_module":active, "required":required, "category":category, "order":order}
    module = lambda identity, slot, active, description=None: {"id":identity, "slot":slot, "active":active, "source":{"kind":"builtin"},
        "version":"1.2", "api_version":"v3", "capabilities":["stream"], "description":description}
    tool = lambda name, safety, enabled=True, registered=True, owner=None: {"name":name, "description":name + " tool", "safety":safety,
        "source":"process:pack/" + name if owner else "builtin:core", "owner":owner, "enabled":enabled, "runtime_managed":False,
        "registered":registered, "input_schema":{"type":"object", "title":name}}
    return {"profile":"codex", "cwd":"/w", "config_path":None, "config_files":[], "module_epoch":4, "permission_mode":"Normal",
            "model":{"provider":"fixture", "name":"astra", "stream":True}, "hooks":["audit", "trim"],
            "slots":[slot("workflow", "orchestrator", 1, "loop"), slot("context", "pipeline", 2, None),
                     slot("hook", "pipeline", 3, "audit", False), slot("model", "backend", 4, "fake"), slot("memory", "post_turn", 5, "notes")],
            "modules":[module("loop", "workflow", True, "Main loop"), module("alt", "workflow", False), module("fake", "model", True),
                       module("audit", "hook", True), module("trim", "hook", False), module("notes", "memory", True),
                       module("a", "hook", False), module("b", "hook", False), module("c", "hook", False)],
            "tools":[tool("read", "ReadOnly"), tool("write", "WritesFiles", enabled=False),
                     tool("grep", "ReadOnly", owner={"component_id":"pack", "module_id":"grep"}), tool("ghost", "Network", registered=False)],
            "edges":[{"from":"config", "to":"slot:workflow", "kind":"selects", "label":"loop"},
                     {"from":"slot:workflow", "to":"module:workflow:loop", "kind":"active_module", "label":"active"},
                     {"from":"slot:workflow", "to":"module:workflow:alt", "kind":"available_module", "label":"available"},
                     {"from":"slot:workflow", "to":"slot:context", "kind":"runtime", "label":"builds context"},
                     {"from":"tools", "to":"tool:read", "kind":"registered_tool", "label":"enabled"},
                     {"from":"slot:workflow", "to":"ghost-node", "kind":"uses", "label":None}],
            "warnings":[{"severity":"error", "message":"context slot is empty"}]}


def text_of(root):
    return json.dumps(root, ensure_ascii=False)


class ArchitectureTests(unittest.TestCase):
    def setUp(self):
        self.snapshot = arch_snapshot()
        self.nodes = arch_topology.nodes(self.snapshot)

    def placed(self, scope, inactive=False):
        placed, edges = arch_layout.layout(self.nodes, self.snapshot["edges"], scope, inactive)
        return {item["id"]: (x, y) for item, x, y in placed}, edges

    def test_assembly_columns_by_category_without_modules(self):
        at, edges = self.placed("assembly")
        column = lambda identity: (at[identity][0] - arch_layout.MARGIN_X) // arch_layout.GAP_X
        self.assertEqual([column(identity) for identity in ("config", "slot:workflow", "slot:context", "tools", "slot:model", "slot:memory")],
                         [0, 0, 1, 2, 3, 3])
        self.assertEqual(column("ghost-node"), 1)
        self.assertFalse(any(identity.startswith(("module:", "tool:")) for identity in at))
        # Three rows in column 1 (context, hook, ghost); the single tools hub is centred.
        self.assertEqual(at["tools"][1], arch_layout.MARGIN_Y + arch_layout.GAP_Y)
        self.assertTrue(all(edge["from"] in at and edge["to"] in at for edge in edges))
        self.assertNotIn("module:workflow:loop", {edge["to"] for edge in edges})

    def test_modules_grid_three_per_row_next_to_slot(self):
        at, _ = self.placed("modules")
        self.assertNotIn("slot:context", at)
        self.assertNotIn("module:workflow:alt", at)
        self.assertEqual(at["module:workflow:loop"][1], at["slot:workflow"][1])
        at, _ = self.placed("modules", inactive=True)
        self.assertIn("slot:context", at)
        hook = at["slot:hook"][1]
        grid = [at[f"module:hook:{name}"] for name in ("audit", "trim", "a", "b", "c")]
        self.assertEqual([x for x, _ in grid], [arch_layout.MARGIN_X + column * arch_layout.GAP_X for column in (1, 2, 3, 1, 2)])
        self.assertEqual([y for _, y in grid], [hook] * 3 + [hook + arch_layout.GAP_Y] * 2)
        self.assertGreater(at["slot:model"][1], hook + arch_layout.GAP_Y)

    def test_tools_hub_and_grid(self):
        at, edges = self.placed("tools")
        self.assertEqual(set(at), {"tools", "tool:read", "tool:grep"})
        self.assertEqual(at["tools"], (arch_layout.MARGIN_X, arch_layout.MARGIN_Y))
        self.assertEqual(len(self.placed("tools", inactive=True)[0]), 5)
        self.assertEqual([(edge["from"], edge["to"]) for edge in edges], [("tools", "tool:read")])

    def test_marking_tones_and_single_meaning_dimming(self):
        by = {item["id"]: item for item in self.nodes}
        self.assertEqual(arch_map.mark(by["slot:context"], None, None, "")["tone"], "danger")
        self.assertNotEqual(arch_map.mark(by["module:model:fake"], None, None, "")["tone"], arch_map.mark(by["config"], None, None, "")["tone"])
        self.assertTrue(arch_map.mark(by["module:workflow:alt"], None, None, "")["dimmed"])
        self.assertFalse(arch_map.mark(by["module:workflow:alt"], None, None, "alt")["dimmed"])
        self.assertTrue(arch_map.mark(by["config"], None, None, "alt")["dimmed"])
        linked = arch_topology.related(self.snapshot["edges"], "slot:workflow")
        self.assertFalse(arch_map.mark(by["module:workflow:alt"], "slot:workflow", linked, "zzz")["dimmed"])
        self.assertTrue(arch_map.mark(by["tools"], "slot:workflow", linked, "")["dimmed"])
        self.assertTrue(arch_map.mark(by["slot:workflow"], "slot:workflow", linked, "")["selected"])

    def test_search_uses_fields_not_repr(self):
        by = {item["id"]: item for item in self.nodes}
        self.assertTrue(arch_topology.matches(by["module:workflow:loop"], " main LOOP "))
        self.assertTrue(arch_topology.matches(by["slot:context"], "slot:con"))
        self.assertFalse(arch_topology.matches(by["module:workflow:loop"], "stream"))
        _, _, _, found = arch_map.graph(self.nodes, self.snapshot["edges"], "assembly", False, None, "context")
        self.assertEqual([item["id"] for item in found], ["slot:context"])

    def test_details_have_facts_relations_and_schema(self):
        panel = text_of(arch_map.details(self.nodes, self.snapshot["edges"], "slot:workflow"))
        self.assertIn("Обязательный: Да", panel)
        self.assertIn("→ loop · Выбран", panel)
        self.assertIn("Доступен", panel)
        self.assertIn('"action": "goto"', panel)
        self.assertIn("Собирает контекст", panel)
        tool = text_of(arch_map.details(self.nodes, self.snapshot["edges"], "tool:read"))
        self.assertIn("Безопасность: Только чтение", tool)
        self.assertIn("map-schema-tool:read", tool)
        module = text_of(arch_map.details(self.nodes, self.snapshot["edges"], "module:workflow:loop"))
        for fact in ("Версия: 1.2", "API: v3", "Возможности: stream", "Main loop"): self.assertIn(fact, module)

    def test_catalog_filters_and_slot_states(self):
        tools = self.snapshot["tools"]
        names = lambda kind: [tool["name"] for tool in tools if arch_catalog.keeps(kind, tool)]
        self.assertEqual([names(kind) for kind, _ in arch_catalog.FILTERS],
                         [["read", "write", "grep", "ghost"], ["read", "grep"], ["write", "ghost"], ["read", "grep"], ["write", "ghost"], ["grep"]])
        text = text_of(arch_catalog.view(self.snapshot, "all", "", "read"))
        for label in ("Активен", "Не выбран", "Обязательный", "Необязательный", "Другие реализации: 1", "alt · встроенный", "tool-schema-read"):
            self.assertIn(label, text)
        self.assertIn("Показано: 1 из 4", text_of(arch_catalog.view(self.snapshot, "all", "GREP", None)))

    def test_page_renders_header_map_and_catalog(self):
        worker = architecture.Architecture(); messages = []
        worker.send = messages.append
        worker.session = "/one"; worker.snapshot = self.snapshot
        worker.render("page", "page", None)
        root = messages[-1]["root"]; check_tree(self, root)
        page = text_of(root)
        for label in ("5 слотов · 3 инструментов · 9 модулей · 1 предупреждений", "Профиль", "fixture/astra", "audit → trim",
                      "9 модулей · 4 инструментов", "1 незарегистрированных", "Предупреждения", "Ошибка"):
            self.assertIn(label, page)
        self.assertLess(page.index("Предупреждения"), page.index('"graph"'))
        worker.action("page", "select", "slot:workflow"); worker.action("page", "query", "main loop")
        worker.render("page", "page", None); check_tree(self, messages[-1]["root"])
        self.assertIn("Ничего не найдено", text_of(messages[-1]["root"]))
        worker.action("page", "scope", "modules"); worker.render("page", "page", None)
        self.assertIn("Найдено: 1", text_of(messages[-1]["root"]))
        worker.action("page", "goto", "module:workflow:loop")
        self.assertEqual(worker.selected, "module:workflow:loop")
        worker.action("page", "tab", "catalog"); worker.render("page", "page", None)
        check_tree(self, messages[-1]["root"])
        self.assertEqual(messages[-1]["root"]["type"], "stack")

    def test_mermaid_reports_the_result(self):
        worker = architecture.Architecture(); messages = []
        worker.send = messages.append
        worker.api = lambda path, text=False: "graph LR"
        worker.action("page", "mermaid", None)
        self.assertEqual((messages[-1]["type"], worker.notice), ("clipboard", "Mermaid скопирован"))
        def broken(path, text=False): raise OSError("offline")
        worker.api = broken
        worker.action("page", "mermaid", None)
        self.assertIn("offline", worker.notice)


if __name__ == "__main__": unittest.main()
