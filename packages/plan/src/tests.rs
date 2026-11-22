use super::*;
use model::{Snapshot, Status};
use proteus_extension::json;

fn call(args: Value) -> Value {
    json!({"role":"assistant", "tool":{
        "name":"update_plan", "status":"completed", "args":args
    }})
}

fn plan(label: &str, status: &str) -> Value {
    call(json!({"plan":[{"step":label,"status":status}]}))
}

fn user() -> Value {
    json!({"role":"user", "tool":null})
}

fn current(entries: &[Value], running: bool) -> Option<Snapshot<'_>> {
    model::latest(entries, Visibility::Current, running)
}

#[test]
fn effective_arguments_and_encoded_arguments_share_the_parser() {
    let mut entry = plan("original", "pending");
    entry["tool"]["effective_args"] = json!({"plan":[
        {"step":"edited", "status":"in_progress"},
        {"step":"finished", "status":"completed"},
        {"step":42, "status":"pending"},
        {"step":"missing status"},
        "bad"
    ]});
    let typed = [entry.clone()];
    let edited = current(&typed, true).unwrap();
    assert_eq!(edited.steps.len(), 2);
    assert_eq!(edited.steps[0].text, "edited");
    assert_eq!(edited.steps[0].status, Status::InProgress);
    assert_eq!(edited.summary(), "План 1/2");
    assert_eq!(edited.current(), Some("edited"));
    entry["tool"]["effective_args"] = json!(entry["tool"]["effective_args"].to_string());
    assert_eq!(current(&[entry], true), Some(edited));

    let encoded = call(json!(
        "{\"plan\":[{\"step\":\"encoded\",\"status\":\"pending\"}]}"
    ));
    assert_eq!(current(&[encoded], false).unwrap().steps[0].text, "encoded");
}

#[test]
fn immediate_plan_borrows_typed_step_text_but_decoded_and_cached_plans_own_it() {
    use std::borrow::Cow;

    let tool = json!({"args":{"plan":[{"step":"previous","status":"pending"}]},
                      "effective_args":{"plan":[{"step":"новый шаг","status":"in_progress"}]}});
    let projected = Snapshot::from_tool(&tool);
    let source = tool["effective_args"]["plan"][0]["step"].as_str().unwrap();
    assert!(matches!(&projected.steps[0].text, Cow::Borrowed(text) if std::ptr::eq(*text, source)));
    assert_eq!(projected.current(), Some("новый шаг"));

    let retained = Snapshot::from_tool_owned(&tool);
    drop(tool);
    assert!(matches!(&retained.steps[0].text, Cow::Owned(text) if text == "новый шаг"));

    let encoded = json!({"args":{"plan":[{"step":"previous","status":"pending"}]},
                         "effective_args":r#"{"plan":[{"step":"строка","status":"completed"}]}"#});
    let decoded = Snapshot::from_tool(&encoded);
    assert!(matches!(&decoded.steps[0].text, Cow::Owned(text) if text == "строка"));
    assert_eq!(decoded.done, 1);
}

#[test]
fn failed_and_denied_updates_cannot_replace_the_last_accepted_plan() {
    for rejected in ["failed", "denied"] {
        let mut next = plan("rejected", "pending");
        next["tool"]["status"] = json!(rejected);
        let entries = [plan("accepted", "pending"), next];
        for visibility in [Visibility::Current, Visibility::Always] {
            assert_eq!(
                model::latest(&entries, visibility, false).unwrap().steps[0].text,
                "accepted"
            );
        }
    }
}

#[test]
fn live_updates_and_native_status_fallback_remain_visible() {
    let mut live = plan("live", "unfamiliar");
    live["tool"]["status"] = json!("running");
    live["tool"]["effective_args"] = Value::Null;
    let unrelated = json!({"role":"assistant", "tool":{
        "name":"shell", "status":"completed", "args":{"plan":[]}
    }});
    let entries = [live, unrelated];
    let snapshot = current(&entries, true).unwrap();
    assert_eq!(snapshot.steps[0].text, "live");
    assert_eq!(snapshot.steps[0].status, Status::Pending);
}

#[test]
fn current_turn_and_last_plan_of_chat_have_distinct_boundaries() {
    let mut entries = vec![user(), plan("earlier turn", "pending"), user()];
    assert!(current(&entries, true).is_none());
    assert_eq!(
        model::latest(&entries, Visibility::Always, false)
            .unwrap()
            .steps[0]
            .text,
        "earlier turn"
    );
    entries.push(plan("current turn", "in_progress"));
    assert_eq!(
        current(&entries, true).unwrap().steps[0].text,
        "current turn"
    );
    assert_eq!(
        model::latest(&entries, Visibility::Always, false)
            .unwrap()
            .steps[0]
            .text,
        "current turn"
    );
}

#[test]
fn completed_plan_hides_when_idle_only_in_current_mode() {
    let entries = [user(), plan("finished", "completed")];
    assert!(current(&entries, false).is_none());
    assert_eq!(current(&entries, true).unwrap().done, 1);
    assert_eq!(current(&entries, true).unwrap().current(), None);
    assert_eq!(
        model::latest(&entries, Visibility::Always, false)
            .unwrap()
            .done,
        1
    );
}

#[test]
fn empty_or_malformed_accepted_update_clears_in_both_modes() {
    for args in [
        json!({"plan":[]}),
        json!({"plan":[{"step":4,"status":"pending"}]}),
        json!({"plan":[{"step":"missing status"}]}),
        json!("{broken"),
        json!({}),
        Value::Null,
    ] {
        let entries = [plan("old", "pending"), call(args)];
        for visibility in [Visibility::Current, Visibility::Always] {
            assert!(model::latest(&entries, visibility, true).is_none());
        }
    }
    for effective in [json!({}), json!([]), json!(false), json!("{broken")] {
        let mut edited = plan("must not revive args", "pending");
        edited["tool"]["effective_args"] = effective;
        assert!(current(&[edited], true).is_none());
    }
}

#[test]
fn no_plan_or_cleared_and_replaced_session_never_fabricates_or_retains_steps() {
    let mut host = Host::default();
    host.entries.push(plan("old session", "pending"));
    assert!(current(&host.entries, false).is_some());
    host.entries.clear();
    for visibility in [Visibility::Current, Visibility::Always] {
        assert!(model::latest(&host.entries, visibility, false).is_none());
    }
    host.entries.push(user());
    assert!(model::latest(&host.entries, Visibility::Always, false).is_none());
    host.entries.push(plan("new session", "pending"));
    assert_eq!(
        current(&host.entries, false).unwrap().steps[0].text,
        "new session"
    );
}

#[test]
fn options_and_individual_reset_follow_the_effective_mode() {
    assert_eq!(
        settings::visibility(&proteus_extension::Preferences::default()),
        Visibility::Current
    );
    let default = settings::page(Visibility::Current);
    let row = &default["children"][0];
    assert_eq!(row["control"]["label"], "По наличию");
    assert!(row.get("reset").is_none());
    let options = row["control"].list("options");
    assert_eq!(options.len(), 2);
    assert_eq!(options[0]["value"], "current");
    assert_eq!(options[0]["selected"], true);
    assert_eq!(options[1]["value"], "always");
    assert_eq!(options[1]["selected"], false);
    let always = settings::page(Visibility::Always);
    assert_eq!(always["children"][0]["control"]["label"], "Всегда");
    assert_eq!(always["children"][0]["reset"]["action"], "reset_visibility");
    assert_eq!(
        always["children"][0]["control"]["options"][1]["selected"],
        true
    );
    assert_eq!(Visibility::parse("invalid"), None);
}

#[test]
fn example_has_no_session_or_retained_state_and_uses_real_details() {
    let settings = settings::page(Visibility::Current);
    assert_eq!(settings["children"].as_array().unwrap().len(), 1);
    let details = model::details(&model::demo());
    assert_eq!(
        details["children"][0]["children"][0]["name"],
        "circle-check"
    );
    assert_eq!(details["children"][1]["children"][0]["name"], "circle-dot");
    assert_eq!(details["children"][2]["children"][0]["name"], "minus");
    let mut extension = Plan::default();
    assert!(extension.poll().is_none());
    let mut host = Host::default();
    host.entries.push(plan("private chat data", "pending"));
    extension.closed(&mut host, "preview");
    assert_eq!(model::demo().steps[0].text, "Изучить задачу");
}

#[test]
fn manifest_owns_one_activity_and_embedded_settings_with_tool_dependency() {
    let manifest: Value = serde_json::from_str(include_str!("../extension.json")).unwrap();
    assert!(manifest["surfaces"]["activity"].is_object());
    assert!(manifest["surfaces"].get("badge").is_none());
    assert_eq!(manifest["surfaces"]["page"]["embedded"], true);
    assert!(manifest["surfaces"]["preview"].is_object());
    assert_eq!(manifest["dependencies"]["tools"], json!(["update_plan"]));
}

#[test]
fn not_ready_has_visible_recovery_without_a_fabricated_empty_plan() {
    let error: Error = ApiError {
        status: 409,
        message: "cache unavailable".into(),
    }
    .into();
    let message = unavailable_message(&error);
    assert!(message.contains("ещё не готова"));
    let root = unavailable_details(&message);
    assert_eq!(root["children"][0]["text"], message);
    assert_eq!(root["children"][1]["action"], "retry_plan");
    assert_eq!(root["children"][1]["label"], "Повторить");
    assert!(!root.to_string().contains("0/0"));
    assert_eq!(UNAVAILABLE, "План недоступен");
}

#[test]
fn projected_completed_rows_keep_typed_status_and_duplicates_in_order() {
    let args = json!({"plan":[
        {"step":"repeat","status":"completed"},
        {"status":"completed"},
        {"step":42,"status":"completed"},
        {"step":"repeat","status":"in_progress"},
        {"step":"","status":"completed"},
        {"step":"repeat","status":"completed"},
        {"step":"later","status":"in_progress"},
        {"step":"case","status":"Completed"},
        {"step":"blank status","status":""},
        {"step":"wrong status","status":42},
        {"step":"missing status"},
        null
    ]});
    let mut tool = json!({"name":"update_plan","status":"done","args":args,"effective_args":null});
    let original = Snapshot::from_tool(&tool);
    assert_eq!(original.done, 3);
    assert_eq!(original.summary(), "План 3/7");
    assert_eq!(original.current(), Some("repeat"));
    let rows: Vec<_> = original
        .steps
        .iter()
        .map(|step| (step.text.to_string(), step.status))
        .collect();
    assert_eq!(
        rows,
        [
            ("repeat".to_owned(), Status::Completed),
            ("repeat".to_owned(), Status::InProgress),
            ("".to_owned(), Status::Completed),
            ("repeat".to_owned(), Status::Completed),
            ("later".to_owned(), Status::InProgress),
            ("case".to_owned(), Status::Pending),
            ("blank status".to_owned(), Status::Pending),
        ]
    );

    let mut edited = tool.clone();
    edited["effective_args"] = json!(tool["args"].to_string());
    assert_eq!(Snapshot::from_tool(&edited), original);

    tool["effective_args"] = json!({"plan":[
        {"step":"","status":"completed"},
        {"step":false,"status":"completed"},
        {"step":"missing status"}
    ]});
    let entry = json!({"role":"assistant","tool":tool});
    let entries = [entry];
    assert!(current(&entries, false).is_none());
    assert_eq!(current(&entries, true).unwrap().summary(), "План 1/1");
}
