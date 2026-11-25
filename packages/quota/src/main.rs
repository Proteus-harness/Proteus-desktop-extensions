mod describe;
mod fetch;
mod view;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use describe::{credits, describe, number};
use proteus_extension::ui::{
    Node as _, badge_preview, button, checkbox, metric_badge, note, row, setting_reset,
    setting_row, stack, text,
};
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, bail, json, run};

const REFRESH: Duration = Duration::from_secs(60);
const HIDE_BY_DEFAULT: bool = false;
const HIDE_ZERO_CREDITS: &str = "hide_zero_credits";
const RESET_ZERO_CREDITS: &str = "reset_hide_zero_credits";
const PREVIEW_TIME: f64 = 1803978000.;

#[derive(Default)]
struct Quota {
    session: Option<String>,
    model: Value,
    loaded: bool,
    snapshot: Value,
    error: Option<String>,
    deadline: Option<Instant>,
    stale: bool,
    request: Option<fetch::Pending>,
}

impl Extension for Quota {
    fn poll(&self) -> Option<Duration> {
        self.session.as_ref()?;
        if self.request.is_some() {
            return Some(fetch::POLL);
        }
        let left = self.deadline.map_or(Duration::ZERO, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        Some(left.max(fetch::POLL))
    }

    fn poll_ready(&self) -> bool {
        if let Some(request) = &self.request {
            return request.is_ready();
        }
        self.stale
            || self
                .deadline
                .is_none_or(|deadline| Instant::now() >= deadline)
    }

    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        let hide_zero = hide_zero_credits(host);
        if surface == Surface::Preview {
            let sample = preview();
            let (left, _) = describe(&sample, PREVIEW_TIME, hide_zero);
            let body = stack(view::render(
                &sample,
                Some("gpt-6.2-sol"),
                PREVIEW_TIME,
                hide_zero,
            ))
            .with("gap", "medium");
            let root = match left {
                Some(left) => badge_preview(host, badge_face(host, left), body),
                None => body,
            };
            host.show(view, root);
            return Ok(());
        }
        if surface == Surface::Page {
            let credits = setting_row(
                "zero-credits",
                "Скрывать нулевой баланс кредитов",
                checkbox("", hide_zero, HIDE_ZERO_CREDITS),
            )
            .maybe(
                "reset",
                (!hide_zero).then(|| setting_reset(RESET_ZERO_CREDITS)),
            );
            host.show(
                view,
                stack(vec![
                    metric_badge::page(host, HIDE_BY_DEFAULT, true),
                    credits,
                ]),
            );
            return Ok(());
        }
        if !matches!(surface, Surface::Badge | Surface::Popup) {
            bail!("Unsupported quota surface");
        }
        let model = host
            .state
            .as_ref()
            .map(|state| state.at("config").at("model"))
            .unwrap_or(&Value::Null);
        // Keep the whole value as the invalidation key, but only copy it when
        // this snapshot will replace the retained model.
        if host.session != self.session || model != &self.model {
            *self = Quota {
                session: host.session.clone(),
                model: model.clone(),
                ..Quota::default()
            };
        }
        self.refresh(host);
        if surface == Surface::Popup {
            let mut children = vec![row(vec![
                text("Лимиты модели").with("size", "large"),
                button("Обновить", "refresh").with("icon", "refresh-cw"),
            ])];
            let model = self.model.opt_str("name");
            if let Some(message) = self.status() {
                children.push(note(message));
                children.extend(view::metadata(&Value::Null, model));
            } else {
                children.extend(view::render(&self.snapshot, model, now(), hide_zero));
            }
            host.show(view, stack(children).with("gap", "medium"));
        } else {
            self.present(host, view, hide_zero);
        }
        Ok(())
    }

    fn event(&mut self, _: &mut Host, event: &str) {
        if event == "run_settled" || event == "session_opened" && self.request.is_none() {
            self.stale = true;
        }
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        if host.surface(view) == Some(Surface::Page) {
            return match action {
                HIDE_ZERO_CREDITS => {
                    if let Some(hide) = value.at("checked").as_bool() {
                        host.set_visual_preference(
                            HIDE_ZERO_CREDITS,
                            (!hide).then(|| json!(false)),
                        )?;
                    }
                    Ok(())
                }
                RESET_ZERO_CREDITS => host.set_visual_preference(HIDE_ZERO_CREDITS, None),
                _ => metric_badge::action(host, action, value, HIDE_BY_DEFAULT, true),
            };
        }
        if action == "refresh" {
            self.stale = true;
        }
        Ok(())
    }
}

impl Quota {
    fn refresh(&mut self, host: &Host) {
        if let Some(request) = &self.request {
            let Some(result) = request.result() else {
                return;
            };
            self.request = None;
            self.finish(result);
        }
        let due = self.stale
            || self
                .deadline
                .is_none_or(|deadline| Instant::now() >= deadline);
        if due && host.session.is_some() && host.server.is_some() {
            match fetch::start(host) {
                Ok(Some(request)) => {
                    self.request = Some(request);
                    self.stale = false;
                }
                Ok(None) => {
                    // Request slots are shared by all live views in the process.
                    // Recheck the current model when a bounded slot opens.
                    self.deadline = Some(Instant::now() + fetch::POLL);
                    self.stale = false;
                }
                Err(error) => {
                    self.stale = false;
                    self.finish(Err(error));
                }
            }
        }
    }

    fn finish(&mut self, result: Result<Value>) {
        match result {
            Ok(snapshot) => (self.snapshot, self.loaded, self.error) = (snapshot, true, None),
            Err(error) => {
                self.snapshot = Value::Null;
                self.error = Some(error.to_string());
            }
        }
        self.deadline = Some(Instant::now() + REFRESH);
    }

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

    fn present(&self, host: &Host, view: &str, hide_zero: bool) {
        if let Some(message) = self.status() {
            host.send_badge(
                view,
                "Лимиты модели",
                vec![message],
                None,
                self.error.is_none() && metric_badge::hide_when_empty(host, HIDE_BY_DEFAULT),
            );
            return;
        }
        let (remaining, lines) = describe(&self.snapshot, now(), hide_zero);
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
            Some(left) => (
                format!("Лимиты: осталось {}%", number(left)),
                Some(badge_face(host, left)),
            ),
            None => ("Лимиты модели".to_owned(), None),
        };
        let empty = buckets.is_empty()
            && self.snapshot.list("notices").is_empty()
            && !credits(&self.snapshot, hide_zero).is_some_and(Value::is_object);
        host.send_badge(
            view,
            &title,
            lines,
            face,
            empty && metric_badge::hide_when_empty(host, HIDE_BY_DEFAULT),
        );
    }
}

fn hide_zero_credits(host: &Host) -> bool {
    host.preferences()
        .get(HIDE_ZERO_CREDITS)
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn badge_face(host: &Host, left: f64) -> Value {
    let tone = if left <= 10. {
        "danger"
    } else if left <= 25. {
        "warning"
    } else {
        "muted"
    };
    metric_badge::ring(host, left, tone)
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0., |since| since.as_secs_f64())
}

fn preview() -> Value {
    json!({"observed_at": PREVIEW_TIME, "plan": "Pro",
        "credits": {"available": false, "unlimited": false, "balance": "0"}, "buckets": [
        {"id": "shared", "name": "Общий лимит", "windows": [
            {"id": "rolling", "duration_seconds": 18000, "used_percent": 58, "resets_at": 1803981600},
            {"id": "weekly", "duration_seconds": 604800, "used_percent": 32, "resets_at": 1804582800},
            {"id": "monthly", "used_percent": 18, "resets_at": 1806570000}]},
        {"id": "budget", "name": "Бюджет", "windows": [{"id":"monthly", "used_percent":24,
            "amount":{"used":12,"limit":50,"unit":"USD"}, "resets_at":1806570000}]}]})
}

fn main() {
    run(Quota::default());
}
