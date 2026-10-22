use proteus_extension::ui::{muted, stack};
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, json, run};

struct Mode;

impl Extension for Mode {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        let control = host.setting("mode").cloned();
        let current = host.setting_value("mode").unwrap_or(Value::Null);
        let options = control
            .as_ref()
            .map(|control| control.list("options"))
            .unwrap_or(&[]);
        match surface {
            Surface::Badge => {
                let face = json!({"type": "text", "caret": true, "text": label(options, &current), "tone": "muted"});
                let lines = vec!["Что агент делает без подтверждения.".to_owned()];
                host.send_badge(
                    view,
                    "Режим разрешений",
                    lines,
                    Some(face),
                    control.is_none(),
                );
            }
            Surface::Popup => {
                let mut children = vec![muted("Режим разрешений")];
                children.extend(options.iter().map(|option| {
                    json!({"type": "choice", "label": option.at("name"), "selected": *option.at("value") == current,
                           "action": "mode", "value": option.at("value")})
                }));
                host.show(view, stack(children));
            }
            _ => {}
        }
        Ok(())
    }

    fn action(&mut self, host: &mut Host, _: &str, action: &str, value: &Value) -> Result {
        if action == "mode" {
            host.set_session("mode", value.clone())?;
            host.close_popup();
        }
        Ok(())
    }
}

/// Name of the current mode, its raw value when no option matches.
fn label(options: &[Value], current: &Value) -> String {
    let name = options
        .iter()
        .find(|option| option.at("value") == current)
        .map(|option| option.at("name"))
        .unwrap_or(current);
    match name {
        Value::Null => "—".to_owned(),
        Value::String(name) if name.is_empty() => "—".to_owned(),
        Value::String(name) => name.clone(),
        other => other.to_string(),
    }
}

fn main() {
    run(Mode);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_names_the_current_mode() {
        let options = [
            json!({"value": "normal", "name": "Обычный"}),
            json!({"value": "plan", "name": "План"}),
        ];
        assert_eq!(label(&options, &json!("plan")), "План");
        assert_eq!(label(&options, &json!("auto")), "auto");
        assert_eq!(label(&options, &Value::Null), "—");
        assert_eq!(label(&[], &json!("")), "—");
    }
}
