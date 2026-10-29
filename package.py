#!/usr/bin/env python3
"""Collect standalone extension packages (ZIPs or a bundled directory).

Rust packages are built with cargo; their manifest runs `bin/<name>`, which
`--dev` also places into the source tree for local runs and tests.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
PACKAGES = ROOT / "packages"
SOURCE = ("src", "Cargo.toml", "bin", "__pycache__", "*.pyc", "update-vendor.py")


def binaries(build=True):
    """Builds the Rust packages; maps each package folder to its binary."""
    crates = [manifest.parent for manifest in sorted(PACKAGES.glob("*/Cargo.toml"))
              if json.loads((manifest.parent / "extension.json").read_text())["command"][0].startswith("bin/")]
    if not crates:
        return {}
    if build:
        outputs = built_binaries(crates, ["cargo", "build", "--release", "--locked", "--quiet",
                                         "--message-format=json-render-diagnostics"])
        return {package: (outputs[package], json.loads((package / "extension.json").read_text())["command"][0])
                for package in crates}
    target = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    if not target.is_absolute():
        target = ROOT / target
    target = target / "release"
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


def assemble(destination, build=True):
    destination.mkdir(parents=True, exist_ok=True)
    built = binaries(build=build)
    for manifest in sorted(PACKAGES.glob("*/extension.json")):
        package = destination / manifest.parent.name
        shutil.copytree(manifest.parent, package, ignore=shutil.ignore_patterns(*SOURCE))
        if manifest.parent in built:
            place(built, manifest.parent, package)
        # copytree follows the crate's LICENSE-GPL/LICENSE-APACHE symlink.
        if not list(package.glob("LICENSE*")):
            raise SystemExit(f"{manifest.parent.name}: no LICENSE-GPL or LICENSE-APACHE")


def develop(build=True):
    """Puts fresh binaries next to the manifests of the source tree."""
    built = binaries(build=build)
    for package in built:
        place(built, package, package)


def selected_package(name):
    """Validate a single shipped package before invoking Cargo or writing files."""
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,47}", name):
        raise SystemExit(f"invalid package id: {name!r}")
    package = PACKAGES / name
    if package.is_symlink() or not package.is_dir():
        raise SystemExit(f"unknown package: {name}")
    for filename in ("Cargo.toml", "extension.json"):
        path = package / filename
        if path.is_symlink() or not path.is_file():
            raise SystemExit(f"{name}: missing regular {filename}")
    try:
        manifest = json.loads((package / "extension.json").read_text())
    except (OSError, ValueError) as error:
        raise SystemExit(f"{name}: invalid extension.json: {error}") from error
    if not isinstance(manifest, dict) or manifest.get("id") != name:
        raise SystemExit(f"{name}: manifest id must equal the package folder")
    command = manifest.get("command")
    if (not isinstance(command, list) or not command
            or not all(isinstance(argument, str) for argument in command)
            or command[0] != f"bin/{name}"):
        raise SystemExit(f"{name}: expected command bin/{name}")
    if not any(path.is_file() for path in package.glob("LICENSE*")):
        raise SystemExit(f"{name}: no LICENSE-GPL or LICENSE-APACHE")
    return package


def selected_binary(package):
    """Use only the selected bin artifact reported by this successful build."""
    name = package.name
    command = ["cargo", "build", "--release", "--locked", "--quiet",
               "-p", f"extension-{name}", "--bin", name,
               "--message-format=json-render-diagnostics"]
    return built_binaries([package], command)[package]


def built_binaries(packages, command):
    """Resolve every requested executable from one successful Cargo invocation."""
    name = ", ".join(package.name for package in packages)
    manifests = {str((package / "Cargo.toml").resolve()): package for package in packages}
    artifacts = {package: [] for package in packages}
    result = subprocess.run(command,
        cwd=ROOT, check=False, stdout=subprocess.PIPE, text=True)
    finished = []
    invalid_json = False
    for line in result.stdout.splitlines():
        # Cargo permits unrelated tool output. JSON-looking damaged records
        # are not treated as harmless log lines or replaced by stale files.
        if not line.lstrip().startswith("{"):
            continue
        try:
            message = json.loads(line)
        except ValueError:
            invalid_json = True
            continue
        if not isinstance(message, dict):
            continue
        diagnostic = message.get("message")
        if message.get("reason") == "compiler-message" and isinstance(diagnostic, dict):
            rendered = diagnostic.get("rendered")
            if isinstance(rendered, str):
                sys.stderr.write(rendered)
        if message.get("reason") == "build-finished":
            finished.append(message.get("success"))
        target = message.get("target", {})
        profile = message.get("profile", {})
        package = manifests.get(message.get("manifest_path"))
        if (message.get("reason") == "compiler-artifact"
                and package is not None
                and isinstance(target, dict) and target.get("kind") == ["bin"]
                and target.get("name") == package.name
                and isinstance(profile, dict) and profile.get("test") is False
                and isinstance(message.get("executable"), str)
                and message["executable"]):
            artifacts[package].append(Path(message["executable"]))
    if result.returncode != 0:
        raise subprocess.CalledProcessError(result.returncode, result.args, output=result.stdout)
    if invalid_json:
        raise SystemExit(f"{name}: invalid Cargo JSON output")
    if len(finished) != 1 or finished[0] is not True or any(len(paths) != 1 for paths in artifacts.values()):
        raise SystemExit(f"{name}: expected one executable from one successful Cargo build")
    outputs = {}
    for package, paths in artifacts.items():
        binary = paths[0]
        if not binary.is_absolute() or not binary.is_file():
            raise SystemExit(f"{package.name}: Cargo executable is not an existing absolute file: {binary}")
        outputs[package] = binary
    return outputs


def atomic_copy(binary, destination):
    """Publish a complete file on the same filesystem; leave the old file on error."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=f".{destination.name}.", dir=destination.parent)
    os.close(descriptor)
    try:
        shutil.copy2(binary, temporary)
        os.replace(temporary, destination)
    finally:
        Path(temporary).unlink(missing_ok=True)


def selected_develop(package, binary):
    destination = package / "bin" / package.name
    if destination.parent.is_symlink():
        raise SystemExit(f"{package.name}: bin must not be a symlink")
    atomic_copy(binary, destination)


def selected_archive(package, binary, output):
    """Stage one complete package and atomically publish only its ZIP."""
    with tempfile.TemporaryDirectory() as directory:
        staged = Path(directory) / package.name
        shutil.copytree(package, staged, ignore=shutil.ignore_patterns(*SOURCE))
        place({package: (binary, f"bin/{package.name}")}, package, staged)
        if not any(path.is_file() for path in staged.glob("LICENSE*")):
            raise SystemExit(f"{package.name}: no LICENSE-GPL or LICENSE-APACHE")
        output.mkdir(parents=True, exist_ok=True)
        archive = output / (package.name + ".zip")
        descriptor, temporary = tempfile.mkstemp(prefix=f".{archive.name}.", dir=output)
        os.close(descriptor)
        try:
            with zipfile.ZipFile(temporary, "w", zipfile.ZIP_DEFLATED) as zipped:
                for path in sorted(staged.rglob("*")):
                    if path.is_file():
                        info = zipfile.ZipInfo.from_file(path, path.relative_to(staged))
                        info.compress_type = zipfile.ZIP_DEFLATED
                        zipped.writestr(info, path.read_bytes())
            with zipfile.ZipFile(temporary) as zipped:
                if zipped.testzip() is not None:
                    raise SystemExit(f"{package.name}: failed ZIP validation")
            os.replace(temporary, archive)
        finally:
            Path(temporary).unlink(missing_ok=True)
        print(archive)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="ZIP directory (default: dist)")
    parser.add_argument("--bundle", type=Path, help="assemble packages into a new directory instead of ZIPs")
    parser.add_argument("--dev", action="store_true", help="build binaries into the source packages")
    parser.add_argument("--no-build", action="store_true", help="package existing release binaries without Cargo")
    parser.add_argument("--package", action="append", metavar="ID", help="build only one package, without rebuilding the client")
    args = parser.parse_args()
    if args.package:
        if len(args.package) != 1:
            parser.error("--package must be supplied exactly once")
        if args.no_build:
            parser.error("--package requires a fresh selected build; --no-build is for reusing the complete set")
        if args.bundle:
            parser.error("--package cannot be combined with --bundle; use a selected ZIP or --dev")
        if args.dev and args.output is not None:
            parser.error("--output cannot be combined with --package --dev")
        package = selected_package(args.package[0])
        if args.dev and (package / "bin").is_symlink():
            parser.error("the selected package bin directory must not be a symlink")
        binary = selected_binary(package)
        if args.dev:
            selected_develop(package, binary)
        else:
            selected_archive(package, binary, args.output or ROOT / "dist")
        return
    args.output = args.output or ROOT / "dist"
    if args.dev:
        develop(build=not args.no_build)
        return
    if args.bundle:
        assemble(args.bundle, build=not args.no_build)
        return
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as directory:
        assemble(Path(directory), build=not args.no_build)
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
