"""Exercise the benchmark against an inert ready executable; never build Cargo."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("native_renderer_bench", ROOT / "bench.py")
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)
FAKE = '''import hashlib, json, sys, time
if sys.argv[1] == 'no-read':
    time.sleep(10)
for line in sys.stdin:
    msg = json.loads(line)
    if msg['type'] == 'render':
        if sys.argv[1] == 'hang':
            time.sleep(10)
        if sys.argv[1] == 'content-error' or sys.argv[1] == 'theme-error' and msg['content']['context']['theme']['foreground'] != '#eeeeee':
            print(json.dumps(dict(type='content_error', view=msg['view'], session_dir=msg['session_dir'],
                source_revision=msg['content']['source_revision'], error=msg['content']['source'])), flush=True)
            continue
        root = dict(type='svg', source='<svg xmlns="http://www.w3.org/2000/svg"><text>fixture</text></svg>')
        if sys.argv[1] == 'generic':
            content = msg['content']
            digest = hashlib.sha256(content['source'].encode()).hexdigest()
            root = dict(type='text', text=content['language'] + ':' + digest + ':' + content['context']['theme']['foreground'])
        print(json.dumps(dict(type='view', view=msg['view'],
            session_dir=msg['session_dir'],
            source_revision=0 if sys.argv[1] == 'stale' else msg['content']['source_revision'],
            root=root)), flush=True)
'''


class BenchmarkTests(unittest.TestCase):
    def fixture(self, folder, mode="ok"):
        package = folder / "package"
        package.mkdir()
        (package / "fake.py").write_text(FAKE)
        (package / "extension.json").write_text(json.dumps(dict(protocol=4, id="fixture", required_features=["fixture.required.v1"],
            command=[sys.executable, "fake.py", mode])))
        source = folder / "synthetic.mmd"
        source.write_text("flowchart LR\n A-->B\n")
        return argparse.Namespace(package=package, source=source, output=folder / "evidence",
            language="mermaid", root_type="svg", cycles=0, changed_source=None, repeats=2, idle=0.1, timeout=0.2,
            sampler=ROOT.parents[2] / "Proteus-core/examples/performance/common/proc_sampler.py")

    def test_two_views_cache_eof_and_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory))
            report = BENCH.run(args)
            self.assertTrue(report["ok"])
            self.assertEqual(len(report["cached"]), 2)
            self.assertEqual(report["after_eof"]["process_count"], 0)
            self.assertEqual(report["idle"]["process_count"], 1)
            self.assertTrue((args.output / "first-1.svg").is_file())
            trace = (args.output / "protocol.jsonl").read_text()
            self.assertNotIn("A-->B", trace)
            self.assertIn("fixture.required.v1", trace)
            self.assertEqual(json.loads((args.output / "report.json").read_text())["source_sha256"],
                             BENCH.sha(args.source))
            samples = [json.loads(line) for line in (args.output / "samples.jsonl").read_text().splitlines()]
            self.assertEqual(samples[-1]["type"], "summary")
            # Evidence must not overwrite a previous run.
            with self.assertRaises(FileExistsError):
                BENCH.run(args)

    def test_cycles_change_source_theme_and_close_views_before_contexts(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory), "generic")
            args.language, args.root_type, args.cycles = "markdown", "any", 2
            args.source.write_text("Synthetic markdown **fixture**")
            report = BENCH.run(args)
            self.assertEqual(len(report["cycles"]), 2)
            self.assertEqual(report["after_eof"]["process_count"], 0)
            self.assertNotIn("Synthetic markdown", json.dumps(report))
            trace = [json.loads(line) for line in (args.output / "protocol.jsonl").read_text().splitlines()]
            sent = [event["message"] for event in trace if event["direction"] == "send"]
            self.assertNotIn("Synthetic markdown", json.dumps(trace))
            for n, cycle in enumerate(report["cycles"]):
                first, second = f"cycle-{n}-first", f"cycle-{n}-second"
                left, right = f"cycle-{n}-left", f"cycle-{n}-right"
                renders = [msg for msg in sent if msg["type"] == "render" and msg["view"] == first]
                self.assertEqual(len(renders), args.repeats + 3)
                revisions = [msg["content"]["source_revision"] for msg in renders]
                self.assertEqual(revisions, sorted(set(revisions)))
                self.assertEqual(renders[-2]["content"]["context"]["theme"], BENCH.THEME)
                self.assertNotEqual(renders[-1]["content"]["context"]["theme"], BENCH.THEME)
                self.assertEqual(cycle["first"]["root_sha256"], cycle["cached"][0]["root_sha256"])
                self.assertNotEqual(cycle["source_changed"]["root_sha256"], cycle["first"]["root_sha256"])
                self.assertNotEqual(cycle["theme_changed"]["root_sha256"], cycle["source_changed"]["root_sha256"])
                self.assertGreater(cycle["theme_changed"]["source_revision"], cycle["source_changed"]["source_revision"])
                close_first = next(i for i, msg in enumerate(sent) if msg["type"] == "close" and msg["view"] == first)
                context_first = next(i for i, msg in enumerate(sent) if msg["type"] == "close_context" and msg["context"] == left)
                close_second = next(i for i, msg in enumerate(sent) if msg["type"] == "close" and msg["view"] == second)
                context_second = next(i for i, msg in enumerate(sent) if msg["type"] == "close_context" and msg["context"] == right)
                self.assertLess(close_first, context_first)
                self.assertLess(context_first, close_second)
                self.assertLess(close_second, context_second)
                root = args.output / cycle["first"]["saved_root"]
                self.assertEqual(json.loads(root.read_text())["type"], "text")
                self.assertEqual(BENCH.sha(root), cycle["first"]["root_sha256"])
                self.assertEqual(cycle["after_last_context_close"]["process_count"], 1)

    def test_json_wire_language_and_explicit_changed_source_are_supported(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory), "generic")
            args.language, args.root_type, args.cycles = "json", "any", 1
            args.source.write_text('{"type":"text","text":"first"}')
            args.changed_source = Path(directory) / "changed.json"
            args.changed_source.write_text('{"type":"text","text":"second"}')
            report = BENCH.run(args)
            self.assertEqual(report["wire_language"], "json-render")
            self.assertEqual(report["changed_source_sha256"], BENCH.sha(args.changed_source))
            self.assertEqual((args.output / "changed-source.txt").read_text(), args.changed_source.read_text())
            root = json.loads((args.output / report["first"]["saved_root"]).read_text())
            self.assertTrue(root["text"].startswith("json-render:"))

    def test_svg_default_rejects_non_svg_and_cleans_up(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory), "generic")
            with self.assertRaisesRegex(RuntimeError, "SVG root"):
                BENCH.run(args)
            report = json.loads((args.output / "report.json").read_text())
            pid = report["resources"]["per_process_vm_hwm_kib"][0]["pid"]
            self.assertFalse(Path(f"/proc/{pid}").exists())

    def test_saved_root_budget_is_bounded_and_failure_cleans_up(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory))
            with mock.patch.object(BENCH, "MAX_SAVED_ROOT_BYTES", 1):
                with self.assertRaisesRegex(RuntimeError, "bounded byte budget"):
                    BENCH.run(args)
            report = json.loads((args.output / "report.json").read_text())
            pid = report["resources"]["per_process_vm_hwm_kib"][0]["pid"]
            self.assertFalse(Path(f"/proc/{pid}").exists())
            self.assertFalse(list(args.output.glob("*.svg")))

    def test_content_errors_cannot_copy_source_into_trace_or_report(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory), "content-error")
            args.source.write_text("large-synthetic-source-marker" * 1000)
            with self.assertRaisesRegex(RuntimeError, "details redacted"):
                BENCH.run(args)
            for name in ("report.json", "protocol.jsonl"):
                self.assertNotIn("large-synthetic-source-marker", (args.output / name).read_text())

    def test_cycle_failure_after_source_update_reaps_the_process(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory), "theme-error")
            args.cycles = 2
            with self.assertRaisesRegex(RuntimeError, "details redacted"):
                BENCH.run(args)
            report = json.loads((args.output / "report.json").read_text())
            self.assertFalse(report["ok"])
            self.assertTrue(list(args.output.glob("*.svg")))
            pid = report["resources"]["per_process_vm_hwm_kib"][0]["pid"]
            self.assertFalse(Path(f"/proc/{pid}").exists())

    def test_cycle_range_is_validated_before_starting_a_process(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory))
            args.cycles = 101
            with mock.patch.object(BENCH, "Process") as process:
                with self.assertRaisesRegex(ValueError, "cycles must be"):
                    BENCH.run(args)
                process.assert_not_called()
            self.assertFalse(args.output.exists())

    def test_session_entries_are_scrubbed_without_mutating_the_message(self):
        import io
        process = BENCH.Process.__new__(BENCH.Process)
        process.trace = io.StringIO()
        entries = [{"text": "synthetic-heavy-transcript", "result": "large-result"}]
        snapshot = dict(type="session_snapshot", entries=entries)
        update = dict(type="session_update", update=dict(start=3, entries=entries))
        process.event("send", snapshot)
        process.event("send", update)
        rows = [json.loads(line)["message"] for line in process.trace.getvalue().splitlines()]
        self.assertEqual(rows[0]["entries"], dict(count=1, saved=False))
        self.assertEqual(rows[1]["update"]["entries"], dict(count=1, saved=False))
        self.assertEqual(rows[1]["update"]["start"], 3)
        self.assertNotIn("synthetic-heavy-transcript", process.trace.getvalue())
        self.assertIs(snapshot["entries"], entries)
        self.assertIs(update["update"]["entries"], entries)

    def test_stale_revision_fails_and_process_is_reaped(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory), "stale")
            with self.assertRaisesRegex(RuntimeError, "stale view"):
                BENCH.run(args)
            report = json.loads((args.output / "report.json").read_text())
            self.assertFalse(report["ok"])
            root = report["resources"]["per_process_vm_hwm_kib"][0]["pid"]
            self.assertFalse(Path(f"/proc/{root}").exists())

    def test_timeout_fails_and_process_is_reaped(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory), "hang")
            with self.assertRaisesRegex(RuntimeError, "timed out"):
                BENCH.run(args)
            report = json.loads((args.output / "report.json").read_text())
            self.assertFalse(report["ok"])
            root = report["resources"]["per_process_vm_hwm_kib"][0]["pid"]
            self.assertFalse(Path(f"/proc/{root}").exists())

    def test_versioned_node_classification_ignores_path_arguments(self):
        self.assertTrue(BENCH.javascript(dict(exe="/usr/bin/node-22", name="node")))
        self.assertFalse(BENCH.javascript(dict(exe="/bin/renderer", name="renderer",
                                              argv=["renderer", "/node/fixture"])))

    def test_nonreading_process_cannot_block_large_request_forever(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory), "no-read")
            args.source.write_text("A" * 200_000)
            with self.assertRaisesRegex(RuntimeError, "stdin timed out"):
                BENCH.run(args)
            report = json.loads((args.output / "report.json").read_text())
            self.assertFalse(report["ok"])
            root = report["resources"]["per_process_vm_hwm_kib"][0]["pid"]
            self.assertFalse(Path(f"/proc/{root}").exists())

    def test_recorder_setup_failure_rolls_back_started_process(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory), "no-read")
            args.output.mkdir()
            module = BENCH.sampler(args.sampler)
            children = []
            real_popen = BENCH.subprocess.Popen

            def spawn(*values, **options):
                child = real_popen(*values, **options)
                children.append(child)
                return child

            with mock.patch.object(BENCH.subprocess, "Popen", side_effect=spawn), \
                 mock.patch.object(module, "Recorder", side_effect=OSError("injected recorder failure")):
                with self.assertRaisesRegex(OSError, "injected recorder failure"):
                    BENCH.Process(args.package, [sys.executable, "fake.py", "no-read"],
                                  args.output, module, args.timeout)
            self.assertEqual(len(children), 1)
            child = children[0]
            self.assertIsNotNone(child.returncode)
            self.assertFalse(Path(f"/proc/{child.pid}").exists())
            self.assertTrue(all(stream.closed for stream in (child.stdin, child.stdout, child.stderr)))

    def test_trace_setup_failure_also_closes_created_recorder(self):
        with tempfile.TemporaryDirectory() as directory:
            args = self.fixture(Path(directory), "no-read")
            (args.output / "protocol.jsonl").mkdir(parents=True)
            module = BENCH.sampler(args.sampler)
            recorders = []
            real_recorder = module.Recorder

            def record(*values, **options):
                recorder = real_recorder(*values, **options)
                recorders.append(recorder)
                return recorder

            with mock.patch.object(module, "Recorder", side_effect=record):
                with self.assertRaises(IsADirectoryError):
                    BENCH.Process(args.package, [sys.executable, "fake.py", "no-read"],
                                  args.output, module, args.timeout)
            self.assertEqual(len(recorders), 1)
            self.assertTrue(recorders[0].file.closed)
            self.assertFalse(Path(f"/proc/{recorders[0].tree.root}").exists())


if __name__ == "__main__":
    unittest.main()
