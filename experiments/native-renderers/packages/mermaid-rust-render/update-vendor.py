#!/usr/bin/env python3
"""Reimport the exact MIT crate archive and apply the recorded small source patch."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def files(root):
    return {str(path.relative_to(root)): digest(path.read_bytes())
            for path in root.rglob("*") if path.is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify existing vendor without changing it")
    args = parser.parse_args()
    provenance = json.loads((HERE / "UPSTREAM.json").read_text())
    patch = ROOT / provenance["patch"]["path"]
    patch_bytes = patch.read_bytes()
    if digest(patch_bytes) != provenance["patch"]["sha256"]:
        raise SystemExit("Mermaid local patch SHA-256 mismatch")
    for backport in provenance.get("backports", []):
        recorded = backport["patch"]
        if digest((ROOT / recorded["path"]).read_bytes()) != recorded["sha256"]:
            raise SystemExit("Mermaid recorded upstream backport SHA-256 mismatch")
    with urllib.request.urlopen(provenance["crate_archive"], timeout=30) as response:
        payload = response.read(8 * 1024 * 1024 + 1)
    if len(payload) > 8 * 1024 * 1024 or digest(payload) != provenance["crate_sha256"]:
        raise SystemExit("Mermaid archive SHA-256 mismatch or oversized archive")
    destination = ROOT / provenance["vendored_path"]
    with tempfile.TemporaryDirectory(prefix="mermaid-vendor-") as temporary:
        source = Path(temporary) / "crate"
        source.mkdir()
        with tarfile.open(fileobj=io.BytesIO(payload)) as archive:
            for member in archive.getmembers():
                parts = Path(member.name).parts
                if not parts or parts[0] != "mermaid-rs-renderer-0.3.1" or ".." in parts:
                    raise SystemExit("Unexpected Mermaid archive path")
                if member.isfile():
                    target = source.joinpath(*parts[1:])
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(archive.extractfile(member).read())
                elif not member.isdir():
                    raise SystemExit("Unexpected non-file Mermaid archive member")
        pristine = files(source)
        subprocess.run(["patch", "--batch", "--forward", "--fuzz=0", "-p1"],
                       input=patch_bytes, cwd=source, check=True)
        expected = files(source)
        modified = sorted(path for path in pristine.keys() | expected.keys()
                          if pristine.get(path) != expected.get(path))
        if modified != sorted(provenance["modified_files"]):
            raise SystemExit("Mermaid modified-files provenance mismatch")
        if args.check:
            if expected != files(destination):
                raise SystemExit("Vendored Mermaid differs from the verified archive plus patch")
        else:
            if destination.is_symlink():
                raise SystemExit("Vendor destination must not be a symlink")
            if destination.exists():
                shutil.rmtree(destination)
            shutil.copytree(source, destination)
        print(f"Mermaid 0.3.1: {len(expected)} verified files, recorded corner/arrow/fan-out patch")


if __name__ == "__main__":
    main()
