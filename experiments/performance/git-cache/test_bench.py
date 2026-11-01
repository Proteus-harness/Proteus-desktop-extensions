"""The synthetic backend must not depend on a real workspace or Git state."""
import importlib.util
import json
from pathlib import Path
import unittest
from urllib.request import urlopen

spec = importlib.util.spec_from_file_location("git_cache_probe", Path(__file__).with_name("bench.py"))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class SyntheticBackendTests(unittest.TestCase):
    def test_unique_hashes_and_full_short_requests_keep_the_same_synthetic_patch(self):
        server, thread, calls, patch = bench.backend(1024)
        try:
            origin = f"http://127.0.0.1:{server.server_port}"
            def get(path):
                with urlopen(origin + path, timeout=2) as response:
                    return json.load(response)
            first = get("/workspace/commit?hash=" + bench.full_hash(1))
            alias = get("/workspace/commit?hash=" + bench.full_hash(1)[:7])
            second = get("/workspace/commit?hash=" + bench.full_hash(2))
            self.assertEqual(first, alias)
            self.assertNotEqual(first["hash"], second["hash"])
            self.assertEqual(first["patch"], patch)
            self.assertEqual(first["patch"], second["patch"])
            diff = get("/workspace/commit/diff?hash=" + bench.full_hash(1) + "&path=synthetic.txt")
            self.assertEqual(diff["patch"], patch)
            self.assertEqual(calls["/workspace/commit"], 3)
            self.assertEqual(calls["/workspace/commit/diff"], 1)
            changes = get("/workspace/changes")
            self.assertEqual(changes["branch"], "synthetic")
            self.assertEqual(changes["entries"], [])
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)
        self.assertFalse(thread.is_alive())


if __name__ == "__main__":
    unittest.main()
