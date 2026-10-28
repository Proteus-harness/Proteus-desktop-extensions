"""Standalone delivery: every bundled package runs without the checkout."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]


class BundleTests(unittest.TestCase):
    def test_selected_zip_runs_without_checkout_or_other_packages(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archives = root / "archives"
            subprocess.run([sys.executable, str(ROOT / "package.py"),
                            "--package", "quota", "--output", str(archives)], check=True)
            self.assertEqual([path.name for path in archives.iterdir()], ["quota.zip"])
            package = root / "installed/quota"
            with zipfile.ZipFile(archives / "quota.zip") as archive:
                self.assertIsNone(archive.testzip())
                archive.extractall(package)
                for info in archive.infolist():
                    (package / info.filename).chmod(info.external_attr >> 16 & 0o777)
            self.assertFalse(list(package.rglob("Cargo.toml")))
            result = subprocess.run([sys.executable, str(ROOT / "tests/wire.py"),
                                     "--packages", str(package.parent)],
                                    capture_output=True, text=True, timeout=45)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("1 packages:", result.stdout)

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
