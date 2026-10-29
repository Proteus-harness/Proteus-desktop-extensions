use super::*;

#[test]
fn encodes_query_values() {
    assert_eq!(encode("/a b/ж"), "%2Fa%20b%2F%D0%B6");
}

#[test]
fn conversation_reader_addresses_the_selected_session_without_switching_it() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for (query, session) in [("%2Factive%20chat", "active"), ("%2Fother", "other")] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                assert_eq!(socket.read(&mut byte).unwrap(), 1);
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with(&format!("GET /conversation?session_dir={query} ")));
            assert!(
                request
                    .to_lowercase()
                    .contains("authorization: bearer fixture\r\n")
            );
            let body = json!({"schema_version":1,"projection":"conversation",
                "source":{"harness":"proteus","version":"test","session_id":session,"revision":3},
                "conversation":{"revision":3,"messages":[],"model_context":[]}})
            .to_string();
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let mut host = Host::default();
    host.server = Some(Server {
        origin,
        token: "fixture".into(),
    });
    host.session = Some("/active chat".into());
    assert_eq!(
        host.conversation().unwrap().source()["session_id"],
        "active"
    );
    assert_eq!(
        host.conversation_for(Some("/other")).unwrap().source()["session_id"],
        "other"
    );
    assert_eq!(host.session.as_deref(), Some("/active chat"));
    server.join().unwrap();
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
    let render = json!({"type": "render", "context":"fixture", "view": "page", "surface": "page", "session_dir": "/one"});
    host.handle(&mut extension, render).unwrap();
    extension.broken = true;
    host.handle(
        &mut extension,
        json!({"type": "event", "context":"fixture", "event": "run_settled", "session_dir": "/one"}),
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
    host.current_context = Some("fixture".into());
    host.contexts.insert(
        "fixture".into(),
        Scope {
            session: host.session.clone(),
            ..Scope::default()
        },
    );
    host.set_session("reasoning_effort", json!("effort:high"))
        .unwrap();
    assert_eq!(
        host.setting_value("reasoning_effort"),
        Some(json!("effort:high"))
    );
    let id = last(&host)["id"].clone();
    let result = json!({"type": "command_result", "context":"fixture", "id": id, "session_dir": "/one", "config": null, "error": "fixture failure"});
    host.handle(&mut extension, result).unwrap();
    assert!(host.pending.is_empty());
    assert_eq!(host.command_error.as_deref(), Some("fixture failure"));
    host.set_session("mode", json!("plan")).unwrap();
    let id = last(&host)["id"].clone();
    let config = json!({"permission_mode": "plan"});
    let result = json!({"type": "command_result", "context":"fixture", "id": id, "session_dir": "/one", "config": config, "error": null});
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
        json!({"type": "session_snapshot", "context":"fixture", "session_dir": "/one", "state": {}, "entries": []}),
    )
    .unwrap();
    host.handle(
        &mut extension,
        json!({"type": "render", "context":"fixture", "view": "badge", "surface": "badge", "session_dir": "/one"}),
    )
    .unwrap();
    let count = host.sent.borrow().len();
    host.handle(
        &mut extension,
        json!({"type": "event", "context":"fixture", "event": "session_opened", "session_dir": "/two"}),
    )
    .unwrap();
    assert_eq!(host.sent.borrow().len(), count);
    host.handle(
        &mut extension,
        json!({"type": "session_snapshot", "context":"fixture", "session_dir": "/two", "state": {}, "entries": []}),
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
        host.view_contexts.insert(view.into(), "fixture".into());
    }
    host.current_context = Some("fixture".into());
    host.contexts.insert(
        "fixture".into(),
        Scope {
            session: host.session.clone(),
            ..Scope::default()
        },
    );
    (host, Ordered::default())
}

#[test]
fn a_render_request_never_reloads_other_retained_surfaces() {
    let (mut host, mut extension) = retained();
    host.handle(
        &mut extension,
        json!({"type":"render", "context":"fixture", "view":"panel", "surface":"panel", "session_dir":"/one"}),
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
        json!({"type":"render", "context":"fixture", "view":"panel", "surface":"panel", "session_dir":"/two"}),
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
        json!({"type":"session_snapshot", "context":"fixture", "session_dir":"/one", "state":{}, "entries":[]}),
    )
    .unwrap();
    assert_eq!(extension.renders.len(), 3);
    extension.renders.clear();
    host.handle(
        &mut extension,
        json!({"type":"event", "context":"fixture", "event":"document_changed", "session_dir":"/one"}),
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
        json!({"type":"render", "context":"fixture", "view":"panel", "surface":"panel", "session_dir":"/one"}),
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
        host.handle(&mut extension, json!({"type":"render", "context":"fixture", "surface":"content", "view":view,
            "session_dir":"/fixture/chat-one", "content":{"language":"future-figure", "source":source}})).unwrap();
    }
    assert_eq!(host.content("content:a").unwrap().source, "one");
    assert_eq!(host.content("content:b").unwrap().source, "two");
    assert_eq!(extension.sources.len(), 2);
    // A declined replacement sends the explicit fallback message rather
    // than displaying the previous successful tree under an error card.
    host.handle(&mut extension, json!({"type":"render", "context":"fixture", "surface":"content", "view":"content:a",
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

#[derive(Default)]
struct ScopedProbe {
    draft: String,
}

impl Extension for ScopedProbe {
    fn render(&mut self, host: &mut Host, view: &str, _: Surface, _: Option<&str>) -> Result {
        host.show(
            view,
            ui::text(format!(
                "{}:{}:{}",
                host.session.as_deref().unwrap_or(""),
                self.draft,
                host.entries.len()
            )),
        );
        Ok(())
    }
    fn action(&mut self, host: &mut Host, _: &str, action: &str, value: &Value) -> Result {
        match action {
            "draft" => self.draft = value.as_str().unwrap_or_default().into(),
            "open" => host.open_document("next", false, true),
            "setting" => host.set_session("mode", value.clone())?,
            _ => {}
        }
        Ok(())
    }
    fn closed(&mut self, host: &mut Host, view: &str) {
        host.send(json!({"type":"closed_probe","view":view,"session":host.session,"context":host.current_context}));
    }
}

fn scoped_message(host: &mut Host, extension: &mut dyn Extension, value: Value) {
    host.handle(extension, value).unwrap();
}

#[test]
fn two_windows_keep_session_instances_and_move_the_original_entries_buffer() {
    let mut host = Host::default();
    let mut extension = instances::Instances::new(ScopedProbe::default());
    for (context, session, view) in [
        ("a", "/one", "document:moved@1"),
        ("b", "/two", "document:other@2"),
    ] {
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"context","context":context,"workspace":"/workspace","session_dir":session}),
        );
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"session_snapshot","context":context,"session_dir":session,"state":{},"entries":vec![json!({"fixture":session});1000]}),
        );
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"render","context":context,"surface":"document","argument":"same-path","view":view,"session_dir":session}),
        );
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"action","view":view,"action":"draft","value":session,"sequence":1}),
        );
    }
    host.activate_context("a");
    let entries = host.entries.as_ptr();
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"render","context":"a","surface":"panel","view":"panel@3","session_dir":"/one"}),
    );
    host.activate_context("b");
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"context","context":"c","workspace":"/workspace","session_dir":"/one"}),
    );
    assert_eq!(
        host.entries.as_ptr(),
        entries,
        "detach must move/reuse the existing session buffer"
    );
    let closed_before = host
        .sent
        .borrow()
        .iter()
        .filter(|message| message["type"] == "closed_probe")
        .count();
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"rehost","view":"document:moved@1","context":"c"}),
    );
    assert_eq!(
        host.sent
            .borrow()
            .iter()
            .filter(|message| message["type"] == "closed_probe")
            .count(),
        closed_before
    );
    for (view, context, session) in [
        ("document:moved@1", "c", "/one"),
        ("panel@3", "a", "/one"),
        ("document:other@2", "b", "/two"),
    ] {
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"action","view":view,"action":"open","sequence":2}),
        );
        let sent = host.sent.borrow();
        let open = sent
            .iter()
            .rev()
            .find(|message| message["type"] == "open_document")
            .unwrap();
        assert_eq!(open["context"], context);
        assert_eq!(open["view"], view);
        let tree = sent
            .iter()
            .rev()
            .find(|message| message["type"] == "view" && message["view"] == view)
            .unwrap();
        assert!(
            tree["root"]["text"]
                .as_str()
                .unwrap()
                .contains(&format!("{session}:{session}:1000"))
        );
    }
    assert_eq!(
        extension.count(),
        2,
        "surfaces and windows do not create extra session instances"
    );
    for (view, context) in [("document:moved@1", "c"), ("panel@3", "a")] {
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"close","view":view}),
        );
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"close_context","context":context}),
        );
    }
    extension.prune(&host);
    assert_eq!(extension.count(), 1);
    assert!(!host.sessions.contains_key(&Some("/one".into())));
}

#[test]
fn settings_failure_and_late_ack_cannot_rollback_another_window_session() {
    let mut host = Host::default();
    host.features.insert("session.settings.v1".into());
    let mut extension = instances::Instances::new(ScopedProbe::default());
    let mut ids = Vec::new();
    for (context, session, view) in [("a", "/one", "panel@1"), ("b", "/two", "panel@2")] {
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"render","context":context,"view":view,"surface":"panel","session_dir":session}),
        );
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"action","view":view,"action":"setting","value":context,"sequence":1}),
        );
        ids.push(
            host.sent
                .borrow()
                .iter()
                .rev()
                .find(|message| message["type"] == "set_session")
                .unwrap()["id"]
                .clone(),
        );
    }
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"context","context":"c","workspace":"/workspace","session_dir":"/one"}),
    );
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"rehost","view":"panel@1","context":"c"}),
    );
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"close_context","context":"a"}),
    );
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"command_result","context":"a","id":ids[0],"session_dir":"/one","error":"denied","config":null}),
    );
    assert_eq!(host.command_error.as_deref(), Some("denied"));
    assert!(!host.contexts.contains_key("a"));
    assert!(host.pending.is_empty());
    host.activate_context("b");
    assert!(host.command_error.is_none());
    assert_eq!(host.pending.len(), 1);
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"context","context":"c","workspace":"/workspace","session_dir":"/two"}),
    );
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"command_result","context":"c","id":ids[0],"session_dir":"/one","error":"late old response","config":{"permission_mode":"wrong"}}),
    );
    assert!(host.command_error.is_none());
    assert_eq!(host.pending.len(), 1);
    assert!(host.state.is_none());
}

#[test]
fn two_pending_settings_survive_rehost_while_the_source_opens_another_session() {
    let mut host = Host::default();
    host.features.insert("session.settings.v1".into());
    let mut extension = instances::Instances::new(ScopedProbe::default());
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"render","context":"source","view":"moved","surface":"panel","session_dir":"/one"}),
    );
    let mut ids = Vec::new();
    for mode in ["auto", "plan"] {
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"action","view":"moved","action":"setting","value":mode,"sequence":ids.len() + 1}),
        );
        let messages = host.sent.borrow();
        let request = messages
            .iter()
            .rev()
            .find(|message| message["type"] == "set_session")
            .unwrap();
        assert_eq!(request["view"], "moved");
        ids.push(request["id"].clone());
    }
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"context","context":"destination","workspace":"/workspace","session_dir":"/one"}),
    );
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"rehost","view":"moved","context":"destination"}),
    );
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"context","context":"source","workspace":"/workspace","session_dir":"/two"}),
    );
    for (index, mode) in ["auto", "plan"].into_iter().enumerate() {
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"command_result","context":"destination","id":ids[index],"session_dir":"/one","config":{"permission_mode":mode},"error":null}),
        );
        assert_eq!(host.session.as_deref(), Some("/one"));
        assert_eq!(host.pending.len(), 1 - index);
        assert_eq!(
            host.state.as_ref().unwrap()["config"]["permission_mode"],
            mode
        );
        if index == 0 {
            assert_eq!(host.setting_value("mode"), Some(json!("plan")));
        }
        host.activate_context("source");
        assert_eq!(host.session.as_deref(), Some("/two"));
        assert!(host.pending.is_empty());
        assert!(host.state.is_none());
        assert!(host.command_error.is_none());
    }
}

#[test]
fn protocol_four_requires_context_and_echoes_content_source_revision() {
    let (mut host, mut extension) = (Host::default(), ContentProbe::default());
    assert!(
        host.handle(
            &mut extension,
            json!({"type":"render","view":"x","surface":"panel"})
        )
        .is_err()
    );
    for (source, revision, kind) in [("one", 7, "view"), ("invalid", 9, "content_error")] {
        scoped_message(
            &mut host,
            &mut extension,
            json!({"type":"render","context":"a","view":"content:opaque@1","surface":"content","session_dir":"/one","content":{"language":"fixture","source":source,"context":{"theme":{"foreground":"#fff"}},"source_revision":revision}}),
        );
        assert_eq!(last(&host)["type"], kind);
        assert_eq!(last(&host)["source_revision"], revision);
    }
}

#[test]
fn a_context_announcement_keeps_the_configured_instance_until_its_first_surface() {
    let mut host = Host::default();
    let mut extension = instances::Instances::new(ScopedProbe {
        draft: "configured".into(),
    });
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"context","context":"a","workspace":"/workspace","session_dir":"/one"}),
    );
    extension.prune(&host);
    scoped_message(
        &mut host,
        &mut extension,
        json!({"type":"render","context":"a","surface":"panel","view":"panel@1","session_dir":"/one"}),
    );
    assert!(
        last(&host)["root"]["text"]
            .as_str()
            .unwrap()
            .contains("configured")
    );
    assert!(
        host.handle(&mut extension, json!({"type":"hello","protocol":3}))
            .is_err()
    );
}
#[test]
fn shared_preference_invalidation_requires_host_support_and_sends_no_session_payload() {
    let mut host = super::Host::default();
    assert!(host.invalidate_content().is_err());
    assert!(host.sent.borrow().is_empty());
    host.features
        .insert("surfaces.content.invalidate.v1".into());
    host.invalidate_content().unwrap();
    assert_eq!(
        *host.sent.borrow(),
        vec![serde_json::json!({"type": "invalidate_content"})]
    );
}
