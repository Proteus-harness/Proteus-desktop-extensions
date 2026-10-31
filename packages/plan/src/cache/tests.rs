use super::*;
use proteus_extension::json;
use std::cell::Cell;

fn host() -> Host {
    let mut host = Host::default();
    host.session = Some("/synthetic/chat".into());
    host
}

fn tool(label: &str) -> Value {
    json!({"call_id":"accepted", "name":"update_plan", "status":"done",
        "args":{"plan":[{"step":label,"status":"completed"}]}, "effective_args":null})
}

#[test]
fn one_compact_read_survives_duplicate_surfaces_and_unrelated_metadata() {
    let mut host = host();
    let mut cache = Cache::default();
    let calls = Cell::new(0);
    let load = || {
        calls.set(calls.get() + 1);
        Ok(Some(tool("outside the resident window")))
    };
    assert_eq!(
        cache.lookup(key(&host), 1, load).unwrap().unwrap().steps[0].text,
        "outside the resident window"
    );
    // Both summary and details, a theme redraw, then unrelated tool metadata.
    cache.lookup(key(&host), 1, load).unwrap();
    host.entries = (0..50)
        .map(|_| json!({"role":"assistant", "tool":null}))
        .collect();
    host.entries.push(json!({"role":"assistant", "tool":{
        "name":"shell", "status":"running", "result":"irrelevant"
    }}));
    host.state = Some(json!({"connected":true, "config":{"theme":"changed"}}));
    cache.lookup(key(&host), 2, load).unwrap();
    assert_eq!(calls.get(), 1);
}

#[test]
fn relevant_plan_update_snapshot_execution_and_session_invalidate_once() {
    let mut host = host();
    let mut cache = Cache::default();
    let calls = Cell::new(0);
    let load = || {
        calls.set(calls.get() + 1);
        Ok(Some(tool("authoritative latest")))
    };
    cache.lookup(key(&host), 1, load).unwrap();
    host.entries
        .push(json!({"role":"assistant", "tool":tool("resident") }));
    cache.lookup(key(&host), 2, load).unwrap();
    host.entries[0]["tool"]["effective_args"] = json!({"plan":[]});
    cache.lookup(key(&host), 3, load).unwrap();
    host.entries.push(json!({"role":"user"}));
    cache.lookup(key(&host), 4, load).unwrap();
    host.state = Some(json!({"execution":{"active":{"run_id":"new","status":"running"}}}));
    cache.lookup(key(&host), 5, load).unwrap();
    let mut snapshot = key(&host);
    snapshot.snapshot_epoch += 1;
    cache.lookup(snapshot, 6, load).unwrap();
    host.session = Some("/different/chat".into());
    cache.lookup(key(&host), 7, load).unwrap();
    cache.clear();
    assert!(cache.attempt.is_none());
    cache.lookup(key(&host), 7, load).unwrap();
    assert_eq!(calls.get(), 8);
}

#[test]
fn stale_viewport_plan_cannot_replace_authoritative_newest_plan() {
    let mut host = host();
    let mut cache = Cache::default();
    cache
        .lookup(key(&host), 1, || Ok(Some(tool("latest of whole chat"))))
        .unwrap();
    host.entries
        .push(json!({"role":"assistant", "tool":tool("older viewport") }));
    let plan = cache
        .lookup(key(&host), 2, || Ok(Some(tool("latest of whole chat"))))
        .unwrap()
        .unwrap();
    assert_eq!(plan.steps[0].text, "latest of whole chat");
}

#[test]
fn authoritative_absence_is_cached_but_not_ready_retries_only_on_an_input_event() {
    let host = host();
    let mut cache = Cache::default();
    let calls = Cell::new(0);
    let pending = || {
        calls.set(calls.get() + 1);
        Err(ApiError {
            status: 409,
            message: "cache not ready".into(),
        }
        .into())
    };
    let error = cache.lookup(key(&host), 1, pending).unwrap_err();
    assert_eq!(error.downcast_ref::<ApiError>().unwrap().status, 409);
    assert!(matches!(
        cache.attempt.as_ref().unwrap().outcome,
        Outcome::NotReady(_)
    ));
    assert!(cache.lookup(key(&host), 1, pending).is_err());
    assert_eq!(calls.get(), 1);
    assert!(cache.lookup(key(&host), 2, pending).is_err());
    assert_eq!(calls.get(), 2);
    assert!(cache.lookup(key(&host), 3, || Ok(None)).unwrap().is_none());
    // A 200/null is a successful result; unrelated metadata does not retry it.
    cache
        .lookup(key(&host), 4, || panic!("authoritative absence was lost"))
        .unwrap();
}

#[test]
fn idle_chat_manual_retry_reads_once_then_reuses_success_without_an_input_event() {
    let host = host();
    let mut cache = Cache::default();
    let calls = Cell::new(0);
    assert!(
        cache
            .lookup(key(&host), 1, || {
                calls.set(calls.get() + 1);
                Err(ApiError {
                    status: 409,
                    message: "not ready".into(),
                }
                .into())
            })
            .is_err()
    );
    assert!(
        cache
            .lookup(key(&host), 1, || panic!("redraw retried an error"))
            .is_err()
    );
    cache.retry();
    let recovered = cache
        .lookup(key(&host), 1, || {
            calls.set(calls.get() + 1);
            Ok(Some(tool("Recovered on idle chat")))
        })
        .unwrap()
        .unwrap();
    assert_eq!(recovered.steps[0].text, "Recovered on idle chat");
    cache
        .lookup(key(&host), 1, || panic!("second surface repeated retry"))
        .unwrap();
    assert_eq!(calls.get(), 2);
}

#[test]
fn malformed_latest_clears_and_unsupported_endpoint_is_useful_without_a_retry_loop() {
    let host = host();
    let mut cache = Cache::default();
    let mut malformed = tool("old");
    malformed["args"] = json!({"plan":[]});
    assert!(
        cache
            .lookup(key(&host), 1, || Ok(Some(malformed)))
            .unwrap()
            .is_none()
    );
    cache.clear();
    let first = cache
        .lookup(key(&host), 2, || {
            Err(ApiError {
                status: 404,
                message: "unknown history endpoint".into(),
            }
            .into())
        })
        .unwrap_err()
        .to_string();
    assert!(first.contains("unknown history endpoint"));
    let again = cache
        .lookup(key(&host), 2, || panic!("repeated render queried again"))
        .unwrap_err()
        .to_string();
    assert_eq!(again, first);
}
