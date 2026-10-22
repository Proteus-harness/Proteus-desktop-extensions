"""Small stdlib SDK for the public Proteus client extension protocol v3."""

import json
from pathlib import PurePosixPath
import queue
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request


class Extension:
    poll_seconds = None

    def __init__(self):
        self.server = None
        self.features = set()
        self.workspace = None
        self.session = None
        self.state = None
        self.awaiting_snapshot = False
        self.entries = []
        self.active_document = None
        self.views = {}
        self.shown = {}
        self.sequences = {}
        self.pending = {}
        self.command_error = None
        self.command_counter = 0
        self.direct = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def send(self, message):
        print(json.dumps(message, ensure_ascii=False, allow_nan=False), flush=True)

    def api(self, path, query=None, body=None, session=None, text=False, timeout=15):
        query = dict(query or {})
        selected = self.session if session is None else session
        if selected is not None:
            query["session_dir"] = selected
        url = self.server["origin"] + path
        if query:
            url += "?" + urllib.parse.urlencode(query)
        headers = {"Authorization": "Bearer " + self.server["token"]}
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(url, data=data, headers=headers)
        try:
            with self.direct.open(request, timeout=timeout) as response:
                result = response.read().decode()
        except urllib.error.HTTPError as error:
            # The server explains failures as {"error": "..."}; show that instead of the status line.
            try: reason = json.loads(error.read().decode()).get("error")
            except Exception: reason = None
            raise RuntimeError(reason or f"HTTP {error.code}") from None
        return result if text else json.loads(result)

    def badge(self, view, title, lines=(), face=None, hidden=False):
        self.send({"type": "badge", "view": view, "session_dir": self.session,
                   "hidden": hidden,
                   "title": title, "lines": list(lines) + ([self.command_error] if self.command_error else []),
                   "face": face or {"type": "icon", "tone": "muted"}})

    def set_session(self, kind, value):
        if "session.settings.v1" not in self.features:
            raise ValueError("The client does not support session.settings.v1")
        if self.session is not None:
            self.command_counter += 1
            identity = f"setting-{self.command_counter}"
            self.pending[identity] = (kind,value)
            self.command_error = None
            self.send({"type": "set_session", "id":identity, "session_dir": self.session,
                       "setting": {"id": kind, "value": value}})

    def session_settings(self):
        config = (self.state or {}).get("config") or {}
        return [setting for setting in config.get("session_settings", [])
                if setting["control"]["type"] == "select"]

    def setting(self, identity):
        return next((setting["control"] for setting in self.session_settings()
                     if setting["id"] == identity), None)

    def setting_value(self, identity):
        control = self.setting(identity)
        value = control["value"] if control else None
        return next((value for kind, value in reversed(list(self.pending.values()))
                     if kind == identity), value)

    def open_document(self, argument, preview=False, focus=True):
        self.send({"type":"open_document", "argument":argument, "preview":preview, "focus":focus})

    def render(self, view, surface, argument):
        raise NotImplementedError

    def render_all(self):
        for view, (surface, argument) in list(self.views.items()):
            if surface == "badge" and self.awaiting_snapshot:
                continue
            try:
                self.render(view, surface, argument)
            except Exception as error:
                if surface == "badge":
                    self.badge(view, "Ошибка расширения", [str(error)])
                else:
                    self.failed(view, error)

    def failed(self, view, error):
        """Keeps the last good view under the error, so its controls stay usable."""
        notice = stack(text(error, "danger"), button("Повторить", "refresh", icon="refresh-cw"), direction="row")
        last = self.shown.get(view)
        self.send({"type": "view", "session_dir":self.session, "view": view, "sequence":self.sequences.get(view,0), "keys":{},
                   "root": {"type": "fill", "children": [notice] + ([last] if last else [])}})

    def show(self, view, root, keys=None):
        self.shown[view] = root
        self.send({"type": "view", "session_dir":self.session, "view": view, "root": root, "sequence":self.sequences.get(view,0), "keys":keys or {}})

    def action(self, view, action, value):
        pass

    def event(self, event):
        pass

    def session_changed(self):
        """The active chat switched to another session; drop selections that belong to the old one."""

    def handle(self, message):
        kind = message["type"]
        if kind == "hello":
            if message["protocol"] != 3:
                raise ValueError("This extension requires protocol 3")
            self.features = set(message["features"])
            self.server, self.workspace = message["server"], message["workspace"]
        elif kind == "command_result":
            self.pending.pop(message["id"],None)
            if message["session_dir"] != self.session: return
            self.command_error = message.get("error")
            if message.get("config") is not None:
                if self.state is None: self.state={}
                self.state["config"]=message["config"]
            self.render_all()
        elif kind == "session_snapshot":
            self.session = message["session_dir"]
            self.awaiting_snapshot = False
            self.state, self.entries = message["state"], message["entries"]
            self.render_all()
        elif kind == "session_update":
            if message["session_dir"] != self.session:
                return
            update = message["update"]
            if update["type"] == "state":
                self.state = update["state"]
            elif update["type"] == "entries":
                start = update["start"]
                self.entries[start:start + update["removed"]] = update["entries"]
            self.render_all()
        elif kind == "render":
            self.session = message.get("session_dir")
            self.views[message["view"]] = (message["surface"], message.get("argument"))
            self.render_all()
        elif kind == "close":
            self.views.pop(message["view"], None)
            self.shown.pop(message["view"], None)
        elif kind == "event":
            self.active_document = message.get("document")
            if message["event"] == "session_opened":
                changed = message.get("session_dir") != self.session
                if changed:
                    # A process fed with session state gets a fresh snapshot right
                    # after this event; badges wait for it instead of blinking empty.
                    self.awaiting_snapshot = self.state is not None
                    self.state, self.entries = None, []
                    self.pending.clear()
                    self.command_error = None
                self.session = message.get("session_dir")
                if changed:
                    self.session_changed()
            self.event(message["event"])
            self.render_all()
        elif kind == "action":
            view = message["view"]
            self.sequences[view] = message["sequence"]
            try:
                self.action(view, message["action"], message.get("value"))
            except Exception as error:
                if self.views.get(view, ("",))[0] != "badge":
                    self.failed(view, error)
                return
            self.render_all()

    def run(self):
        inbox = queue.Queue()
        def read():
            try:
                for line in sys.stdin:
                    inbox.put(json.loads(line))
            finally:
                inbox.put(None)
        threading.Thread(target=read, daemon=True).start()
        while True:
            try:
                message = inbox.get(timeout=self.poll_seconds if self.views else None)
            except queue.Empty:
                self.render_all()
                continue
            if message is None:
                return
            try:
                self.handle(message)
            except Exception as error:
                if message.get("type") == "hello":
                    raise
                print(f"extension: {type(error).__name__}: {error}", file=sys.stderr, flush=True)


def tokens(value):
    if value is None: return "—"
    if value < 1000:
        return str(value)
    if value < 1000000:
        return f"{value / 1000:.0f}k"
    return f"{value / 1000000:.1f}M"


ICONS = {
    "file-code": "rs py pyi js mjs cjs jsx ts mts cts tsx c h cc cpp cxx hpp hh hxx go java kt swift rb php lua zig css scss html htm vue svelte",
    "file-braces": "json jsonc jsonl",
    "file-cog": "toml yaml yml ini cfg conf env",
    "file-text": "md markdown txt rst adoc",
    "file-terminal": "sh bash zsh fish ps1",
    "file-image": "png jpg jpeg gif svg webp ico bmp",
    "file-archive": "zip tar gz xz bz2 7z zst",
    "file-spreadsheet": "csv tsv xlsx ods",
    "file-diff": "diff patch",
    "file-lock": "lock",
}


def file_icon(path):
    """Lucide icon for a file by its extension, like the file icons in Zed."""
    name = PurePosixPath(path).name
    if name in ("Cargo.lock", "package-lock.json", "yarn.lock", "poetry.lock", "uv.lock"): return "file-lock"
    if name in ("Makefile", "Dockerfile", "justfile"): return "file-terminal"
    if name.startswith(".git") or name.startswith(".env"): return "file-cog"
    suffix = PurePosixPath(path).suffix.lstrip(".").lower()
    return next((icon for icon, extensions in ICONS.items() if suffix in extensions.split()), "file")


def text(value, tone="default"):
    return {"type": "text", "text": str(value), "tone": tone}


def stack(*children, direction="column"):
    return {"type": "stack", "direction": direction, "children": list(children)}


def button(label, action, value=None, variant=None, icon=None, selected=False, hint=None, confirm=None):
    node = {"type": "button", "label": label, "action": action, "value": value}
    if variant: node["variant"] = variant
    if icon: node["icon"] = icon
    if selected: node["selected"] = True
    if hint: node["hint"] = hint
    if confirm: node["confirm"] = confirm
    return node


def confirm(title, message=None, ok=None, danger=False):
    """Asks the user before a button or menu item sends its action."""
    node = {"title": title}
    if message: node["message"] = message
    if ok: node["ok"] = ok
    if danger: node["danger"] = True
    return node


def item(label, action, value=None, icon=None, danger=False, confirm=None, separated=False):
    """One entry of a `menu` node or a tree row's context menu."""
    node = {"label": label, "action": action}
    if value is not None: node["value"] = value
    if icon: node["icon"] = icon
    if danger: node["danger"] = True
    if confirm: node["confirm"] = confirm
    if separated: node["separated"] = True
    return node


def menu(items, label="", icon=None, hint=None):
    node = {"type": "menu", "label": label, "items": list(items)}
    if icon: node["icon"] = icon
    if hint: node["hint"] = hint
    return node


def checkbox(label, checked, action, value=None):
    node = {"type": "checkbox", "label": label, "checked": bool(checked), "action": action}
    if value is not None: node["value"] = value
    return node


def field(identity, value, placeholder, action, lines=1, submit=None):
    node = {"type": "input", "id": identity, "value": value, "placeholder": placeholder, "action": action}
    if lines != 1: node["lines"] = lines
    if submit: node["submit"] = submit
    return node


def segments(action, choices, selected):
    """A row of ghost buttons where the selected one is highlighted, like tabs."""
    return stack(*[button(label, action, value, variant="ghost", selected=value == selected)
                   for value, label in choices], direction="row")


def select(identity, label, choices, selected, action):
    return {"type":"select", "id":identity, "label":str(label), "action":action,
            "options":[{"label":str(label), "value":value, "selected":value == selected} for value,label in choices]}


def metric(label, value, detail=None):
    return {"type":"metric", "label":label, "value":str(value), "detail":detail}


def code(identity, value, height=280):
    return {"type":"code", "id":identity, "text":json.dumps(value,ensure_ascii=False,indent=2), "language":"json", "height":height}


class Analytics(Extension):
    poll_seconds = 5

    def __init__(self):
        super().__init__()
        self.chosen = None
        self.cached = {}

    @property
    def selected_session(self):
        return self.chosen or self.session

    def fetch(self, path, query=None, active=False):
        session = self.session if active else self.selected_session
        key = (path, session, json.dumps(query,sort_keys=True))
        cached = self.cached.get(key)
        if cached is None or (self.poll_seconds and time.monotonic() - cached[0] >= self.poll_seconds):
            value = self.api(path, query, session=session)
            self.cached[key] = (time.monotonic(), value)
        return self.cached[key][1]

    def session_picker(self):
        sessions = self.fetch("/sessions")
        choices = [(session["session_dir"], (session.get("preview") or "Новая сессия")[:60]
                    + f" · {session['message_count']} сообщ.") for session in sessions]
        title = next((label for value,label in choices if value == self.selected_session), "Сохранённая сессия")
        return select("session",title,choices,self.selected_session,"session")

    def session_changed(self):
        if self.chosen is None:
            self.selection_changed()

    def selection_changed(self):
        """The inspected session changed; drop filters that belong to the old one."""

    def action(self, view, action, value):
        if action == "session":
            self.chosen = value
            self.cached.clear()
            self.selection_changed()
        elif action == "refresh":
            self.cached.clear()
