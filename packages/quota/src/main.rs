mod describe;
mod view;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use describe::{describe, number};
use proteus_extension::ui::{Node as _, button, note, row, stack, text};
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, bail, json, run};

const REFRESH: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Quota {
    session: Option<String>,
    model: Value,
    loaded: bool,
    snapshot: Value,
    error: Option<String>,
    deadline: Option<Instant>,
    stale: bool,
}

impl Extension for Quota {
    fn poll(&self) -> Option<Duration> {
        self.session.as_ref()?;
        let left = self.deadline.map_or(Duration::ZERO, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        Some(left.max(Duration::from_millis(50)))
    }

    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Preview {
            host.show(
                view,
                stack(view::render(&preview(), 1803978000.)).with("gap", "medium"),
            );
            return Ok(());
        }
        if !matches!(surface, Surface::Badge | Surface::Popup) {
            bail!("Quota only declares badge and popup surfaces");
        }
        let model = host
            .state
            .as_ref()
            .map(|state| state.at("config").at("model"))
            .cloned()
            .unwrap_or_default();
        if host.session != self.session || model != self.model {
            *self = Quota {
                session: host.session.clone(),
                model,
                ..Quota::default()
            };
        }
        let due = self.stale
            || self
                .deadline
                .is_none_or(|deadline| Instant::now() >= deadline);
        if due && host.session.is_some() && host.server.is_some() {
            self.stale = false;
            match host.api("/model/quota", &[]) {
                Ok(snapshot) => (self.snapshot, self.loaded, self.error) = (snapshot, true, None),
                Err(error) => {
                    self.snapshot = Value::Null;
                    self.error = Some(error.to_string());
                }
            }
            self.deadline = Some(Instant::now() + REFRESH);
        }
        if surface == Surface::Popup {
            let mut children = vec![row(vec![
                text("Лимиты модели").with("size", "large"),
                button("Обновить", "refresh").with("icon", "refresh-cw"),
            ])];
            if let Some(model) = self.model.opt_str("name") {
                children.push(text(model).with("tone", "muted"));
            }
            if let Some(message) = self.status() {
                children.push(note(message));
            } else {
                children.extend(view::render(&self.snapshot, now()));
            }
            host.show(view, stack(children).with("gap", "medium"));
        } else {
            self.present(host, view);
        }
        Ok(())
    }

    fn event(&mut self, _: &mut Host, event: &str) {
        if matches!(event, "session_opened" | "run_settled") {
            self.stale = true;
        }
    }

    fn action(&mut self, _: &mut Host, _: &str, action: &str, _: &Value) -> Result {
        if action == "refresh" {
            self.stale = true;
        }
        Ok(())
    }
}

impl Quota {
    fn status(&self) -> Option<String> {
        if let Some(error) = &self.error {
            Some(format!("Не удалось получить: {error}"))
        } else if self.session.is_none() {
            Some("Нет активной сессии.".to_owned())
        } else if !self.loaded {
            Some("Загрузка…".to_owned())
        } else if self.snapshot.is_null() {
            Some("Провайдер не сообщает лимиты.".to_owned())
        } else {
            None
        }
    }

    fn present(&self, host: &Host, view: &str) {
        if let Some(message) = self.status() {
            host.badge(view, "Лимиты модели", vec![message], None);
            return;
        }
        let (remaining, lines) = describe(&self.snapshot, now());
        let buckets = self.snapshot.list("buckets");
        let blocked = if buckets.iter().any(|bucket| {
            !bucket.bool("informational") && bucket.at("limit_reached").as_bool() == Some(true)
        }) {
            Some(("Лимит исчерпан", "danger"))
        } else if buckets.iter().any(|bucket| {
            !bucket.bool("informational") && bucket.at("allowed").as_bool() == Some(false)
        }) {
            Some(("Использование недоступно", "warning"))
        } else {
            None
        };
        if let Some((title, tone)) = blocked {
            host.badge(
                view,
                title,
                lines,
                Some(json!({"type":"icon", "tone":tone})),
            );
            return;
        }
        let (title, face) = match remaining {
            Some(left) => {
                let tone = if left <= 10. {
                    "danger"
                } else if left <= 25. {
                    "warning"
                } else {
                    "muted"
                };
                (
                    format!("Лимиты: осталось {}%", number(left)),
                    Some(json!({"type": "ring", "percent": whole(100. - left), "tone": tone})),
                )
            }
            None => ("Лимиты модели".to_owned(), None),
        };
        host.badge(view, &title, lines, face);
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0., |since| since.as_secs_f64())
}

fn preview() -> Value {
    json!({"observed_at": 1803978000, "plan": "Пример", "buckets": [
        {"id": "shared", "name": "Общий лимит", "windows": [
            {"id": "rolling", "duration_seconds": 18000, "used_percent": 58, "resets_at": 1803981600},
            {"id": "weekly", "duration_seconds": 604800, "used_percent": 32, "resets_at": 1804582800},
            {"id": "monthly", "used_percent": 18, "resets_at": 1806570000}]},
        {"id": "budget", "name": "Бюджет", "windows": [{"id":"monthly", "used_percent":24,
            "amount":{"used":12,"limit":50,"unit":"USD"}, "resets_at":1806570000}]}]})
}

fn whole(value: f64) -> Value {
    if value.fract() == 0. {
        json!(value as i64)
    } else {
        json!(value)
    }
}

fn main() {
    run(Quota::default());
}
