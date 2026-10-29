//! The real process preserves scoped source revisions and releases closed views.
use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn real_process_routes_fences_updates_source_and_reopens_a_closed_view() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_markdown-render"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "{}", json!({"type":"hello","protocol":4,"workspace":"/fixture","features":["surfaces.content.v1","widgets.rich_text.v1","widgets.content.v1","widgets.code_embedded.v1"]})).unwrap();
    for (revision, source) in [
        (1, "# Первый\n\n```mermaid\nА-->Б\n```\n"),
        (2, "# Второй\n\n```mermaid\nА-->В\n```\n"),
    ] {
        writeln!(input, "{}", json!({"type":"render","context":"scope-one","surface":"content","view":"opaque","session_dir":"/session-one",
            "content":{"language":"text/markdown","source":source,"source_revision":revision,"context":{"languages":["mermaid"]}}})).unwrap();
    }
    writeln!(
        input,
        "{}",
        json!({"type":"close","context":"scope-one","session_dir":"/session-one","view":"opaque"})
    )
    .unwrap();
    writeln!(input, "{}", json!({"type":"render","context":"scope-two","surface":"content","view":"opaque","session_dir":"/session-two",
        "content":{"language":"text/markdown","source":"Заново","source_revision":1,"context":{"languages":[]}}})).unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let messages = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert_eq!(messages[0]["source_revision"], 1);
    assert_eq!(messages[1]["source_revision"], 2);
    assert_eq!(messages[0]["root"]["children"][1]["source"], "А-->Б\n");
    assert_eq!(messages[1]["root"]["children"][1]["source"], "А-->В\n");
    assert_eq!(
        messages[0]["root"]["children"][1]["id"],
        messages[1]["root"]["children"][1]["id"]
    );
    assert_eq!(messages[2]["session_dir"], "/session-two");
    assert_eq!(messages[2]["root"]["children"][0]["html"], "<p>Заново</p>");
}
