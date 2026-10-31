use proteus_extension::ui::{Node, picker, stack};
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, json, run};

#[derive(Default)]
struct Mode {
    preview: Option<Value>,
}

impl Extension for Mode {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Preview {
            let options = vec![
                json!({"value":"normal","name":"Обычный"}),
                json!({"value":"plan","name":"План"}),
                json!({"value":"auto","name":"Автоматический"}),
            ];
            host.show(
                view,
                popup(&options, self.preview.as_ref().unwrap_or(&json!("normal"))),
            );
            return Ok(());
        }
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
                host.show(view, popup(options, &current));
            }
            _ => {}
        }
        Ok(())
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        if host.surface(view) == Some(Surface::Preview) {
            if action == "mode" {
                self.preview = Some(value.clone());
            }
            return Ok(());
        }
        if action == "mode" {
            if host.setting_value("mode").as_ref() != Some(value) {
                host.set_session("mode", value.clone())?;
            }
            host.close_popup();
        }
        Ok(())
    }
}

fn popup(options: &[Value], current: &Value) -> Value {
    let choices: Vec<_> = options
        .iter()
        .map(|option| (option.at("value").clone(), option.str("name").to_owned()))
        .collect();
    stack(vec![
        picker("modes", &choices, current, "mode")
            .with("title", "Режим разрешений")
            .with("search", false),
    ])
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
    run(Mode::default());
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
