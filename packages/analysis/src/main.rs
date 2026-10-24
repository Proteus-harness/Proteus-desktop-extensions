mod details;
mod facts;
mod nodes;
mod summary;
#[cfg(test)]
mod tests;
mod text;

use std::time::Duration;

use proteus_extension::ui::{Node, button};
use proteus_extension::{
    Analytics, Extension, Host, Result, Surface, Value, ValueExt, bail, json, run,
};

use nodes::{Open, badge, column, ghost, label, large, row, small};
use text::{plain, short_id};

const POLL: Duration = Duration::from_secs(5);
const LONGEST: usize = 400;

struct Analysis {
    analytics: Analytics,
    turn: Option<String>,
    /// Selected step; `-1` when the user collapsed it.
    step: Option<i64>,
    kind: String,
    problems: bool,
    query: String,
    all_turns: bool,
}

impl Default for Analysis {
    fn default() -> Self {
        Self {
            analytics: Analytics::new(Some(POLL)),
            turn: None,
            step: None,
            kind: "all".to_owned(),
            problems: false,
            query: String::new(),
            all_turns: false,
        }
    }
}

impl Analysis {
    /// The inspected session changed; drop selections that belong to the old one.
    fn selection_changed(&mut self) {
        self.turn = None;
        self.step = None;
    }

    fn page(&mut self, data: &Value, picker: Value) -> Value {
        let selected = data.at("selected");
        let mut body = vec![
            large("Анализ ходов"),
            label(
                "От задачи до результата — по сохранённым фактам. Просмотр не запускает ход и не меняет активный чат.",
                "muted",
            ),
            row(vec![
                picker,
                button("Обновить", "refresh").with("icon", "refresh-cw"),
            ]),
            small(
                format!(
                    "Сессия {} · версия журнала {}",
                    short_id(data.at("session_id")),
                    plain(data.at("revision"))
                ),
                "muted",
            ),
        ];
        if !selected.truthy() {
            body.push(label("В журнале пока нет ходов.", "muted"));
            return column(body).with("gap", "medium");
        }
        let turn = data
            .list("turns")
            .iter()
            .find(|turn| turn.at("turn_id") == selected.at("turn_id"))
            .unwrap_or(&Value::Null);
        let steps = selected.list("steps");
        if self.step != Some(-1) && !steps.iter().any(|step| step.i64("sequence") == self.step) {
            self.step = facts::default_step(steps);
        }
        let current = steps.iter().find(|step| step.i64("sequence") == self.step);
        let open = Open;
        let mut details = vec![summary::turn_summary(
            selected,
            turn.at("number"),
            turn.at("status"),
            &open,
        )];
        details.extend(current.map(|step| details::step_card(step, &open)));
        body.push(summary::turn_list(
            data.list("turns"),
            selected.at("turn_id"),
            self.all_turns,
        ));
        // Turns, steps and details side by side, each column scrolling on its own.
        let split = json!({"type": "split", "widths": [260, 320, null], "children": [
            column(body).with("gap", "medium"),
            self.step_list(steps),
            column(details).with("gap", "medium"),
        ]});
        json!({"type": "fill", "children": [split]})
    }

    fn step_list(&self, steps: &[Value]) -> Value {
        let count = |kind: &str| {
            steps
                .iter()
                .filter(|step| facts::kind(step) == kind)
                .count()
        };
        let mut kinds = vec![("all".to_owned(), format!("Все · {}", steps.len()))];
        for (kind, name) in facts::KINDS {
            if count(kind) > 0 || kind == self.kind {
                kinds.push((kind.to_owned(), format!("{name} · {}", count(kind))));
            }
        }
        let shown: Vec<&Value> = steps
            .iter()
            .filter(|step| facts::matches(step, &self.kind, self.problems, &self.query))
            .collect();
        let problems = steps.iter().filter(|step| facts::problem(step)).count();
        let mut filters: Vec<Value> = kinds
            .into_iter()
            .map(|(kind, name)| ghost(name, "kind", Value::from(kind.as_str()), kind == self.kind))
            .collect();
        filters.push(ghost(
            format!("Ошибки и без результата · {problems}"),
            "problems",
            Value::from(!self.problems),
            self.problems,
        ));
        let mut nodes = vec![
            row(filters),
            json!({"type": "input", "id": "query", "value": self.query, "placeholder": "Название шага", "action": "query"}),
        ];
        if shown.is_empty() {
            let text = if steps.is_empty() {
                "В ходе нет записанных шагов."
            } else {
                "Подходящих шагов нет."
            };
            nodes.push(label(text, "muted"));
        }
        for step in shown.iter().take(LONGEST) {
            let state = facts::status(step);
            let sequence = step.at("sequence");
            let selected = step.i64("sequence") == self.step;
            let timing = facts::duration(step.at("started_at_ms"), step.at("finished_at_ms"));
            nodes.push(column(vec![
                row(vec![
                    ghost(
                        format!("#{} · {}", plain(sequence), facts::title(step)),
                        "step",
                        sequence.clone(),
                        selected,
                    ),
                    badge(state, facts::tone(state)),
                ]),
                small(
                    format!("{} · {timing}", facts::kind_name(facts::kind(step))),
                    "muted",
                ),
            ]));
        }
        if shown.len() > LONGEST {
            nodes.push(small(
                format!(
                    "Показаны первые {LONGEST} из {} — уточните фильтр.",
                    shown.len()
                ),
                "muted",
            ));
        }
        let heading = if shown.len() != steps.len() {
            format!("Шаги · {} из {}", shown.len(), steps.len())
        } else {
            format!("Шаги · {}", steps.len())
        };
        let mut children = vec![large(heading)];
        children.extend(nodes);
        column(children)
    }
}

impl Extension for Analysis {
    fn poll(&self) -> Option<Duration> {
        Some(POLL)
    }

    fn render(&mut self, host: &mut Host, view: &str, _: Surface, _: Option<&str>) -> Result {
        if self
            .analytics
            .selected_session(host)
            .is_none_or(|session| session.is_empty())
        {
            host.show(view, label("Нет выбранной сессии.", "muted"));
            return Ok(());
        }
        let turn = self.turn.clone().filter(|turn| !turn.is_empty());
        let query: Vec<(&str, &str)> = turn.iter().map(|turn| ("turn_id", turn.as_str())).collect();
        let data = self.analytics.fetch(host, "/analysis", &query, false)?;
        let picker = self.analytics.session_picker(host)?;
        let root = self.page(&data, picker);
        host.show(view, root);
        Ok(())
    }

    fn action(&mut self, _: &mut Host, _: &str, action: &str, value: &Value) -> Result {
        if self.analytics.action(action, value) {
            self.selection_changed();
        }
        match action {
            "turn" => {
                self.turn = value.as_str().map(str::to_owned);
                self.step = None;
            }
            "kind" => self.kind = value.as_str().unwrap_or_default().to_owned(),
            "problems" => self.problems = value.truthy(),
            "query" => self.query = value.as_str().unwrap_or_default().to_owned(),
            "all-turns" => self.all_turns = !self.all_turns,
            "step" => {
                let sequence = integer(if value.is_object() {
                    value.at("id")
                } else {
                    value
                })?;
                self.step = Some(if Some(sequence) == self.step {
                    -1
                } else {
                    sequence
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn event(&mut self, _: &mut Host, event: &str) {
        if event == "run_settled" && self.analytics.chosen.is_none() {
            self.analytics.forget("/analysis");
        }
    }

    fn session_changed(&mut self, _: &mut Host) {
        if self.analytics.chosen.is_none() {
            self.selection_changed();
        }
    }
}

/// Python's `int()` of an action value.
fn integer(value: &Value) -> Result<i64> {
    match value {
        Value::Number(number) => Ok(number
            .as_i64()
            .unwrap_or_else(|| number.as_f64().unwrap_or(0.) as i64)),
        Value::Bool(flag) => Ok(*flag as i64),
        Value::String(text) => Ok(text.trim().parse()?),
        other => bail!("int() argument must be a string or a number, not {other}"),
    }
}

fn main() {
    run(Analysis::default());
}
