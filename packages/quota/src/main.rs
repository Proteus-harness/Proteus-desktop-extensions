mod describe;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use describe::{describe, number};
use proteus_extension::{Extension, Host, Result, Surface, Value, bail, json, run};

const REFRESH: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Quota {
    session: Option<String>,
    loaded: bool,
    /// The last answer; `null` when the provider reports no limits.
    snapshot: Value,
    error: Option<String>,
    /// When the next fetch is due; `None` fetches on the next render.
    deadline: Option<Instant>,
    /// A chat opened or a run settled: fetch before the deadline.
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
        if surface != Surface::Badge {
            bail!("Quota only declares a badge surface");
        }
        if host.session != self.session {
            *self = Quota {
                session: host.session.clone(),
                ..Quota::default()
            };
        }
        let due = self.stale
            || self
                .deadline
                .is_none_or(|deadline| Instant::now() >= deadline);
        if due && host.session.is_some() && host.server.is_some() {
            self.stale = false;
            self.present(host, view);
            match host.api("/model/quota", &[]) {
                Ok(snapshot) => (self.snapshot, self.loaded, self.error) = (snapshot, true, None),
                Err(error) => self.error = Some(error.to_string()),
            }
            self.deadline = Some(Instant::now() + REFRESH);
        }
        self.present(host, view);
        Ok(())
    }

    fn event(&mut self, _: &mut Host, event: &str) {
        if matches!(event, "session_opened" | "run_settled") {
            self.stale = true;
        }
    }
}

impl Quota {
    fn present(&self, host: &Host, view: &str) {
        let mut title = "Лимиты модели".to_owned();
        let mut lines = Vec::new();
        let mut face = None;
        if self.loaded && !self.snapshot.is_null() {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0., |since| since.as_secs_f64());
            let (left, described) = describe(&self.snapshot, now);
            lines = described;
            if let Some(left) = left {
                title = format!("Лимиты: осталось {}%", number(left));
                let tone = if left <= 10. {
                    "danger"
                } else if left <= 25. {
                    "warning"
                } else {
                    "muted"
                };
                face = Some(json!({"type": "ring", "percent": whole(100. - left), "tone": tone}));
            }
        }
        if let Some(error) = &self.error {
            lines = vec![format!("Не удалось получить: {error}")];
        } else if self.session.is_none() {
            lines = vec!["Нет активной сессии.".to_owned()];
        } else if !self.loaded {
            lines = vec!["Загрузка…".to_owned()];
        } else if self.snapshot.is_null() {
            lines = vec!["Провайдер не сообщает лимиты.".to_owned()];
        }
        host.badge(view, &title, lines, face);
    }
}

/// Whole percents stay integers on the wire.
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
