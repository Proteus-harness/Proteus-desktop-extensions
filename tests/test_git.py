"""Behaviour of the git panel and git colours in the files tree."""
import importlib.util
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "sdk"))
sys.path.insert(0, str(ROOT / "packages" / "git"))
sys.path.insert(0, str(ROOT / "tests"))
import schema


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


git = load("git_main", "packages/git/main.py")
sys.path.insert(0, str(ROOT / "packages" / "files"))
files = load("files_main", "packages/files/main.py")

ENTRIES = [
    {"path":"src/a.rs", "original":None, "status":"modified", "staged":"modified", "unstaged":"modified"},
    {"path":"b.py", "original":None, "status":"deleted", "staged":None, "unstaged":"deleted"},
    {"path":"new.md", "original":None, "status":"untracked", "staged":None, "unstaged":"untracked"},
    {"path":"both.txt", "original":None, "status":"conflict", "staged":"conflict", "unstaged":"conflict"},
]
CHANGES = {"repository":True, "branch":"main", "head":{"short":"abc1234", "summary":"Last"}, "upstream":None,
           "ahead":0, "behind":0, "entries":ENTRIES, "truncated":False}


class Fake(git.Git):
    def __init__(self):
        super().__init__()
        self.session = "/s"
        self.calls, self.sent = [], []

    def api(self, path, query=None, body=None, session=None, text=False, timeout=15):
        self.calls.append((path, body))
        if path == "/workspace/branches": return {"branches":[{"name":"main"}, {"name":"topic"}]}
        if path == "/workspace/log": return {"entries":[]}
        if path == "/workspace/commit": return {"commit":{"short":"x", "summary":"y"}, "changes":CHANGES}
        return CHANGES

    def send(self, message):
        self.sent.append(message)


class GitPanelTest(unittest.TestCase):
    def test_sections_follow_zed(self):
        panel = Fake()
        panel.refresh()
        root = panel.panel("main")
        schema.check(self, root)
        rows = next(node for node in schema.walk(root) if node["type"] == "tree")["rows"]
        self.assertEqual([row["id"] for row in rows if row["depth"] == 0],
                         ["section:conflicts", "section:tracked", "section:untracked", "section:history"])
        partial = next(row for row in rows if row["id"] == "src/a.rs")
        self.assertTrue(partial["checked"])
        self.assertIn("частично", partial["detail"])
        self.assertEqual(partial["icon"], "file-code")
        deleted = next(row for row in rows if row["id"] == "b.py")
        self.assertTrue(deleted["strike"])
        self.assertEqual(deleted["tone"], "danger")
        self.assertFalse(next(row for row in rows if row["id"] == "section:tracked")["checked"])

    def test_checkboxes_stage_and_unstage(self):
        panel = Fake()
        panel.refresh()
        panel.action("panel", "tree", {"id":"b.py", "event":"check"})
        self.assertEqual(panel.calls[-1], ("/workspace/stage", {"session_dir":"/s", "paths":["b.py"]}))
        panel.action("panel", "tree", {"id":"src/a.rs", "event":"check"})
        self.assertEqual(panel.calls[-1][0], "/workspace/unstage")
        panel.action("panel", "tree", {"id":"section:tracked", "event":"check"})
        self.assertEqual(panel.calls[-1], ("/workspace/stage", {"session_dir":"/s", "paths":["src/a.rs", "b.py"]}))

    def test_commit_clears_the_message_and_amend_prefills_it(self):
        panel = Fake()
        panel.refresh()
        panel.action("panel", "amend", {"checked":True, "value":None})
        self.assertEqual(panel.message, "Last")
        panel.action("panel", "commit", "Fix")
        self.assertEqual(panel.calls[-1], ("/workspace/commit", {"session_dir":"/s", "message":"Fix", "amend":True}))
        self.assertEqual((panel.message, panel.amend), ("", False))

    def test_server_errors_are_shown(self):
        panel = Fake()
        panel.refresh()
        def fail(*_, **__): raise RuntimeError("nothing is staged to commit")
        panel.api = fail
        panel.action("panel", "commit", "Fix")
        self.assertEqual(panel.error, "nothing is staged to commit")
        self.assertEqual(panel.message, "Fix")


class FilesColoursTest(unittest.TestCase):
    def test_folders_take_the_strongest_status(self):
        tree = files.Files()
        tree.session = "/s"
        tree.api = lambda path, query=None, **_: {"entries":[{"path":"src/a/x.py", "status":"untracked"},
                                                             {"path":"src/b.rs", "status":"modified"},
                                                             {"path":"lib/gone.c", "status":"deleted"}]}
        tree.refresh()
        self.assertEqual(tree.folders, {"src/a":"success", "src":"warning", "lib":"warning"})
        row = tree.row("lib/gone.c", "gone.c")
        self.assertTrue(row["strike"])
        self.assertEqual(row["icon"], "file-code")
        self.assertEqual([entry["action"] for entry in row["menu"]], ["open", "diff", "copy"])


if __name__ == "__main__":
    unittest.main()
