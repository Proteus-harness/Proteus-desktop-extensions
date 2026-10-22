mod popup;
mod settings;

use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, json, run};

use settings::DEFAULT;

#[derive(Default)]
struct Model {
    /// The popup lists models instead of the reasoning stepper.
    models_open: bool,
}

impl Extension for Model {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        match surface {
            Surface::Badge => {
                let hidden = host.setting("model").is_none()
                    && settings::efforts(host).is_empty()
                    && settings::extra(host).is_empty();
                let face =
                    json!({"type": "text", "caret": true, "text": label(host), "tone": "muted"});
                let lines = vec![format!("Профиль {}", settings::config(host).str("profile"))];
                host.send_badge(view, "Модель и рассуждения", lines, Some(face), hidden);
            }
            Surface::Popup => {
                let root = popup::render(host, self.models_open);
                host.show(view, root);
            }
            _ => {}
        }
        Ok(())
    }

    fn event(&mut self, _: &mut Host, event: &str) {
        if event == "session_opened" {
            self.models_open = false;
        }
    }

    fn action(&mut self, host: &mut Host, _: &str, action: &str, value: &Value) -> Result {
        let current = |host: &Host, id: &str| host.setting_value(id).unwrap_or(Value::Null);
        match action {
            "escape" if self.models_open => self.models_open = false,
            "escape" => host.close_popup(),
            "models" => self.models_open = true,
            "back" => self.models_open = false,
            "model" => {
                if *value != current(host, "model") {
                    host.set_session("model", value.clone())?;
                }
                self.models_open = false;
            }
            "effort" => {
                let efforts = settings::efforts(host);
                // The stepper repeats its edge value; only real changes reach the server.
                let chosen = value
                    .as_u64()
                    .and_then(|index| efforts.get(index as usize))
                    .map(|option| option.at("value"));
                if let Some(chosen) =
                    chosen.filter(|chosen| **chosen != current(host, "reasoning_effort"))
                {
                    host.set_session("reasoning_effort", chosen.clone())?;
                }
            }
            "effort_reset" => {
                if current(host, "reasoning_effort").as_str() != Some(DEFAULT) {
                    host.set_session("reasoning_effort", DEFAULT.into())?;
                }
            }
            _ => {
                if let Some(id) = action.strip_prefix("setting:") {
                    host.set_session(id, value.clone())?;
                }
            }
        }
        Ok(())
    }
}

/// Badge text: the model and its reasoning effort.
fn label(host: &Host) -> String {
    let name = settings::model_name(host).unwrap_or_else(|| "model".to_owned());
    let efforts = settings::efforts(host);
    if efforts.is_empty() {
        return name;
    }
    if settings::by_profile(host) {
        return format!("{name} · по профилю");
    }
    format!(
        "{name} · {}",
        efforts[settings::effort_position(host, &efforts)].str("name")
    )
}

fn main() {
    run(Model::default());
}

#[cfg(test)]
mod tests;
