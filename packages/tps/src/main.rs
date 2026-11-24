mod sample;

use std::time::{Duration, Instant};

use proteus_extension::ui::{Node, button, kv, metric_badge, note, row, stack, text};
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, bail, json, run};
use sample::{EXPLANATION, Model, Sample, decimal};

const POLL: Duration = Duration::from_secs(2);
const HIDE_BY_DEFAULT: bool = true;

enum Issue {
    Unmeasured(&'static str),
    FetchFailed(String),
}

impl Issue {
    fn message(&self) -> &str {
        match self {
            Self::Unmeasured(message) => message,
            Self::FetchFailed(message) => message,
        }
    }
}

#[derive(Default)]
struct Tps {
    session: Option<String>,
    model: Option<Model>,
    sample: Option<Sample>,
    issue: Option<Issue>,
    loaded: bool,
    stale: bool,
    running: bool,
    deadline: Option<Instant>,
}

impl Extension for Tps {
    fn poll(&self) -> Option<Duration> {
        if self.session.is_none() || !self.running {
            return None;
        }
        Some(self.deadline.map_or(POLL, |deadline| {
            deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(50))
        }))
    }

    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Preview {
            host.show(view, popup(Some(&preview()), None));
            return Ok(());
        }
        if surface == Surface::Page {
            host.show(view, metric_badge::page(host, HIDE_BY_DEFAULT, false));
            return Ok(());
        }
        if !matches!(surface, Surface::Badge | Surface::Popup) {
            bail!("Unsupported TPS surface");
        }
        let state = host.state.as_ref().unwrap_or(&Value::Null);
        let model = Model::selected(state);
        if self.session != host.session || self.model.as_ref().map(Model::key) != model {
            *self = Self {
                session: host.session.clone(),
                model: model.map(|(provider, name)| Model {
                    provider: provider.to_owned(),
                    name: name.to_owned(),
                }),
                ..Self::default()
            };
        }
        self.running = !state.at("execution").at("active").is_null()
            || state
                .at("activity")
                .i64("running_runs")
                .is_some_and(|count| count > 0);
        let due = !self.loaded
            || self.stale
            || self.running
                && self
                    .deadline
                    .is_none_or(|deadline| Instant::now() >= deadline);
        if due && self.session.is_some() && host.server.is_some() {
            self.stale = false;
            let result = host
                .api("/usage", &[])
                .map_err(|error| {
                    Issue::FetchFailed(format!("Не удалось получить скорость: {error}"))
                })
                .and_then(|snapshot| {
                    sample::latest(&snapshot, self.model.as_ref()).map_err(Issue::Unmeasured)
                });
            match result {
                Ok(sample) => {
                    self.sample = sample;
                    self.issue = None;
                }
                Err(issue) => {
                    self.sample = None;
                    self.issue = Some(issue);
                }
            }
            self.loaded = true;
            self.deadline = Some(Instant::now() + POLL);
        }
        let status = if self.session.is_none() {
            Some("Нет активной сессии.")
        } else if let Some(issue) = &self.issue {
            Some(issue.message())
        } else if !self.loaded {
            Some("Загрузка…")
        } else if self.sample.is_none() {
            Some("Появится после завершённого запроса выбранной модели.")
        } else {
            None
        };
        if surface == Surface::Popup {
            host.show(view, popup(self.sample.as_ref(), status));
        } else {
            let face_text = self.sample.as_ref().map_or("— TPS".into(), |sample| {
                format!("{} TPS", decimal(sample.rate()))
            });
            let lines = match self.sample.as_ref() {
                Some(sample) => vec![
                    "Последний завершённый запрос выбранной модели.".into(),
                    sample.model.label(),
                    format!(
                        "{} выходных токенов за {} с",
                        sample.output,
                        decimal(sample.elapsed_ms as f64 / 1000.)
                    ),
                    EXPLANATION.into(),
                ],
                None => vec![status.unwrap_or_default().into()],
            };
            let hidden = self.sample.is_none()
                && !matches!(&self.issue, Some(Issue::FetchFailed(_)))
                && metric_badge::hide_when_empty(host, HIDE_BY_DEFAULT);
            host.send_badge(
                view,
                "Средний TPS модели",
                lines,
                Some(json!({"type":"text","text":face_text,"tone":"muted"})),
                hidden,
            );
        }
        Ok(())
    }

    fn event(&mut self, _: &mut Host, event: &str) {
        if matches!(event, "session_opened" | "run_settled") {
            self.stale = true;
        }
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        if host.surface(view) == Some(Surface::Page) {
            return metric_badge::action(host, action, value, HIDE_BY_DEFAULT, false);
        }
        if action == "refresh" {
            self.stale = true;
        }
        Ok(())
    }
}

fn popup(sample: Option<&Sample>, status: Option<&str>) -> Value {
    let mut children = vec![row(vec![
        text("Скорость модели").with("size", "large"),
        button("Обновить", "refresh").with("icon", "refresh-cw"),
    ])];
    if let Some(sample) = sample {
        children.push(text(format!("{} TPS", decimal(sample.rate()))).with("size", "large"));
        children.push(kv(vec![
            ("Модель", sample.model.label()),
            ("Выходные токены", sample.output.to_string()),
            (
                "Время запроса",
                format!("{} с", decimal(sample.elapsed_ms as f64 / 1000.)),
            ),
        ]));
    } else if let Some(status) = status {
        children.push(note(status));
    }
    children.push(note(EXPLANATION));
    stack(children).with("gap", "medium")
}

fn preview() -> Sample {
    Sample {
        model: Model {
            provider: "Пример".into(),
            name: "Модель".into(),
        },
        output: 240,
        elapsed_ms: 5000,
    }
}

fn main() {
    run(Tps::default());
}
