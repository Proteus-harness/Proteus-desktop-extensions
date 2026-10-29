"""Synthetic evidence only: never run a renderer, Cargo or a native application."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("renderer_summary", HERE / "summarize.py")
SUMMARY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUMMARY)


class SummaryTests(unittest.TestCase):
    def setUp(self):
        root = HERE.parents[1] / "target/verification/summary-unit"
        root.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=root)
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.case = self.root / "rust-mermaid-01"
        self.case.mkdir()
        self.source = b"flowchart LR\n A-->B\n"
        self.binary = self.root / "package/bin/renderer"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(b"inert synthetic binary; never executed")
        manifest = dict(id="mermaid-rust-render", command=["bin/renderer"])
        self.manifest = self.binary.parent.parent / "extension.json"
        self.manifest.write_text(json.dumps(manifest))
        self.hashes = dict(benchmark_sha256="benchmark", sampler_sha256="sampler")
        theme = {key: 8 if key in {"radius", "radius_lg"} else "synthetic"
                 for key in SUMMARY.THEME_KEYS}
        messages = [dict(type="hello", protocol=4)]
        addresses = [("left", "first", i) for i in range(1, 5)]
        addresses += [("right", "second", 1), ("right", "second", 2)]
        messages += [dict(type="render", context=context, view=view, surface="content",
                          content=dict(language="mermaid", source_revision=revision,
                                       context=dict(theme=theme)))
                     for context, view, revision in addresses]
        messages += [dict(type="close", view=view) for view in ("first", "second")]
        self.write_jsonl("protocol.jsonl", [dict(direction="send", message=m) for m in messages])
        row = dict(pid=123, exe=str(self.binary), Pss=8, Rss=16, SwapPss=0)
        base = dict(type="sample", memory_incomplete_pids=[], process_count=1,
                    processes=[row], pss_kib=8, rss_kib=16, pss_swap_kib=8,
                    swap_pss_kib=0, scan_duration_seconds=0.001, monotonic_ns=10)
        samples = [dict(base, phase=phase) for phase in SUMMARY.PHASES for _ in range(2)]
        eof = dict(base, phase="eof", process_count=0, processes=[], pss_kib=0,
                   rss_kib=0, pss_swap_kib=0)
        resources = dict(sampled_simultaneous_peaks_kib=dict(pss_kib=8, rss_kib=16, pss_swap_kib=8),
                         per_process_vm_hwm_kib=[dict(pid=123, VmHWM=16)])
        self.events = samples + [eof, dict(type="summary", **resources)]
        self.write_jsonl("samples.jsonl", self.events)
        svg = b'<svg xmlns="http://www.w3.org/2000/svg"/>'
        reply = dict(svg_sha256=SUMMARY.digest(svg), render_ms=1, since_launch_ms=2)
        for name in [f"first-{i}.svg" for i in range(1, 5)] + ["second-1.svg", "second-2.svg"]:
            (self.case / name).write_bytes(svg)
        (self.case / "source.txt").write_bytes(self.source)
        self.report = dict(ok=True, package=manifest["id"], language="mermaid", **self.hashes,
                           source_sha256=SUMMARY.digest(self.source),
                           binary_sha256=SUMMARY.digest(self.binary.read_bytes()),
                           manifest_sha256=SUMMARY.digest(self.manifest.read_bytes()),
                           command=[str(self.binary)], resources=resources, idle=samples[0],
                           after_first_close=samples[2], after_last_close=samples[4], after_eof=eof,
                           first=reply, cached=[reply] * 3, second_session=reply,
                           javascript_processes=[], environment={
                               key: str(self.case / key.lower())
                               for key in ("HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME")})

    def write_jsonl(self, name, values):
        (self.case / name).write_text("".join(json.dumps(value) + "\n" for value in values))

    def collect(self):
        (self.case / "report.json").write_text(json.dumps(self.report))
        return SUMMARY.collect_case(self.root, self.case.name, self.source, self.hashes)

    def test_valid_evidence_exports_without_machine_paths(self):
        result, _, _, _ = self.collect()
        self.assertEqual(result["memory_phase_medians_mib"]["two-view-idle"]["pss_mib"], 8 / 1024)
        self.assertEqual(result["after_eof_process_count"], 0)
        SUMMARY.portable(result)

    def test_ambiguous_and_nonfinite_json_is_rejected(self):
        for raw in ('{"a":1,"a":2}', '{"nested":{"a":1,"a":2}}', "NaN", "Infinity", "-Infinity"):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                SUMMARY.strict(raw)
        for number in (float("nan"), float("inf"), True, -1):
            with self.subTest(number=number), self.assertRaises(ValueError):
                SUMMARY.number(number)

    def test_nested_machine_paths_are_rejected(self):
        for path in ("/var/cache/private", "evidence /home/example/log", "see /tmp/report"):
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, "machine path"):
                SUMMARY.portable(dict(note=[dict(path=path)]))
        SUMMARY.portable(dict(note=["fixtures/input.mmd", "renderer", "https://example.org/docs"]))

    def test_replaced_binary_is_rejected(self):
        self.binary.write_bytes(b"replacement")
        with self.assertRaisesRegex(ValueError, "current executable"):
            self.collect()

    def test_changed_source_is_rejected_even_with_updated_report_hash(self):
        replacement = b"flowchart LR\n C-->D\n"
        (self.case / "source.txt").write_bytes(replacement)
        self.report["source_sha256"] = SUMMARY.digest(replacement)
        with self.assertRaisesRegex(ValueError, "fixed synthetic fixture"):
            self.collect()

    def test_stale_measurement_tools_are_rejected(self):
        for key in self.hashes:
            with self.subTest(key=key):
                original = self.report[key]
                self.report[key] = "older revision"
                with self.assertRaisesRegex(ValueError, "stale " + key):
                    self.collect()
                self.report[key] = original

    def test_report_snapshot_must_exist_in_raw_samples(self):
        self.report["idle"] = dict(self.report["idle"], monotonic_ns=999)
        with self.assertRaisesRegex(ValueError, "snapshot absent"):
            self.collect()

    def test_raw_sample_memory_must_equal_its_process_rows(self):
        self.events[0] = dict(self.events[0], pss_kib=999)
        self.write_jsonl("samples.jsonl", self.events)
        with self.assertRaisesRegex(ValueError, "memory total differs"):
            self.collect()

    def test_raw_summary_must_match_report_resources(self):
        self.events[-1] = dict(self.events[-1], per_process_vm_hwm_kib=[])
        self.write_jsonl("samples.jsonl", self.events)
        with self.assertRaisesRegex(ValueError, "sampler summary differs"):
            self.collect()


if __name__ == "__main__":
    unittest.main()
