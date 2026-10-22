import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
from proteus_extension import Analytics, stack, text, button, metric, segments
from blocks import badge, card, heading, padded, row, small
import catalog
import mapview
from topology import nodes

TABS = [("map", "Карта связей"), ("catalog", "Каталог сборки")]
SEVERITIES = {"error":("Ошибка", "danger"), "warning":("Предупреждение", "warning"),
              "warn":("Предупреждение", "warning"), "info":("Сведения", "muted")}


def status(snapshot):
    registered = sum(tool["registered"] for tool in snapshot["tools"])
    return (f"{len(snapshot['slots'])} слотов · {registered} инструментов · "
            f"{len(snapshot['modules'])} модулей · {len(snapshot['warnings'])} предупреждений")


def metrics(snapshot):
    model = snapshot.get("model")
    hooks = " → ".join(snapshot.get("hooks") or []) or "Отключены"
    return row(metric("Профиль", snapshot["profile"] or "default"),
               metric("Модель", f"{model['provider']}/{model['name']}" if model else "Модель не выбрана"),
               metric("Hooks · порядок", hooks),
               metric("Состав", f"{len(snapshot['modules'])} модулей · {len(snapshot['tools'])} инструментов"))


def warnings(snapshot):
    if not snapshot["warnings"]: return None
    lines = []
    for warning in snapshot["warnings"]:
        label, tone = SEVERITIES.get(warning["severity"], (warning["severity"], "warning"))
        lines.append(row(badge(label, tone), text(warning["message"])))
    return card("Предупреждения", *lines)


class Architecture(Analytics):
    poll_seconds = None

    def __init__(self):
        super().__init__()
        self.tab = "map"
        self.scope = "assembly"
        self.inactive = False
        self.query = ""
        self.selected = None
        self.tool_filter = "all"
        self.tool_query = ""
        self.schema = None
        self.notice = None
        self.snapshot = None

    def event(self, event):
        if event in ("session_opened", "run_settled"): self.snapshot = None

    def session_changed(self):
        self.selected = self.schema = self.notice = None

    def header(self, snapshot):
        unregistered = sum(not tool["registered"] for tool in snapshot["tools"])
        controls = row(segments("tab", TABS, self.tab),
                       button("Mermaid", "mermaid", variant="ghost", icon="copy", hint="Копировать как Mermaid"),
                       button("", "refresh", variant="ghost", icon="refresh-cw", hint="Обновить"),
                       small(f"{len(snapshot['warnings'])} предупреждений · {unregistered} незарегистрированных инструментов"))
        notice = text(self.notice, "danger" if self.notice.startswith("Не удалось") else "success") if self.notice else None
        top = stack(heading("Архитектура сборки"), small(status(snapshot)), controls, *([notice] if notice else []))
        alerts = warnings(snapshot)
        return [top, metrics(snapshot)] + ([alerts] if alerts else [])

    def render(self, view, surface, argument):
        if not self.session:
            self.show(view, text("Нет активной сессии.", "muted"))
            return
        if self.snapshot is None: self.snapshot = self.api("/inspect/topology")
        snapshot = self.snapshot
        graph = nodes(snapshot)
        if self.selected and not any(item["id"] == self.selected for item in graph): self.selected = None
        head = self.header(snapshot)
        if self.tab == "catalog":
            self.show(view, stack(*head, *catalog.view(snapshot, self.tool_filter, self.tool_query, self.schema)))
            return
        head = [part if part["type"] == "stack" else padded(part) for part in head]
        body = mapview.view(snapshot, graph, self.scope, self.inactive, self.selected, self.query)
        self.show(view, {"type":"fill", "children":head + body})

    def action(self, view, action, value):
        if action in ("tab", "scope", "inactive", "query"): setattr(self, action, value)
        elif action == "tool-filter": self.tool_filter = value
        elif action == "tool-query": self.tool_query = value
        elif action == "schema": self.schema = None if self.schema == value else value
        elif action == "refresh": self.snapshot, self.notice = None, None
        elif action == "select": self.selected = None if self.selected == value else value
        elif action == "goto": self.selected = value
        elif action == "mermaid": self.copy_mermaid()

    def copy_mermaid(self):
        try:
            diagram = self.api("/inspect/topology.mmd", text=True)
        except Exception as error:
            self.notice = f"Не удалось получить Mermaid: {error}"
            return
        self.send({"type":"clipboard", "text":diagram})
        self.notice = "Mermaid скопирован"


if __name__ == "__main__":
    Architecture().run()
