//! The real process against a loopback fixture of the public API.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{Value, json};

/// Serves `/model/quota`; returns the origin and the count of answered requests.
fn serve() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let served = Arc::new(AtomicUsize::new(0));
    let counter = served.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                request.push_str(&line);
            }
            let path = request.split(' ').nth(1).unwrap_or("");
            let (status, body) = if !path.starts_with("/model/quota?")
                || !request
                    .to_lowercase()
                    .contains("authorization: bearer fixture")
            {
                ("403 Forbidden", json!({"error": "forbidden"}))
            } else if path.ends_with("session_dir=%2Fempty") {
                ("200 OK", Value::Null)
            } else if path.ends_with("session_dir=%2Ffail") {
                (
                    "500 Internal Server Error",
                    json!({"error": "fixture failure"}),
                )
            } else {
                (
                    "200 OK",
                    json!({"observed_at": 1_700_000_000, "plan": "test", "buckets": [{"id": "main", "windows": [
                    {"id": "weekly", "duration_seconds": 604_800, "used_percent": 92.5},
                    {"id": "other", "duration_seconds": 604_800, "used_percent": 80}]}]}),
                )
            };
            let body = body.to_string();
            counter.fetch_add(1, Ordering::SeqCst);
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (origin, served)
}

struct Process {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
}

impl Process {
    fn start(origin: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quota"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(serde_json::from_str(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take().unwrap();
        let mut process = Self {
            child,
            stdin,
            messages,
        };
        process.send(json!({"type": "hello", "protocol": 3, "features": ["session.settings.v1"], "workspace": "/tmp",
                            "server": {"origin": origin, "token": "fixture"}}));
        process
    }

    fn send(&mut self, message: Value) {
        writeln!(self.stdin, "{message}").unwrap();
    }

    fn badge(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        loop {
            let message = self
                .messages
                .recv_timeout(Duration::from_secs(5))
                .expect("badge");
            if predicate(&message) {
                return message;
            }
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn has_line(badge: &Value, text: &str) -> bool {
    badge["lines"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line.as_str().unwrap().contains(text))
}

#[test]
fn api_session_switch_and_refresh() {
    let (origin, served) = serve();
    let mut process = Process::start(&origin);
    process
        .send(json!({"type": "render", "view": "badge", "surface": "badge", "session_dir": null}));
    let badge = process.badge(|_| true);
    assert_eq!(
        (badge["title"].as_str(), badge["lines"].clone()),
        (Some("Лимиты модели"), json!(["Нет активной сессии."]))
    );

    process.send(json!({"type": "event", "event": "session_opened", "session_dir": "/a"}));
    let badge = process.badge(|badge| badge["face"]["type"] == "ring");
    assert_eq!(badge["title"], "Лимиты: осталось 7,5%");
    assert_eq!(
        badge["face"],
        json!({"type": "ring", "percent": 92.5, "tone": "danger"})
    );
    assert!(has_line(&badge, "7 дн."));

    process.send(json!({"type": "event", "event": "session_opened", "session_dir": "/empty"}));
    let badge = process.badge(|badge| {
        badge["session_dir"] == "/empty" && has_line(badge, "Провайдер не сообщает лимиты.")
    });
    assert_eq!(badge["face"]["type"], "icon");

    let before = served.load(Ordering::SeqCst);
    process.send(json!({"type": "event", "event": "run_settled", "session_dir": "/empty"}));
    process.badge(|_| true);
    process.badge(|_| true);
    assert_eq!(served.load(Ordering::SeqCst), before + 1);

    process.send(json!({"type": "event", "event": "session_opened", "session_dir": "/fail"}));
    let badge = process.badge(|badge| has_line(badge, "Не удалось получить"));
    assert_eq!(
        badge["lines"],
        json!(["Не удалось получить: fixture failure"])
    );
}
