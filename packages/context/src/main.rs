mod view;
mod window;

use proteus_extension::ui::{Node, muted, stack};
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, json, run, tokens};

#[derive(Default)]
struct Context {
    /// The snapshot with the session and window usage it was read for.
    snapshot: Option<(String, Value)>,
}

impl Extension for Context {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Popup {
            let root = self.popup(host, view)?;
            host.show(view, root);
            return Ok(());
        }
        let usage = host
            .state
            .as_ref()
            .map(|state| state.at("context"))
            .filter(|usage| usage.is_object());
        let Some(usage) = usage else {
            host.badge(
                view,
                "Контекст",
                vec!["Появится после первого запроса к модели.".into()],
                None,
            );
            return Ok(());
        };
        let (used, maximum) = (
            usage.f64("used").unwrap_or(0.),
            usage.f64("max").unwrap_or(1.).max(1.),
        );
        let percent = (used / maximum * 100.).min(100.);
        let mut lines = vec![format!(
            "Занято {} из {}",
            tokens(Some(used)),
            tokens(Some(maximum))
        )];
        let mut tone = "muted";
        if let Some(trigger) = usage.f64("trigger") {
            lines.push(format!("Автокомпакт при {}", tokens(Some(trigger))));
            tone = if used >= trigger {
                "danger"
            } else if used >= trigger * 0.8 {
                "warning"
            } else {
                "muted"
            };
        }
        let face = json!({"type": "ring", "percent": percent, "tone": tone});
        host.badge(view, &format!("Контекст: {percent:.0}%"), lines, Some(face));
        Ok(())
    }
}

impl Context {
    fn popup(&mut self, host: &mut Host, view: &str) -> Result<Value> {
        let Some(session) = host.session.clone() else {
            return Ok(muted("Сессия не открыта."));
        };
        // The snapshot follows the window usage the session reports, so
        // streaming updates reuse it; a freshly opened popup always asks anew.
        let usage = host
            .state
            .as_ref()
            .map(|state| state.at("context").to_string())
            .unwrap_or_default();
        let key = format!("{session}\n{usage}");
        let stale = self
            .snapshot
            .as_ref()
            .is_none_or(|(known, _)| *known != key);
        if !host.is_shown(view) || stale {
            self.snapshot = Some((key, host.api("/context", &[])?));
        }
        let snapshot = &self.snapshot.as_ref().expect("fetched above").1;
        Ok(stack(view::render(snapshot)).with("gap", "medium"))
    }
}

fn main() {
    run(Context::default());
}
