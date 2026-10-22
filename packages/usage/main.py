import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from proteus_extension import Analytics, stack, button, segments
import context_view
import request_view
from widgets import label, row


TABS = [("requests", "Запросы и расход"), ("context", "Контекст и инструменты")]


class Usage(Analytics):
    def __init__(self):
        super().__init__()
        self.tab = "requests"
        self.period = "all"
        self.model = None
        self.status = "all"
        self.order = "oldest"
        self.query = ""
        self.page = 0
        self.expanded = set()

    def render(self, view, surface, argument):
        if not self.selected_session:
            self.show(view, label("Нет выбранной сессии.", "muted"))
            return
        header = [row(stack(label("Расход и контекст", size="large"),
                            label("Запросы модели, оценка стоимости и окно контекста выбранной сессии.", "muted")),
                      self.session_picker(), button("Обновить", "refresh", icon="refresh"), gap="medium"),
                  segments("tab", TABS, self.tab)]
        if self.tab == "context":
            body = context_view.render(self.fetch("/context"))
        else:
            snapshot = self.fetch("/usage")
            body = (request_view.render(self, snapshot) if snapshot is not None
                    else [label("Журнал расхода недоступен для этого чата.", "muted")])
        self.show(view, {**stack(*header, *body), "gap": "medium"})

    def selection_changed(self):
        self.period, self.model, self.page = "all", None, 0
        self.expanded.clear()

    def action(self, view, action, value):
        super().action(view, action, value)
        if action in ("tab", "period", "model", "status", "order", "query", "page"):
            setattr(self, action, value)
            if action != "page": self.page = 0
        elif action == "request":
            self.expanded.symmetric_difference_update({value})


if __name__ == "__main__":
    Usage().run()
