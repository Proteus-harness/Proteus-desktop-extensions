use super::*;

#[derive(Default)]
struct Observe {
    observations: Vec<Value>,
}

impl Observe {
    fn record(&mut self, host: &Host, callback: &str) {
        self.observations.push(json!({
            "callback":callback, "session":host.session, "state":host.state,
            "entries":host.entries, "awaiting":host.awaiting_snapshot,
            "epochs":[host.session_event_epoch(), host.session_snapshot_epoch()]
        }));
    }
}

impl Extension for Observe {
    fn render(&mut self, host: &mut Host, _: &str, _: Surface, _: Option<&str>) -> Result {
        self.record(host, "render");
        Ok(())
    }

    fn session_changed(&mut self, host: &mut Host) {
        self.record(host, "session_changed");
    }

    fn closed(&mut self, host: &mut Host, _: &str) {
        self.record(host, "closed");
    }
}

fn snapshot(session: &str, state: Value, entries: Value) -> Value {
    let mut message = json!({"type":"session_snapshot", "context":"fixture",
        "session_dir":session, "state":null, "entries":null});
    message["state"] = state;
    message["entries"] = entries;
    message
}

#[test]
fn snapshot_moves_state_and_the_original_entry_allocation() {
    let (mut host, mut extension) = (Host::default(), Observe::default());
    let message = snapshot(
        "/one",
        json!({"nested":["owned state 🦀"]}),
        json!([{"text":"first owned entry"}, {"text":"second owned entry"}]),
    );
    let state_array = message["state"]["nested"].as_array().unwrap().as_ptr();
    let state_text = message["state"]["nested"][0].as_str().unwrap().as_ptr();
    let entries = message["entries"].as_array().unwrap().as_ptr();
    let entry_text = message["entries"][0]["text"].as_str().unwrap().as_ptr();
    host.handle(&mut extension, message).unwrap();
    let state = host.state.as_ref().unwrap();
    assert_eq!(state["nested"].as_array().unwrap().as_ptr(), state_array);
    assert_eq!(state["nested"][0].as_str().unwrap().as_ptr(), state_text);
    assert_eq!(host.entries.as_ptr(), entries);
    assert_eq!(
        host.entries[0]["text"].as_str().unwrap().as_ptr(),
        entry_text
    );
    assert_eq!(host.entries.len(), 2);
    assert_eq!(
        (host.session_event_epoch(), host.session_snapshot_epoch()),
        (1, 1)
    );
}

#[test]
fn state_updates_and_entry_splices_move_nested_payloads_and_keep_range_rules() {
    let (mut host, mut extension) = (Host::default(), Observe::default());
    host.handle(
        &mut extension,
        snapshot(
            "/one",
            json!({}),
            json!([{"text":"keep"}, {"text":"remove"}, {"text":"tail"}]),
        ),
    )
    .unwrap();
    let keep = host.entries[0]["text"].as_str().unwrap().as_ptr();
    let tail = host.entries[2]["text"].as_str().unwrap().as_ptr();
    let message = json!({"type":"session_update", "context":"fixture", "session_dir":"/one",
        "update":{"type":"state", "state":{"nested":["new owned state 🦀"]}}});
    let nested = message["update"]["state"]["nested"]
        .as_array()
        .unwrap()
        .as_ptr();
    let text = message["update"]["state"]["nested"][0]
        .as_str()
        .unwrap()
        .as_ptr();
    host.handle(&mut extension, message).unwrap();
    let state = host.state.as_ref().unwrap();
    assert_eq!(state["nested"].as_array().unwrap().as_ptr(), nested);
    assert_eq!(state["nested"][0].as_str().unwrap().as_ptr(), text);
    let message = json!({"type":"session_update", "context":"fixture", "session_dir":"/one",
        "update":{"type":"entries", "start":1, "removed":1,
            "entries":[{"text":"inserted owned value", "nested":["owned array"]}]}});
    let inserted = message["update"]["entries"][0]["text"]
        .as_str()
        .unwrap()
        .as_ptr();
    let inserted_array = message["update"]["entries"][0]["nested"]
        .as_array()
        .unwrap()
        .as_ptr();
    host.handle(&mut extension, message).unwrap();
    assert_eq!(host.entries[0]["text"].as_str().unwrap().as_ptr(), keep);
    assert_eq!(host.entries[1]["text"].as_str().unwrap().as_ptr(), inserted);
    assert_eq!(
        host.entries[1]["nested"].as_array().unwrap().as_ptr(),
        inserted_array
    );
    assert_eq!(host.entries[2]["text"].as_str().unwrap().as_ptr(), tail);
    // Out-of-range start appends; missing start removes from zero. Both
    // ranges clamp to the retained history, as before the move optimization.
    for update in [
        json!({"type":"entries", "start":99, "removed":99, "entries":["appended"]}),
        json!({"type":"entries", "removed":99, "entries":["replacement"]}),
    ] {
        host.handle(
            &mut extension,
            json!({"type":"session_update", "context":"fixture",
            "session_dir":"/one", "update":update}),
        )
        .unwrap();
    }
    assert_eq!(host.entries, [json!("replacement")]);
    assert_eq!(
        (host.session_event_epoch(), host.session_snapshot_epoch()),
        (5, 1)
    );
}

#[test]
fn missing_and_mistyped_projections_keep_permissive_input_semantics() {
    let (mut host, mut extension) = (Host::default(), Observe::default());
    for state in [
        Value::Null,
        json!(false),
        json!(13),
        json!("state"),
        json!([1]),
    ] {
        for entries in [
            Value::Null,
            json!(false),
            json!(1),
            json!("entries"),
            json!({}),
        ] {
            host.handle(&mut extension, snapshot("/one", state.clone(), entries))
                .unwrap();
            assert_eq!(host.state, Some(state.clone()));
            assert!(host.entries.is_empty());
            host.handle(
                &mut extension,
                json!({"type":"session_update", "context":"fixture",
                "session_dir":"/one", "update":{"type":"state", "state":state}}),
            )
            .unwrap();
            assert_eq!(host.state, Some(state.clone()));
        }
    }
    host.handle(
        &mut extension,
        json!({"type":"session_snapshot", "context":"fixture",
        "session_dir":"/one"}),
    )
    .unwrap();
    assert_eq!(host.state, Some(Value::Null));
    assert!(host.entries.is_empty());
    for update in [
        json!({"type":"state"}),
        json!({"type":"entries", "removed":99}),
        json!({"type":"entries", "removed":99, "entries":false}),
        Value::Null,
        json!("unknown update"),
    ] {
        host.entries = vec![json!("old")];
        host.handle(
            &mut extension,
            json!({"type":"session_update", "context":"fixture",
            "session_dir":"/one", "update":update}),
        )
        .unwrap();
        assert_eq!(host.state, Some(Value::Null));
        if update["type"] == "entries" {
            assert!(host.entries.is_empty());
        } else {
            assert_eq!(host.entries, [json!("old")]);
        }
    }
}

#[test]
fn stale_updates_do_not_touch_owned_state_epochs_or_render_callbacks() {
    let (mut host, mut extension) = (Host::default(), Observe::default());
    host.handle(
        &mut extension,
        snapshot("/one", json!(["kept state"]), json!(["kept entry"])),
    )
    .unwrap();
    host.handle(
        &mut extension,
        json!({"type":"render", "context":"fixture",
        "session_dir":"/one", "view":"page", "surface":"page"}),
    )
    .unwrap();
    extension.observations.clear();
    let state = host.state.as_ref().unwrap()[0].as_str().unwrap().as_ptr();
    let entries = host.entries.as_ptr();
    for update in [
        json!({"type":"state", "state":false}),
        json!({"type":"entries", "removed":99, "entries":["stale"]}),
    ] {
        host.handle(
            &mut extension,
            json!({"type":"session_update", "context":"fixture",
            "session_dir":"/old", "update":update}),
        )
        .unwrap();
    }
    assert_eq!(
        host.state.as_ref().unwrap()[0].as_str().unwrap().as_ptr(),
        state
    );
    assert_eq!(host.entries.as_ptr(), entries);
    assert_eq!(
        (host.session_event_epoch(), host.session_snapshot_epoch()),
        (1, 1)
    );
    assert!(extension.observations.is_empty());
}

#[test]
fn snapshot_switch_callbacks_see_old_then_empty_then_new_projection() {
    let (mut host, mut extension) = (Host::default(), Observe::default());
    host.handle(
        &mut extension,
        snapshot("/one", json!("old state"), json!(["old entry"])),
    )
    .unwrap();
    host.handle(
        &mut extension,
        json!({"type":"render", "context":"fixture",
        "session_dir":"/one", "view":"page", "surface":"page"}),
    )
    .unwrap();
    extension.observations.clear();
    host.handle(
        &mut extension,
        snapshot("/two", json!("new state"), json!(["new entry"])),
    )
    .unwrap();
    assert_eq!(
        extension.observations,
        [
            json!({"callback":"closed", "session":"/one", "state":"old state",
            "entries":["old entry"], "awaiting":false, "epochs":[2,2]}),
            json!({"callback":"session_changed", "session":"/two", "state":null,
            "entries":[], "awaiting":true, "epochs":[2,2]}),
            json!({"callback":"render", "session":"/two", "state":"new state",
            "entries":["new entry"], "awaiting":false, "epochs":[2,2]}),
        ]
    );
}

#[test]
fn borrowed_view_sends_keep_exact_envelopes_owned_fallback_and_public_send() {
    let mut host = Host::default();
    host.session = Some("/fixture/🦀".into());
    host.sequences.insert("figure".into(), 17);
    host.content.insert(
        "figure".into(),
        Content {
            language: "figure".into(),
            source: "source".into(),
            context: Value::Null,
            source_revision: Some(42),
        },
    );
    for titled in [false, true] {
        let root = json!({"type":"text", "text":"owned tree 🦀\n\"quoted\""});
        let text = root["text"].as_str().unwrap().as_ptr();
        let mut expected = json!({"type":"view", "session_dir":"/fixture/🦀", "view":"figure",
            "root":root, "sequence":17, "source_revision":42, "keys":{"Enter":"open"}});
        if titled {
            expected = json!({"type":"view", "session_dir":"/fixture/🦀", "view":"figure",
                "root":root, "title":"Caption 🦀", "sequence":17, "source_revision":42});
            host.show_with_title("figure", root, "Caption 🦀");
        } else {
            host.show_with_keys("figure", root, json!({"Enter":"open"}));
        }
        assert_eq!(
            host.sent.borrow().last().unwrap().to_string(),
            expected.to_string()
        );
        assert_eq!(
            host.shown["figure"]["text"].as_str().unwrap().as_ptr(),
            text
        );
        host.views.insert("figure".into(), (Surface::Content, None));
        host.failed("figure", "failed content");
        assert_eq!(
            host.sent.borrow().last().unwrap(),
            &json!({"type":"content_error",
            "session_dir":"/fixture/🦀", "view":"figure", "error":"failed content",
            "sequence":17, "source_revision":42})
        );
        // Reuse the view ID for an ordinary page. Its failed replacement
        // still uses the last successful tree, even if Content failed first.
        host.views.insert("figure".into(), (Surface::Page, None));
        host.content.remove("figure");
        host.failed("figure", "failed page");
        let sent = host.sent.borrow();
        let fallback = sent.last().unwrap();
        assert_eq!(fallback["sequence"], 17);
        assert_eq!(fallback["session_dir"], "/fixture/🦀");
        assert_eq!(fallback["keys"], json!({}));
        assert_eq!(fallback["root"]["children"][1], expected["root"]);
        assert_eq!(
            host.shown["figure"]["text"].as_str().unwrap().as_ptr(),
            text
        );
        drop(sent);
        host.content.insert(
            "figure".into(),
            Content {
                language: "figure".into(),
                source: "source".into(),
                context: Value::Null,
                source_revision: Some(42),
            },
        );
    }
    let public = json!({"type":"fixture", "custom":["unchanged"]});
    host.send(public.clone());
    assert_eq!(host.sent.borrow().last(), Some(&public));
}
