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
        {"path": "src/a.rs", "original": null, "status": "modified", "staged": "modified", "unstaged": "modified", "added": 3, "deleted": 1},
        {"path": "b.py", "original": null, "status": "deleted", "staged": null, "unstaged": "deleted"},
        {"path": "new.md", "original": null, "status": "untracked", "staged": null, "unstaged": "untracked"},
        {"path": "both.txt", "original": null, "status": "conflict", "staged": "conflict", "unstaged": "conflict"}]})
}

fn reply(path: &str) -> Value {
    match path {
        "/workspace/branches" => json!({"branches": [{"name": "main"}, {"name": "topic"}]}),
        "/workspace/log" => json!({"head": "abc1234", "has_more": true, "entries": [
            {"hash": "abc1234", "short": "abc1234", "parents": ["0000000"], "summary": "Last", "author": "A",
             "email": "a@x", "time": 0, "refs": [{"name": "main", "kind": "branch"}]}]}),
        "/workspace/commit" => {
            json!({"commit": {"short": "x", "summary": "y"}, "changes": changes(),
                   "hash": "abc1234000000000000000000000000000000000", "short": "abc1234", "summary": "Last", "body": "Body", "parents": ["0000000"]})
        }
        "/workspace/delete" => json!({"ok": true}),
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

fn find<'a>(node: &'a Value, kind: &str) -> Option<&'a Value> {
    if node.str("type") == kind {
        return Some(node);
    }
    let tabs = node
        .list("tabs")
        .iter()
        .flat_map(|tab| tab.list("children"));
    node.list("children")
        .iter()
        .chain(tabs)
        .find_map(|child| find(child, kind))
}

fn buttons(node: &Value, out: &mut Vec<Value>) {
    if node.str("type") == "button" {
        out.push(node.clone());
    }
    let tabs = node
        .list("tabs")
        .iter()
        .flat_map(|tab| tab.list("children"));
    for child in node.list("children").iter().chain(tabs) {
        buttons(child, out);
    }
}

fn button<'a>(all: &'a [Value], action: &str) -> &'a Value {
    all.iter()
        .find(|node| node.str("action") == action)
        .unwrap()
}

#[test]
fn menus_distinguish_exclusive_values_from_boolean_toggles() {
    let mut fake = Fake::new();
    let root = fake.git.panel(&fake.host, "main").unwrap();
    let grouping = find(&root, "menu").unwrap().list("items");
    let status = grouping
        .iter()
        .find(|item| item.str("value") == "status")
        .unwrap();
    let staging = grouping
        .iter()
        .find(|item| item.str("value") == "staging")
        .unwrap();
    assert!(status.bool("selected") && !staging.bool("selected"));
    assert!(status.get("checked").is_none() && staging.get("checked").is_none());
    fake.act("group", json!("staging"));
    let root = fake.git.panel(&fake.host, "main").unwrap();
    let grouping = find(&root, "menu").unwrap().list("items");
    assert!(
        grouping
            .iter()
            .find(|item| item.str("value") == "staging")
            .unwrap()
            .bool("selected")
    );

    let branches = fake.git.branch_menu(&fake.host, "main").unwrap();
    let main = branches
        .iter()
        .find(|item| item.str("value") == "main")
        .unwrap();
    let topic = branches
        .iter()
        .find(|item| item.str("value") == "topic")
        .unwrap();
    assert!(main.bool("selected") && !topic.bool("selected"));
    assert!(main.get("checked").is_none() && topic.get("checked").is_none());

    let mut all = Vec::new();
    buttons(&root, &mut all);
    for action in ["signoff", "no_verify"] {
        let option = button(&all, "commit")
            .list("menu")
            .iter()
            .find(|item| item.str("action") == action)
            .unwrap();
        assert!(option.get("checked").is_some() && option.get("selected").is_none());
    }
}

#[test]
fn panel_follows_zed() {
    let mut fake = Fake::new();
    let root = fake.git.panel(&fake.host, "main").unwrap();
    let tabs = find(&root, "tabs").unwrap();
    assert_eq!(
        (tabs["tabs"][0].str("label"), &tabs["tabs"][0]["count"]),
        ("Changes", &json!(4))
    );
    let tree = find(&root, "tree").unwrap();
    assert_eq!(tree["chevrons"], false);
    let rows = tree.list("rows");
    let headers: Vec<&str> = rows
        .iter()
        .filter(|row| row.str("kind") == "header")
        .map(|row| row.str("label"))
        .collect();
    assert_eq!(headers, ["Conflicts", "Tracked", "Новые"]);
    let row = |id: &str| rows.iter().find(|row| row.str("id") == id).unwrap();
    let partial = row("file:src/a.rs");
    assert_eq!(partial["checked"], "mixed");
    assert_eq!(
        (
            partial.str("label"),
            partial.str("detail"),
            partial.str("icon")
        ),
        ("a.rs", "src", "square-dot")
    );
    assert_eq!(partial["stats"], json!({"added": 3, "deleted": 1}));
    let deleted = row("file:b.py");
    assert!(deleted.bool("strike"));
    assert_eq!(deleted.str("icon_tone"), "danger");
    assert_eq!(row("section:tracked")["checked"], "mixed");
    assert_eq!(row("section:tracked")["count"], 2);
    let menu: Vec<&str> = row("file:new.md")
        .list("menu")
        .iter()
        .map(|item| item.str("label"))
        .collect();
    assert_eq!(menu[1], "Удалить файл");
    let mut all = Vec::new();
    buttons(&root, &mut all);
    assert_eq!(button(&all, "stage_all").str("label"), "Stage All");
    let commit = button(&all, "commit");
    assert!(commit.bool("disabled"));
    assert_eq!(commit.str("hint"), "Сначала разрешите конфликты");
    assert_eq!(commit.str("variant"), "default");
    assert!(commit.get("icon").is_none());
    assert_eq!(button(&all, "publish").str("label"), "Publish");
    assert!(all.iter().any(|node| node.str("action") == "uncommit"));
    assert_eq!(
        button(&all, "toggle_editor").str("hint"),
        "Скрыть редактор коммита"
    );
    // Zed's footer order: branch bar, commit editor with its bar, last commit.
    let body = tabs["tabs"][0].list("children");
    let kinds: Vec<&str> = body.iter().map(|node| node.str("type")).collect();
    assert_eq!(kinds[kinds.len() - 3..], ["bar", "panel_editor", "bar"]);
    let branch_bar = &body[kinds.len() - 3];
    assert!(!branch_bar.bool("border"));
    assert_eq!(branch_bar["children"][0]["name"], "git-branch");
    assert_eq!(branch_bar["children"][1].str("label"), "main");
    let input = find(&root, "input").unwrap();
    assert_eq!((&input["lines"], input.bool("flush")), (&json!(6), true));
    assert_eq!(input.str("placeholder"), "Введите сообщение коммита");

    fake.act("toggle_editor", Value::Null);
    let root = fake.git.panel(&fake.host, "main").unwrap();
    assert!(find(&root, "input").is_none());
    let mut all = Vec::new();
    buttons(&root, &mut all);
    assert_eq!(button(&all, "toggle_editor").str("icon"), "chevron-up");
    fake.act("toggle_editor", Value::Null);

    fake.act("group", json!("staging"));
    let root = fake.git.panel(&fake.host, "main").unwrap();
    let rows = find(&root, "tree").unwrap().list("rows").to_vec();
    let ids: Vec<&str> = rows.iter().map(|row| row.str("id")).collect();
    assert!(ids.contains(&"staged:src/a.rs") && ids.contains(&"unstaged:src/a.rs"));
    fake.act("tree", json!({"id": "section:staged", "event": "collapse"}));
    let root = fake.git.panel(&fake.host, "main").unwrap();
    let rows = find(&root, "tree").unwrap().list("rows").to_vec();
    assert!(!rows.iter().any(|row| row.str("id") == "staged:src/a.rs"));
}

#[test]
fn checkboxes_stage_and_unstage() {
    let mut fake = Fake::new();
    fake.act("tree", json!({"id": "file:b.py", "event": "check"}));
    assert_eq!(
        fake.last(),
        (
            "/workspace/stage".to_owned(),
            json!({"session_dir": "/s", "paths": ["b.py"]})
        )
    );
    fake.act("tree", json!({"id": "file:src/a.rs", "event": "check"}));
    assert_eq!(fake.last().0, "/workspace/stage");
    fake.act("tree", json!({"id": "staged:src/a.rs", "event": "check"}));
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
fn commit_sends_options_and_amend_prefills_the_message() {
    let mut fake = Fake::new();
    fake.git.changes.as_mut().unwrap()["entries"]
        .as_array_mut()
        .unwrap()
        .pop();
    fake.act("amend", Value::Null);
    assert_eq!(fake.git.message, "Last\n\nBody");
    fake.act("signoff", Value::Null);
    fake.act("commit", json!("Fix"));
    assert_eq!(
        fake.last(),
        (
            "/workspace/commit".to_owned(),
            json!({"session_dir": "/s", "message": "Fix", "amend": true, "signoff": true,
                   "no_verify": false, "all_tracked": false})
        )
    );
    assert_eq!((fake.git.message.as_str(), fake.git.amend), ("", false));
    fake.act("amend", Value::Null);
    fake.act("amend", Value::Null);
    assert_eq!(fake.git.message, "");
}

#[test]
fn conflicts_block_the_commit_without_a_request() {
    let mut fake = Fake::new();
    let calls = fake.calls.lock().unwrap().len();
    fake.act("commit", json!("Fix"));
    assert_eq!(fake.calls.lock().unwrap().len(), calls);
    assert_eq!(
        fake.git.error.as_deref(),
        Some("Сначала разрешите конфликты")
    );
}

#[test]
fn panel_actions_reach_the_api() {
    let mut fake = Fake::new();
    for (action, value, path, body) in [
        (
            "publish",
            Value::Null,
            "/workspace/push",
            json!({"set_upstream": true}),
        ),
        (
            "pull_rebase",
            Value::Null,
            "/workspace/pull",
            json!({"rebase": true}),
        ),
        (
            "stash_staged",
            Value::Null,
            "/workspace/stash",
            json!({"staged_only": true}),
        ),
        (
            "exclude",
            json!(["new.md"]),
            "/workspace/ignore",
            json!({"paths": ["new.md"], "target": "exclude"}),
        ),
        (
            "trash_untracked",
            Value::Null,
            "/workspace/delete",
            json!({"paths": ["new.md"]}),
        ),
        (
            "create_branch",
            json!({"value": null, "text": " topic "}),
            "/workspace/checkout",
            json!({"branch": "topic", "create": true}),
        ),
        ("uncommit", Value::Null, "/workspace/uncommit", json!({})),
    ] {
        fake.act(action, value);
        let mut expected = json!({"session_dir": "/s"});
        expected
            .as_object_mut()
            .unwrap()
            .extend(body.as_object().unwrap().clone());
        assert_eq!(fake.last(), (path.to_owned(), expected), "{action}");
    }
}

#[test]
fn history_and_documents_render() {
    let mut fake = Fake::new();
    let root = fake.git.panel(&fake.host, "main").unwrap();
    let history = &find(&root, "tabs").unwrap()["tabs"][1];
    assert_eq!(history["children"][0]["text"], "Загрузка истории…");
    fake.act("tab", json!(1));
    let root = fake.git.panel(&fake.host, "main").unwrap();
    let list = find(&root, "list").unwrap();
    let row = &list["rows"][0];
    let meta = row.list("meta");
    assert_eq!(
        (meta[0].as_str(), meta[2].as_str()),
        (Some("A"), Some("abc1234"))
    );
    assert!(row.bool("avatar"));
    assert_eq!(row["menu"][0].str("action"), "open_commit");
    assert!(find(&root, "lanes").is_none());
    let mut all = Vec::new();
    buttons(&root, &mut all);
    assert!(all.iter().any(|node| node.str("action") == "more"));
    fake.act("log", json!({"id": "abc1234", "event": "open"}));
    assert_eq!(fake.git.commit.as_deref(), Some("abc1234"));
    assert_eq!(fake.git.commit_argument("abc1234"), "Коммит abc1234");
    let document = crate::document::Document::parse("Коммит abc1234");
    let root = fake.git.document(&fake.host, &document);
    assert!(find(&root, "diff").is_some());
    let root = fake
        .git
        .document(&fake.host, &crate::document::Document::Graph);
    assert!(find(&root, "lanes").is_some());
}

#[test]
fn server_errors_are_shown() {
    let mut fake = Fake::new();
    fake.fail.store(true, Ordering::SeqCst);
    fake.act("fetch", Value::Null);
    assert_eq!(
        fake.git.error.as_deref(),
        Some("nothing is staged to commit")
    );
}

#[test]
fn a_changed_graph_query_starts_at_first_match_and_navigation_wraps() {
    let mut fake = Fake::new();
    fake.git.graph = Some(crate::Log {
        entries: vec![
            json!({"hash":"a", "summary":"Fix parser"}),
            json!({"hash":"b", "summary":"Fix picker"}),
            json!({"hash":"c", "summary":"fix lower"}),
        ],
        more: false,
    });
    fake.git.commit = Some("b".into());
    fake.act("graph_search", json!("Fi"));
    assert_eq!(fake.git.commit.as_deref(), Some("a"));
    fake.act("graph_next", Value::Null);
    assert_eq!(fake.git.commit.as_deref(), Some("b"));
    fake.act("graph_search", json!("Fix"));
    assert_eq!(fake.git.commit.as_deref(), Some("a"));
    fake.act("graph_case", Value::Null);
    assert_eq!(fake.git.commit.as_deref(), Some("a"));
    fake.act("graph_previous", Value::Null);
    assert_eq!(fake.git.commit.as_deref(), Some("b"));
    fake.git.commit = None;
    fake.act("graph_previous", Value::Null);
    assert_eq!(fake.git.commit.as_deref(), Some("b"));
    fake.act("graph_search", json!(""));
    assert_eq!(fake.git.commit.as_deref(), Some("b"));
}

#[test]
fn inspector_and_short_document_share_one_immutable_commit_request() {
    let mut fake = Fake::new();
    let full = "abc1234000000000000000000000000000000000";
    let inspector = fake.git.commit_details(&fake.host, full).unwrap();
    let document = fake.git.commit_details(&fake.host, "abc1234").unwrap();
    assert_eq!(inspector, document);
    let calls = fake.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _)| path == "/workspace/commit")
            .count(),
        1
    );
}

#[test]
fn source_revisions_expire_only_changed_working_patches() {
    let mut fake = Fake::new();
    let mut initial = changes();
    initial["entries"][0]["revision"] = json!("a-v1");
    initial["entries"][1]["revision"] = json!("b-v1");
    fake.git.update(initial.clone());
    fake.git.diffs.insert(
        "src/a.rs".into(),
        json!({"revision":"a-v1", "patch":"old a"}),
    );
    fake.git
        .diffs
        .insert("b.py".into(), json!({"revision":"b-v1", "patch":"b"}));
    fake.git.update(initial.clone());
    assert_eq!(fake.git.diffs.len(), 2);
    // Same statuses and +N/−M; only source bytes/revision changed.
    initial["entries"][0]["revision"] = json!("a-v2");
    fake.git.update(initial.clone());
    assert!(!fake.git.diffs.contains_key("src/a.rs"));
    assert_eq!(fake.git.diffs["b.py"]["patch"], "b");
    initial["entries"].as_array_mut().unwrap().remove(1);
    fake.git.update(initial);
    assert!(!fake.git.diffs.contains_key("b.py"));
}
