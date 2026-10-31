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
        if surface == Surface::Preview {
            host.show(
                view,
                stack(view::render(&preview_snapshot())).with("gap", "medium"),
            );
            return Ok(());
        }
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
                "Context",
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
        host.badge(view, &format!("Context: {percent:.0}%"), lines, Some(face));
        Ok(())
    }
}

fn preview_snapshot() -> Value {
    json!({
        "history": {"messages": 12, "user_messages": 4, "assistant_messages": 4, "tool_results": 4, "estimated_tokens": 18400},
        "tools": {"requested": 4, "finished": 4, "failed": 0, "names": ["read_file", "shell"]},
        "diagnostics": [],
        "latest_context": {"chunks": 8, "token_estimate": 18400},
        "latest_usage": {"source": "provider", "phase": "request", "model_provider": "Пример", "model_name": "Модель", "max_input_tokens": 64000, "compaction_trigger_tokens": 56000,
            "actual": {"input_tokens": 18400, "cached_input_tokens": 12000, "cache_creation_input_tokens": 0},
            "categories": [{"name":"instructions","tokens":2400},{"name":"messages","tokens":12000},{"name":"tool_results","tokens":4000}]},
        "latest_compaction": {"status": "Готово", "summary_present": true, "report": {"changed": true, "input_messages": 24, "output_messages": 12, "original_token_estimate": 32000, "output_token_estimate": 18400}}
    })
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
