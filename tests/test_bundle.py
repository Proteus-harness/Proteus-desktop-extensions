"""Standalone delivery: every bundled package runs without the checkout."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class BundleTests(unittest.TestCase):
    def test_every_standalone_package_runs_without_checkout_imports(self):
        with tempfile.TemporaryDirectory() as directory:
            subprocess.run([sys.executable, str(ROOT / "package.py"), "--bundle", directory], check=True)
            result = subprocess.run([sys.executable, str(ROOT / "tests/wire.py"), "--packages", directory],
                                    capture_output=True, text=True, timeout=45)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("11 packages:", result.stdout)
            for package in Path(directory).iterdir():
                self.assertEqual(json.loads((package / "extension.json").read_text())["author"], "Proteus Project")
                self.assertFalse(list(package.rglob("*.py")), package.name)
            visualization = Path(directory) / "json-render"
            self.assertTrue((visualization / "README.md").is_file())
            self.assertTrue((visualization / "skills/visualize/SKILL.md").is_file())
            self.assertTrue((visualization / "examples/rich-content.md").is_file())


if __name__ == "__main__":
    unittest.main()
