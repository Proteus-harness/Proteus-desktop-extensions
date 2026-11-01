"""The terminal package needs only a folder, never a live Core server."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "packages/terminal"
MANIFEST = json.loads((PACKAGE / "extension.json").read_text())


class TerminalLocalTests(unittest.TestCase):
    def test_local_dependency_is_explicit_and_requires_host_support(self):
        self.assertIs(MANIFEST["dependencies"]["core"], False)
        self.assertIn("manifest.dependencies.core.v1", MANIFEST["required_features"])
        for package in ("files", "git"):
            manifest = json.loads((ROOT / "packages" / package / "extension.json").read_text())
            self.assertIs(manifest.get("dependencies", {}).get("core", True), True)

    @unittest.skipUnless((PACKAGE / "bin/terminal").is_file(), "build terminal package before wire check")
    def test_native_terminal_wire_runs_with_empty_server_and_no_session(self):
        fixtures = ROOT / "build/test-wire"
        fixtures.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="terminal-local-", dir=fixtures) as directory:
            messages = [
                {"type": "hello", "protocol": 4, "features": MANIFEST["required_features"],
                 "workspace": "/synthetic/project", "server": {"origin": "", "token": ""}},
                {"type": "context", "context": "codex-folder", "workspace": "/synthetic/project", "session_dir": None},
                {"type": "render", "context": "codex-folder", "view": "terminal:1", "surface": "document", "argument": "1", "session_dir": None},
                {"type": "context", "context": "codex-folder", "workspace": "/synthetic/other", "session_dir": None},
                {"type": "render", "context": "codex-folder", "view": "terminal:2", "surface": "document", "argument": "2", "session_dir": None},
            ]
            result = subprocess.run(
                MANIFEST["command"], cwd=PACKAGE,
                env={**os.environ, "XDG_CONFIG_HOME": directory},
                input="".join(json.dumps(message) + "\n" for message in messages),
                capture_output=True, text=True, timeout=10,
            )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        views = {message["view"]: message["root"] for line in result.stdout.splitlines()
                 if (message := json.loads(line))["type"] == "view"}
        for view, folder in [("terminal:1", "/synthetic/project"), ("terminal:2", "/synthetic/other")]:
            self.assertEqual(views[view], {"type": "fill", "children": [
                {"type": "terminal", "id": "shell", "cwd": folder},
            ]})


if __name__ == "__main__":
    unittest.main()
