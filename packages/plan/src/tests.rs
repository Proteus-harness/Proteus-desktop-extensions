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

fn current(entries: &[Value], running: bool) -> Option<Snapshot> {
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
    let edited = current(&[entry.clone()], true).unwrap();
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
    let snapshot = current(&[live, unrelated], true).unwrap();
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
        settings::visibility(&Preferences::load("")),
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
