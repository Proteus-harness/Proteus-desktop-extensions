import sys
import time
from pathlib import Path, PurePosixPath
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
from proteus_extension import Extension, stack, text, button, segments, item, field, file_icon
from tree import search_rows, language

# Git status colours the tree like Zed: changed files and the folders above them.
TONES = {"conflict":"danger", "deleted":"danger", "untracked":"success", "added":"success",
         "renamed":"warning", "modified":"warning"}
FOLDER = ("danger", "warning", "success")


class Files(Extension):
    poll_seconds = 5

    def __init__(self):
        super().__init__()
        self.query = ""
        self.expanded = {""}
        self.listings = {}
        self.selected = None
        self.statuses = {}
        self.folders = {}
        self.deadline = 0
        self.truncated = False
        self.documents = {}
        self.modes = {}
        self.search = None

    def session_changed(self):
        self.query = ""
        self.modes.clear()

    def event(self, event):
        if event == "navigation_changed" and self.active_document:
            self.selected = self.active_document
            self.expanded.update(str(parent) for parent in PurePosixPath(self.active_document).parents if str(parent) != ".")
        if event == "session_opened":
            self.listings.clear()
            self.expanded = {""}
            self.selected = None
        if event in ("session_opened", "run_settled"):
            self.documents.clear()
            self.deadline = 0

    def refresh(self):
        if time.monotonic() < self.deadline:
            return
        changes = self.api("/workspace/changes")
        self.statuses = {entry["path"]: entry["status"] for entry in changes["entries"]}
        self.folders = {}
        for path, status in self.statuses.items():
            tone = TONES.get(status, "warning")
            if tone == "danger" and status != "conflict": tone = "warning"
            for parent in PurePosixPath(path).parents:
                parent = str(parent)
                if parent == ".": break
                current = self.folders.get(parent)
                if current is None or FOLDER.index(tone) < FOLDER.index(current):
                    self.folders[parent] = tone
        self.listings.clear()
        self.search = None
        self.deadline = time.monotonic() + self.poll_seconds

    def listing(self, path):
        if path not in self.listings:
            self.listings[path] = self.api("/workspace/list", {"path":path})
        return self.listings[path]

    def row(self, path, label, depth=0, directory=False):
        status = self.statuses.get(path)
        tone = self.folders.get(path, "default") if directory else TONES.get(status, "default")
        menu = [item("Открыть", "open", path, icon="external-link")] if not directory else []
        if status and not directory:
            menu.append(item("Показать изменения", "diff", path, icon="file-diff"))
        menu.append(item("Копировать путь", "copy", path, icon="copy", separated=bool(menu)))
        row = {"id":path, "label":label, "depth":depth, "expanded":path in self.expanded if directory else None,
               "tone":tone, "selected":path == self.selected, "menu":menu}
        if not directory: row["icon"] = file_icon(path)
        if status == "deleted": row["strike"] = True
        return row

    def rows(self):
        self.truncated = False
        if self.query:
            if self.search is None or self.search[0] != self.query:
                self.search = (self.query, self.api("/workspace/search", {"q":self.query}))
            result = self.search[1]
            self.truncated = result["truncated"]
            return search_rows(result, self.row)
        rows = []
        def visit(path, depth):
            try:
                listing = self.listing(path)
            except Exception:
                if not path:
                    raise
                # The directory vanished, for example the agent removed it.
                self.expanded.discard(path)
                return
            self.truncated |= listing["truncated"]
            for entry in listing["entries"]:
                directory = entry["kind"] == "directory"
                rows.append(self.row(entry["path"], entry["name"], depth, directory))
                if directory and entry["path"] in self.expanded:
                    visit(entry["path"], depth + 1)
        visit("", 0)
        return rows

    def render(self, view, surface, argument):
        if not self.session:
            self.show(view, text("Нет активной сессии.", "muted"))
            return
        if surface == "panel":
            self.refresh()
            rows = self.rows()
            warning = [text("Список ограничен сервером; уточните поиск.", "warning")] if self.truncated else []
            empty = [] if rows else [text("Ничего не найдено." if self.query else "Папка пуста.", "muted")]
            search = field("search", self.query, "Поиск по проекту…", "search")
            self.show(view, {"type":"fill", "children":[search, *warning, *empty,
                {"type":"tree", "id":"files", "rows":rows, "action":"file"}]},
                keys={"ctrl-f":"search-focus", "/":"search-focus", "escape":"escape"})
        elif surface == "document":
            self.show(view, self.document(argument))

    def document(self, path):
        mode = self.modes.get(path, "file")
        key = (path, mode)
        try:
            if key not in self.documents:
                self.documents[key] = self.api("/workspace/diff" if mode == "diff" else "/workspace/file", {"path":path})
            document = self.documents[key]
            content = document.get("patch") if mode == "diff" else document.get("text")
            syntax = "diff" if mode == "diff" else language(path)
            body = {"type":"code", "id":"content", "text":content, "language":syntax} if content is not None else text({"binary":"Двоичный файл", "too_large":"Файл превышает предел предпросмотра 512 КБ", "unavailable":"Изменений нет"}.get(document["kind"], "Нет текстового содержимого"), "muted")
        except Exception as error:
            body = text(error, "danger")
        header = stack(text(path),
                       segments("document-mode", [([path, "file"], "Файл"), ([path, "diff"], "Изменения")], [path, mode]),
                       button("", "copy", path, variant="ghost", icon="copy", hint="Копировать путь"), direction="row")
        return {"type":"fill", "children":[header, body]}

    def action(self, view, action, value):
        if action == "search-focus":
            self.send({"type":"focus", "view":view, "control":"search"})
        elif action == "escape":
            if self.query:
                self.query = ""
                control = "files"
            else: control = None
            self.send({"type":"focus", "view":view, "control":control})
        elif action == "search":
            self.query = value
        elif action == "copy":
            self.send({"type":"clipboard", "text":value})
        elif action == "document-mode":
            self.modes[value[0]] = value[1]
        elif action in ("open", "diff"):
            self.selected = value
            self.modes[value] = "diff" if action == "diff" else "file"
            self.documents.pop((value, self.modes[value]), None)
            self.open_document(value, preview=False, focus=True)
        elif action == "file":
            path, event = value["id"], value["event"]
            directory = any(row["id"] == path and row["expanded"] is not None for row in self.rows())
            self.selected = path
            if directory:
                if self.query and event in ("select", "open"):
                    self.query = ""
                    self.expanded.update(str(parent) for parent in PurePosixPath(path).parents if str(parent) != ".")
                if event in ("select", "open"):
                    self.expanded.symmetric_difference_update({path})
                elif event == "expand": self.expanded.add(path)
                elif event == "collapse": self.expanded.discard(path)
            elif event in ("select", "preview", "open"):
                self.modes[path] = "file"
                self.open_document(path, preview=event != "open", focus=event == "open")


if __name__ == "__main__":
    Files().run()
