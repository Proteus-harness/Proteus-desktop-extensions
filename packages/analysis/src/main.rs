mod details;
mod facts;
mod nodes;
mod summary;
#[cfg(test)]
mod tests;
mod text;

use std::{collections::HashSet, time::Duration};

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
    views: HashSet<String>,
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
            views: HashSet::new(),
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
        let query = if steps.is_empty() {
            String::new()
        } else {
            self.query.trim().to_lowercase()
        };
        let mut counts = [0usize; facts::KINDS.len()];
        let mut problems = 0;
        let mut shown = 0;
        // Reserve the heading and filter slots, which need the full totals.
        let mut children = vec![
            Value::Null,
            Value::Null,
            json!({"type": "input", "id": "query", "value": self.query, "placeholder": "Название шага", "action": "query"}),
        ];
        for step in steps {
            let kind = facts::kind(step);
            if let Some(index) = facts::KINDS.iter().position(|(known, _)| *known == kind) {
                counts[index] += 1;
            }
            let state = facts::status(step);
            let problem = facts::is_problem(state);
            problems += usize::from(problem);
            if (self.kind != "all" && self.kind != kind) || (self.problems && !problem) {
                continue;
            }
            let title = if query.is_empty() {
                None
            } else {
                let title = facts::title(step);
                if !title.to_lowercase().contains(&query) {
                    continue;
                }
                Some(title)
            };
            shown += 1;
            if shown > LONGEST {
                continue;
            }
            let title = title.unwrap_or_else(|| facts::title(step));
            let sequence = step.at("sequence");
            let selected = step.i64("sequence") == self.step;
            let timing = facts::duration(step.at("started_at_ms"), step.at("finished_at_ms"));
            children.push(column(vec![
                row(vec![
                    ghost(
                        format!("#{} · {title}", plain(sequence)),
                        "step",
                        sequence.clone(),
                        selected,
                    ),
                    badge(state, facts::tone(state)),
                ]),
                small(format!("{} · {timing}", facts::kind_name(kind)), "muted"),
            ]));
        }
        let mut filters = vec![ghost(
            format!("Все · {}", steps.len()),
            "kind",
            Value::from("all"),
            self.kind == "all",
        )];
        for ((kind, name), count) in facts::KINDS.into_iter().zip(counts) {
            if count > 0 || kind == self.kind {
                filters.push(ghost(
                    format!("{name} · {count}"),
                    "kind",
                    Value::from(kind),
                    kind == self.kind,
                ));
            }
        }
        filters.push(ghost(
            format!("Ошибки и без результата · {problems}"),
            "problems",
            Value::from(!self.problems),
            self.problems,
        ));
        if shown == 0 {
            let text = if steps.is_empty() {
                "В ходе нет записанных шагов."
            } else {
                "Подходящих шагов нет."
            };
            children.push(label(text, "muted"));
        }
        if shown > LONGEST {
            children.push(small(
                format!("Показаны первые {LONGEST} из {shown} — уточните фильтр."),
                "muted",
            ));
        }
        let heading = if shown != steps.len() {
            format!("Шаги · {shown} из {}", steps.len())
        } else {
            format!("Шаги · {}", steps.len())
        };
        children[0] = large(heading);
        children[1] = row(filters);
        column(children)
    }
}

impl Extension for Analysis {
    fn poll(&self) -> Option<Duration> {
        Some(POLL)
    }

    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Preview {
            let steps = preview_steps();
            host.show(
                view,
                column(vec![
                    small("Синтетический ход: запрос модели → Hooks → Tools.", "muted"),
                    summary::statistics(&steps),
                ])
                .with("gap", "medium"),
            );
            return Ok(());
        }
        self.views.insert(view.to_owned());
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
        let data = self
            .analytics
            .fetch_shared(host, "/analysis", &query, false)?;
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
            self.analytics.clear();
            self.selection_changed();
        }
    }

    fn closed(&mut self, _: &mut Host, view: &str) {
        self.views.remove(view);
        if self.views.is_empty() {
            self.analytics.clear();
        }
    }
}

fn preview_steps() -> Vec<Value> {
    vec![
        json!({"sequence":1,"data":{"kind":"model","response":{"usage":{"input_tokens":2400,"output_tokens":180}}}}),
        json!({"sequence":2,"data":{"kind":"hook","trace":{"steps":[{"outcome":{"status":"ok"}}]}}}),
        json!({"sequence":3,"data":{"kind":"tool","result":{"ok":true}}}),
    ]
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
