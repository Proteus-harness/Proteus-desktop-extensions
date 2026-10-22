import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
from proteus_extension import Extension, stack, text, button, select

DEFAULT = "_default"


class Model(Extension):
    def __init__(self):
        super().__init__()
        self.models_open = False

    def config(self):
        return (self.state or {}).get("config") or {}

    def efforts(self):
        """Explicit efforts, weakest first; the profile default is the reset button."""
        control = self.setting("reasoning_effort")
        return [option for option in (control or {}).get("options", []) if option["value"] != DEFAULT]

    def effort_position(self, efforts):
        values = [option["value"] for option in efforts]
        effort = self.setting_value("reasoning_effort")
        if effort in values:
            return values.index(effort)
        fallback = self.current_option().get("default_reasoning_effort")
        return values.index("effort:" + fallback) if fallback and "effort:" + fallback in values else 0

    def current_option(self):
        name = self.setting_value("model") or (self.config().get("model") or {}).get("name")
        return next((option for option in self.config().get("model_options", []) if option["name"] == name), {})

    def provider(self):
        providers = {option["provider"] for option in self.config().get("model_options", [])}
        return providers.pop() if len(providers) == 1 else None

    def model_label(self, value, name):
        option = next((option for option in self.config().get("model_options", []) if option["name"] == value), None)
        return f"{option['provider']}/{name}" if option and self.provider() is None else name

    def render(self, view, surface, argument):
        config = self.config()
        model = self.setting("model")
        name = self.setting_value("model") or (config.get("model") or {}).get("name", "model")
        efforts = self.efforts()
        effort = self.setting_value("reasoning_effort")
        extra = [setting for setting in self.session_settings()
                 if setting["id"] not in ("mode", "model", "reasoning_effort")]
        if surface == "badge":
            label = name
            if efforts:
                label += " · " + (efforts[self.effort_position(efforts)]["name"] if effort != DEFAULT else "по профилю")
            self.badge(view, "Модель и рассуждения", [f"Профиль {config.get('profile', '')}"],
                       {"type": "text", "caret": True, "text": label, "tone": "muted"},
                       hidden=not model and not efforts and not extra)
        elif surface == "popup":
            if self.models_open or len(efforts) < 2 and not extra:
                self.show(view, stack(*self.model_list(model, name, back=len(efforts) >= 2 or bool(extra))))
                return
            rows = [button(name + " ›", "models", variant="ghost", hint="Выбрать модель") if model else text(name)]
            if len(efforts) >= 2:
                shown = self.effort_position(efforts)
                title = "Рассуждения · " + (efforts[shown]["name"] if effort != DEFAULT else "как в профиле")
                header = [text(title, "muted")]
                if effort != DEFAULT:
                    header.append(button("", "effort_reset", variant="ghost", icon="reset", hint="Как в профиле"))
                rows += [{"type": "divider"}, stack(*header, direction="row"),
                         {"type": "steps", "id": "reasoning", "options": [option["name"] for option in efforts],
                          "selected": shown, "action": "effort"}]
            for setting in extra:
                rows.append(select(setting["id"], setting["name"],
                                   [(option["value"], option["name"]) for option in setting["control"]["options"]],
                                   self.setting_value(setting["id"]), "setting:" + setting["id"]))
            self.show(view, stack(*rows))

    def model_list(self, model, name, back):
        provider = self.provider()
        title = text("Модель · " + provider if provider else "Модель", "muted")
        header = stack(button("", "back", variant="ghost", icon="back", hint="Назад"), title, direction="row") if back else title
        return [header] + [{"type": "choice", "label": self.model_label(option["value"], option["name"]),
                            "selected": option["value"] == name, "action": "model", "value": option["value"]}
                           for option in (model or {}).get("options", [])]

    def event(self, event):
        if event == "session_opened": self.models_open = False

    def action(self, view, action, value):
        if action == "escape":
            if self.models_open: self.models_open = False
            else: self.send({"type": "close_popup"})
        elif action == "models": self.models_open = True
        elif action == "back": self.models_open = False
        elif action == "model":
            if value != self.setting_value("model"):
                self.set_session("model", value)
            self.models_open = False
        elif action == "effort":
            efforts = self.efforts()
            # The stepper repeats its edge value; only real changes reach the server.
            if isinstance(value, int) and 0 <= value < len(efforts) and efforts[value]["value"] != self.setting_value("reasoning_effort"):
                self.set_session("reasoning_effort", efforts[value]["value"])
        elif action == "effort_reset":
            if self.setting_value("reasoning_effort") != DEFAULT:
                self.set_session("reasoning_effort", DEFAULT)
        elif action.startswith("setting:"):
            self.set_session(action.removeprefix("setting:"), value)


if __name__ == "__main__":
    Model().run()
