#!/usr/bin/env python3
"""Run vendored flowchart anchors and the package's final-route/SVG regressions."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
EXTENSIONS = ROOT.parents[1]
VENDOR = ROOT / "vendor/mermaid-rs-renderer-0.3.1"
VERIFICATION = EXTENSIONS / "target/verification/native-renderers"
RUNNER = VERIFICATION / "arrow-lib-test"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="use only cached Cargo dependencies")
    args = parser.parse_args()
    VERIFICATION.mkdir(parents=True, exist_ok=True)
    if RUNNER.exists():
        shutil.rmtree(RUNNER)
    shutil.copytree(VENDOR, RUNNER)
    manifest = RUNNER / "Cargo.toml"
    manifest.write_text(manifest.read_text() + "\n[workspace]\n")
    # The published crate excludes docs/comparison_sources, yet unrelated upstream
    # unit tests include those files at compile time. Disable their test-only modules
    # in this generated copy; production code and the tested render modules stay exact.
    for path in (RUNNER / "src").rglob("*.rs"):
        if path.relative_to(RUNNER).as_posix() not in ("src/render.rs", "src/render/anchors.rs"):
            source = path.read_text()
            path.write_text(source.replace("#[cfg(test)]", "#[cfg(any())]"))
    temporary = VERIFICATION / "tmp"
    temporary.mkdir(exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(EXTENSIONS / "target"),
               CARGO_PROFILE_TEST_DEBUG="0", TMPDIR=str(temporary))
    env.setdefault("CARGO_BUILD_JOBS", "2")
    commands = [
        ["cargo", "test", "--manifest-path", str(manifest),
         "--no-default-features", "--lib", "render::anchors::tests"],
        ["cargo", "test", "--manifest-path", str(ROOT / "Cargo.toml"), "--locked",
         "-p", "extension-mermaid-rust-render", "render::routing_tests"],
    ]
    log_path = VERIFICATION / "arrow-tests.log"
    with log_path.open("w") as log:
        for command in commands:
            if args.offline:
                command.append("--offline")
            log.write(" ".join(command) + "\n")
            log.flush()
            with subprocess.Popen(command, cwd=EXTENSIONS, env=env, stdout=subprocess.PIPE,
                                  stderr=subprocess.STDOUT, text=True) as process:
                for line in process.stdout:
                    print(line, end="", flush=True)
                    log.write(line)
                status = process.wait()
            if status:
                break
    print(f"Arrow test log: {log_path}")
    return status


if __name__ == "__main__":
    sys.exit(main())
