use super::*;

fn host(config: Value) -> Host {
    let mut host = Host::default();
    host.features.insert("session.settings.v1".to_owned());
    host.session = Some("/one".to_owned());
    host.state = Some(json!({"config": config}));
    host
}

fn select(id: &str, value: &str, options: &[&str]) -> Value {
    let options: Vec<Value> = options
        .iter()
        .map(|option| json!({"value": option, "name": option.replace("effort:", "")}))
        .collect();
    json!({"id": id, "name": id, "control": {"type": "select", "value": value, "options": options}})
}

fn fixture(effort: &str) -> Host {
    host(
        json!({"profile": "fixture", "model": {"provider": "fixture", "name": "astra"},
                "model_options": [{"provider": "fixture", "name": "astra", "default_reasoning_effort": "high"},
                                  {"provider": "fixture", "name": "nova"}],
                "session_settings": [select("model", "astra", &["astra", "nova"]),
                                     select("reasoning_effort", effort, &["_default", "effort:low", "effort:high"])]}),
    )
}

fn steps(root: &Value) -> &Value {
    root.list("children")
        .iter()
        .find(|child| child.str("type") == "steps")
        .unwrap()
}

#[test]
fn new_select_settings_need_no_native_client_change() {
    let mut host = host(json!({"session_settings": [
        {"id": "org.example/style", "name": "Style", "category": "future-category",
         "control": {"type": "select", "value": "short", "options": [{"value": "short", "name": "Short"}, {"value": "long", "name": "Long"}]}},
        {"id": "future-slider", "name": "Future", "control": {"type": "slider", "value": 1}},
    ]}));
    let root = popup::render(&host, false).to_string();
    assert!(root.contains("setting:org.example/style"));
    assert!(!root.contains("future-slider"));
    let mut model = Model::default();
    model
        .action(
            &mut host,
            "popup",
            "setting:org.example/style",
            &json!("long"),
        )
        .unwrap();
    assert_eq!(host.setting_value("org.example/style"), Some(json!("long")));
    host.features.clear();
    assert!(
        host.set_session("org.example/style", json!("short"))
            .is_err()
    );
}

#[test]
fn profile_effort_follows_the_model_default() {
    let mut host = fixture("_default");
    assert_eq!(label(&host), "astra · по профилю");
    let root = popup::render(&host, false);
    let text = root.to_string();
    assert!(!text.contains("Рассуждения") && !text.contains("effort_reset"));
    assert_eq!(root.list("children").len(), 3);
    assert_eq!(steps(&root)["selected"], json!(1));
    Model::default()
        .action(&mut host, "popup", "effort", &json!(0))
        .unwrap();
    assert_eq!(label(&host), "astra · low");
    let root = popup::render(&host, false);
    assert_eq!(root.list("children").len(), 3);
    let header = root.at("children")[0].list("children");
    assert_eq!(header.len(), 2);
    assert_eq!(header[1]["action"], "effort_reset");
    Model::default()
        .action(&mut host, "popup", "effort_reset", &Value::Null)
        .unwrap();
    assert_eq!(
        host.setting_value("reasoning_effort"),
        Some(json!("_default"))
    );
    assert_eq!(steps(&popup::render(&host, false))["selected"], json!(1));
}

#[test]
fn repeated_effort_values_have_one_step_and_keep_action_indices() {
    let mut host = fixture("_default");
    let options =
        &mut host.state.as_mut().unwrap()["config"]["session_settings"][1]["control"]["options"];
    let low = options[1].clone();
    let high = options[2].clone();
    *options = json!([
        {"value": "_default", "name": "Default"},
        low,
        {"value": "effort:low", "name": "Duplicate low"},
        high,
        {"value": "effort:high", "name": "Duplicate high"}
    ]);
    let root = popup::render(&host, false);
    assert_eq!(steps(&root)["options"], json!(["low", "high"]));
    assert_eq!(steps(&root)["selected"], json!(1));
    Model::default()
        .action(&mut host, "popup", "effort", &json!(1))
        .unwrap();
    assert_eq!(
        host.setting_value("reasoning_effort"),
        Some(json!("effort:high"))
    );
    assert_eq!(label(&host), "astra · high");
}

#[test]
fn equal_effort_labels_do_not_merge_distinct_values() {
    let host = host(json!({"session_settings": [{
        "id": "reasoning_effort", "control": {"type": "select", "value": "effort:high", "options": [
            {"value": "effort:low", "name": "Custom"},
            {"value": "effort:high", "name": "Custom"}
        ]}
    }]}));
    let efforts = settings::efforts(&host);
    assert_eq!(efforts.len(), 2);
    assert_eq!(settings::effort_position(&host, &efforts), 1);
}

#[test]
fn model_list_opens_and_closes() {
    let mut host = fixture("effort:low");
    let mut model = Model::default();
    model
        .action(&mut host, "popup", "models", &Value::Null)
        .unwrap();
    let root = popup::render(&host, model.models_open);
    let header = &root.at("children")[0];
    assert_eq!(header.at("children")[0]["action"], "back");
    assert_eq!(header.at("children")[1]["text"], "Model · fixture");
    let choices: Vec<(&str, bool)> = root.at("children")[1]
        .list("options")
        .iter()
        .map(|choice| (choice.str("label"), choice.bool("selected")))
        .collect();
    assert_eq!(choices, [("astra", true), ("nova", false)]);
    model
        .action(&mut host, "popup", "escape", &Value::Null)
        .unwrap();
    assert!(!model.models_open);
    model
        .action(&mut host, "popup", "models", &Value::Null)
        .unwrap();
    model
        .action(&mut host, "popup", "model", &json!("nova"))
        .unwrap();
    assert!(!model.models_open);
    assert_eq!(settings::model_name(&host).as_deref(), Some("nova"));
}

#[test]
fn several_providers_prefix_model_names() {
    let host = host(
        json!({"model_options": [{"provider": "a", "name": "x"}, {"provider": "b", "name": "y"}],
                           "session_settings": [select("model", "x", &["x", "y"])]}),
    );
    let root = popup::render(&host, false);
    assert_eq!(
        root.at("children")[0],
        json!({"type": "text", "text": "Model", "tone": "muted"})
    );
    assert_eq!(root.at("children")[1].at("options")[1]["label"], "b/y");
}
