#!/usr/bin/env python3
"""Explicitly package one experimental renderer using the maintained packager."""
import importlib.util
import os
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("proteus_extension_packager", ROOT.parents[1] / "package.py")
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)
packager.ROOT = ROOT
packager.PACKAGES = ROOT / "packages"

if __name__ == "__main__":
    options = sys.argv[1:]
    help_requested = options in (["--help"], ["-h"])
    selected = any(value == "--package" or value.startswith("--package=") for value in options)
    bundled = any(value == "--bundle" or value.startswith("--bundle=") for value in options)
    if not help_requested and (not selected or bundled):
        raise SystemExit("Select one experiment explicitly: --package ID [--dev]; no default bundle")
    os.environ.setdefault("CARGO_TARGET_DIR", str(ROOT.parents[1] / "target"))
    os.environ.setdefault("CARGO_BUILD_JOBS", "2")
    packager.main()
