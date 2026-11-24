mod view;
mod window;

use proteus_extension::ui::{Node, badge_preview, metric_badge, muted, stack};
use proteus_extension::{
    Extension, Host, Result, Surface, Value, ValueExt, bail, json, run, tokens,
};

const HIDE_BY_DEFAULT: bool = false;
const PREVIEW_USED: u32 = 18_400;
const PREVIEW_MAXIMUM: u32 = 64_000;
const PREVIEW_TRIGGER: u32 = 56_000;

#[derive(Default)]
struct Context {
    /// The snapshot with the session and window usage it was read for.
    snapshot: Option<(String, Value)>,
}

impl Extension for Context {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        match surface {
            Surface::Preview => {
                let (_, face) = usage_face(
                    host,
                    PREVIEW_USED.into(),
                    PREVIEW_MAXIMUM.into(),
                    Some(PREVIEW_TRIGGER.into()),
                );
                let body = stack(view::render(&preview_snapshot())).with("gap", "medium");
                host.show(view, badge_preview(host, face, body));
            }
            Surface::Page => host.show(view, metric_badge::page(host, HIDE_BY_DEFAULT, true)),
            Surface::Popup => {
                let root = self.popup(host, view)?;
                host.show(view, root);
            }
            Surface::Badge => present(host, view),
            _ => bail!("Unsupported Context surface"),
        }
        Ok(())
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        if host.surface(view) == Some(Surface::Page) {
            metric_badge::action(host, action, value, HIDE_BY_DEFAULT, true)?;
        }
        Ok(())
    }
}

fn present(host: &Host, view: &str) {
    let usage = host.state.as_ref().unwrap_or(&Value::Null).at("context");
    let used = usage.f64("used").filter(|used| *used >= 0.);
    let maximum = usage.f64("max").filter(|max| *max > 0.);
    let Some((used, maximum)) = used.zip(maximum) else {
        host.send_badge(
            view,
            "Context",
            vec!["Появится после первого запроса к модели.".into()],
            None,
            metric_badge::hide_when_empty(host, HIDE_BY_DEFAULT),
        );
        return;
    };
    let trigger = usage.f64("trigger");
    let (percent, face) = usage_face(host, used, maximum, trigger);
    let mut lines = vec![format!(
        "Занято {} из {}",
        tokens(Some(used)),
        tokens(Some(maximum))
    )];
    if let Some(trigger) = trigger {
        lines.push(format!("Автокомпакт при {}", tokens(Some(trigger))));
    }
    host.badge(view, &format!("Context: {percent:.0}%"), lines, Some(face));
}

fn usage_face(host: &Host, used: f64, maximum: f64, trigger: Option<f64>) -> (f64, Value) {
    let percent = (used / maximum * 100.).min(100.);
    let tone = match trigger {
        Some(trigger) if used >= trigger => "danger",
        Some(trigger) if used >= trigger * 0.8 => "warning",
        _ => "muted",
    };
    (percent, metric_badge::ring(host, percent, tone))
}

fn preview_snapshot() -> Value {
    json!({
        "history": {"messages": 12, "user_messages": 4, "assistant_messages": 4, "tool_results": 4, "estimated_tokens": PREVIEW_USED},
        "tools": {"requested": 4, "finished": 4, "failed": 0, "names": ["read_file", "shell"]},
        "diagnostics": [],
        "latest_context": {"chunks": 8, "token_estimate": PREVIEW_USED},
        "latest_usage": {"source": "provider", "phase": "request", "model_provider": "Пример", "model_name": "Модель", "max_input_tokens": PREVIEW_MAXIMUM, "compaction_trigger_tokens": PREVIEW_TRIGGER,
            "actual": {"input_tokens": PREVIEW_USED, "cached_input_tokens": 12000, "cache_creation_input_tokens": 0},
            "categories": [{"name":"instructions","tokens":2400},{"name":"messages","tokens":12000},{"name":"tool_results","tokens":4000}]},
        "latest_compaction": {"status": "Готово", "summary_present": true, "report": {"changed": true, "input_messages": 24, "output_messages": 12, "original_token_estimate": 32000, "output_token_estimate": PREVIEW_USED}}
    })
}

impl Context {
    fn popup(&mut self, host: &mut Host, view: &str) -> Result<Value> {
        let Some(session) = host.session.as_deref() else {
            return Ok(muted("Сессия не открыта."));
        };
        // The snapshot follows the window usage the session reports, so
        // streaming updates reuse it; a freshly opened popup always asks anew.
        let usage = host
            .state
            .as_ref()
            .map(|state| state.at("context").to_string())
            .unwrap_or_default();
        let hit = host.is_shown(view)
            && self
                .snapshot
                .as_ref()
                .is_some_and(|(known, _)| key_matches(known, session, &usage));
        if !hit {
            let key = format!("{session}\n{usage}");
            let mut snapshot = host.api("/context", &[])?;
            // The popup reads compaction totals, not the per-message lineage.
            // This snapshot can outlive the popup in the session instance.
            if let Some(lineage) = snapshot
                .get_mut("latest_compaction")
                .and_then(|compaction| compaction.get_mut("report"))
                .and_then(|report| report.get_mut("user_message_replacements"))
            {
                *lineage = Value::Null;
            }
            self.snapshot = Some((key, snapshot));
        }
        let snapshot = &self.snapshot.as_ref().expect("fetched above").1;
        Ok(stack(view::render(snapshot)).with("gap", "medium"))
    }
}

fn key_matches(known: &str, session: &str, usage: &str) -> bool {
    known
        .strip_prefix(session)
        .and_then(|suffix| suffix.strip_prefix('\n'))
        == Some(usage)
}

#[cfg(test)]
mod popup_tests {
    use super::*;

    #[test]
    fn borrowed_key_requires_exact_delimiter_and_usage() {
        let stored = "/путь\nchild\nnull";
        assert!(key_matches(stored, "/путь\nchild", "null"));
        assert!(!key_matches(stored, "/путь", "null"));
        assert!(!key_matches(stored, "/путь\nchild", ""));
    }
}

fn main() {
    run(Context::default());
}
