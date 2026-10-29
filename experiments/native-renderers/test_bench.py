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
FAKE = '''import json, sys, time
if sys.argv[1] == 'no-read':
    time.sleep(10)
for line in sys.stdin:
    msg = json.loads(line)
    if msg['type'] == 'render':
        if sys.argv[1] == 'hang':
            time.sleep(10)
        print(json.dumps(dict(type='view', view=msg['view'],
            session_dir=msg['session_dir'],
            source_revision=0 if sys.argv[1] == 'stale' else msg['content']['source_revision'],
            root=dict(type='svg', source='<svg xmlns="http://www.w3.org/2000/svg"><text>fixture</text></svg>'))), flush=True)
'''


class BenchmarkTests(unittest.TestCase):
    def fixture(self, folder, mode="ok"):
        package = folder / "package"
        package.mkdir()
        (package / "fake.py").write_text(FAKE)
        (package / "extension.json").write_text(json.dumps(dict(protocol=4, id="fixture",
            command=[sys.executable, "fake.py", mode])))
        source = folder / "synthetic.mmd"
        source.write_text("flowchart LR\n A-->B\n")
        return argparse.Namespace(package=package, source=source, output=folder / "evidence",
            language="mermaid", repeats=2, idle=0.1, timeout=0.2,
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
            self.assertEqual(json.loads((args.output / "report.json").read_text())["source_sha256"],
                             BENCH.sha(args.source))
            samples = [json.loads(line) for line in (args.output / "samples.jsonl").read_text().splitlines()]
            self.assertEqual(samples[-1]["type"], "summary")
            # Evidence must not overwrite a previous run.
            with self.assertRaises(FileExistsError):
                BENCH.run(args)

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
