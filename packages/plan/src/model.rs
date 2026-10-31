use proteus_extension::{Value, ValueExt, json, ui};
use ui::Node as _;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    Current,
    Always,
}

impl Visibility {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "current" => Some(Self::Current),
            "always" => Some(Self::Always),
            _ => None,
        }
    }

    pub fn value(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Always => "always",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub text: String,
    pub status: Status,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub steps: Vec<Step>,
    pub done: usize,
}

impl Snapshot {
    pub fn from_tool(tool: &Value) -> Self {
        // effective_args is optional, not truthy: an explicit empty object or
        // malformed edited argument must clear the old plan rather than revive args.
        let args = tool
            .get("effective_args")
            .filter(|value| !value.is_null())
            .unwrap_or_else(|| tool.at("args"));
        let decoded;
        let args = match args.as_str() {
            Some(encoded) => {
                decoded = serde_json::from_str::<Value>(encoded).unwrap_or(Value::Null);
                &decoded
            }
            None => args,
        };
        let steps: Vec<_> = args
            .list("plan")
            .iter()
            .filter_map(|step| {
                Some(Step {
                    text: step.opt_str("step")?.to_owned(),
                    status: match step.opt_str("status")? {
                        "completed" => Status::Completed,
                        "in_progress" => Status::InProgress,
                        _ => Status::Pending,
                    },
                })
            })
            .collect();
        let done = steps
            .iter()
            .filter(|step| step.status == Status::Completed)
            .count();
        Self { steps, done }
    }

    pub fn summary(&self) -> String {
        format!("План {}/{}", self.done, self.steps.len())
    }

    pub fn current(&self) -> Option<&str> {
        self.steps
            .iter()
            .find(|step| step.status == Status::InProgress)
            .or_else(|| {
                self.steps
                    .iter()
                    .find(|step| step.status == Status::Pending)
            })
            .map(|step| step.text.as_str())
    }
}

/// The latest accepted update is authoritative, even if it clears the plan.
/// Current mode has the same user-turn boundary and idle completion rule as
/// the composer strip; Always uses the last real plan across the entire chat.
pub fn latest(entries: &[Value], visibility: Visibility, running: bool) -> Option<Snapshot> {
    let tool = entries
        .iter()
        .rev()
        .take_while(|entry| visibility == Visibility::Always || entry.str("role") != "user")
        .map(|entry| entry.at("tool"))
        .find(|tool| {
            tool.str("name") == "update_plan" && !matches!(tool.str("status"), "failed" | "denied")
        })?;
    let plan = Snapshot::from_tool(tool);
    if plan.steps.is_empty()
        || (visibility == Visibility::Current && plan.done == plan.steps.len() && !running)
    {
        return None;
    }
    Some(plan)
}

/// Real details and the opt-in settings example share every rendering rule.
pub fn details(plan: &Snapshot) -> Value {
    ui::stack(
        plan.steps
            .iter()
            .map(|step| {
                let (icon, tone) = match step.status {
                    Status::Completed => ("circle-check", "success"),
                    Status::InProgress => ("circle-dot", "warning"),
                    Status::Pending => ("minus", "muted"),
                };
                ui::row(vec![
                    ui::icon(icon).with("tone", tone),
                    ui::text(&step.text).with(
                        "tone",
                        if step.status == Status::Completed {
                            "muted"
                        } else {
                            "default"
                        },
                    ),
                ])
            })
            .collect(),
    )
}

pub fn demo() -> Snapshot {
    Snapshot::from_tool(&json!({"args":{"plan":[
        {"step":"Изучить задачу", "status":"completed"},
        {"step":"Подготовить изменения", "status":"in_progress"},
        {"step":"Проверить результат", "status":"pending"}
    ]}}))
}
