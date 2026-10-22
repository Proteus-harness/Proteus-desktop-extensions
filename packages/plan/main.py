import json
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
from proteus_extension import Extension


def plan_steps(tool):
    """Valid steps of an update_plan call; malformed arguments yield none."""
    args = tool.get("effective_args") or tool.get("args")
    if isinstance(args, str):
        try:
            args = json.loads(args)
        except ValueError:
            return []
    plan = args.get("plan") if isinstance(args, dict) else None
    return [step for step in plan if isinstance(step, dict) and isinstance(step.get("step"), str)] if isinstance(plan, list) else []


class Plan(Extension):
    def render(self, view, surface, argument):
        steps = []
        for entry in reversed(self.entries):
            tool = entry.get("tool")
            if tool and tool.get("name") == "update_plan" and (steps := plan_steps(tool)):
                break
        if not steps:
            self.badge(view, "План", ["Агент ещё не составил план."])
            return
        done = sum(step.get("status") == "completed" for step in steps)
        marks = {"completed": "✓", "in_progress": "•", "pending": "○"}
        self.badge(view, f"План: {done} из {len(steps)}",
                   [f"{marks.get(step.get('status'), '○')} {step['step']}" for step in steps],
                   {"type": "text", "icon":True, "text": f"{done}/{len(steps)}", "tone": "muted"})


if __name__ == "__main__":
    Plan().run()
