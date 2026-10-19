import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
from proteus_extension import Extension, stack, text, button, select


class Model(Extension):
    def __init__(self):
        super().__init__()
        self.models_open = False

    def render(self, view, surface, argument):
        config = (self.state or {}).get("config") or {}
        model = self.setting("model")
        reasoning = self.setting("reasoning_effort")
        name = self.setting_value("model") or (config.get("model") or {}).get("name", "model")
        effort = self.setting_value("reasoning_effort")
        extra = [setting for setting in self.session_settings()
                 if setting["id"] not in ("mode", "model", "reasoning_effort")]
        if surface == "badge":
            label = name
            if reasoning:
                selected = next((option for option in reasoning["options"] if option["value"] == effort), None)
                if selected: label += " · " + selected["name"]
            self.badge(view, "Модель и рассуждения", [f"Профиль {config.get('profile', '')}"],
                       {"type": "text", "caret": True, "text": label, "tone": "muted"},
                       hidden=not model and not reasoning and not extra)
        elif surface == "popup":
            if self.models_open or not reasoning and not extra:
                rows = [text("Модель", "muted")]
                if self.models_open and (reasoning or extra): rows.insert(0, button("‹ Назад", "back"))
                rows += [{"type": "choice", "label": option["name"], "selected": option["value"] == name,
                          "action": "model", "value": option["value"]} for option in (model or {}).get("options", [])]
            else:
                rows = [button(name + " ›", "models") if model else text(name)]
                if reasoning:
                    values = [option["value"] for option in reasoning["options"]]
                    rows += [{"type": "divider"}, text("Рассуждения", "muted"),
                             {"type": "steps", "id": "reasoning", "options": [option["name"] for option in reasoning["options"]],
                              "selected": values.index(effort) if effort in values else 0, "action": "effort"}]
                for setting in extra:
                    rows.append(select(setting["id"], setting["name"],
                                       [(option["value"], option["name"]) for option in setting["control"]["options"]],
                                       self.setting_value(setting["id"]), "setting:" + setting["id"]))
            self.show(view, stack(*rows))

    def event(self, event):
        if event == "session_opened": self.models_open = False

    def action(self, view, action, value):
        if action == "escape":
            if self.models_open: self.models_open = False
            else: self.send({"type": "close_popup"})
        elif action == "models": self.models_open = True
        elif action == "back": self.models_open = False
        elif action == "model":
            self.set_session("model", value)
            self.models_open = False
        elif action == "effort":
            control = self.setting("reasoning_effort")
            if control and isinstance(value, int) and 0 <= value < len(control["options"]):
                self.set_session("reasoning_effort", control["options"][value]["value"])
        elif action.startswith("setting:"):
            self.set_session(action.removeprefix("setting:"), value)


if __name__ == "__main__":
    Model().run()
