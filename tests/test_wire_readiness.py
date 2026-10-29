"""Protocol exercise never starts an implicit Cargo/package build."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("wire_readiness_candidate", Path(__file__).with_name("wire.py"))
wire = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(wire)


class ReadinessTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.packages = self.root / "packages"
        self.package = self.packages / "sample"
        self.package.mkdir(parents=True)
        (self.package / "extension.json").write_text(json.dumps({"command": ["bin/sample"]}))

    def test_exercise_fails_before_server_or_build_when_missing(self):
        with patch.object(wire.subprocess, "run") as build, patch.object(wire.http.server, "ThreadingHTTPServer") as server:
            with self.assertRaisesRegex(SystemExit, "missing executable.*--build"):
                wire.exercise(self.packages)
            build.assert_not_called()
            server.assert_not_called()

    def test_default_source_directory_does_not_trigger_build(self):
        # The old implementation built when packages == PACKAGES.
        with patch.object(wire, "PACKAGES", self.packages), patch.object(wire.subprocess, "run") as build:
            with self.assertRaisesRegex(SystemExit, "missing executable"):
                wire.exercise(self.packages)
            build.assert_not_called()

    def test_ready_executable_and_execute_permission(self):
        binary = self.package / "bin/sample"
        binary.parent.mkdir()
        binary.write_text("#!/bin/sh\nexit 0\n")
        binary.chmod(0o644)
        with self.assertRaisesRegex(SystemExit, "missing executable"):
            wire.require_ready(self.packages)
        binary.chmod(0o755)
        wire.require_ready(self.packages)

    def test_cli_wire_without_build_is_read_only(self):
        with patch.object(wire, "exercise", return_value=[{"type": "fixture"}]) as exercise, patch.object(wire.subprocess, "run") as build, contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(wire.main(["--wire", "--packages", str(self.packages)]), 0)
            build.assert_not_called()
            exercise.assert_called_once_with(self.packages)
            self.assertEqual(json.loads(output.getvalue()), {"type": "fixture"})

    def test_cli_build_is_explicit_and_precedes_exercise(self):
        builder = self.root / "package.py"
        builder.write_text("# fake builder; never executed\n")
        events = []
        with patch.object(wire.subprocess, "run", side_effect=lambda *args, **kwargs: events.append("build")) as build, patch.object(wire, "exercise", side_effect=lambda *_: events.append("exercise") or []), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(wire.main(["--build", "--packages", str(self.packages)]), 0)
            self.assertEqual(events, ["build", "exercise"])
            self.assertEqual(build.call_args.args[0], [wire.sys.executable, str(builder), "--dev"])


if __name__ == "__main__":
    unittest.main()
