"""Wire fixtures isolate preferences and expose compact tool-state input."""
import contextlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import threading
import unittest
import urllib.request
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("wire_activity_candidate", Path(__file__).with_name("wire.py"))
wire = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(wire)


class ActivityWireFixtureTests(unittest.TestCase):
    def setUp(self):
        wire.TEST_WIRE.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="unit-", dir=wire.TEST_WIRE)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def package(self, failure=False):
        package = self.root / "fixture"
        package.mkdir()
        program = """
import json, os, pathlib, sys
hello = json.loads(sys.stdin.readline())
root = pathlib.Path(os.environ['XDG_CONFIG_HOME'])
(root / 'synthetic-setting.json').write_text('{"fixture":true}')
print(json.dumps({'type':'fixture', 'config':str(root), 'hello':hello}), flush=True)
for line in sys.stdin:
    pass
"""
        if failure:
            program += "sys.stderr.write('synthetic failure')\n"
        (package / "extension.json").write_text(json.dumps({
            "id":"wire-fixture", "command":[sys.executable, "-c", program], "surfaces":{},
        }))
        return package

    def test_process_preferences_are_private_and_removed_on_close(self):
        real = self.root / "existing-user-config"
        real.mkdir()
        marker = real / "do-not-change.json"
        marker.write_text('{"existing":true}')
        with patch.dict(os.environ, {"XDG_CONFIG_HOME":str(real)}):
            process = wire.Process(self.package(), "http://fixture.invalid")
            try:
                response = process.wait(lambda message: message["type"] == "fixture")
                isolated = Path(response["config"])
                self.assertEqual(isolated.parent, wire.TEST_WIRE)
                self.assertNotEqual(isolated, real)
                self.assertTrue((isolated / "synthetic-setting.json").is_file())
                self.assertIn("session.tool_state.v1", response["hello"]["features"])
                self.assertIn("manifest.dependencies.tools.v1", response["hello"]["features"])
            finally:
                process.close()
        self.assertFalse(isolated.exists())
        self.assertEqual(list(real.iterdir()), [marker])
        self.assertEqual(marker.read_text(), '{"existing":true}')

    def test_failed_process_close_still_removes_preferences(self):
        process = wire.Process(self.package(failure=True), "", preview=True)
        response = process.wait(lambda message: message["type"] == "fixture")
        isolated = Path(response["config"])
        self.assertEqual(response["hello"]["server"]["token"], "")
        self.assertEqual(response["hello"]["workspace"], "")
        with self.assertRaisesRegex(AssertionError, "synthetic failure"):
            process.close()
        self.assertFalse(isolated.exists())

    def test_api_compact_tool_fixture_has_explicit_revision_and_exact_name(self):
        reads, queries = len(wire.READS), len(wire.TOOL_QUERIES)
        with contextlib.ExitStack() as stack:
            server = wire.http.server.ThreadingHTTPServer(("127.0.0.1", 0), wire.Api)
            stack.callback(server.server_close)
            stack.callback(server.shutdown)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            server.tool_state = wire.historical_tool_state()
            query = wire.urllib.parse.urlencode({"session_dir":"/synthetic chat", "name":"update_plan"})
            request = urllib.request.Request(
                f"http://127.0.0.1:{server.server_port}/history/tool-state?{query}",
                headers={"Authorization":"Bearer fixture"},
            )
            with urllib.request.urlopen(request) as response:
                payload = json.load(response)
            self.assertIsInstance(payload["revision"], str)
            self.assertEqual(payload["tool"]["name"], "update_plan")
            self.assertEqual(set(payload["tool"]), {"call_id", "name", "status", "args", "effective_args"})
        self.assertEqual(wire.READS[reads:], [("/history/tool-state", "/synthetic chat")])
        self.assertEqual(wire.TOOL_QUERIES[queries:], [("update_plan", "/synthetic chat")])


if __name__ == "__main__":
    unittest.main()
