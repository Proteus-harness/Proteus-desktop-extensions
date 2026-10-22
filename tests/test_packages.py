"""Behavioral checks for moved features and standalone delivery."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "sdk"))
from proteus_extension import Extension


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


report = load("usage_report", "packages/usage/report.py")
usage_main = load("usage_main", "packages/usage/main.py")
window = load("context_window", "packages/context/window.py")
context_main = load("context_main", "packages/context/main.py")
tree = load("file_tree", "packages/files/tree.py")
analysis = load("turn_analysis", "packages/analysis/main.py")
facts = load("analysis_facts", "packages/analysis/facts.py")
summary = load("analysis_summary", "packages/analysis/summary.py")
plan = load("plan", "packages/plan/main.py")
model = load("model", "packages/model/main.py")
sys.path.insert(0, str(ROOT / "tests"))
import wire
import schema
from schema import walk as nodes


def check_tree(test, root):
    schema.check(test, root)


def flat(root):
    return json.dumps(root, ensure_ascii=False)


class PackagesTests(unittest.TestCase):
    def request(self, number, model="gpt-6-astra", **fields):
        request = {"exchange_id": f"request-{number}", "turn_id": f"turn-{number % 2}", "model": {"provider": "fixture", "model": model},
                   "origin": "direct", "started_at_ms": number * 1000, "finished_at_ms": number * 1000 + 500, "status": "completed",
                   "finish_reason": "Stop", "usage": {"input_tokens": 1000 * number, "output_tokens": 10, "cached_input_tokens": None,
                                                      "cache_creation_input_tokens": None, "reasoning_output_tokens": None},
                   "message_count": 2, "tool_count": 1, "reasoning_effort": None, "max_output_tokens": None}
        request.update(fields)
        return request

    def usage_view(self, snapshot, **state):
        worker = usage_main.Usage(); messages = []; worker.send = messages.append
        worker.session = "/one"
        worker.fetch = lambda path, query=None, active=False: {"/sessions": [], "/usage": snapshot}[path]
        for name, value in state.items(): setattr(worker, name, value)
        worker.render("page", "page", None)
        check_tree(self, messages[-1]["root"])
        return worker, messages[-1]["root"]

    def context_popup(self, snapshot):
        worker = context_main.Context(); messages = []; worker.send = messages.append
        worker.session = "/one"
        worker.api = lambda path, query=None, **options: {"/context": snapshot}[path]
        worker.render("popup", "popup", None)
        check_tree(self, messages[-1]["root"])
        return messages[-1]["root"]

    def test_unpriced_requests_explain_why(self):
        request = self.request(1)
        self.assertEqual(report.price(dict(request, usage=None)), "Нет данных о токенах")
        self.assertEqual(report.price(dict(request, model={"provider": "x", "model": "local"})), "Тариф не задан")
        broken = dict(request, usage=dict(request["usage"], cached_input_tokens=900, cache_creation_input_tokens=200))
        self.assertEqual(report.price(broken), "Некорректная разбивка токенов")
        long = dict(request, model={"provider": "x", "model": "gpt-5.6"}, usage=dict(request["usage"], input_tokens=1000000, output_tokens=1000, cached_input_tokens=500000))
        cost_in, cost_out, flag = report.price(long)
        self.assertTrue(flag)
        self.assertAlmostEqual(cost_in, (500000 * 4 + 500000 * .4) * 2 / 1e6)
        self.assertAlmostEqual(cost_out, 1000 * 20 * 1.5 / 1e6)
        self.assertAlmostEqual(report.cost(long), cost_in + cost_out)

    def test_money_keeps_two_to_six_decimals(self):
        self.assertEqual(report.money(.5), "$0.50")
        self.assertEqual(report.money(.001234), "$0.001234")
        self.assertEqual(report.money(12), "$12.00")
        self.assertEqual(report.money(.1234567), "$0.123457")

    def test_cache_status_follows_provider_counters(self):
        status = lambda actual: window.cache({"actual": actual})
        self.assertEqual(window.cache(None)["status"], "n/a")
        self.assertEqual(status({"input_tokens": 0, "output_tokens": 0})["status"], "n/a")
        self.assertEqual(status({"input_tokens": 100, "cached_input_tokens": 0, "cache_creation_input_tokens": None})["status"], "cold")
        self.assertEqual(status({"input_tokens": 100, "cached_input_tokens": None, "cache_creation_input_tokens": 50})["status"], "warming")
        self.assertEqual(status({"input_tokens": 100, "cached_input_tokens": 49})["status"], "warming")
        hot = status({"input_tokens": 200, "cached_input_tokens": 101})
        self.assertEqual((hot["status"], hot["hit"]), ("hot", 51))

    def test_search_requires_every_word_and_unknowns_sort_last(self):
        rows = [self.request(1), self.request(2, model="local", usage=None), self.request(3, status="unfinished", finished_at_ms=None)]
        self.assertEqual([row["exchange_id"] for row in report.filtered(rows, "all", "FIXTURE local", "oldest")], ["request-2"])
        self.assertEqual(report.filtered(rows, "all", "fixture nothing", "oldest"), [])
        self.assertEqual([row["exchange_id"] for row in report.filtered(rows, "all", " request-3  turn-1 ", "oldest")], ["request-3"])
        self.assertEqual([row["exchange_id"] for row in report.filtered(rows, "all", "", "tokens")], ["request-3", "request-1", "request-2"])
        self.assertEqual([row["exchange_id"] for row in report.filtered(rows, "all", "", "longest")][-1], "request-3")
        self.assertEqual([row["exchange_id"] for row in report.filtered(rows, "all", "", "cost")][-1], "request-2")

    def test_requests_tab_renders_cards_summary_and_pages(self):
        requests = [self.request(number) for number in range(1, 22)] + [self.request(22, model="local")]
        requests.append(self.request(23, model="local", usage=None, origin="auxiliary", status="error", turn_id=None))
        snapshot = {"session_id": "s", "revision": 1, "latest_turn_id": "turn-1", "requests": requests}
        worker, root = self.usage_view(snapshot, expanded={"request-23"}, order="newest")
        text = flat(root)
        for fragment in ("Итог выбранного периода", "Токены известны для 22 из 23", "Стоимость учтена для 21 из 23",
                         "Вспомогательный", "Вне хода", "Ошибка", "Нет данных о токенах",
                         "Запросы: 23 из 23", "Все модели", "Дальше", "входят в выход", "#23 · local"):
            self.assertIn(fragment, text)
        self.assertNotIn("Назад", text)
        periods = next(node for node in nodes(root) if node.get("id") == "period")["options"]
        self.assertEqual([option["label"] for option in periods][2:4], ["Ход 2", "Ход 1"])
        worker.action("page", "page", 1)
        worker.render("page", "page", None)
        self.assertIn("Назад", flat(worker.shown["page"]))
        self.assertNotIn("Дальше", flat(worker.shown["page"]))

    def test_requests_tab_empty_states_and_single_model(self):
        _, root = self.usage_view({"session_id": "s", "revision": 1, "latest_turn_id": None, "requests": []})
        self.assertIn("В этой сессии ещё нет записанных запросов.", flat(root))
        _, root = self.usage_view({"session_id": "s", "revision": 1, "latest_turn_id": None, "requests": [self.request(1)]}, query="absent")
        text = flat(root)
        self.assertIn("По этим фильтрам запросов нет.", text)
        self.assertNotIn("Все модели", text)
        self.assertNotIn("Дальше", text)

    def test_context_popup_shows_panels_instead_of_raw_json(self):
        snapshot = wire.context()
        root = self.context_popup(snapshot)
        text = flat(root)
        for fragment in ("fixture/gpt-6-astra · request", "данные провайдера + локальная оценка", "История",
                         "Кэш провайдера", "Инструменты", "Компакция", "9\u202f000 → 800", "Диагностика", "fixture warning", "warming"):
            self.assertIn(fragment, text)
        self.assertNotIn('"code"', text)
        self.assertFalse(any(node["type"] == "code" for node in nodes(root)))
        bare = {"history": {"messages": 0, "user_messages": 0, "assistant_messages": 0, "system_messages": 0, "tool_results": 0,
                            "estimated_tokens": 0}, "tools": {"requested": 0, "finished": 0, "failed": 0, "names": []}, "diagnostics": []}
        root = self.context_popup(bare)
        for fragment in ("размер окна неизвестен", "Оценка по истории", "Сжатий не было", "n/a"):
            self.assertIn(fragment, flat(root))
        self.assertNotIn("Диагностика", flat(root))

    def test_optional_usage_and_cache_cost(self):
        request = {"model":{"provider":"fixture", "model":"gpt-6-astra"}, "origin":"direct", "status":"completed",
                   "usage":{"input_tokens":10000, "output_tokens":1000, "cached_input_tokens":2000, "reasoning_output_tokens":400}}
        self.assertAlmostEqual(report.cost(request), .132)
        total = report.totals([request])
        self.assertEqual(total["reasoning_output_tokens"],400)
        self.assertIsNone(total["cache_creation_input_tokens"])
        request["usage"]["cached_input_tokens"] = 10001
        self.assertIsNone(report.cost(request))
        request["usage"] = None
        total = report.totals([request])
        self.assertEqual((total["reported"], total["priced"]), (0,0))
        self.assertIsNone(total["reasoning_output_tokens"])

    def test_context_keeps_small_nonzero_categories(self):
        segments = window.cells({"history":{"estimated_tokens":100000}, "latest_usage":{
            "max_input_tokens":200000, "compaction_trigger_tokens":180000,
            "categories":[{"name":"messages","tokens":99999},{"name":"tiny","tokens":1},
                          {"name":"provider_cache_read","tokens":50000}]}})
        self.assertEqual(sum(segment["cells"] for segment in segments),200)
        self.assertEqual([segment["label"] for segment in segments],
                         ["история сообщений","tiny","свободно","резерв автокомпакта"])
        self.assertTrue(all(segment["cells"] >= 1 for segment in segments))
        self.assertEqual(segments[3]["detail"],"10.0%")

    def test_search_retains_ancestors_and_directory_order(self):
        result={"entries":[{"path":"src/z.rs","kind":"file"},{"path":"src/a/b.py","kind":"file"},{"path":"Cargo.lock","kind":"file"}]}
        rows=tree.search_rows(result,lambda path,label,depth,directory:dict(id=path,depth=depth,expanded=False if directory else None))
        self.assertEqual([(row["id"],row["depth"]) for row in rows],[("src",0),("src/a",1),("src/a/b.py",2),("src/z.rs",1),("Cargo.lock",0)])
        self.assertTrue(rows[0]["expanded"])
        self.assertEqual(tree.language("Cargo.lock"),"toml")
        self.assertEqual(tree.language("src/X.HPP"),"cpp")

    def test_hook_failures_follow_public_contract(self):
        step={"data":{"kind":"hook","trace":{"input":{"event":{"event":"before_tool"}},"steps":[{"outcome":{"status":"failed","message":"fixture","model_failure":None}}]}}}
        self.assertEqual(analysis.title(step),"Перед инструментом")
        self.assertEqual(analysis.status(step),"Ошибка")
        step["data"]["trace"]["input"]["event"]["event"]="future_event"
        self.assertEqual(analysis.title(step),"future_event")
        model={"data":{"kind":"model","response":{"finish_reason":"Error"}}}
        self.assertEqual(analysis.status(model),"Ошибка")

    def test_turn_metrics_and_problem_filter(self):
        steps=wire.analysis()["selected"]["steps"]
        missing={"sequence":9,"started_at_ms":0,"finished_at_ms":None,"data":{"kind":"tool","call":{"name":"late"},"result":None}}
        summary=facts.metrics(steps+[missing])
        self.assertEqual((summary["models"],summary["tools"],summary["errors"],summary["missing"],summary["with_usage"]),(1,2,1,1,1))
        self.assertEqual(facts.usage_text(summary),"10\u202f000 / 1\u202f000")
        self.assertEqual(facts.usage_text(facts.metrics(steps[1:])),"нет данных")
        self.assertEqual([step["sequence"] for step in steps+[missing] if facts.matches(step,problems=True)],[2,9])
        self.assertEqual([step["sequence"] for step in steps if facts.matches(step,"tool",query=" PRO")],[3])
        self.assertEqual(facts.default_step(steps),2)
        self.assertEqual(facts.default_step(steps[2:]),3)
        self.assertIsNone(facts.default_step([]))
        self.assertEqual(facts.turn_duration(steps),"3.10 с")

    def test_step_texts_are_readable(self):
        call={"name":"probe","args":{"a":1}}
        message={"role":"Assistant","parts":[{"payload":{"Reasoning":{"text":"hidden","signature":None}}},{"payload":{"Text":{"text":" hi "}}},
                                             {"payload":{"ToolCall":{"call":call}}}]}
        self.assertEqual(facts.message_text(message),'hi\nВызов probe: {"a": 1}')
        self.assertIn('"Patch"',facts.message_text({"role":"User","parts":[{"payload":{"Patch":{"patch":{}}}}]}))
        self.assertEqual(facts.role(message),"Ассистент")
        self.assertEqual(facts.preview("a\n b "*40,12),"a b a b a b…")
        self.assertEqual(facts.finish_text({"finish_reason":"ToolCalls","end_turn":False}),"вызов инструментов · конец хода: нет")
        self.assertEqual(facts.tokens_text({"input_tokens":1500,"output_tokens":2,"cached_input_tokens":0,"cache_creation_input_tokens":None}),
                         "вход 1\u202f500 · выход 2 · из кэша 0")
        self.assertEqual(facts.compaction_tokens({"original_token_estimate":150000,"output_token_estimate":None}),"150\u202f000")
        self.assertEqual(facts.compaction_tokens({}),"нет данных")
        self.assertEqual(facts.resolution_text({"decision":"hook_blocked","reason":"guard"}),"Заблокирован хуком: guard")
        self.assertEqual(facts.tagged({"retryable":{"retry_delay_ms":5}}),"retryable")
        self.assertEqual(facts.duration(1000,None),"—")

    def test_turns_are_newest_first_and_keep_the_selection(self):
        turns=wire.analysis()["turns"]
        self.assertEqual([turn["number"] for turn in summary.turn_rows(turns,"turn-10",False)],[10,9,8,7,6,5,4,3])
        self.assertEqual([turn["number"] for turn in summary.turn_rows(turns,"turn-1",False)][-2:],[3,1])
        self.assertEqual(len(summary.turn_rows(turns,"turn-1",True)),10)

    def test_analysis_page_matches_the_closed_widget_schema(self):
        worker=analysis.Analysis();messages=[];worker.send=messages.append;worker.session="/session"
        responses={"/analysis":wire.analysis(),"/sessions":[{"session_dir":"/session","preview":"Synthetic","message_count":2}]}
        worker.fetch=lambda path,query=None,active=False: responses[path]
        worker.render("page","page",None)
        page=json.dumps(messages[-1]["root"],ensure_ascii=False)
        for expected in ("Перед инструментом","ошибка","Synthetic hook failure","Причина завершения: Synthetic turn error",
                         "версия журнала 7","usage у 1 из 1 запросов","Ошибки и без результата · 1","Полная запись","Запись 2 · execution execution-1"):
            self.assertIn(expected,page)
        for sequence,expected in ((1,"из кэша 2\u202f000"),(3,"Подтверждён пользователем"),(4,"12 → 3 · версия истории 2")):
            worker.action("page","step",sequence);worker.render("page","page",None)
            self.assertIn(expected,json.dumps(messages[-1]["root"],ensure_ascii=False))
        worker.action("page","open","1:raw");worker.action("page","step",1);worker.render("page","page",None)
        check_tree(self,messages[-1]["root"])
        self.assertIn('"id": "analysis-1:raw"',json.dumps(messages[-1]["root"]))
        worker.action("page","step",1);worker.render("page","page",None)
        self.assertNotIn("analysis-1:raw",json.dumps(messages[-1]["root"]))
        worker.action("page","problems",True);worker.render("page","page",None)
        self.assertIn("Шаги · 1 из 4",json.dumps(messages[-1]["root"],ensure_ascii=False))

    def test_failures_keep_the_last_view_and_the_process(self):
        class Flaky(Extension):
            broken=False
            def render(self, view, surface, argument):
                if self.broken: raise RuntimeError("fixture outage")
                self.show(view,{"type":"text","text":"ok"})
            def action(self, view, action, value):
                raise RuntimeError("fixture action")
        worker=Flaky();messages=[];worker.send=messages.append
        worker.handle({"type":"render","view":"page","surface":"page","session_dir":"/one"})
        worker.broken=True
        worker.handle({"type":"event","event":"run_settled","session_dir":"/one"})
        children=messages[-1]["root"]["children"]
        self.assertIn("fixture outage",json.dumps(children[0],ensure_ascii=False))
        self.assertEqual(children[1],{"type":"text","text":"ok"})
        worker.handle({"type":"action","view":"page","sequence":1,"action":"x","value":None})
        self.assertIn("fixture action",json.dumps(messages[-1],ensure_ascii=False))

    def test_switching_chats_drops_the_old_turn(self):
        worker=analysis.Analysis();worker.send=lambda message: None;worker.render=lambda *args: None
        worker.handle({"type":"event","event":"session_opened","session_dir":"/one"})
        worker.turn,worker.step="turn-1",3
        worker.handle({"type":"event","event":"session_opened","session_dir":"/two"})
        self.assertEqual((worker.turn,worker.step),(None,None))

    def test_plan_follows_incremental_updates_without_text(self):
        worker=plan.Plan()
        messages=[]
        worker.send=messages.append
        worker.handle({"type":"session_snapshot","session_dir":"/one","state":{},"entries":[]})
        worker.handle({"type":"render","view":"badge","surface":"badge","session_dir":"/one"})
        entry={"role":"assistant","tool":{"name":"update_plan","args":{"plan":[{"step":"one","status":"pending"}]},"effective_args":None}}
        worker.handle({"type":"session_update","session_dir":"/one","update":{"type":"entries","start":0,"removed":0,"entries":[entry]}})
        self.assertEqual(messages[-1]["face"]["text"],"0/1")
        worker.handle({"type":"session_update","session_dir":"/other","update":{"type":"entries","start":0,"removed":1,"entries":[]}})
        self.assertEqual(len(worker.entries),1)
        count=len(messages)
        worker.handle({"type":"event","event":"session_opened","session_dir":"/two"})
        self.assertEqual(len(messages),count)
        worker.handle({"type":"session_snapshot","session_dir":"/two","state":{},"entries":[]})
        self.assertNotIn("0/1",json.dumps(messages[-1]))

    def test_session_command_acknowledges_errors_and_changes(self):
        worker=Extension();messages=[];worker.send=messages.append
        worker.features={"session.settings.v1"};worker.session="/one";worker.state={"config":{"profile":"before"}}
        worker.set_session("reasoning_effort","effort:high")
        identity=messages[-1]["id"]
        worker.handle({"type":"command_result","id":identity,"session_dir":"/one","config":None,"error":"fixture failure"})
        self.assertFalse(worker.pending)
        self.assertEqual(worker.command_error,"fixture failure")
        worker.set_session("mode","plan")
        worker.handle({"type":"command_result","id":messages[-1]["id"],"session_dir":"/one","config":{"permission_mode":"plan"},"error":None})
        self.assertEqual(worker.state["config"]["permission_mode"],"plan")

    def test_new_select_settings_need_no_native_client_change(self):
        worker=model.Model();messages=[];worker.send=messages.append
        worker.features={"session.settings.v1"};worker.session="/one"
        worker.state={"config":{"session_settings":[
            {"id":"org.example/style","name":"Style","category":"future-category",
             "control":{"type":"select","value":"short","options":[{"value":"short","name":"Short"},{"value":"long","name":"Long"}]}},
            {"id":"future-slider","name":"Future","control":{"type":"slider","value":1}},
        ]}}
        worker.render("popup","popup",None)
        self.assertIn("setting:org.example/style",json.dumps(messages[-1]))
        self.assertNotIn("future-slider",json.dumps(messages[-1]))
        worker.action("popup","setting:org.example/style","long")
        self.assertEqual(messages[-1]["setting"],{"id":"org.example/style","value":"long"})
        worker.features.clear()
        with self.assertRaises(ValueError): worker.set_session("org.example/style","short")

    def test_every_standalone_package_runs_without_checkout_imports(self):
        with tempfile.TemporaryDirectory() as directory:
            subprocess.run([sys.executable,str(ROOT/"package.py"),"--bundle",directory],check=True)
            result=subprocess.run([sys.executable,str(ROOT/"tests/wire.py"),"--packages",directory],capture_output=True,text=True,timeout=45)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertIn("10 packages:",result.stdout)
            for package in Path(directory).iterdir():
                self.assertEqual(json.loads((package/"extension.json").read_text())["author"],"Proteus Project")


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
