use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, json, run};

/// Valid steps of an `update_plan` call; malformed arguments yield none.
fn steps(tool: &Value) -> Vec<Value> {
    let effective = tool.at("effective_args");
    let mut args = if effective.truthy() {
        effective.clone()
    } else {
        tool.at("args").clone()
    };
    if let Some(text) = args.as_str() {
        match serde_json::from_str(text) {
            Ok(value) => args = value,
            Err(_) => return Vec::new(),
        }
    }
    let plan = if args.is_object() {
        args.list("plan")
    } else {
        &[]
    };
    plan.iter()
        .filter(|step| step.is_object() && step.at("step").is_string())
        .cloned()
        .collect()
}

/// Steps of the latest `update_plan` call that has any.
fn latest(entries: &[Value]) -> Vec<Value> {
    entries
        .iter()
        .rev()
        .map(|entry| entry.at("tool"))
        .filter(|tool| tool.str("name") == "update_plan")
        .map(steps)
        .find(|steps| !steps.is_empty())
        .unwrap_or_default()
}

fn mark(status: &str) -> &'static str {
    match status {
        "completed" => "✓",
        "in_progress" => "•",
        _ => "○",
    }
}

struct Plan;

impl Extension for Plan {
    fn render(&mut self, host: &mut Host, view: &str, _: Surface, _: Option<&str>) -> Result {
        let steps = latest(&host.entries);
        if steps.is_empty() {
            host.badge(
                view,
                "План",
                vec!["Агент ещё не составил план.".into()],
                None,
            );
            return Ok(());
        }
        let done = steps
            .iter()
            .filter(|step| step.str("status") == "completed")
            .count();
        let lines = steps
            .iter()
            .map(|step| format!("{} {}", mark(step.str("status")), step.str("step")))
            .collect();
        let face = json!({"type": "text", "icon": true, "text": format!("{done}/{}", steps.len()), "tone": "muted"});
        host.badge(
            view,
            &format!("План: {done} из {}", steps.len()),
            lines,
            Some(face),
        );
        Ok(())
    }
}

fn main() {
    run(Plan);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(args: Value, effective: Value) -> Value {
        json!({"role": "assistant", "tool": {"name": "update_plan", "args": args, "effective_args": effective}})
    }

    #[test]
    fn latest_valid_plan_wins() {
        let first = call(
            json!({"plan": [{"step": "one", "status": "pending"}]}),
            Value::Null,
        );
        assert_eq!(
            latest(std::slice::from_ref(&first)),
            [json!({"step": "one", "status": "pending"})]
        );
        let encoded = call(
            json!("{\"plan\": [{\"step\": \"two\"}, {\"step\": 3}, \"bad\"]}"),
            Value::Null,
        );
        assert_eq!(latest(&[first.clone(), encoded]), [json!({"step": "two"})]);
        let broken = call(json!("{not json"), Value::Null);
        let effective = call(json!({"plan": []}), json!({"plan": [{"step": "edited"}]}));
        assert_eq!(latest(&[effective, broken]), [json!({"step": "edited"})]);
        let other = json!({"role": "assistant", "tool": {"name": "shell", "args": {"plan": [{"step": "x"}]}}});
        assert_eq!(
            latest(&[first, other, json!({"role": "user", "tool": null})]).len(),
            1
        );
        assert!(latest(&[]).is_empty());
    }
}
