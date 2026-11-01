#!/usr/bin/env python3
"""Synthetic Git history retention probe. Ready package only; no Cargo or real Git.

One badge keeps its session alive while commit document previews are replaced.
All HTTP data is generated locally. Output roots are consumed then discarded;
only scalar samples and address-only protocol traces are retained by the driver.
"""
from __future__ import annotations

import argparse
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
from pathlib import Path
import queue
import threading
import time
from urllib.parse import parse_qs, urlsplit

EXTENSIONS = Path(__file__).resolve().parents[3]


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def full_hash(index):
    return f"{index:07x}" + "0" * 33


def backend(payload_bytes):
    calls = Counter()
    lock = threading.Lock()
    # One shared immutable synthetic string, no growing server-side body cache.
    patch = "diff --git a/synthetic.txt b/synthetic.txt\n--- a/synthetic.txt\n+++ b/synthetic.txt\n@@ -1 +1 @@\n-previous\n+" + "x" * payload_bytes + "\n"

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            path = urlsplit(self.path)
            query = parse_qs(path.query)
            with lock:
                calls[path.path] += 1
            if path.path == "/workspace/changes":
                data = dict(repository=True, branch="synthetic", head=dict(short="0000001", summary="Synthetic"), entries=[], ahead=0, behind=0)
            elif path.path == "/workspace/commit":
                supplied = query["hash"][0]
                index = int(supplied[:7], 16)
                data = dict(hash=full_hash(index), short=full_hash(index)[:7], summary=f"Synthetic {index}", body="", author="Synthetic", email="synthetic@invalid", time=0, parents=[], patch=patch, files=[dict(path="synthetic.txt", status="modified", added=1, deleted=1)])
            elif path.path == "/workspace/commit/diff":
                data = dict(path="synthetic.txt", kind="text", patch=patch)
            else:
                self.send_error(404)
                return
            body = json.dumps(data).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread, calls, patch


def wait_view(process, view, kind="view", sequence=None):
    deadline = time.monotonic() + process.timeout
    while time.monotonic() < deadline:
        process.recorder.sample()
        if process.reader_error:
            raise RuntimeError(process.reader_error)
        try:
            _, response = process.queue.get(timeout=.05)
        except queue.Empty:
            if process.child.poll() is not None:
                raise RuntimeError(f"Git exited {process.child.returncode}")
            continue
        process.event("receive", response)
        if response.get("type") == kind and response.get("view") == view and (sequence is None or response.get("sequence") == sequence):
            return  # Do not retain the root in the harness.
    raise RuntimeError(f"Timed out waiting for {view}/{kind}")


def run(args):
    bench = load(EXTENSIONS / "experiments/native-renderers/bench.py", "git_probe_process")
    sampler_path = EXTENSIONS.parent / "Proteus-core/examples/performance/common/proc_sampler.py"
    module = bench.sampler(sampler_path)
    package, output = args.package.resolve(), args.output.resolve()
    manifest_path = package / "extension.json"
    manifest = json.loads(manifest_path.read_text())
    if manifest["id"] != "git" or manifest["protocol"] != 4:
        raise ValueError("Expected a ready protocol-4 Git package")
    command = [(package / manifest["command"][0]).resolve(), *manifest["command"][1:]]
    output.mkdir(parents=True, exist_ok=False)
    server, thread, calls, patch = backend(args.payload_kib * 1024)
    report = dict(ok=False, measurement="Synthetic ready Git process only; loopback backend and driver excluded; warm OS cache; sampled peaks are not heap-live bytes", binary_sha256=bench.sha(command[0]), manifest_sha256=bench.sha(manifest_path), benchmark_sha256=bench.sha(__file__), process_helper_sha256=bench.sha(EXTENSIONS / "experiments/native-renderers/bench.py"), sampler_sha256=bench.sha(sampler_path), unique_commits=args.commits, patch_utf8_bytes=len(patch.encode()), points=[])
    process = None
    try:
        process = bench.Process(package, list(map(str, command)), output, module, args.timeout)
        process.send(dict(type="hello", protocol=4, features=manifest.get("required_features", []), server=dict(origin=f"http://127.0.0.1:{server.server_port}", token="synthetic-unused")))
        context = dict(context="git-probe", session_dir="/synthetic/git-session")
        process.send(dict(type="context", workspace="/synthetic/workspace", **context))
        process.send(dict(type="render", view="badge", surface="badge", **context))
        wait_view(process, "badge", "badge")
        report["baseline"] = process.idle("badge-only-baseline", args.idle)
        for index in range(1, args.commits + 1):
            hash_ = full_hash(index)
            process.send(dict(type="render", view="preview", surface="document", argument=f"Коммит {hash_[:7]}", **context))
            wait_view(process, "preview")
            # This is how graph-file previews select the per-file patch. The
            # badge action avoids an unrelated graph/log root in the harness.
            process.send(dict(type="action", view="badge", action="open_commit", value=hash_, **context))
            wait_view(process, "preview")
            process.send(dict(type="action", view="badge", action="detail_file", value=dict(id="synthetic.txt", event="preview"), **context))
            wait_view(process, "preview")
            process.send(dict(type="close", view="preview", **context))
            # Ordered badge reply acts as a barrier after close.
            barrier = f"barrier-{index}"
            process.send(dict(type="render", view=barrier, surface="badge", **context))
            wait_view(process, barrier, "badge")
            process.send(dict(type="close", view=barrier, **context))
            if index % args.sample_every == 0 or index == args.commits:
                sample = process.idle(f"closed-previews-{index}", args.idle)
                report["points"].append(dict(commits=index, sample=sample, http_calls=dict(calls)))
        report["badge_only_after_previews"] = process.idle("badge-only-after-previews", args.idle)
        process.send(dict(type="close", view="badge", **context))
        process.send(dict(type="close_context", context="git-probe"))
        report["after_close_context"] = process.idle("after-close-context", args.idle)
        report["calls_before_reopen"] = dict(calls)
        # Reopening the identical hash after closing the context must miss:
        # logical ownership can end even when malloc keeps its resident arena.
        reopened = dict(context="git-reopened", session_dir="/synthetic/git-session")
        process.send(dict(type="context", workspace="/synthetic/workspace", **reopened))
        process.send(dict(type="render", view="reopened", surface="document", argument=f"Коммит {full_hash(args.commits)[:7]}", **reopened))
        wait_view(process, "reopened")
        if calls["/workspace/commit"] != report["calls_before_reopen"].get("/workspace/commit", 0) + 1:
            raise RuntimeError("close_context retained the old commit owner")
        report["reopen_new_commit_request"] = True
        process.send(dict(type="close", view="reopened", **reopened))
        process.send(dict(type="close_context", context="git-reopened"))
        process.finish()
        report.update(ok=True, http_calls=dict(calls))
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        if process is not None:
            process.cleanup()
            report["resources"] = process.tree.totals()
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--commits", type=int, default=48)
    parser.add_argument("--payload-kib", type=int, default=384)
    parser.add_argument("--sample-every", type=int, default=8)
    parser.add_argument("--idle", type=float, default=.3)
    parser.add_argument("--timeout", type=float, default=15)
    args = parser.parse_args()
    if not 1 <= args.commits <= 64 or not 1 <= args.payload_kib <= 512 or not 1 <= args.sample_every <= args.commits or not .1 <= args.idle <= 10 or not 1 <= args.timeout <= 120:
        parser.error("bounds: commits1..64; payload1..512KiB; sample-every1..commits; idle.1..10; timeout1..120")
    report = run(args)
    print(json.dumps(dict(ok=report["ok"], baseline_pss_kib=report["baseline"]["pss_kib"], badge_only_after_previews_pss_kib=report["badge_only_after_previews"]["pss_kib"], after_close_context_pss_kib=report["after_close_context"]["pss_kib"], calls=report["http_calls"])))


if __name__ == "__main__":
    main()
