#!/usr/bin/env python3
"""Synthetic state-only subscription cost in READY widget processes.

This emulates the host's manifest-based projection admission. It excludes GUI,
backend and driver memory; production admission is covered by Desktop tests.
No API endpoint is available and no real config/history is read.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import queue
import time

ROOT = Path(__file__).resolve().parents[3]


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def state(updated=False):
    model, mode = ("Synthetic B", "plan") if updated else ("Synthetic A", "normal")
    return dict(connected=True, context=dict(used=500 if updated else 250, max=1000), config=dict(
        profile="Synthetic", model=dict(provider="Synthetic", name=model), session_settings=[
            dict(id="model", control=dict(type="select", value=model, options=[dict(value=model, name=model)])),
            dict(id="mode", control=dict(type="select", value=mode, options=[
                dict(value="normal", name="Normal"), dict(value="plan", name="Plan")]))]))


def entries(count, payload_bytes):
    # Deliberately large tool output, not the assistant's text projection.
    return [dict(role="assistant", tool=dict(name="synthetic", args={}, result="x" * payload_bytes), subagent=None)
            for _ in range(count)]


def wait_badge(process, view):
    deadline = time.monotonic() + process.timeout
    while time.monotonic() < deadline:
        process.recorder.sample()
        if process.reader_error:
            raise RuntimeError(process.reader_error)
        try:
            _, response = process.queue.get(timeout=.05)
        except queue.Empty:
            if process.child.poll() is not None:
                raise RuntimeError("widget exited before response")
            continue
        process.event("receive", response)
        if response.get("type") == "badge" and response.get("view") == view:
            return json.dumps(response, sort_keys=True)
    raise RuntimeError("widget badge timed out")


def run(args):
    helper = ROOT / "experiments/native-renderers/bench.py"
    bench = load(helper, "projection_process")
    sampler_path = ROOT.parent / "Proteus-core/examples/performance/common/proc_sampler.py"
    module = bench.sampler(sampler_path)
    args.output.mkdir(parents=True, exist_ok=False)
    report = dict(ok=False, measurement=__doc__, policy="full" if args.full else "manifest",
                  fixture=dict(entries=args.entries, payload_bytes=args.payload_kib * 1024),
                  helper_sha256=bench.sha(helper), sampler_sha256=bench.sha(sampler_path),
                  benchmark_sha256=bench.sha(__file__), widgets=[])
    try:
        for name in ("model", "mode", "context"):
            package = args.packages / name
            manifest_path = package / "extension.json"
            manifest = json.loads(manifest_path.read_text())
            command = [str((package / manifest["command"][0]).resolve()), *manifest["command"][1:]]
            output = args.output / name
            output.mkdir()
            process = bench.Process(package, command, output, module, args.timeout)
            row = dict(id=name, binary_sha256=bench.sha(command[0]), manifest_sha256=bench.sha(manifest_path))
            report["widgets"].append(row)
            try:
                process.send(dict(type="hello", protocol=4, features=manifest.get("required_features", [])))
                process.send(dict(type="context", context="synthetic", session_dir="/synthetic/projection"))
                row["baseline"] = process.idle("baseline", args.idle)
                full = args.full or manifest.get("session_entries", True)
                message = dict(type="session_snapshot", context="synthetic", session_dir="/synthetic/projection",
                               state=state(), entries=entries(args.entries, args.payload_kib * 1024) if full else [])
                row["snapshot_encoded_bytes"] = len(json.dumps(message).encode())
                process.send(message)
                del message
                process.send(dict(type="render", context="synthetic", session_dir="/synthetic/projection", view="badge", surface="badge"))
                first = wait_badge(process, "badge")
                row["loaded"] = process.idle("snapshot-idle", args.idle)
                process.send(dict(type="session_update", context="synthetic", session_dir="/synthetic/projection",
                                  update=dict(type="state", state=state(True))))
                if wait_badge(process, "badge") == first:
                    raise RuntimeError("state update did not change widget badge")
                row["state_update_visible"] = True
                process.send(dict(type="close", view="badge"))
                process.send(dict(type="close_context", context="synthetic"))
                row["closed"] = process.idle("closed-context", args.idle)
                row["after_eof"] = process.finish()
            finally:
                process.cleanup()
                row["resources"] = process.tree.totals()
        report["ok"] = True
    finally:
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packages", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--full", action="store_true", help="Before-policy control: send full projection")
    parser.add_argument("--entries", type=int, default=8)
    parser.add_argument("--payload-kib", type=int, default=512)
    parser.add_argument("--idle", type=float, default=.5)
    parser.add_argument("--timeout", type=float, default=10)
    args = parser.parse_args()
    if not 1 <= args.entries <= 32 or not 1 <= args.payload_kib <= 512 or args.entries * args.payload_kib > 6144:
        parser.error("fixture budget: 1..32 entries, 1..512 KiB each, total <=6 MiB")
    if not .1 <= args.idle <= 10 or not .1 <= args.timeout <= 60:
        parser.error("idle .1..10s, timeout .1..60s")
    report = run(args)
    print(json.dumps(dict(ok=report["ok"], widgets=[dict(id=row["id"], loaded_pss_kib=row["loaded"]["pss_kib"],
        snapshot_bytes=row["snapshot_encoded_bytes"]) for row in report["widgets"]])))


if __name__ == "__main__":
    main()
