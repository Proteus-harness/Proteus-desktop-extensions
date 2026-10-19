import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
from proteus_extension import Extension, stack, text


class Mode(Extension):
    def render(self, view, surface, argument):
        control = self.setting("mode")
        current = self.setting_value("mode")
        if surface == "badge":
            self.badge(view, "Режим разрешений", ["Что агент делает без подтверждения."],
                       {"type": "text", "caret":True, "text": current or "—", "tone": "muted"}, hidden=control is None)
        elif surface == "popup":
            self.show(view, stack(text("Режим разрешений", "muted"), *[
                {"type": "choice", "label": option["name"], "selected": option["value"] == current,
                 "action": "mode", "value": option["value"]} for option in (control or {}).get("options", [])]))

    def action(self, view, action, value):
        if action == "mode":
            self.set_session("mode", value)
            self.send({"type": "close_popup"})


if __name__ == "__main__":
    Mode().run()
