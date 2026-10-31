mod entry;
mod report;
#[cfg(test)]
mod tests;
mod view;
mod widgets;

use std::collections::HashSet;
use std::time::Duration;

use proteus_extension::ui::{Node, button, row, stack};
use proteus_extension::{Analytics, Extension, Host, Result, Surface, Value, run};

use widgets::{label, sized};

const POLL: Duration = Duration::from_secs(5);

struct Usage {
    analytics: Analytics,
    period: String,
    model: Option<String>,
    status: String,
    order: String,
    query: String,
    page: i64,
    expanded: HashSet<String>,
}

impl Default for Usage {
    fn default() -> Self {
        Self {
            analytics: Analytics::new(Some(POLL)),
            period: "all".to_owned(),
            model: None,
            status: "all".to_owned(),
            order: "oldest".to_owned(),
            query: String::new(),
            page: 0,
            expanded: HashSet::new(),
        }
    }
}

impl Usage {
    /// The inspected session changed; drop filters that belong to the old one.
    fn selection_changed(&mut self) {
        self.period = "all".to_owned();
        self.model = None;
        self.page = 0;
        self.expanded.clear();
    }

    fn page(&mut self, picker: Value, snapshot: &Value) -> Result<Value> {
        let title = stack(vec![
            sized("Расход", "default", "large"),
            label(
                "Запросы модели и оценка стоимости выбранной сессии.",
                "muted",
            ),
        ]);
        let refresh = button("Обновить", "refresh").with("icon", "refresh-cw");
        let mut children = vec![row(vec![title, picker, refresh]).with("gap", "medium")];
        if snapshot.is_null() {
            children.push(label("Журнал расхода недоступен для этого чата.", "muted"));
        } else {
            children.extend(view::render(self, snapshot)?);
        }
        Ok(stack(children).with("gap", "medium"))
    }
}

impl Extension for Usage {
    fn poll(&self) -> Option<Duration> {
        Some(POLL)
    }

    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Preview {
            let snapshot = preview_snapshot();
            host.show(
                view,
                stack(view::render(self, &snapshot)?).with("gap", "medium"),
            );
            return Ok(());
        }
        if self
            .analytics
            .selected_session(host)
            .is_none_or(|session| session.is_empty())
        {
            host.show(view, label("Нет выбранной сессии.", "muted"));
            return Ok(());
        }
        let picker = self.analytics.session_picker(host)?;
        let snapshot = self.analytics.fetch(host, "/usage", &[], false)?;
        let root = self.page(picker, &snapshot)?;
        host.show(view, root);
        Ok(())
    }

    fn action(&mut self, _: &mut Host, _: &str, action: &str, value: &Value) -> Result {
        if self.analytics.action(action, value) {
            self.selection_changed();
        }
        let text = || value.as_str().unwrap_or_default().to_owned();
        match action {
            "period" => self.period = text(),
            "model" => self.model = value.as_str().map(str::to_owned),
            "status" => self.status = text(),
            "order" => self.order = text(),
            "query" => self.query = text(),
            "page" => self.page = value.as_i64().unwrap_or(0),
            "request" => {
                let id = text();
                if !self.expanded.remove(&id) {
                    self.expanded.insert(id);
                }
            }
            _ => {}
        }
        if matches!(action, "period" | "model" | "status" | "order" | "query") {
            self.page = 0;
        }
        Ok(())
    }

    fn session_changed(&mut self, _: &mut Host) {
        if self.analytics.chosen.is_none() {
            self.selection_changed();
        }
    }
}

fn preview_snapshot() -> Value {
    proteus_extension::json!({"requests":[
        {"exchange_id":"demo-request-1","turn_id":"demo-turn","model":{"provider":"Пример","model":"gpt-6-astra"},"origin":"direct","started_at_ms":1803978000000_i64,"finished_at_ms":1803978001500_i64,"status":"completed","finish_reason":"Stop","usage":{"input_tokens":2400,"output_tokens":180,"cached_input_tokens":1200},"message_count":2,"tool_count":1},
        {"exchange_id":"demo-request-2","turn_id":"demo-turn","model":{"provider":"Пример","model":"gpt-6-astra"},"origin":"direct","started_at_ms":1803978002000_i64,"finished_at_ms":1803978003200_i64,"status":"completed","finish_reason":"ToolCalls","usage":{"input_tokens":3100,"output_tokens":90,"cached_input_tokens":2400},"message_count":3,"tool_count":2}
    ]})
}

fn main() {
    run(Usage::default());
}
