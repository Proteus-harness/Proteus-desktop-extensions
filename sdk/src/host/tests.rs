use super::*;

#[test]
fn encodes_query_values() {
    assert_eq!(encode("/a b/ж"), "%2Fa%20b%2F%D0%B6");
}

#[derive(Default)]
struct Flaky {
    broken: bool,
}

impl Extension for Flaky {
    fn render(&mut self, host: &mut Host, view: &str, _: Surface, _: Option<&str>) -> Result {
        if self.broken {
            bail!("fixture outage");
        }
        host.show(view, ui::text("ok"));
        Ok(())
    }

    fn action(&mut self, _: &mut Host, _: &str, action: &str, _: &Value) -> Result {
        if action == "panic" {
            panic!("fixture panic");
        }
        bail!("fixture action")
    }
}

fn last(host: &Host) -> Value {
    host.sent.borrow().last().cloned().unwrap_or_default()
}

#[test]
fn failures_keep_the_last_view() {
    let (mut host, mut extension) = (Host::default(), Flaky::default());
    let render =
        json!({"type": "render", "view": "page", "surface": "page", "session_dir": "/one"});
    host.handle(&mut extension, render).unwrap();
    extension.broken = true;
    host.handle(
        &mut extension,
        json!({"type": "event", "event": "run_settled", "session_dir": "/one"}),
    )
    .unwrap();
    let children = last(&host)["root"]["children"].clone();
    assert!(children[0].to_string().contains("fixture outage"));
    assert_eq!(children[1], ui::text("ok"));
    for action in ["x", "panic"] {
        let message = json!({"type": "action", "view": "page", "sequence": 1, "action": action, "value": null});
        host.handle(&mut extension, message).unwrap();
        assert!(last(&host).to_string().contains("fixture"), "{action}");
    }
}

#[test]
fn session_commands_acknowledge_errors_and_changes() {
    let (mut host, mut extension) = (Host::default(), Flaky::default());
    host.features.insert("session.settings.v1".into());
    host.session = Some("/one".into());
    host.state = Some(json!({"config": {"profile": "before"}}));
    host.set_session("reasoning_effort", json!("effort:high"))
        .unwrap();
    assert_eq!(
        host.setting_value("reasoning_effort"),
        Some(json!("effort:high"))
    );
    let id = last(&host)["id"].clone();
    let result = json!({"type": "command_result", "id": id, "session_dir": "/one", "config": null, "error": "fixture failure"});
    host.handle(&mut extension, result).unwrap();
    assert!(host.pending.is_empty());
    assert_eq!(host.command_error.as_deref(), Some("fixture failure"));
    host.set_session("mode", json!("plan")).unwrap();
    let id = last(&host)["id"].clone();
    let config = json!({"permission_mode": "plan"});
    let result = json!({"type": "command_result", "id": id, "session_dir": "/one", "config": config, "error": null});
    host.handle(&mut extension, result).unwrap();
    assert_eq!(
        host.state.as_ref().unwrap()["config"]["permission_mode"],
        "plan"
    );
    host.features.clear();
    assert!(host.set_session("mode", json!("auto")).is_err());
}

#[test]
fn a_new_chat_holds_badges_until_its_snapshot() {
    let (mut host, mut extension) = (Host::default(), Flaky::default());
    host.handle(
        &mut extension,
        json!({"type": "session_snapshot", "session_dir": "/one", "state": {}, "entries": []}),
    )
    .unwrap();
    host.handle(
        &mut extension,
        json!({"type": "render", "view": "badge", "surface": "badge", "session_dir": "/one"}),
    )
    .unwrap();
    let count = host.sent.borrow().len();
    host.handle(
        &mut extension,
        json!({"type": "event", "event": "session_opened", "session_dir": "/two"}),
    )
    .unwrap();
    assert_eq!(host.sent.borrow().len(), count);
    host.handle(
        &mut extension,
        json!({"type": "session_snapshot", "session_dir": "/two", "state": {}, "entries": []}),
    )
    .unwrap();
    assert_eq!(last(&host)["session_dir"], "/two");
}

#[derive(Default)]
struct Ordered {
    renders: Vec<String>,
    sessions: Vec<Option<String>>,
    fail: bool,
    slow_document: bool,
}

impl Extension for Ordered {
    fn render(&mut self, host: &mut Host, view: &str, _: Surface, _: Option<&str>) -> Result {
        self.renders.push(view.to_owned());
        if self.slow_document && view.starts_with("document:") {
            // A heavy retained document is allowed to take time. The panel
            // action's response must already have been emitted before it.
            assert_eq!(host.sent.borrow().first().unwrap()["view"], "panel");
            std::thread::sleep(Duration::from_millis(25));
        }
        if self.fail {
            bail!("new-session fixture outage");
        }
        host.show(view, ui::text(format!("{view} {:?}", host.session)));
        Ok(())
    }

    fn session_changed(&mut self, host: &mut Host) {
        self.sessions.push(host.session.clone());
        assert!(host.state.is_none());
        assert!(host.entries.is_empty());
        assert!(host.pending.is_empty());
        assert!(host.command_error.is_none());
        assert!(host.shown.is_empty());
    }
}

fn retained() -> (Host, Ordered) {
    let mut host = Host::default();
    host.session = Some("/one".into());
    for (view, surface) in [
        ("document:Changes", Surface::Document),
        ("panel", Surface::Panel),
        ("popup", Surface::Popup),
    ] {
        host.views.insert(view.into(), (surface, None));
    }
    (host, Ordered::default())
}

#[test]
fn a_render_request_never_reloads_other_retained_surfaces() {
    let (mut host, mut extension) = retained();
    host.handle(
        &mut extension,
        json!({"type":"render", "view":"panel", "surface":"panel", "session_dir":"/one"}),
    )
    .unwrap();
    assert_eq!(extension.renders, ["panel"]);
    assert!(extension.sessions.is_empty());
    assert_eq!(host.sent.borrow().len(), 1);
}

#[test]
fn action_returns_its_surface_before_slow_documents_and_refreshes_the_rest() {
    let (mut host, mut extension) = retained();
    extension.slow_document = true;
    host.handle(
        &mut extension,
        json!({"type":"action", "view":"panel", "action":"refresh", "sequence":9}),
    )
    .unwrap();
    assert_eq!(extension.renders, ["panel", "document:Changes", "popup"]);
    let sent = host.sent.borrow();
    assert_eq!(sent[0]["view"], "panel");
    assert_eq!(sent[0]["sequence"], 9);
    assert_eq!(sent[1]["sequence"], 0);
    assert_eq!(sent.len(), 3);
}

#[test]
fn a_session_switch_via_render_clears_stale_views_and_prioritizes_the_requester() {
    let (mut host, mut extension) = retained();
    host.state = Some(json!({"profile":"old"}));
    host.entries = vec![json!({"text":"old fixture entry"})];
    host.pending
        .push(("old-id".into(), "model".into(), json!("old")));
    host.command_error = Some("old command".into());
    host.shown
        .insert("panel".into(), ui::text("old fixture view"));
    extension.fail = true;
    host.handle(
        &mut extension,
        json!({"type":"render", "view":"panel", "surface":"panel", "session_dir":"/two"}),
    )
    .unwrap();
    assert_eq!(extension.sessions, [Some("/two".into())]);
    assert_eq!(extension.renders, ["panel", "document:Changes", "popup"]);
    assert!(host.awaiting_snapshot);
    let sent = host.sent.borrow();
    assert_eq!(sent[0]["view"], "panel");
    assert!(sent.iter().all(|message| message["session_dir"] == "/two"));
    assert!(
        sent.iter()
            .all(|message| !message.to_string().contains("old fixture"))
    );
}

#[test]
fn snapshots_and_events_still_refresh_all_open_surfaces() {
    let (mut host, mut extension) = retained();
    host.handle(
        &mut extension,
        json!({"type":"session_snapshot", "session_dir":"/one", "state":{}, "entries":[]}),
    )
    .unwrap();
    assert_eq!(extension.renders.len(), 3);
    extension.renders.clear();
    host.handle(
        &mut extension,
        json!({"type":"event", "event":"document_changed", "session_dir":"/one"}),
    )
    .unwrap();
    assert_eq!(extension.renders.len(), 3);
}

#[test]
fn closing_a_surface_resets_its_action_sequence_before_reopening() {
    let (mut host, mut extension) = retained();
    host.sequences.insert("panel".into(), 25);
    host.handle(&mut extension, json!({"type":"close", "view":"panel"}))
        .unwrap();
    host.handle(
        &mut extension,
        json!({"type":"render", "view":"panel", "surface":"panel", "session_dir":"/one"}),
    )
    .unwrap();
    assert_eq!(last(&host)["sequence"], 0);
    assert_eq!(extension.renders, ["panel"]);
}

#[derive(Default)]
struct ContentProbe {
    sources: Vec<(String, Content)>,
}

impl Extension for ContentProbe {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        assert_eq!(surface, Surface::Content);
        let content = host.content(view).expect("typed source").clone();
        self.sources.push((view.into(), content.clone()));
        if content.source == "invalid" {
            bail!("invalid fixture source");
        }
        host.show(view, ui::text(content.source));
        Ok(())
    }
}

#[test]
fn content_views_retain_independent_payloads_and_release_them_on_close() {
    let (mut host, mut extension) = (Host::default(), ContentProbe::default());
    for (view, source) in [("content:a", "one"), ("content:b", "two")] {
        host.handle(&mut extension, json!({"type":"render", "surface":"content", "view":view,
            "session_dir":"/fixture/chat-one", "content":{"language":"future-figure", "source":source}})).unwrap();
    }
    assert_eq!(host.content("content:a").unwrap().source, "one");
    assert_eq!(host.content("content:b").unwrap().source, "two");
    assert_eq!(extension.sources.len(), 2);
    // A declined replacement sends the explicit fallback message rather
    // than displaying the previous successful tree under an error card.
    host.handle(&mut extension, json!({"type":"render", "surface":"content", "view":"content:a",
        "session_dir":"/fixture/chat-one", "content":{"language":"future-figure", "source":"invalid"}})).unwrap();
    assert_eq!(last(&host)["type"], "content_error");
    assert!(last(&host).get("root").is_none());
    assert_eq!(host.content("content:b").unwrap().source, "two");
    host.handle(&mut extension, json!({"type":"close", "view":"content:a"}))
        .unwrap();
    assert!(host.content("content:a").is_none());
    assert!(!host.is_shown("content:a"));
    assert!(host.content("content:b").is_some());
}
