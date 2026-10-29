//! Actual protocol/child lifecycle; synthetic content only, no backend/API.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

struct Harness {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<Value>,
}

impl Harness {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mermaid-render"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (send, output) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(value) = serde_json::from_str(&line) else {
                    break;
                };
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        let mut harness = Self {
            child,
            input,
            output,
        };
        harness.send(json!({"type":"hello", "protocol":4,
            "features":["surfaces.content.v1","widgets.svg.v1"],
            "server":{"origin":"http://127.0.0.1:1","token":"synthetic-unused"}}));
        harness
    }

    fn send(&mut self, value: Value) {
        let stdin = self.input.as_mut().unwrap();
        writeln!(stdin, "{value}").unwrap();
        stdin.flush().unwrap();
    }

    fn receive(&self) -> Value {
        self.output
            .recv_timeout(Duration::from_secs(8))
            .expect("extension response")
    }

    fn context(&mut self, context: &str, session: &str) {
        self.send(json!({"type":"context", "context":context,
            "workspace":"/synthetic/workspace", "session_dir":session}));
    }

    fn render(&mut self, context: &str, session: &str, view: &str, source: &str) -> Value {
        self.send(
            json!({"type":"render", "context":context, "session_dir":session,
            "view":view, "surface":"content", "content":{
                "language":"mermaid", "source":source, "source_revision":1,
                "context":{"theme":{
                    "foreground":"#eeeeee", "background":"#111111", "muted":"#999999",
                    "border":"#555555", "accent":"#99aaff", "danger":"#ff9999",
                    "surface":"#222222", "surface_alt":"#333333",
                    "font_family":"sans-serif", "font_family_mono":"monospace"
                }}
            }}),
        );
        self.receive()
    }

    fn children(&self) -> Vec<u32> {
        let mut children = Vec::new();
        let tasks = std::fs::read_dir(format!("/proc/{}/task", self.child.id())).unwrap();
        for task in tasks.flatten() {
            if let Ok(text) = std::fs::read_to_string(task.path().join("children")) {
                children.extend(
                    text.split_whitespace()
                        .filter_map(|pid| pid.parse::<u32>().ok()),
                );
            }
        }
        children.sort_unstable();
        children.dedup();
        children
    }

    fn finish(&mut self) {
        self.input.take();
        wait_until(|| self.child.try_wait().unwrap().is_some());
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        // Only a failed test reaches the forced path; terminate children while
        // their parent still owns them, then reap the test extension itself.
        if self.child.try_wait().ok().flatten().is_none() {
            for pid in self.children() {
                let _ = Command::new("kill")
                    .args(["-KILL", &pid.to_string()])
                    .status();
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if condition() {
            return;
        }
        assert!(Instant::now() < deadline, "lifecycle condition timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn different_sessions_share_one_worker_and_last_close_and_eof_reap_it() {
    let mut harness = Harness::new();
    assert!(harness.children().is_empty(), "worker must be lazy");
    harness.context("left", "/synthetic/session-one");
    let first = harness.render(
        "left",
        "/synthetic/session-one",
        "first",
        "flowchart LR\n A[Привет]-->B[Мир]",
    );
    assert_eq!(first["type"], "view", "{first}");
    assert_eq!(first["root"]["type"], "svg");
    assert!(first["root"]["source"].as_str().unwrap().contains("Привет"));
    let worker = harness.children();
    assert_eq!(worker.len(), 1);
    harness.context("right", "/synthetic/session-two");
    let second = harness.render(
        "right",
        "/synthetic/session-two",
        "second",
        "sequenceDiagram\n A->>B: Готово",
    );
    assert_eq!(second["type"], "view", "{second}");
    assert_eq!(
        harness.children(),
        worker,
        "one child across session instances"
    );
    harness.send(json!({"type":"close", "view":"first"}));
    let cached = harness.render(
        "right",
        "/synthetic/session-two",
        "second",
        "sequenceDiagram\n A->>B: Готово",
    );
    assert_eq!(cached["root"], second["root"]);
    assert_eq!(
        harness.children(),
        worker,
        "closing one view keeps the shared child"
    );
    harness.send(json!({"type":"close", "view":"second"}));
    wait_until(|| harness.children().is_empty());
    assert!(!Path::new(&format!("/proc/{}", worker[0])).exists());
    let reopened = harness.render(
        "right",
        "/synthetic/session-two",
        "third",
        "flowchart TB\n A-->B",
    );
    assert_eq!(reopened["type"], "view", "{reopened}");
    let worker = harness.children();
    assert_eq!(worker.len(), 1);
    harness.finish();
    wait_until(|| !Path::new(&format!("/proc/{}", worker[0])).exists());
}

#[test]
fn unsupported_mermaid_is_an_addressed_content_error() {
    let mut harness = Harness::new();
    harness.context("left", "/synthetic/session");
    let result = harness.render("left", "/synthetic/session", "bad", "unsupported Привет");
    assert_eq!(result["type"], "content_error", "{result}");
    assert_eq!(result["view"], "bad");
    assert!(
        result["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    wait_until(|| harness.children().is_empty());
}
