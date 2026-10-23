#!/usr/bin/env python3
"""Collect standalone extension packages (ZIPs or a bundled directory).

Rust packages are built with cargo; their manifest runs `bin/<name>`, which
`--dev` also places into the source tree for local runs and tests.
"""
import argparse
import json
import os
import shutil
import subprocess
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
PACKAGES = ROOT / "packages"
SOURCE = ("src", "Cargo.toml", "bin", "__pycache__", "*.pyc")


def binaries():
    """Builds the Rust packages; maps each package folder to its binary."""
    crates = [manifest.parent for manifest in sorted(PACKAGES.glob("*/Cargo.toml"))
              if json.loads((manifest.parent / "extension.json").read_text())["command"][0].startswith("bin/")]
    if not crates:
        return {}
    subprocess.run(["cargo", "build", "--release", "--locked", "--quiet"], cwd=ROOT, check=True)
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release"
    built = {}
    for package in crates:
        program = json.loads((package / "extension.json").read_text())["command"][0]
        built[package] = (target / Path(program).name, program)
    return built


def place(built, package, destination):
    binary, program = built[package]
    target = destination / program
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, target)


def assemble(destination):
    destination.mkdir(parents=True, exist_ok=True)
    built = binaries()
    for manifest in sorted(PACKAGES.glob("*/extension.json")):
        package = destination / manifest.parent.name
        shutil.copytree(manifest.parent, package, ignore=shutil.ignore_patterns(*SOURCE))
        if manifest.parent in built:
            place(built, manifest.parent, package)
        # copytree follows the crate's LICENSE-GPL/LICENSE-APACHE symlink.
        if not list(package.glob("LICENSE*")):
            raise SystemExit(f"{manifest.parent.name}: no LICENSE-GPL or LICENSE-APACHE")


def develop():
    """Puts fresh binaries next to the manifests of the source tree."""
    built = binaries()
    for package in built:
        place(built, package, package)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    parser.add_argument("--bundle", type=Path, help="assemble packages into a new directory instead of ZIPs")
    parser.add_argument("--dev", action="store_true", help="build binaries into the source packages")
    args = parser.parse_args()
    if args.dev:
        develop()
        return
    if args.bundle:
        assemble(args.bundle)
        return
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as directory:
        assemble(Path(directory))
        for package in sorted(Path(directory).iterdir()):
            archive = args.output / (package.name + ".zip")
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
                for path in sorted(package.rglob("*")):
                    if path.is_file():
                        info = zipfile.ZipInfo.from_file(path, path.relative_to(package))
                        info.compress_type = zipfile.ZIP_DEFLATED
                        output.writestr(info, path.read_bytes())
            print(archive)


if __name__ == "__main__":
    main()
