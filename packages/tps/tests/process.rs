//! Real extension process against a loopback usage API; no model requests.
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{Value, json};

struct Api {
    origin: String,
    requests: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
}

fn serve() -> Api {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicBool::new(false));
    let (count, failure) = (requests.clone(), fail.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut request = String::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                request.push_str(&line);
            }
            let path = request.split(' ').nth(1).unwrap_or("");
            assert!(path.starts_with("/usage?"));
            assert!(
                request
                    .to_lowercase()
                    .contains("authorization: bearer fixture")
            );
            let (status, body) = if failure.load(Ordering::SeqCst) {
                (
                    "500 Internal Server Error",
                    json!({"error":"fixture failure"}),
                )
            } else {
                let mut sample = json!({"origin":"direct","status":"completed","model":{"provider":"fixture","model":"model"},
                    "started_at_ms":1000,"finished_at_ms":3000,"usage":{"output_tokens":120,"reasoning_output_tokens":40}});
                if path.ends_with("%2Fb") {
                    sample["usage"]["output_tokens"] = json!(60);
                }
                if path.ends_with("%2Fmissing") {
                    sample["usage"] = Value::Null;
                }
                ("200 OK", json!({"requests":[sample]}))
            };
            count.fetch_add(1, Ordering::SeqCst);
            let body = body.to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Api {
        origin,
        requests,
        fail,
    }
}

struct Process {
    child: Child,
    input: ChildStdin,
    messages: Receiver<Value>,
}

impl Process {
    fn start(origin: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tps"))
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
        let input = child.stdin.take().unwrap();
        let mut process = Self {
            child,
            input,
            messages,
        };
        process.send(
            json!({"type":"hello","protocol":4,"features":["session.state_only.v1"],
            "workspace":"/fixture","server":{"origin":origin,"token":"fixture"}}),
        );
        process
    }

    fn send(&mut self, message: Value) {
        writeln!(self.input, "{message}").unwrap();
    }

    fn render(&mut self, view: &str, surface: &str, session: Option<&str>) {
        self.send(json!({"type":"render","context":"fixture","view":view,"surface":surface,"session_dir":session}));
    }

    fn read(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        loop {
            let message = self
                .messages
                .recv_timeout(Duration::from_secs(5))
                .expect("extension response");
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

#[test]
fn exact_rate_popup_session_switch_missing_and_refresh_failure() {
    let api = serve();
    let mut process = Process::start(&api.origin);
    process.render("badge", "badge", None);
    let badge = process.read(|m| m["view"] == "badge");
    assert_eq!(badge["face"]["text"], "— TPS");
    assert_eq!(api.requests.load(Ordering::SeqCst), 0);
    process.render("badge", "badge", Some("/a"));
    let badge = process.read(|m| m["face"]["text"] == "60,0 TPS");
    assert!(badge.to_string().contains("Последний завершённый"));
    process.render("popup", "popup", Some("/a"));
    let popup = process.read(|m| m["view"] == "popup");
    let body = popup.to_string();
    for text in [
        "60,0 TPS",
        "fixture/model",
        "120",
        "2,0 с",
        "Ожидание ответа включено",
    ] {
        assert!(body.contains(text), "missing {text}");
    }
    assert_eq!(api.requests.load(Ordering::SeqCst), 1);
    process.render("badge", "badge", Some("/b"));
    let badge = process.read(|m| m["session_dir"] == "/b");
    assert_eq!(badge["face"]["text"], "30,0 TPS");
    process.render("badge", "badge", Some("/missing"));
    let badge = process.read(|m| m["session_dir"] == "/missing");
    assert_eq!(badge["face"]["text"], "— TPS");
    assert!(
        badge
            .to_string()
            .contains("не сообщил число выходных токенов")
    );
    process.render("badge", "badge", Some("/a"));
    process.read(|m| m["face"]["text"] == "60,0 TPS");
    api.fail.store(true, Ordering::SeqCst);
    process
        .send(json!({"type":"action","view":"badge","action":"refresh","value":null,"sequence":1}));
    let badge =
        process.read(|m| m["face"]["text"] == "— TPS" && m.to_string().contains("fixture failure"));
    assert!(badge.to_string().contains("Не удалось получить скорость"));
}

#[test]
fn state_updates_and_extra_surfaces_do_not_refetch_usage_but_settlement_does() {
    let api = serve();
    let mut process = Process::start(&api.origin);
    process.render("badge", "badge", Some("/a"));
    process.read(|m| m["face"]["text"] == "60,0 TPS");
    for _ in 0..8 {
        process.send(json!({"type":"session_update","context":"fixture","session_dir":"/a",
            "update":{"type":"state","state":{"execution":{"active":{"run_id":"r","status":"running"}}}}}));
    }
    process.render("popup", "popup", Some("/a"));
    process.read(|m| m["view"] == "popup");
    assert_eq!(api.requests.load(Ordering::SeqCst), 1);
    process
        .send(json!({"type":"event","context":"fixture","event":"run_settled","session_dir":"/a"}));
    // A new view acts as a response barrier even when unchanged badge trees are deduplicated.
    process.render("after", "popup", Some("/a"));
    process.read(|m| m["view"] == "after");
    assert_eq!(api.requests.load(Ordering::SeqCst), 2);
    process.send(json!({"type":"session_update","context":"fixture","session_dir":"/a",
        "update":{"type":"state","state":{"config":{"model":{"provider":"fixture","name":"other"}},"execution":{"active":null}}}}));
    let badge = process.read(|m| m["view"] == "badge" && m["face"]["text"] == "— TPS");
    assert!(badge.to_string().contains("выбранной модели"));
    assert_eq!(api.requests.load(Ordering::SeqCst), 3);
}
