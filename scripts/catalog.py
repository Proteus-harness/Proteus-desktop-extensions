#!/usr/bin/env python3
"""Generate the source catalog from shipped manifests, without running a build."""

import argparse
import json
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
INDEX = "proteus-catalog.json"
CATEGORIES = {
    "panels": "Панели и вкладки",
    "content": "Отображение ответов",
    "controls": "Управление чатом",
}


def category(manifest):
    surfaces = manifest.get("surfaces", {})
    if "content" in surfaces:
        return "content"
    if any(surface in surfaces for surface in ("panel", "document", "page")):
        return "panels"
    return "controls"


def catalog(root=ROOT):
    """Describe source delivery; packages/<id> alone is not a buildable workspace."""
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    entries = []
    for path in sorted((root / "packages").glob("*/extension.json")):
        manifest = json.loads(path.read_text())
        identity = path.parent.name
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,47}", identity):
            raise ValueError(f"invalid package folder: {identity!r}")
        if manifest.get("id") != identity:
            raise ValueError(f"{identity}: manifest id must equal the package folder")
        command = manifest.get("command")
        if (not isinstance(command, list) or not command
                or command[0] != f"bin/{identity}"):
            raise ValueError(f"{identity}: expected command bin/{identity}")
        spec = tomllib.loads(path.with_name("Cargo.toml").read_text())
        if (spec["package"].get("name") != f"extension-{identity}"
                or not any(binary.get("name") == identity for binary in spec.get("bin", []))):
            raise ValueError(f"{identity}: Cargo package/bin does not match the package selector")
        version = spec["package"].get("version")
        if isinstance(version, dict) and version == {"workspace": True}:
            version = workspace["package"]["version"]
        if not isinstance(version, str) or not version.strip():
            raise ValueError(f"{identity}: missing Cargo package version")
        title = manifest.get("title")
        description = manifest.get("description", "")
        if not isinstance(title, str) or not title.strip() or not isinstance(description, str):
            raise ValueError(f"{identity}: invalid catalog title or description")
        entries.append({
            "id": identity,
            "kind": "client_extension",
            "title": title,
            "description": description,
            "category": category(manifest),
            "version": version,
            "path": f"packages/{identity}",
            "source": {
                "path": ".",
                "build": ["python3", "package.py", "--package", identity],
                "requirements": ["Rust", "Git", "Python 3"],
            },
        })
    if not entries:
        raise ValueError("no shipped extension packages found")
    used = {entry["category"] for entry in entries}
    return {
        "format": 1,
        "title": "Расширения Proteus",
        "categories": [{"id": key, "title": title} for key, title in CATEGORIES.items() if key in used],
        "entries": entries,
    }


def encoded(index):
    return json.dumps(index, ensure_ascii=False, indent=2) + "\n"


def main(argv=None, *, root=ROOT):
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true", help="check the committed index without writing")
    mode.add_argument("--write", action="store_true", help="regenerate proteus-catalog.json")
    args = parser.parse_args(argv)
    destination = root / INDEX
    try:
        text = encoded(catalog(root))
        if args.write:
            destination.write_text(text)
            print(f"Updated {destination}")
            return 0
        if not destination.is_file() or destination.read_text() != text:
            print(f"{destination}: index is stale; run python3 scripts/catalog.py --write", file=sys.stderr)
            return 1
        print(f"{destination}: current")
        return 0
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"Catalog: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
