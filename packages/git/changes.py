"""Rows of the git panel tree, built from /workspace/changes and /workspace/log; independent of the client."""
import time
from pathlib import PurePosixPath

TONES = {"conflict":"danger", "deleted":"danger", "untracked":"success", "added":"success",
         "renamed":"warning", "modified":"warning"}
LABELS = {"conflict":"конфликт", "deleted":"удалён", "untracked":"новый", "added":"добавлен",
          "renamed":"переименован", "modified":"изменён"}
SECTIONS = (("conflicts", "Конфликты"), ("tracked", "Отслеживаемые"), ("untracked", "Новые"))


def section(entry):
    if entry["status"] == "conflict": return "conflicts"
    if entry.get("unstaged") == "untracked" and not entry.get("staged"): return "untracked"
    return "tracked"


def groups(entries):
    result = {}
    for entry in entries:
        result.setdefault(section(entry), []).append(entry)
    return result


def ago(seconds, now=None):
    delta = max(0, int((now or time.time()) - seconds))
    for size, unit in ((86400 * 365, "г"), (86400 * 30, "мес"), (86400, "дн"), (3600, "ч"), (60, "мин")):
        if delta >= size: return f"{delta // size} {unit} назад"
    return "только что"


def file_menu(entry, item, confirm):
    path = entry["path"]
    menu = [item("Показать изменения", "diff", path, icon="file-diff")]
    if entry.get("unstaged"): menu.append(item("Добавить в индекс", "stage", [path], icon="plus"))
    if entry.get("staged"): menu.append(item("Убрать из индекса", "unstage", [path], icon="minus"))
    menu.append(item("Копировать путь", "copy", path, icon="copy", separated=True))
    menu.append(item("Отменить изменения", "discard", [path], icon="undo-2", danger=True,
                     confirm=confirm(f"Отменить изменения в {PurePosixPath(path).name}?",
                                     "Новый файл будет удалён, изменённый вернётся к HEAD. Это нельзя отменить.",
                                     "Отменить изменения", danger=True)))
    return menu


def change_rows(entries, collapsed, selected, file_icon, item, confirm):
    rows = []
    sections = groups(entries)
    for key, title in SECTIONS:
        items = sections.get(key)
        if not items: continue
        staged = sum(1 for entry in items if entry.get("staged"))
        rows.append({"id":f"section:{key}", "label":title, "detail":str(len(items)), "depth":0,
                     "expanded":key not in collapsed, "checked":staged == len(items), "tone":"muted"})
        if key in collapsed: continue
        for entry in items:
            path = PurePosixPath(entry["path"])
            parts = [str(path.parent)] if str(path.parent) != "." else []
            if entry.get("original"): parts.append(f"← {PurePosixPath(entry['original']).name}")
            if entry.get("staged") and entry.get("unstaged"): parts.append("частично")
            row = {"id":entry["path"], "label":path.name, "detail":"  ".join(parts), "depth":1, "expanded":None,
                   "tone":TONES.get(entry["status"], "warning"), "selected":entry["path"] == selected,
                   "checked":bool(entry.get("staged")), "icon":file_icon(entry["path"]),
                   "menu":file_menu(entry, item, confirm)}
            if entry["status"] == "deleted": row["strike"] = True
            rows.append(row)
    return rows


def history_rows(log, expanded, item, now=None):
    if log is None and not expanded: log = []
    rows = [{"id":"section:history", "label":"История", "detail":"", "depth":0, "expanded":expanded, "tone":"muted"}]
    if expanded:
        for entry in log or []:
            rows.append({"id":f"commit:{entry['hash']}", "label":entry["summary"] or "(без описания)",
                         "detail":f"{entry['short']}  {entry['author']}  {ago(entry['time'], now)}", "depth":1, "expanded":None,
                         "icon":"git-commit-horizontal", "tone":"default",
                         "menu":[item("Копировать хеш", "copy", entry["hash"], icon="copy")]})
    return rows


def section_paths(entries, key, staged):
    """Paths of a section to stage (staged=False) or unstage (staged=True)."""
    items = groups(entries).get(key, [])
    return [entry["path"] for entry in items if (entry.get("staged") if staged else entry.get("unstaged"))]
