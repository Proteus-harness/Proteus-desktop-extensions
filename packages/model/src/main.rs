mod popup;
mod settings;

use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, json, run};

use settings::DEFAULT;

#[derive(Default)]
struct Model {
    /// The popup lists models instead of the reasoning stepper.
    models_open: bool,
    /// Synthetic selector state belongs to the explicit preview only.
    preview: Option<Host>,
}

impl Extension for Model {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        match surface {
            Surface::Preview => {
                let sample = self.preview.get_or_insert_with(preview_host);
                host.show(view, popup::render(sample, self.models_open));
            }
            Surface::Badge => {
                let error = settings::catalog_error(host).is_some();
                let face = json!({"type": "text", "caret": true, "text": label(host), "tone": if error { "warning" } else { "muted" }});
                let mut lines = vec![format!("Профиль {}", settings::config(host).str("profile"))];
                if error {
                    lines.push("Каталог моделей недоступен".into());
                }
                host.send_badge(view, "Model и Reasoning", lines, Some(face), hidden(host));
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

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        if host.surface(view) == Some(Surface::Preview) {
            match action {
                "models" => self.models_open = true,
                "back" | "escape" => self.models_open = false,
                "model" | "effort" | "effort_reset" => {
                    let sample = self.preview.get_or_insert_with(preview_host);
                    let (id, chosen) = match action {
                        "model" => ("model", value.clone()),
                        "effort_reset" => ("reasoning_effort", json!(DEFAULT)),
                        _ => (
                            "reasoning_effort",
                            value
                                .as_u64()
                                .and_then(|index| {
                                    settings::efforts(sample).get(index as usize).cloned()
                                })
                                .map(|option| option.at("value").clone())
                                .unwrap_or(Value::Null),
                        ),
                    };
                    if let Some(rows) = sample
                        .state
                        .as_mut()
                        .and_then(|state| state["config"]["session_settings"].as_array_mut())
                    {
                        for row in rows.iter_mut().filter(|row| row.str("id") == id) {
                            row["control"]["value"] = chosen.clone();
                        }
                    }
                    if action == "model" {
                        self.models_open = false;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        let current = |host: &Host, id: &str| host.setting_value(id).unwrap_or(Value::Null);
        match action {
            "escape" if self.models_open && settings::catalog_error(host).is_none() => {
                self.models_open = false;
            }
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

fn preview_host() -> Host {
    let mut host = Host::default();
    host.state = Some(json!({"config": {
        "profile":"Пример", "model":{"provider":"Пример","name":"Модель A"},
        "model_options":[{"provider":"Пример","name":"Модель A","default_reasoning_effort":"high"},{"provider":"Пример","name":"Модель B","default_reasoning_effort":"low"}],
        "session_settings":[
            {"id":"model","control":{"type":"select","value":"Модель A","options":[{"value":"Модель A","name":"Модель A"},{"value":"Модель B","name":"Модель B"}]}},
            {"id":"reasoning_effort","control":{"type":"select","value":"_default","options":[{"value":"_default","name":"По профилю"},{"value":"effort:low","name":"Низкое"},{"value":"effort:high","name":"Высокое"}]}}
        ]
    }}));
    host
}

/// Badge text: the model and its reasoning effort.
fn label(host: &Host) -> String {
    let name = settings::model_name(host).unwrap_or_else(|| "Model".to_owned());
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

fn hidden(host: &Host) -> bool {
    settings::model_name(host).is_none()
        && settings::catalog_error(host).is_none()
        && host.setting("model").is_none()
        && settings::efforts(host).is_empty()
        && settings::extra(host).is_empty()
}

fn main() {
    run(Model::default());
}

#[cfg(test)]
mod tests;
