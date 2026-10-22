import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from proteus_extension import Analytics, button
import facts
from facts import title, status
from details import step_card
from nodes import badge, column, ghost, label, row
from summary import turn_list, turn_summary

LONGEST = 400
OPEN = (":args", ":output", ":answer", ":summary")


class Analysis(Analytics):
    def __init__(self):
        super().__init__()
        self.turn = None
        self.step = None
        self.kind = "all"
        self.problems = False
        self.query = ""
        self.all_turns = False
        self.toggled = set()

    def is_open(self, key):
        """Disclosures with an OPEN suffix start revealed; a click flips any of them."""
        return (key in self.toggled) != key.endswith(OPEN)

    def render(self, view, surface, argument):
        if not self.selected_session:
            self.show(view, label("Нет выбранной сессии.", "muted"))
            return
        data = self.fetch("/analysis", {"turn_id":self.turn} if self.turn else None)
        selected = data.get("selected")
        body = [label("Анализ ходов", size="large"),
                label("От задачи до результата — по сохранённым фактам. Просмотр не запускает ход и не меняет активный чат.", "muted"),
                row(self.session_picker(), button("Обновить", "refresh", icon="refresh-cw")),
                label(f"Сессия {facts.short_id(data['session_id'])} · версия журнала {data['revision']}", "muted", "small")]
        if not selected:
            self.show(view, column(*body, label("В журнале пока нет ходов.", "muted"), gap="medium"))
            return
        turn = next((turn for turn in data["turns"] if turn["turn_id"] == selected["turn_id"]), {})
        steps = selected["steps"]
        if self.step != -1 and not any(step["sequence"] == self.step for step in steps):
            self.step = facts.default_step(steps)
        current = next((step for step in steps if step["sequence"] == self.step), None)
        details = [turn_summary(selected, turn.get("number"), turn.get("status"), self.is_open)]
        if current: details.append(step_card(current, self.is_open))
        # Turns, steps and details side by side, each column scrolling on its own.
        self.show(view, {"type":"fill", "children":[{"type":"split", "widths":[260, 320, None], "children":[
            column(*body, turn_list(data["turns"], selected["turn_id"], self.all_turns), gap="medium"),
            self.step_list(steps),
            column(*details, gap="medium")]}]})

    def step_list(self, steps):
        counts = {kind:sum(facts.kind(step) == kind for step in steps) for kind in facts.KINDS}
        kinds = [("all", f"Все · {len(steps)}")] + [(kind, f"{name} · {counts[kind]}") for kind, name in facts.KINDS.items() if counts[kind] or kind == self.kind]
        shown = [step for step in steps if facts.matches(step, self.kind, self.problems, self.query)]
        nodes = [row(*[ghost(name, "kind", kind, kind == self.kind) for kind, name in kinds],
                     ghost(f"Ошибки и без результата · {sum(map(facts.problem, steps))}", "problems", not self.problems, self.problems)),
                 {"type":"input", "id":"query", "value":self.query, "placeholder":"Название шага…", "action":"query"}]
        if not shown: nodes.append(label("Подходящих шагов нет." if steps else "В ходе нет записанных шагов.", "muted"))
        for step in shown[:LONGEST]:
            state = status(step)
            nodes.append(column(row(ghost(f"#{step['sequence']} · {title(step)}", "step", step["sequence"], step["sequence"] == self.step), badge(state, facts.tone(state))),
                                label(f"{facts.KINDS[facts.kind(step)]} · {facts.duration(step['started_at_ms'], step.get('finished_at_ms'))}", "muted", "small")))
        if len(shown) > LONGEST: nodes.append(label(f"Показаны первые {LONGEST} из {len(shown)} — уточните фильтр.", "muted", "small"))
        heading = f"Шаги · {len(shown)} из {len(steps)}" if len(shown) != len(steps) else f"Шаги · {len(steps)}"
        return column(label(heading, size="large"), *nodes)

    def selection_changed(self):
        self.turn, self.step = None, None
        self.toggled.clear()

    def event(self, event):
        if event == "run_settled" and self.chosen is None:
            self.cached = {key:value for key, value in self.cached.items() if key[0] != "/analysis"}

    def action(self, view, action, value):
        super().action(view, action, value)
        if action == "turn":
            self.turn, self.step = value, None
        elif action == "kind": self.kind = value
        elif action == "problems": self.problems = bool(value)
        elif action == "query": self.query = value or ""
        elif action == "all-turns": self.all_turns = not self.all_turns
        elif action == "open": self.toggled ^= {value}
        elif action == "step":
            sequence = int(value["id"] if isinstance(value, dict) else value)
            self.step = -1 if sequence == self.step else sequence


if __name__ == "__main__":
    Analysis().run()
