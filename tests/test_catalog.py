"""Source-catalog discovery and freshness, without Cargo or extension execution."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("source_catalog", ROOT / "scripts/catalog.py")
catalog = importlib.util.module_from_spec(spec)
spec.loader.exec_module(catalog)


class CatalogTests(unittest.TestCase):
    def fixture(self, root, identity="demo", *, version="version.workspace = true", surfaces=None):
        (root / "Cargo.toml").write_text('[workspace.package]\nversion = "2.3.4"\n')
        package = root / "packages" / identity
        package.mkdir(parents=True)
        manifest = {"id": identity, "title": "Проверка", "description": "Описание",
                    "command": [f"bin/{identity}"], "surfaces": surfaces or {"panel": {}}}
        (package / "extension.json").write_text(json.dumps(manifest))
        (package / "Cargo.toml").write_text(
            f'[package]\nname = "extension-{identity}"\n{version}\n'
            f'[[bin]]\nname = "{identity}"\npath = "src/main.rs"\n')
        return package

    def test_committed_index_lists_every_production_package_as_workspace_sources(self):
        index = catalog.catalog(ROOT)
        shipped = {path.parent.name for path in (ROOT / "packages").glob("*/extension.json")}
        self.assertEqual({entry["id"] for entry in index["entries"]}, shipped)
        self.assertEqual(json.loads((ROOT / catalog.INDEX).read_text()), index)
        for entry in index["entries"]:
            with self.subTest(package=entry["id"]):
                self.assertEqual(entry["kind"], "client_extension")
                self.assertEqual(entry["source"]["path"], ".")
                self.assertEqual(entry["source"]["build"],
                                 ["python3", "package.py", "--package", entry["id"]])
                self.assertEqual(entry["path"], f'packages/{entry["id"]}')
                self.assertNotIn("artifacts", entry)

    def test_manifest_updates_and_inherited_or_explicit_cargo_versions_reach_index(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = self.fixture(root, surfaces={"content": {}})
            entry = catalog.catalog(root)["entries"][0]
            self.assertEqual((entry["version"], entry["category"]), ("2.3.4", "content"))
            manifest = json.loads((package / "extension.json").read_text())
            manifest.update(title="Новое название", description="Новое описание", surfaces={"badge": {}})
            (package / "extension.json").write_text(json.dumps(manifest))
            cargo = package / "Cargo.toml"
            cargo.write_text(cargo.read_text().replace("version.workspace = true", 'version = "5.0.1"'))
            updated = catalog.catalog(root)["entries"][0]
            self.assertEqual((updated["title"], updated["description"], updated["version"], updated["category"]),
                             ("Новое название", "Новое описание", "5.0.1", "controls"))

    def test_mismatched_manifest_identity_or_cargo_selector_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = self.fixture(root)
            path = package / "extension.json"
            good = json.loads(path.read_text())
            for key, value in (("id", "other"), ("command", ["bin/other"])):
                with self.subTest(key=key):
                    invalid = dict(good, **{key: value})
                    path.write_text(json.dumps(invalid))
                    with self.assertRaises(ValueError):
                        catalog.catalog(root)
            path.write_text(json.dumps(good))
            cargo = package / "Cargo.toml"
            cargo.write_text(cargo.read_text().replace('name = "extension-demo"', 'name = "extension-other"'))
            with self.assertRaises(ValueError):
                catalog.catalog(root)

    def test_check_does_not_overwrite_stale_index_and_write_makes_it_current(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            index = root / catalog.INDEX
            index.write_text("previous index\n")
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(catalog.main(["--check"], root=root), 1)
                self.assertEqual(index.read_text(), "previous index\n")
                self.assertEqual(catalog.main(["--write"], root=root), 0)
                self.assertEqual(catalog.main(["--check"], root=root), 0)
            self.assertEqual(index.read_text(), catalog.encoded(catalog.catalog(root)))


if __name__ == "__main__":
    unittest.main()
