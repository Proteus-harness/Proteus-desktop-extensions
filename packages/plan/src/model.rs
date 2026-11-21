use std::borrow::Cow;

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
pub struct Step<'a> {
    pub text: Cow<'a, str>,
    pub status: Status,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot<'a> {
    pub steps: Vec<Step<'a>>,
    pub done: usize,
}

impl<'a> Snapshot<'a> {
    pub fn from_tool(tool: &'a Value) -> Self {
        Self::from_tool_with(tool, Cow::Borrowed)
    }

    fn from_tool_with<'b>(tool: &'b Value, text: impl Fn(&'b str) -> Cow<'a, str>) -> Self {
        // effective_args is optional, not truthy: an explicit empty object or
        // malformed edited argument must clear the old plan rather than revive args.
        let args = tool
            .get("effective_args")
            .filter(|value| !value.is_null())
            .unwrap_or_else(|| tool.at("args"));
        if let Some(encoded) = args.as_str() {
            let decoded = serde_json::from_str::<Value>(encoded).unwrap_or(Value::Null);
            // A decoded argument is temporary, so its steps must be owned.
            Self::from_args(&decoded, |step| Cow::Owned(step.to_owned()))
        } else {
            Self::from_args(args, text)
        }
    }

    fn from_args<'b>(args: &'b Value, text: impl Fn(&'b str) -> Cow<'a, str>) -> Self {
        let steps: Vec<_> = args
            .list("plan")
            .iter()
            .filter_map(|step| {
                Some(Step {
                    text: text(step.opt_str("step")?),
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
            .map(|step| step.text.as_ref())
    }
}

impl Snapshot<'static> {
    /// A cached projection outlives the API response, not just this render.
    pub fn from_tool_owned(tool: &Value) -> Self {
        Self::from_tool_with(tool, |step| Cow::Owned(step.to_owned()))
    }
}

/// The latest accepted update is authoritative, even if it clears the plan.
/// Current mode has the same user-turn boundary and idle completion rule as
/// the composer strip; Always uses the last real plan across the entire chat.
pub fn latest(entries: &[Value], visibility: Visibility, running: bool) -> Option<Snapshot<'_>> {
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
pub fn details(plan: &Snapshot<'_>) -> Value {
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

pub fn demo() -> Snapshot<'static> {
    Snapshot::from_tool_owned(&json!({"args":{"plan":[
        {"step":"Изучить задачу", "status":"completed"},
        {"step":"Подготовить изменения", "status":"in_progress"},
        {"step":"Проверить результат", "status":"pending"}
    ]}}))
}
