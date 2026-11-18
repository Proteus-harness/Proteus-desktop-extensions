use proteus_extension::ui::{Node, picker, stack};
use proteus_extension::{Extension, Host, Result, Surface, Value, ValueExt, json, run};

#[derive(Default)]
struct Mode {
    preview_permission: Option<Value>,
    preview_behavior: Option<Value>,
}

impl Extension for Mode {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface == Surface::Preview {
            host.show(
                view,
                popup(
                    &permission_preview(),
                    self.preview_permission.as_ref().unwrap_or(&json!("normal")),
                    Some((
                        &behavior_preview(),
                        self.preview_behavior.as_ref().unwrap_or(&json!("_default")),
                    )),
                ),
            );
            return Ok(());
        }
        let permission = host.setting("mode");
        let options = permission
            .map(|control| control.list("options"))
            .unwrap_or(&[]);
        let current = host.setting_value("mode").unwrap_or(Value::Null);
        let behavior = behavior_control(host);
        let behavior_current = host.setting_value("behavior_mode").unwrap_or(Value::Null);
        match surface {
            Surface::Badge => {
                let (text, lines) = match behavior {
                    Some(options) => (
                        label(options, &behavior_current),
                        vec![format!(
                            "Разрешения: {}",
                            label(options_for(host, "mode"), &current)
                        )],
                    ),
                    None => (label(options, &current), Vec::new()),
                };
                let face = json!({"type":"text","caret":true,"text":text,"tone":"muted"});
                host.send_badge(
                    view,
                    if behavior.is_some() {
                        "Режим"
                    } else {
                        "Разрешения"
                    },
                    lines,
                    Some(face),
                    permission.is_none() && behavior.is_none(),
                );
            }
            Surface::Popup => host.show(
                view,
                popup(
                    options,
                    &current,
                    behavior.map(|options| (options, &behavior_current)),
                ),
            ),
            _ => {}
        }
        Ok(())
    }

    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        if host.surface(view) == Some(Surface::Preview) {
            match action {
                "mode" => self.preview_permission = Some(value.clone()),
                "behavior_mode" => self.preview_behavior = Some(value.clone()),
                _ => {}
            }
            return Ok(());
        }
        if matches!(action, "mode" | "behavior_mode")
            && options_for(host, action)
                .iter()
                .any(|option| option.at("value") == value)
        {
            if host.setting_value(action).as_ref() != Some(value) {
                host.set_session(action, value.clone())?;
            }
            host.close_popup();
        }
        Ok(())
    }
}

fn options_for<'a>(host: &'a Host, id: &str) -> &'a [Value] {
    host.setting(id)
        .map(|control| control.list("options"))
        .unwrap_or(&[])
}

/// A single baseline option needs no second selector. Clients expose no
/// package-specific behavior; this extension consumes the generic setting ID.
fn behavior_control(host: &Host) -> Option<&[Value]> {
    let options = options_for(host, "behavior_mode");
    options
        .iter()
        .any(|option| {
            option
                .at("value")
                .as_str()
                .is_some_and(|id| id != "_default")
        })
        .then_some(options)
}

fn popup(options: &[Value], current: &Value, behavior: Option<(&[Value], &Value)>) -> Value {
    let mut children = Vec::new();
    if let Some((options, current)) = behavior {
        children.push(selector(
            "behavior-modes",
            "Режим",
            options,
            current,
            "behavior_mode",
        ));
    }
    if !options.is_empty() {
        children.push(selector(
            "permission-modes",
            "Разрешения",
            options,
            current,
            "mode",
        ));
    }
    stack(children)
}
fn selector(id: &str, title: &str, options: &[Value], current: &Value, action: &str) -> Value {
    let choices = options
        .iter()
        .map(|option| (option.at("value").clone(), option.str("name").to_owned()))
        .collect::<Vec<_>>();
    picker(id, &choices, current, action)
        .with("title", title)
        .with("search", false)
}
fn permission_preview() -> Vec<Value> {
    vec![
        json!({"value":"normal","name":"С подтверждением"}),
        json!({"value":"full_access","name":"Полный доступ"}),
        json!({"value":"plan","name":"Только чтение"}),
    ]
}
fn behavior_preview() -> Vec<Value> {
    vec![
        json!({"value":"_default","name":"По умолчанию"}),
        json!({"value":"review","name":"Ревью"}),
    ]
}

/// Name of the current mode, its exact raw value when no option matches.
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
    fn host(behavior: Vec<Value>) -> Host {
        let mut host = Host::default();
        host.state = Some(json!({"config":{"session_settings":[
            {"id":"mode","control":{"type":"select","value":"normal","options":permission_preview()}},
            {"id":"behavior_mode","control":{"type":"select","value":"_default","options":behavior}}
        ]}}));
        host
    }
    #[test]
    fn badge_names_the_current_mode_and_preserves_unknown_values() {
        assert_eq!(
            label(&permission_preview(), &json!("plan")),
            "Только чтение"
        );
        assert_eq!(label(&[], &json!("custom")), "custom");
        assert_eq!(label(&[], &Value::Null), "—");
        assert_eq!(label(&[], &json!("")), "—");
    }
    #[test]
    fn baseline_only_hides_behavior_but_keeps_permission_and_named_modes_use_distinct_actions() {
        let baseline = host(vec![behavior_preview().remove(0)]);
        assert!(behavior_control(&baseline).is_none());
        let host = host(behavior_preview());
        let options = behavior_control(&host).unwrap();
        let root = popup(
            options_for(&host, "mode"),
            &json!("normal"),
            Some((options, &json!("_default"))),
        );
        let children = root.list("children");
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].str("action"), "behavior_mode");
        assert_eq!(children[1].str("action"), "mode");
    }
    #[test]
    fn changing_behavior_updates_only_that_session_setting_and_rejects_stale_unknown_choices() {
        let mut host = host(behavior_preview());
        host.session = Some("/synthetic/current-chat".into());
        host.features.insert("session.settings.v1".into());
        let mut mode = Mode::default();
        mode.action(&mut host, "popup", "behavior_mode", &json!("review"))
            .unwrap();
        assert_eq!(host.setting_value("behavior_mode"), Some(json!("review")));
        assert_eq!(host.setting_value("mode"), Some(json!("normal")));
        mode.action(&mut host, "popup", "mode", &json!("plan"))
            .unwrap();
        assert_eq!(host.setting_value("mode"), Some(json!("plan")));
        assert_eq!(host.setting_value("behavior_mode"), Some(json!("review")));
        mode.action(&mut host, "popup", "behavior_mode", &json!("removed-mode"))
            .unwrap();
        assert_eq!(host.setting_value("behavior_mode"), Some(json!("review")));
    }

    #[test]
    fn legacy_permission_is_selectable_only_while_the_server_advertises_it() {
        let mut host = host(behavior_preview());
        host.session = Some("/synthetic/current-chat".into());
        host.features.insert("session.settings.v1".into());
        let options =
            host.state.as_mut().unwrap()["config"]["session_settings"][0]["control"]["options"]
                .as_array_mut()
                .unwrap();
        options.push(json!({"value":"auto","name":"Автоматически (legacy)"}));
        let mut mode = Mode::default();
        mode.action(&mut host, "popup", "mode", &json!("auto"))
            .unwrap();
        assert_eq!(host.setting_value("mode"), Some(json!("auto")));
        assert_eq!(
            label(options_for(&host, "mode"), &json!("auto")),
            "Автоматически (legacy)"
        );
        mode.action(&mut host, "popup", "mode", &json!("full_access"))
            .unwrap();
        assert_eq!(host.setting_value("mode"), Some(json!("full_access")));
        host.state.as_mut().unwrap()["config"]["session_settings"][0]["control"]["options"]
            .as_array_mut()
            .unwrap()
            .retain(|option| option.at("value") != &json!("auto"));
        mode.action(&mut host, "popup", "mode", &json!("auto"))
            .unwrap();
        assert_eq!(host.setting_value("mode"), Some(json!("full_access")));
    }
}
