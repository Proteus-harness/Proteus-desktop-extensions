#!/usr/bin/env python3
"""Protocol-4 smoke and Linux memory probe of a READY renderer package.

No Cargo, GUI or backend is started. Inputs must be synthetic: the source and
returned SVGs are saved in the new output directory. Example:
  python3 experiments/native-renderers/bench.py --package /path/to/package \
    --source diagram.mmd --language mermaid --output /path/to/new-evidence
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import select
import shutil
import signal
import subprocess
import threading
import time


THEME = dict(foreground="#eeeeee", background="#111111", muted="#999999",
             border="#555555", accent="#99aaff", danger="#ff9999",
             surface="#222222", surface_alt="#333333",
             font_family="sans-serif", font_family_mono="monospace", radius=8, radius_lg=16)
MAX_LINE = 8 * 1024 * 1024


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def sampler(path):
    spec = importlib.util.spec_from_file_location("renderer_proc_sampler", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def javascript(row):
    # Versioned Fedora executables (node-22) still have comm=node. Only the
    # executable basename / comm are considered, never arbitrary argv text.
    names = {Path(row.get("exe", "")).name.lower(), row.get("name", "").lower()}
    return any(name in {"node", "nodejs", "bun", "deno", "qjs", "quickjs", "js"}
               or (name.startswith("node-") and name[5:].replace(".", "").isdigit())
               for name in names)


class Process:
    def __init__(self, package, command, output, module, timeout):
        self.started = time.monotonic()
        self.module = module
        self.tree = self.recorder = self.trace = None
        self.readers = []
        self.child = subprocess.Popen(command, cwd=package, stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                      start_new_session=True)
        try:
            self.tree = module.Tree(self.child.pid, root_group="extensions",
                                    classifier=lambda _: "extensions")
            self.recorder = module.Recorder(self.tree, output / "samples.jsonl")
            self.trace = (output / "protocol.jsonl").open("w", encoding="utf-8")
            self.queue = queue.Queue(maxsize=32)
            self.timeout = timeout
            self.readers = [threading.Thread(target=self.read_stdout, daemon=True),
                            threading.Thread(target=self.read_stderr, args=(output,), daemon=True)]
            self.reader_error = None
            os.set_blocking(self.child.stdin.fileno(), False)
            for reader in self.readers:
                reader.start()
        except BaseException:
            # The caller cannot enter its finally block until construction
            # succeeds. Roll back even a partial Tree/Recorder/trace setup here.
            self.cleanup()
            raise

    def read_stdout(self):
        try:
            total = 0
            while line := self.child.stdout.readline(MAX_LINE + 1):
                total += len(line)
                if len(line) > MAX_LINE or not line.endswith(b"\n") or total > 128 * 1024 * 1024:
                    raise RuntimeError("renderer stdout exceeds bounded JSONL budget")
                self.queue.put_nowait((time.monotonic(), json.loads(line)))
        except Exception as error:
            self.reader_error = f"stdout: {error}"

    def read_stderr(self, output):
        try:
            with (output / "stderr.log").open("wb") as stream:
                remaining = 256 * 1024
                while block := self.child.stderr.read(4096):
                    if remaining:
                        stream.write(block[:remaining])
                        remaining = max(0, remaining - len(block))
                    # Continue draining after the saved-log bound to avoid a
                    # blocked renderer; excessive stderr cannot grow evidence.
        except OSError:
            pass

    def event(self, direction, message):
        safe = dict(message)
        if "content" in safe:
            safe["content"] = {k: v for k, v in safe["content"].items() if k != "source"}
        if "root" in safe:
            safe["root"] = {"type": safe["root"].get("type"), "saved_separately": True}
        self.trace.write(json.dumps(dict(direction=direction, message=safe,
                                        monotonic=time.monotonic()), ensure_ascii=False) + "\n")
        self.trace.flush()

    def send(self, message):
        if self.child.poll() is not None:
            raise RuntimeError(f"renderer exited: {self.child.returncode}")
        self.event("send", message)
        payload = memoryview((json.dumps(message, ensure_ascii=False) + "\n").encode())
        deadline = time.monotonic() + self.timeout
        while payload:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([], [self.child.stdin], [], max(0, remaining))[1]:
                raise RuntimeError("renderer stdin timed out")
            try:
                payload = payload[os.write(self.child.stdin.fileno(), payload):]
            except BlockingIOError:
                continue

    def idle(self, phase, duration):
        self.recorder.mark(phase)
        deadline = time.monotonic() + duration
        while True:
            sample = self.recorder.sample()
            if time.monotonic() >= deadline:
                return sample
            time.sleep(min(0.05, max(0, deadline - time.monotonic())))

    def render(self, context, session, view, source, language, revision, output):
        self.recorder.mark(f"render-{view}-{revision}")
        started = time.monotonic()
        self.send(dict(type="render", context=context, session_dir=session, view=view,
                       surface="content", content=dict(language=language, source=source,
                       source_revision=revision, context=dict(theme=THEME))))
        deadline = started + self.timeout
        while time.monotonic() < deadline:
            self.recorder.sample()
            if self.reader_error:
                raise RuntimeError(self.reader_error)
            try:
                received, response = self.queue.get(timeout=0.05)
            except queue.Empty:
                if self.child.poll() is not None:
                    raise RuntimeError(f"renderer exited before view: {self.child.returncode}")
                continue
            self.event("receive", response)
            if response.get("type") not in {"view", "content_error"}:
                continue  # Some SDKs acknowledge hello; others do not.
            if (response.get("view"), response.get("session_dir"), response.get("source_revision")) != (view, session, revision):
                raise RuntimeError("renderer returned an incorrectly addressed/stale view")
            if response["type"] == "content_error":
                raise RuntimeError(f"content_error: {response.get('error')}")
            root = response.get("root", {})
            if root.get("type") != "svg" or "<svg" not in root.get("source", ""):
                raise RuntimeError("renderer did not return an SVG root")
            svg = root["source"]
            (output / f"{view}-{revision}.svg").write_text(svg, encoding="utf-8")
            return dict(render_ms=(received - started) * 1000,
                        since_launch_ms=(received - self.started) * 1000,
                        svg_sha256=hashlib.sha256(svg.encode()).hexdigest())
        raise RuntimeError(f"view timed out after {self.timeout}s")

    def finish(self):
        self.child.stdin.close()
        deadline = time.monotonic() + self.timeout
        while time.monotonic() < deadline:
            sample = self.idle("eof-cleanup", 0)
            if self.child.poll() is not None and not sample["process_count"] and not self.group_members():
                if self.child.returncode != 0:
                    raise RuntimeError(f"renderer exited {self.child.returncode}")
                return sample
            time.sleep(0.05)
        raise RuntimeError("renderer or its observed children survived EOF")

    def group_members(self):
        members = []
        for pid, row in self.module.table().items():
            try:
                if row["state"] != "Z" and os.getpgid(pid) == self.child.pid:
                    members.append(pid)
            except ProcessLookupError:
                pass
        return members

    def cleanup(self):
        # A dedicated process group covers also very short-lived children that
        # sampling could miss. Tree identities detect any observed escapees.
        if self.child.poll() is None or self.group_members():
            try:
                os.killpg(self.child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        for pid, start in (self.tree.owned if self.tree is not None else {}).items():
            try:
                current = (Path("/proc") / str(pid) / "stat").read_text()
                if int(current[current.rindex(")") + 2:].split()[19]) == start:
                    os.kill(pid, signal.SIGKILL)
            except (OSError, ValueError, IndexError):
                pass
        self.child.wait(timeout=3)
        for reader in self.readers:
            if reader.ident is not None:
                reader.join(timeout=1)
        for stream in (self.child.stdin, self.child.stdout, self.child.stderr):
            if not stream.closed:
                stream.close()
        if self.recorder is not None:
            self.recorder.close()
        if self.trace is not None:
            self.trace.close()


def children(sample, root):
    return {(row["pid"], row["start_ticks"]) for row in sample["processes"]
            if row["pid"] != root and row["state"] not in {"Z", "X", "x"}}


def run(args):
    package, output = args.package.resolve(), args.output.resolve()
    manifest_path = package / "extension.json"
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("protocol") != 4:
        raise ValueError("only protocol 4 packages are supported")
    command = manifest.get("command")
    if not isinstance(command, list) or not command or not all(isinstance(c, str) for c in command):
        raise ValueError("manifest command must be a nonempty string array")
    executable = (package / command[0]).resolve() if "/" in command[0] else Path(shutil.which(command[0]) or "")
    if not executable.is_file() or not os.access(executable, os.X_OK):
        raise ValueError("package executable is missing; build/install it separately")
    source = args.source.read_text(encoding="utf-8")
    if len(source.encode()) > 256 * 1024:
        raise ValueError("synthetic source exceeds 256 KiB")
    command = [str(executable), *command[1:]]
    module = sampler(args.sampler.resolve())
    output.mkdir(parents=True, exist_ok=False)
    (output / "source.txt").write_text(source, encoding="utf-8")
    report = dict(package=manifest["id"], command=command, language=args.language,
                  source_sha256=sha(args.source), binary_sha256=sha(executable),
                  manifest_sha256=sha(manifest_path), sampler_sha256=sha(args.sampler),
                  benchmark_sha256=sha(__file__),
                  environment={name: os.environ.get(name) for name in ("HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME")},
                  measurement="GUI-free ready-package process; sampled peaks, warm OS cache; no model or backend")
    process = Process(package, command, output, module, args.timeout)
    try:
        process.send(dict(type="hello", protocol=4,
                          features=["surfaces.content.v1", "widgets.svg.v1", "surfaces.content.invalidate.v1"],
                          server=dict(origin="http://127.0.0.1:1", token="synthetic-unused")))
        process.idle("before-render", 0)
        for context, session in [("left", "/synthetic/one"), ("right", "/synthetic/two")]:
            process.send(dict(type="context", context=context, workspace="/synthetic/workspace", session_dir=session))
        report["first"] = process.render("left", "/synthetic/one", "first", source, args.language, 1, output)
        report["cached"] = [process.render("left", "/synthetic/one", "first", source, args.language, n + 2, output)
                            for n in range(args.repeats)]
        if any(item["svg_sha256"] != report["first"]["svg_sha256"] for item in report["cached"]):
            raise RuntimeError("same-source repeat produced different SVG")
        report["second_session"] = process.render("right", "/synthetic/two", "second", source, args.language, 1, output)
        report["idle"] = process.idle("two-view-idle", args.idle)
        before = children(report["idle"], process.child.pid)
        process.send(dict(type="close", view="first"))
        report["after_first_close"] = process.idle("one-view-idle", 0.25)
        if children(report["after_first_close"], process.child.pid) != before:
            raise RuntimeError("closing first view changed the second view's child processes")
        process.render("right", "/synthetic/two", "second", source, args.language, 2, output)
        process.send(dict(type="close", view="second"))
        report["after_last_close"] = process.idle("closed-view-idle", args.idle)
        if children(report["after_last_close"], process.child.pid) or process.child.poll() is not None:
            raise RuntimeError("last close must release children and leave extension alive")
        report["after_eof"] = process.finish()
        js = [row for row in process.tree.last.values() if javascript(row)]
        report["javascript_processes"] = [dict(pid=row["pid"], exe=row["exe"], name=row["name"]) for row in js]
        if manifest["id"] in {"mermaid-rust-render", "latex-render"} and js:
            raise RuntimeError("native renderer launched a JavaScript runtime")
        report["ok"] = True
    except Exception as error:
        report.update(ok=False, error=str(error))
        raise
    finally:
        process.cleanup()
        report["resources"] = process.tree.totals()
        (output / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("package", "source", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--language", choices=("mermaid", "latex"), required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--idle", type=float, default=1.0)
    parser.add_argument("--timeout", type=float, default=15.0)
    parser.add_argument("--sampler", type=Path, default=Path(__file__).resolve().parents[3] /
                        "Proteus-core/examples/performance/common/proc_sampler.py")
    args = parser.parse_args()
    if not 1 <= args.repeats <= 100 or not 0.1 <= args.idle <= 60 or not 0.1 <= args.timeout <= 120:
        parser.error("repeats: 1..100; idle: 0.1..60s; timeout: 0.1..120s")
    try:
        report = run(args)
    except Exception as error:
        parser.exit(1, f"renderer benchmark failed: {error}\n")
    print(json.dumps(dict(ok=True, first_ms=report["first"]["render_ms"],
                          cached_ms=[item["render_ms"] for item in report["cached"]],
                          idle_pss_kib=report["idle"]["pss_kib"], output=str(args.output))))


if __name__ == "__main__":
    main()
