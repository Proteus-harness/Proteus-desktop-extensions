import sys
import time
from pathlib import Path, PurePosixPath
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
from proteus_extension import Extension, stack, text, button, item, menu, confirm, checkbox, field, file_icon
from changes import change_rows, history_rows, section_paths, groups, LABELS


class Git(Extension):
    poll_seconds = 5

    def __init__(self):
        super().__init__()
        self.changes = None
        self.deadline = 0
        self.branches = None
        self.log = None
        self.head = None
        self.collapsed = set()
        self.history = False
        self.selected = None
        self.message = ""
        self.amend = False
        self.error = None
        self.documents = {}

    def session_changed(self):
        self.message, self.amend, self.error, self.selected = "", False, None, None

    def event(self, event):
        if event in ("session_opened", "run_settled"):
            self.deadline = 0

    def refresh(self):
        if time.monotonic() >= self.deadline:
            self.update(self.api("/workspace/changes"))

    def update(self, changes):
        if changes != self.changes: self.documents.clear()
        head = (changes.get("branch"), (changes.get("head") or {}).get("short"))
        if head != self.head: self.branches, self.log, self.head = None, None, head
        self.changes = changes
        self.deadline = time.monotonic() + self.poll_seconds

    def write(self, path, **body):
        try:
            result = self.api(path, body={"session_dir":self.session, **body}, timeout=120)
        except Exception as error:
            self.error = str(error)
            return None
        self.error = None
        self.update(result["changes"] if "commit" in result else result)
        return result

    def branch_list(self):
        if self.branches is None:
            self.branches = self.api("/workspace/branches")["branches"]
        return self.branches

    def render(self, view, surface, argument):
        if not self.session:
            if surface == "badge": self.badge(view, "Git", hidden=True)
            else: self.show(view, text("Нет активной сессии.", "muted"))
            return
        self.refresh()
        changes = self.changes
        branch = changes.get("branch") or (changes.get("head") or {}).get("short") or "—"
        if surface == "badge":
            lines = [f"Изменений: {len(changes['entries'])}"] + self.sync_lines()
            self.badge(view, f"Ветка {branch}", lines, {"type":"text", "text":branch, "icon":True, "caret":True, "tone":"muted"},
                       hidden=not changes["repository"])
        elif surface == "popup":
            self.show(view, self.branch_picker(branch))
        elif surface == "panel":
            self.show(view, self.panel(branch), keys={"ctrl-enter":"commit"})
        elif surface == "document":
            self.show(view, self.document(argument))

    def sync_lines(self):
        changes = self.changes
        if not changes.get("upstream"): return ["Без upstream"]
        return [f"{changes['upstream']}: ↑{changes['ahead']} ↓{changes['behind']}"]

    def branch_picker(self, current):
        choices = [{"type":"choice", "label":entry["name"], "selected":entry["name"] == current,
                    "action":"checkout", "value":entry["name"]} for entry in self.branch_list()]
        notice = [text(self.error, "danger")] if self.error else []
        return stack(text("Переключить ветку", "muted"), *notice, *choices or [text("Веток нет.", "muted")])

    def panel(self, branch):
        changes = self.changes
        if not changes["repository"]:
            return text("Папка не является git-репозиторием.", "muted")
        entries = changes["entries"]
        staged = sum(1 for entry in entries if entry.get("staged"))
        branches = [item(entry["name"], "checkout", entry["name"], icon="check" if entry["name"] == branch else None)
                    for entry in self.branch_list()[:40]]
        actions = menu([item("Добавить всё в индекс", "stage_all", icon="plus"),
                        item("Убрать всё из индекса", "unstage_all", icon="minus"),
                        item("Отменить все изменения", "discard_all", icon="undo-2", danger=True, separated=True,
                             confirm=confirm("Отменить все изменения?", "Изменённые файлы вернутся к HEAD, новые будут удалены. Это нельзя отменить.",
                                             "Отменить всё", danger=True))], hint="Действия")
        header = stack(menu(branches or [item("Веток нет", "noop")], label=branch, icon="git-branch", hint="Переключить ветку"),
                       text("  ".join(self.sync_lines()), "muted"), actions, direction="row")
        rows = change_rows(entries, self.collapsed, self.selected, file_icon, item, confirm)
        if self.history and self.log is None:
            self.log = self.api("/workspace/log", {"limit":30})["entries"]
        rows += history_rows(self.log, self.history, item)
        notice = [text(self.error, "danger")] if self.error else []
        empty = [] if entries else [text("Изменений нет.", "muted")]
        label = f"Закоммитить {staged} {plural(staged)}" if staged else "Закоммитить"
        commit = stack(field("message", self.message, "Сообщение коммита (Ctrl+Enter)", "message", lines=4, submit="commit"),
                       stack(checkbox("Изменить последний коммит", self.amend, "amend"),
                             button(label, "commit", variant="primary", icon="git-commit-horizontal"), direction="row"))
        return {"type":"fill", "children":[header, *notice, *empty, {"type":"tree", "id":"changes", "rows":rows, "action":"tree"}, commit]}

    def document(self, path):
        entry = next((entry for entry in (self.changes or {}).get("entries", []) if entry["path"] == path), None)
        try:
            if path not in self.documents:
                self.documents[path] = self.api("/workspace/diff", {"path":path})
            diff = self.documents[path]
            body = ({"type":"code", "id":"diff", "text":diff["patch"], "language":"diff"} if diff.get("patch") is not None
                    else text({"binary":"Двоичный файл", "too_large":"Diff превышает предел 512 КБ"}.get(diff["kind"], "Изменений нет"), "muted"))
        except Exception as error:
            body = text(error, "danger")
        buttons = []
        if entry and entry.get("unstaged"): buttons.append(button("В индекс", "stage", [path], icon="plus"))
        if entry and entry.get("staged"): buttons.append(button("Из индекса", "unstage", [path], icon="minus"))
        if entry:
            buttons.append(button("Отменить", "discard", [path], variant="ghost", icon="undo-2",
                                  confirm=confirm(f"Отменить изменения в {PurePosixPath(path).name}?", "Это нельзя отменить.", "Отменить", danger=True)))
        status = [text(LABELS.get(entry["status"], entry["status"]), "muted")] if entry else []
        header = stack(text(path), *status, *buttons, button("", "copy", path, variant="ghost", icon="copy", hint="Копировать путь"), direction="row")
        notice = [text(self.error, "danger")] if self.error else []
        return {"type":"fill", "children":[header, *notice, body]}

    def action(self, view, action, value):
        entries = (self.changes or {}).get("entries", [])
        if action == "message":
            self.message = value
        elif action == "amend":
            self.amend = value["checked"]
            if self.amend and not self.message.strip():
                self.message = (self.changes.get("head") or {}).get("summary", "")
        elif action == "commit":
            message = value if isinstance(value, str) else self.message
            if self.write("/workspace/commit", message=message, amend=self.amend):
                self.message, self.amend = "", False
            else: self.message = message
        elif action in ("stage", "unstage", "discard"):
            self.write(f"/workspace/{action}", paths=value)
        elif action in ("stage_all", "unstage_all", "discard_all"):
            self.write(f"/workspace/{action.removesuffix('_all')}", all=True)
        elif action == "checkout":
            if self.write("/workspace/checkout", branch=value) and view == "popup":
                self.send({"type":"close_popup"})
        elif action == "copy":
            self.send({"type":"clipboard", "text":value})
        elif action == "diff":
            self.selected = value
            self.open_document(value, preview=False, focus=True)
        elif action == "refresh":
            self.deadline = 0
        elif action == "tree":
            self.tree(value["id"], value["event"], entries)

    def tree(self, identity, event, entries):
        if identity == "section:history":
            if event in ("select", "open", "expand", "collapse"):
                self.history = event == "expand" or (event != "collapse" and not self.history)
            return
        if identity.startswith("section:"):
            key = identity.removeprefix("section:")
            if event == "check":
                unstage = all(entry.get("staged") for entry in groups(entries).get(key, []))
                paths = section_paths(entries, key, unstage)
                if paths: self.write("/workspace/unstage" if unstage else "/workspace/stage", paths=paths)
            elif event in ("select", "open"): self.collapsed.symmetric_difference_update({key})
            elif event == "expand": self.collapsed.discard(key)
            elif event == "collapse": self.collapsed.add(key)
            return
        if identity.startswith("commit:"):
            return
        entry = next((entry for entry in entries if entry["path"] == identity), None)
        if entry is None: return
        self.selected = identity
        if event == "check":
            self.write("/workspace/unstage" if entry.get("staged") else "/workspace/stage", paths=[identity])
        elif event in ("select", "preview", "open"):
            self.open_document(identity, preview=event != "open", focus=event == "open")


def plural(count):
    if count % 10 == 1 and count % 100 != 11: return "файл"
    if count % 10 in (2, 3, 4) and count % 100 not in (12, 13, 14): return "файла"
    return "файлов"


if __name__ == "__main__":
    Git().run()
