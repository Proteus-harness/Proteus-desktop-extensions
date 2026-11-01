"""Inert protocol peer: verify policy selection, state updates and cleanup."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('projection_bench', HERE / 'bench.py')
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)
FAKE = '''import json,sys
state = {}; live = False
def badge():
 print(json.dumps(dict(type='badge',view='badge',text=str(state.get('context')))),flush=True)
for line in sys.stdin:
 message=json.loads(line)
 kind=message['type']
 if kind=='session_snapshot': state=message['state']
 if kind=='render': live=True; badge()
 if kind=='session_update':
  state=message['update']['state']
  if live: badge()
 if kind=='close': live=False
'''


class ProjectionTests(unittest.TestCase):
    def test_manifest_policy_excludes_payload_but_preserves_live_state(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for name in ('model', 'mode', 'context'):
                package = root / 'packages' / name
                package.mkdir(parents=True)
                (package / 'fake.py').write_text(FAKE)
                (package / 'extension.json').write_text(json.dumps(dict(
                    protocol=4, id=name, session_entries=False,
                    command=[sys.executable, str(package / 'fake.py')]
                )))
            def run(full):
                return bench.run(argparse.Namespace(packages=root/'packages', output=root/str(full),
                    full=full, entries=2, payload_kib=8, idle=.1, timeout=2))
            before, after = run(True), run(False)
            for old, new in zip(before['widgets'], after['widgets']):
                self.assertGreater(old['snapshot_encoded_bytes'], 16000)
                self.assertLess(new['snapshot_encoded_bytes'], 2000)
                self.assertTrue(new['state_update_visible'])
                self.assertEqual(new['after_eof']['process_count'], 0)
            trace = (root/'True/model/protocol.jsonl').read_text()
            self.assertNotIn('x'*32, trace)
            self.assertIn('"count": 2', trace)


if __name__ == '__main__':
    unittest.main()
