use super::*;

#[derive(Default)]
struct VisualExtension {
    rendered: Vec<(String, Option<String>)>,
}

impl Extension for VisualExtension {
    fn render(&mut self, host: &mut Host, view: &str, _: Surface, _: Option<&str>) -> Result {
        let value = host
            .preferences()
            .get("visibility")
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.rendered.push((view.into(), value.clone()));
        host.show(view, ui::note(value.as_deref().unwrap_or("default")));
        Ok(())
    }

    fn action(&mut self, host: &mut Host, _: &str, _: &str, value: &Value) -> Result {
        host.set_visual_preference("visibility", Some(value.clone()))
    }
}

fn setup() -> (Host, VisualExtension) {
    let mut host = Host::default();
    host.features.insert(FEATURE.into());
    let mut extension = VisualExtension::default();
    context(&mut host, &mut extension, "a", 3, "always");
    context(&mut host, &mut extension, "b", 7, "current");
    for id in ["a", "b"] {
        host.handle(
            &mut extension,
            json!({"type":"render", "context":id, "view":id,
            "surface":"page", "session_dir":"/same-session"}),
        )
        .unwrap();
    }
    (host, extension)
}

fn context(host: &mut Host, extension: &mut VisualExtension, id: &str, revision: u64, value: &str) {
    host.handle(
        extension,
        json!({"type":"context", "context":id,
        "workspace":"/workspace", "session_dir":"/same-session",
        "window_visual":{"revision":revision,"preferences":{"visibility":value}}}),
    )
    .unwrap();
}

fn action(host: &mut Host, extension: &mut VisualExtension, view: &str, value: &str) {
    host.handle(
        extension,
        json!({"type":"action", "view":view,
        "action":"visibility", "sequence":1, "value":value}),
    )
    .unwrap();
}

fn writes(host: &Host) -> Vec<Value> {
    host.sent
        .borrow()
        .iter()
        .filter(|message| message.str("type") == "set_visual_preferences")
        .cloned()
        .collect()
}

fn ack(
    host: &mut Host,
    extension: &mut VisualExtension,
    write: &Value,
    revision: u64,
    values: Value,
    error: Option<&str>,
) {
    host.handle(
        extension,
        json!({"type":"visual_preferences_result", "context":write["context"],
        "id":write["id"], "revision":revision, "values":values, "error":error}),
    )
    .unwrap();
}

#[test]
fn same_session_windows_read_independent_visual_values_and_ack_renders_only_owner() {
    let (mut host, mut extension) = setup();
    host.activate_context("a");
    assert_eq!(host.preferences().get("visibility"), Some(&json!("always")));
    host.activate_context("b");
    assert_eq!(
        host.preferences().get("visibility"),
        Some(&json!("current"))
    );
    extension.rendered.clear();
    action(&mut host, &mut extension, "a", "changed");
    let write = writes(&host).pop().unwrap();
    assert_eq!(write["context"], "a");
    assert_eq!(write["view"], "a");
    assert_eq!(write["expected_revision"], 3);
    assert_eq!(write["values"], json!({"visibility":"changed"}));
    assert!(extension.rendered.iter().all(|(view, _)| view == "a"));
    extension.rendered.clear();
    ack(
        &mut host,
        &mut extension,
        &write,
        4,
        json!({"visibility":"changed"}),
        None,
    );
    assert_eq!(
        extension.rendered,
        vec![("a".into(), Some("changed".into()))]
    );
    host.activate_context("b");
    assert_eq!(
        host.preferences().get("visibility"),
        Some(&json!("current"))
    );
    assert!(host.server.is_none());
    assert!(host.state.is_none());
    assert!(host.pending.is_empty());
    assert!(
        host.sent
            .borrow()
            .iter()
            .all(|message| message.str("type") != "set_session")
    );
}

#[test]
fn queued_edits_wait_for_actual_revision_instead_of_guessing_a_future_revision() {
    let (mut host, mut extension) = setup();
    action(&mut host, &mut extension, "a", "first");
    action(&mut host, &mut extension, "a", "second");
    assert_eq!(writes(&host).len(), 1);
    let first = writes(&host).pop().unwrap();
    ack(
        &mut host,
        &mut extension,
        &first,
        10,
        json!({"visibility":"first"}),
        None,
    );
    let all = writes(&host);
    assert_eq!(all.len(), 2);
    assert_eq!(all[1]["expected_revision"], 10);
    assert_eq!(all[1]["values"]["visibility"], "second");
}

#[test]
fn hot_visual_selection_invalidates_pending_reply_without_leaking_an_error() {
    let (mut host, mut extension) = setup();
    action(&mut host, &mut extension, "a", "pending");
    let write = writes(&host).pop().unwrap();
    context(&mut host, &mut extension, "a", 9, "new-config");
    ack(
        &mut host,
        &mut extension,
        &write,
        4,
        json!({"visibility":"stale"}),
        Some("old failure"),
    );
    host.activate_context("a");
    assert_eq!(
        host.preferences().get("visibility"),
        Some(&json!("new-config"))
    );
    assert_eq!(host.visual_preferences_error(), None);
    assert!(host.contexts["a"].visual.pending.is_empty());
}

#[test]
fn closed_context_is_not_resurrected_by_a_late_reply() {
    let (mut host, mut extension) = setup();
    action(&mut host, &mut extension, "a", "pending");
    let write = writes(&host).pop().unwrap();
    host.handle(&mut extension, json!({"type":"close", "view":"a"}))
        .unwrap();
    host.handle(
        &mut extension,
        json!({"type":"close_context", "context":"a"}),
    )
    .unwrap();
    assert!(!host.contexts.contains_key("a"));
    ack(
        &mut host,
        &mut extension,
        &write,
        4,
        json!({"visibility":"stale"}),
        None,
    );
    assert!(!host.contexts.contains_key("a"));
}

#[test]
fn moved_surface_uses_destination_visuals_and_drops_its_source_write() {
    let (mut host, mut extension) = setup();
    action(&mut host, &mut extension, "a", "pending-source");
    let write = writes(&host).pop().unwrap();
    host.handle(
        &mut extension,
        json!({"type":"rehost", "view":"a", "context":"b"}),
    )
    .unwrap();
    assert_eq!(
        extension.rendered.last(),
        Some(&("a".into(), Some("current".into())))
    );
    ack(
        &mut host,
        &mut extension,
        &write,
        4,
        json!({"visibility":"stale"}),
        None,
    );
    action(&mut host, &mut extension, "a", "destination");
    let next = writes(&host).pop().unwrap();
    assert_eq!(next["context"], "b");
    assert_eq!(next["view"], "a");
    assert_eq!(next["expected_revision"], 7);
}

#[test]
fn failures_belong_to_context_not_shared_session() {
    let (mut host, mut extension) = setup();
    action(&mut host, &mut extension, "a", "pending");
    let write = writes(&host).pop().unwrap();
    ack(
        &mut host,
        &mut extension,
        &write,
        3,
        json!({"visibility":"always"}),
        Some("disk unavailable"),
    );
    host.activate_context("a");
    assert_eq!(host.visual_preferences_error(), Some("disk unavailable"));
    host.activate_context("b");
    assert_eq!(host.visual_preferences_error(), None);
    assert_eq!(host.command_error, None);
}

#[test]
fn absent_feature_is_read_only_and_write_requires_an_addressed_live_view() {
    let mut host = Host::default();
    assert!(host.preferences().get("visibility").is_none());
    assert!(
        host.set_visual_preference("visibility", Some(json!("always")))
            .unwrap_err()
            .to_string()
            .contains(FEATURE)
    );
    let (mut host, _) = setup();
    assert!(
        host.set_visual_preference("visibility", Some(json!("always")))
            .is_err()
    );
    host.current_view = Some("closed".into());
    assert!(
        host.set_visual_preference("visibility", Some(json!("always")))
            .is_err()
    );
    assert!(writes(&host).is_empty());
}

#[test]
fn oversized_map_and_pending_admission_do_not_send_or_mutate_accepted_values() {
    let (mut host, mut extension) = setup();
    host.activate_context("a");
    host.current_view = Some("a".into());
    assert!(
        host.set_visual_preference("visibility", Some(json!("\n".repeat(40_000))))
            .is_err()
    );
    assert!(writes(&host).is_empty());
    assert_eq!(host.preferences().get("visibility"), Some(&json!("always")));
    for _ in 0..MAX_PENDING {
        host.set_visual_preference("visibility", Some(json!("queued")))
            .unwrap();
    }
    assert!(
        host.set_visual_preference("visibility", Some(json!("overflow")))
            .is_err()
    );
    assert_eq!(writes(&host).len(), 1);
    host.current_view = None;
    context(&mut host, &mut extension, "a", 8, "reset");
    assert!(host.contexts["a"].visual.pending.is_empty());
}

#[test]
fn pending_payload_budget_includes_superseded_values_not_only_latest_map() {
    let (mut host, _) = setup();
    host.activate_context("a");
    host.current_view = Some("a".into());
    host.set_visual_preference("visibility", Some(json!("x".repeat(40_000))))
        .unwrap();
    assert!(
        host.set_visual_preference("visibility", Some(json!("y".repeat(40_000))))
            .is_err()
    );
    assert_eq!(host.contexts["a"].visual.pending.len(), 1);
    assert_eq!(writes(&host).len(), 1);
}

#[test]
fn matching_id_with_old_revision_is_ignored_until_authoritative_reply_arrives() {
    let (mut host, mut extension) = setup();
    action(&mut host, &mut extension, "a", "pending");
    let write = writes(&host).pop().unwrap();
    ack(
        &mut host,
        &mut extension,
        &write,
        2,
        json!({"visibility":"old"}),
        None,
    );
    host.activate_context("a");
    assert_eq!(host.preferences().get("visibility"), Some(&json!("always")));
    assert_eq!(host.contexts["a"].visual.pending.len(), 1);
    ack(
        &mut host,
        &mut extension,
        &write,
        4,
        json!({"visibility":"accepted"}),
        None,
    );
    assert_eq!(
        host.preferences().get("visibility"),
        Some(&json!("accepted"))
    );
}

#[test]
fn badge_frames_keep_hidden_default_null_and_context_error_order() {
    let host = Host::default();
    host.hide_badge("idle", "");
    assert_eq!(
        host.sent.borrow().last().unwrap().to_string(),
        r#"{"type":"badge","view":"idle","session_dir":null,"hidden":true,"title":"","lines":[],"face":{"type":"icon","tone":"muted"}}"#
    );

    let (mut host, mut extension) = setup();
    action(&mut host, &mut extension, "a", "pending");
    let write = writes(&host).pop().unwrap();
    ack(
        &mut host,
        &mut extension,
        &write,
        3,
        json!({"visibility":"always"}),
        Some("fixture visual error"),
    );
    host.activate_context("a");
    host.command_error = Some("fixture command error".into());
    let face = json!({"type":"text","z":[null,-0.0,u64::MAX,{"z":"Пример λ","a":false}],"a":true});
    host.send_badge(
        "derived",
        "Пример",
        vec!["Первая \"строка\"".into(), "".into()],
        Some(face),
        false,
    );
    assert_eq!(
        host.sent.borrow().last().unwrap().to_string(),
        json!({"type":"badge","view":"derived","session_dir":"/same-session","hidden":false,
            "title":"Пример","lines":["Первая \"строка\"","","fixture command error","fixture visual error"],
            "face":{"type":"text","z":[null,-0.0,u64::MAX,{"z":"Пример λ","a":false}],"a":true}})
        .to_string()
    );
    assert!(!host.is_shown("derived"));

    host.activate_context("b");
    host.badge(
        "nested-null",
        "Default omitted",
        Vec::new(),
        Some(Value::Null),
    );
    assert_eq!(
        host.sent.borrow().last().unwrap().to_string(),
        r#"{"type":"badge","view":"nested-null","session_dir":"/same-session","hidden":false,"title":"Default omitted","lines":["fixture command error"],"face":null}"#
    );
    assert!(!host.is_shown("nested-null"));
}
