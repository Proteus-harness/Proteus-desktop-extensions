import json
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from proteus_extension import Extension, tokens
import context_view
from widgets import label


class Context(Extension):
    def __init__(self):
        super().__init__()
        self.snapshot = None

    def render(self, view, surface, argument):
        if surface == "popup":
            self.show(view, self.popup(view))
            return
        usage = (self.state or {}).get("context")
        if not usage:
            self.badge(view, "Контекст", ["Появится после первого запроса к модели."])
            return
        used, maximum, trigger = usage["used"], usage["max"], usage["trigger"]
        percent = min(100, used / maximum * 100)
        lines = [f"Занято {tokens(used)} из {tokens(maximum)}"]
        tone = "muted"
        if trigger is not None:
            lines.append(f"Автокомпакт при {tokens(trigger)}")
            tone = "danger" if used >= trigger else "warning" if used >= trigger * .8 else "muted"
        self.badge(view, f"Контекст: {percent:.0f}%", lines,
                   {"type": "ring", "percent": percent, "tone": tone})

    def popup(self, view):
        if self.session is None:
            return label("Сессия не открыта.", "muted")
        # The snapshot changes with the window usage the session reports, so
        # streaming updates reuse it; a freshly opened popup always asks anew.
        key = (self.session, json.dumps((self.state or {}).get("context"), sort_keys=True))
        if view not in self.shown or self.snapshot is None or self.snapshot[0] != key:
            self.snapshot = (key, self.api("/context"))
        return {"type": "stack", "gap": "medium", "children": context_view.render(self.snapshot[1])}


if __name__ == "__main__":
    Context().run()
