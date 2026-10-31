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

#[test]
fn compact_tool_state_encodes_exact_names_and_distinguishes_absence_from_not_ready() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let expected = json!({"call_id":"fixture", "name":"tools/план & x", "status":"done",
        "args":"{\"plan\":[]}", "effective_args":{"plan":[]}});
    let returned = expected.clone();
    let server = std::thread::spawn(move || {
        for (status, body) in [
            (200, json!({"revision":"r1","tool":returned})),
            (200, json!({"revision":"r1","tool":null})),
            (409, json!({"error":"tool-state cache not ready"})),
            (200, json!({"revision":"r1"})),
            (200, json!({"revision":"r1","tool":{"name":"different"}})),
        ] {
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
            assert!(request.starts_with("GET /history/tool-state?name=tools%2F%D0%BF%D0%BB%D0%B0%D0%BD%20%26%20x&session_dir=%2Fchat%20%26%20%D0%B6 "));
            assert!(
                request
                    .to_lowercase()
                    .contains("authorization: bearer fixture\r\n")
            );
            let body = body.to_string();
            write!(socket, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let mut host = Host::default();
    host.features.insert("session.tool_state.v1".into());
    host.server = Some(Server {
        origin,
        token: "fixture".into(),
    });
    host.session = Some("/chat & ж".into());
    assert_eq!(
        host.latest_tool_state("tools/план & x").unwrap(),
        Some(expected)
    );
    assert_eq!(host.latest_tool_state("tools/план & x").unwrap(), None);
    let not_ready = host.latest_tool_state("tools/план & x").unwrap_err();
    assert_eq!(not_ready.downcast_ref::<ApiError>().unwrap().status, 409);
    assert_eq!(not_ready.to_string(), "tool-state cache not ready");
    assert!(
        host.latest_tool_state("tools/план & x")
            .unwrap_err()
            .to_string()
            .contains("отсутствует tool")
    );
    assert!(
        host.latest_tool_state("tools/план & x")
            .unwrap_err()
            .to_string()
            .contains("неверный ответ")
    );
    server.join().unwrap();
    host.features.clear();
    assert!(
        host.latest_tool_state("update_plan")
            .unwrap_err()
            .to_string()
            .contains("session.tool_state.v1")
    );
    host.features.insert("session.tool_state.v1".into());
    host.session = None;
    assert!(host.latest_tool_state("update_plan").is_err());
    host.session = Some("/chat".into());
    host.server = None;
    assert!(host.latest_tool_state("update_plan").is_err());
}

#[test]
fn session_input_epochs_exclude_redraws_but_track_snapshot_and_metadata_updates() {
    let (mut host, mut extension) = (Host::default(), Flaky::default());
    host.handle(
        &mut extension,
        json!({"type":"session_snapshot","context":"fixture",
        "session_dir":"/one","state":{},"entries":[]}),
    )
    .unwrap();
    assert_eq!(
        (host.session_event_epoch(), host.session_snapshot_epoch()),
        (1, 1)
    );
    host.handle(
        &mut extension,
        json!({"type":"render","context":"fixture",
        "session_dir":"/one","view":"page","surface":"page"}),
    )
    .unwrap();
    assert_eq!(
        (host.session_event_epoch(), host.session_snapshot_epoch()),
        (1, 1)
    );
    host.handle(
        &mut extension,
        json!({"type":"session_update","context":"fixture",
        "session_dir":"/one","update":{"type":"state","state":{"connected":true}}}),
    )
    .unwrap();
    assert_eq!(
        (host.session_event_epoch(), host.session_snapshot_epoch()),
        (2, 1)
    );
    host.handle(
        &mut extension,
        json!({"type":"session_snapshot","context":"fixture",
        "session_dir":"/one","state":{},"entries":[]}),
    )
    .unwrap();
    assert_eq!(
        (host.session_event_epoch(), host.session_snapshot_epoch()),
        (3, 2)
    );
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

#[test]
fn preview_is_sessionless_demand_driven_and_releases_its_retained_source() {
    #[derive(Default)]
    struct PreviewProbe {
        closed: Vec<String>,
    }
    impl Extension for PreviewProbe {
        fn poll(&self) -> Option<Duration> {
            Some(Duration::from_millis(1))
        }
        fn render(
            &mut self,
            host: &mut Host,
            view: &str,
            surface: Surface,
            _: Option<&str>,
        ) -> Result {
            assert!(matches!(surface, Surface::Preview | Surface::Content));
            assert!(host.server.is_none());
            assert!(host.session.is_none());
            assert!(host.state.is_none());
            assert!(host.entries.is_empty());
            assert_eq!(
                host.content(view).unwrap().context["theme"]["foreground"],
                "#abcdef"
            );
            host.show(view, ui::text("Synthetic preview"));
            Ok(())
        }
        fn closed(&mut self, _: &mut Host, view: &str) {
            self.closed.push(view.into());
        }
    }
    let (mut host, mut extension) = (Host::default(), PreviewProbe::default());
    host.handle(
        &mut extension,
        json!({"type":"hello","protocol":4,"workspace":"","server":{"origin":"","token":""}}),
    )
    .unwrap();
    assert!(host.poll(&extension).is_none());
    host.handle(
        &mut extension,
        json!({"type":"context","context":"preview","workspace":""}),
    )
    .unwrap();
    host.handle(
        &mut extension,
        json!({"type":"render","context":"preview","view":"preview","surface":"preview",
        "content":{"language":"","source":"","context":{"theme":{"foreground":"#abcdef"}}}}),
    )
    .unwrap();
    assert_eq!(last(&host)["session_dir"], Value::Null);
    assert!(
        host.poll(&extension).is_none(),
        "production poll must not wake previews"
    );
    host.handle(&mut extension, json!({"type":"render","context":"preview","view":"nested-content","surface":"content",
        "content":{"language":"latex","source":"x+y","context":{"theme":{"foreground":"#abcdef"}}}})).unwrap();
    assert_eq!(last(&host)["session_dir"], Value::Null);
    assert!(
        host.poll(&extension).is_none(),
        "a nested Content surface must not enable production polling"
    );
    host.handle(
        &mut extension,
        json!({"type":"close","view":"nested-content"}),
    )
    .unwrap();
    assert!(host.content("nested-content").is_none());
    host.handle(&mut extension, json!({"type":"close","view":"preview"}))
        .unwrap();
    assert_eq!(extension.closed, ["nested-content", "preview"]);
    assert!(host.content("preview").is_none());
    assert!(host.shown.is_empty());
    assert!(host.poll(&extension).is_none());
    // An ordinary retained surface still enables its existing production timer.
    host.views.insert("badge".into(), (Surface::Badge, None));
    assert!(host.poll(&extension).is_none());
    host.server = Some(Server {
        origin: "http://fixture.invalid".into(),
        token: "fixture".into(),
    });
    assert_eq!(host.poll(&extension), Some(Duration::from_millis(1)));
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
fn content_render_moves_owned_buffers_and_keeps_them_across_session_refresh() {
    let (mut host, mut extension) = (Host::default(), ContentProbe::default());
    for revision in [7, 8] {
        let source = format!("revision {revision}: Привет 🦀\r\n\"quoted\"\\\0").repeat(128);
        let message = json!({"type":"render", "context":"fixture", "surface":"content", "view":"content:moved",
            "session_dir":"/fixture/chat-one", "content":{"language":"future-figure", "source":source,
                "context":{"languages":["mermaid", "latex"], "theme":{"unknown":"owned context 🦀"}},
                "source_revision":revision, "future_field":{"ignored":true}}});
        let payload = &message["content"];
        let language_ptr = payload["language"].as_str().unwrap().as_ptr();
        let source_ptr = payload["source"].as_str().unwrap().as_ptr();
        let languages_ptr = payload["context"]["languages"].as_array().unwrap().as_ptr();
        let theme_ptr = payload["context"]["theme"]["unknown"]
            .as_str()
            .unwrap()
            .as_ptr();
        let expected_context = payload["context"].clone();
        host.handle(&mut extension, message).unwrap();
        let retained = host.content("content:moved").unwrap();
        assert_eq!(retained.language, "future-figure");
        assert_eq!(retained.source, source);
        assert_eq!(retained.context, expected_context);
        assert_eq!(retained.source_revision, Some(revision));
        assert_eq!(retained.language.as_ptr(), language_ptr);
        assert_eq!(retained.source.as_ptr(), source_ptr);
        assert_eq!(
            retained.context["languages"].as_array().unwrap().as_ptr(),
            languages_ptr
        );
        assert_eq!(
            retained.context["theme"]["unknown"]
                .as_str()
                .unwrap()
                .as_ptr(),
            theme_ptr
        );
        assert_eq!(last(&host)["source_revision"], revision);
        host.handle(
            &mut extension,
            json!({"type":"session_update", "context":"fixture", "session_dir":"/fixture/chat-one",
            "update":{"type":"state", "state":{"fixture":revision}}}),
        )
        .unwrap();
        assert_eq!(
            host.content("content:moved").unwrap().source.as_ptr(),
            source_ptr
        );
        assert_eq!(last(&host)["source_revision"], revision);
    }
    assert_eq!(extension.sources.len(), 4);
}

#[test]
fn malformed_content_preserves_validation_order_and_previous_payload() {
    let (mut host, mut extension) = (Host::default(), ContentProbe::default());
    host.handle(&mut extension, json!({"type":"render", "context":"fixture", "surface":"content", "view":"content:valid",
        "session_dir":"/fixture/chat-one", "content":{"language":"future-figure", "source":"kept", "source_revision":3}})).unwrap();
    let source_ptr = host.content("content:valid").unwrap().source.as_ptr();
    let previous = host.content("content:valid").unwrap().clone();
    let sent = host.sent.borrow().len();
    for (content, expected) in [
        (Value::Null, "content.language is required"),
        (json!([]), "content.language is required"),
        (json!("not an object"), "content.language is required"),
        (json!({}), "content.language is required"),
        (
            json!({"language":0, "source":false}),
            "content.language is required",
        ),
        (
            json!({"language":"future-figure"}),
            "content.source is required",
        ),
        (
            json!({"language":"future-figure", "source":null}),
            "content.source is required",
        ),
        (
            json!({"language":"future-figure", "source":[]}),
            "content.source is required",
        ),
    ] {
        let message = json!({"type":"render", "context":"fixture", "surface":"content", "view":"content:valid",
            "session_dir":"/fixture/chat-one", "content":content});
        assert_eq!(
            host.handle(&mut extension, message)
                .unwrap_err()
                .to_string(),
            expected
        );
        assert_eq!(host.content("content:valid"), Some(&previous));
        assert_eq!(
            host.content("content:valid").unwrap().source.as_ptr(),
            source_ptr
        );
        assert_eq!(host.sent.borrow().len(), sent);
        assert_eq!(extension.sources.len(), 1);
    }
    let missing = json!({"type":"render", "context":"fixture", "surface":"content", "view":"content:valid",
        "session_dir":"/fixture/chat-one"});
    assert_eq!(
        host.handle(&mut extension, missing)
            .unwrap_err()
            .to_string(),
        "content.language is required"
    );
    assert_eq!(host.content("content:valid"), Some(&previous));
}

#[test]
fn malformed_content_still_switches_session_after_the_old_close_callback() {
    let (mut host, mut extension) = (Host::default(), ScopedProbe::default());
    host.handle(&mut extension, json!({"type":"render", "context":"fixture", "surface":"content", "view":"changing",
        "session_dir":"/one", "content":{"language":"future-figure", "source":"kept", "source_revision":3}})).unwrap();
    let previous = host.content("changing").unwrap().clone();
    let source_ptr = host.content("changing").unwrap().source.as_ptr();
    let error = host
        .handle(
            &mut extension,
            json!({"type":"render", "context":"fixture", "surface":"content", "view":"changing",
        "session_dir":"/two", "content":{"language":"future-figure", "source":null}}),
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "content.source is required");
    assert_eq!(host.session.as_deref(), Some("/two"));
    assert_eq!(last(&host)["type"], "closed_probe");
    assert_eq!(last(&host)["view"], "changing");
    assert_eq!(last(&host)["session"], "/one");
    assert_eq!(host.content("changing"), Some(&previous));
    assert_eq!(
        host.content("changing").unwrap().source.as_ptr(),
        source_ptr
    );
    assert!(!host.is_shown("changing"));
    assert_eq!(host.sent.borrow().len(), 2);
}

#[test]
fn moved_content_keeps_optional_metadata_and_empty_string_rules() {
    for (content, context, revision) in [
        (json!({"language":"", "source":""}), Value::Null, None),
        (
            json!({"language":"", "source":"", "context":false, "source_revision":0}),
            json!(false),
            Some(0),
        ),
        (
            json!({"language":"", "source":"", "context":["future"], "source_revision":u64::MAX}),
            json!(["future"]),
            Some(u64::MAX),
        ),
        (
            json!({"language":"", "source":"", "context":{"future":1}, "source_revision":-1}),
            json!({"future":1}),
            None,
        ),
        (
            json!({"language":"", "source":"", "source_revision":1.5}),
            Value::Null,
            None,
        ),
        (
            json!({"language":"", "source":"", "source_revision":"7"}),
            Value::Null,
            None,
        ),
    ] {
        let mut message = json!({"content":content});
        assert_eq!(
            Content::take_from(&mut message).unwrap(),
            Content {
                language: String::new(),
                source: String::new(),
                context,
                source_revision: revision,
            }
        );
    }
}

#[test]
fn non_content_render_ignores_payload_and_releases_retained_content() {
    let (mut host, mut extension) = (Host::default(), Flaky::default());
    host.handle(
        &mut extension,
        json!({"type":"render", "context":"fixture", "surface":"content", "view":"changing",
        "session_dir":"/fixture/chat-one", "content":{"language":"future-figure", "source":"old"}}),
    )
    .unwrap();
    assert!(host.content("changing").is_some());
    host.handle(
        &mut extension,
        json!({"type":"render", "context":"fixture", "surface":"page", "view":"changing",
        "session_dir":"/fixture/chat-one", "content":{"language":false, "source":null}}),
    )
    .unwrap();
    assert!(host.content("changing").is_none());
    assert_eq!(host.surface("changing"), Some(Surface::Page));
    assert_eq!(last(&host)["source_revision"], Value::Null);
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

#[test]
fn generic_surface_invalidation_is_explicit_and_independent_of_content_capability() {
    let host = Host::default();
    assert!(host.invalidate_surfaces().is_err());
    assert!(host.sent.borrow().is_empty());
    let mut host = host;
    host.features.insert("surfaces.invalidate.v1".into());
    host.invalidate_surfaces().unwrap();
    assert_eq!(
        *host.sent.borrow(),
        vec![json!({"type":"invalidate_surfaces"})]
    );
    assert!(host.invalidate_content().is_err());
}
