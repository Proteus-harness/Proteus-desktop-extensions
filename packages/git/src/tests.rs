//! Behaviour of the git panel against a loopback API.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use proteus_extension::{Extension, Host, Server, Value, ValueExt, json};

use crate::Git;

fn changes() -> Value {
    json!({"repository": true, "branch": "main", "head": {"short": "abc1234", "summary": "Last"}, "upstream": null,
           "ahead": 0, "behind": 0, "truncated": false, "entries": [
        {"path": "src/a.rs", "original": null, "status": "modified", "staged": "modified", "unstaged": "modified"},
        {"path": "b.py", "original": null, "status": "deleted", "staged": null, "unstaged": "deleted"},
        {"path": "new.md", "original": null, "status": "untracked", "staged": null, "unstaged": "untracked"},
        {"path": "both.txt", "original": null, "status": "conflict", "staged": "conflict", "unstaged": "conflict"}]})
}

fn reply(path: &str) -> Value {
    match path {
        "/workspace/branches" => json!({"branches": [{"name": "main"}, {"name": "topic"}]}),
        "/workspace/log" => json!({"entries": []}),
        "/workspace/commit" => {
            json!({"commit": {"short": "x", "summary": "y"}, "changes": changes()})
        }
        _ => changes(),
    }
}

struct Fake {
    git: Git,
    host: Host,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    fail: Arc<AtomicBool>,
}

impl Fake {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (calls, fail) = (
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let (log, failing) = (calls.clone(), fail.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line
                    .split(' ')
                    .nth(1)
                    .unwrap_or("")
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_owned();
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    let Some((key, value)) = header.trim().split_once(':') else {
                        break;
                    };
                    if key.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                log.lock().unwrap().push((
                    path.clone(),
                    serde_json::from_slice(&body).unwrap_or(Value::Null),
                ));
                let (status, answer) = if failing.load(Ordering::SeqCst) {
                    (400, json!({"error": "nothing is staged to commit"}))
                } else {
                    (200, reply(&path))
                };
                let text = answer.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        let mut host = Host::default();
        host.server = Some(Server {
            origin,
            token: "fixture".to_owned(),
        });
        host.session = Some("/s".to_owned());
        let mut fake = Self {
            git: Git::default(),
            host,
            calls,
            fail,
        };
        fake.git.refresh(&fake.host).unwrap();
        fake
    }

    fn act(&mut self, action: &str, value: Value) {
        self.git
            .action(&mut self.host, "panel", action, &value)
            .unwrap();
    }

    fn last(&self) -> (String, Value) {
        self.calls.lock().unwrap().last().cloned().unwrap()
    }
}

#[test]
fn sections_follow_zed() {
    let mut fake = Fake::new();
    let root = fake.git.panel(&fake.host, "main").unwrap();
    let tree = root
        .list("children")
        .iter()
        .find(|node| node.str("type") == "tree")
        .unwrap();
    let rows = tree.list("rows");
    let sections: Vec<&str> = rows
        .iter()
        .filter(|row| row.i64("depth") == Some(0))
        .map(|row| row.str("id"))
        .collect();
    assert_eq!(
        sections,
        [
            "section:conflicts",
            "section:tracked",
            "section:untracked",
            "section:history"
        ]
    );
    let row = |id: &str| rows.iter().find(|row| row.str("id") == id).unwrap();
    let partial = row("src/a.rs");
    assert!(partial.bool("checked"));
    assert!(partial.str("detail").contains("частично"));
    assert_eq!(partial.str("icon"), "file-code");
    let deleted = row("b.py");
    assert!(deleted.bool("strike"));
    assert_eq!(deleted.str("tone"), "danger");
    assert!(!row("section:tracked").bool("checked"));
}

#[test]
fn checkboxes_stage_and_unstage() {
    let mut fake = Fake::new();
    fake.act("tree", json!({"id": "b.py", "event": "check"}));
    assert_eq!(
        fake.last(),
        (
            "/workspace/stage".to_owned(),
            json!({"session_dir": "/s", "paths": ["b.py"]})
        )
    );
    fake.act("tree", json!({"id": "src/a.rs", "event": "check"}));
    assert_eq!(fake.last().0, "/workspace/unstage");
    fake.act("tree", json!({"id": "section:tracked", "event": "check"}));
    assert_eq!(
        fake.last(),
        (
            "/workspace/stage".to_owned(),
            json!({"session_dir": "/s", "paths": ["src/a.rs", "b.py"]})
        )
    );
}

#[test]
fn commit_clears_the_message_and_amend_prefills_it() {
    let mut fake = Fake::new();
    fake.act("amend", json!({"checked": true, "value": null}));
    assert_eq!(fake.git.message, "Last");
    fake.act("commit", json!("Fix"));
    assert_eq!(
        fake.last(),
        (
            "/workspace/commit".to_owned(),
            json!({"session_dir": "/s", "message": "Fix", "amend": true})
        )
    );
    assert_eq!((fake.git.message.as_str(), fake.git.amend), ("", false));
}

#[test]
fn server_errors_are_shown() {
    let mut fake = Fake::new();
    fake.fail.store(true, Ordering::SeqCst);
    fake.act("commit", json!("Fix"));
    assert_eq!(
        fake.git.error.as_deref(),
        Some("nothing is staged to commit")
    );
    assert_eq!(fake.git.message, "Fix");
}
