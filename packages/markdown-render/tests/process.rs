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
    writeln!(input, "{}", json!({"type":"hello","protocol":4,"workspace":"/fixture","features":["surfaces.content.v1","widgets.rich_text.v1","widgets.rich_text.inlines.v1","widgets.content.v1","widgets.code_embedded.v1"]})).unwrap();
    for (revision, source) in [
        (1, "# Первый\n\n```mermaid\nА-->Б\n```\n"),
        (2, "# Второй\n\n```mermaid\nА-->В\n```\n"),
        (
            3,
            "### Формула\n\n$$\n\\frac{\\text{кофе}}{1 + \\text{количество багов}}\n$$\n",
        ),
        (4, "### Формула\n\n$$\n\\frac{\\text{кофе}}{"),
        (
            5,
            "Пишешь:\n\n```md\n| Игрок | Монеты |\n|---|---|\n| Аня | 100 |\n```\n\nПолучаешь:\n\n| Игрок | Монеты |\n|---|---|\n| Аня | 100 |\n",
        ),
        (6, "````markdown\n```mermaid\nA-->B\n```\n````\n"),
        (7, "Энергия **$E=mc^2$** и `$literal$`."),
    ] {
        writeln!(input, "{}", json!({"type":"render","context":"scope-one","surface":"content","view":"opaque","session_dir":"/session-one",
            "content":{"language":"text/markdown","source":source,"source_revision":revision,"context":{"languages":["mermaid","latex","md","markdown"]}}})).unwrap();
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
    assert_eq!(messages.len(), 8, "{messages:?}");
    assert_eq!(messages[0]["source_revision"], 1);
    assert_eq!(messages[1]["source_revision"], 2);
    assert_eq!(messages[0]["root"]["children"][1]["source"], "А-->Б\n");
    assert_eq!(messages[1]["root"]["children"][1]["source"], "А-->В\n");
    assert_eq!(
        messages[0]["root"]["children"][1]["id"],
        messages[1]["root"]["children"][1]["id"]
    );
    assert_eq!(messages[2]["source_revision"], 3);
    assert_eq!(messages[2]["root"]["children"][1]["language"], "latex");
    assert_eq!(
        messages[2]["root"]["children"][1]["source"],
        "\\frac{\\text{кофе}}{1 + \\text{количество багов}}\n"
    );
    assert_eq!(messages[3]["source_revision"], 4);
    assert!(
        messages[3]["root"]["children"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| node["type"] != "content")
    );
    assert!(
        messages[3]["root"]["children"][0]["html"]
            .as_str()
            .unwrap()
            .contains("$$")
    );
    let tutorial = messages[4]["root"]["children"].as_array().unwrap();
    assert_eq!(tutorial[1]["type"], "code");
    assert_eq!(tutorial[1]["language"], "md");
    assert_eq!(
        tutorial[1]["text"],
        "| Игрок | Монеты |\n|---|---|\n| Аня | 100 |\n"
    );
    assert_eq!(
        tutorial
            .iter()
            .filter_map(|node| node["html"].as_str())
            .map(|html| html.matches("<table>").count())
            .sum::<usize>(),
        1
    );
    assert!(tutorial.iter().all(|node| node["type"] != "content"));
    let nested = messages[5]["root"]["children"].as_array().unwrap();
    assert_eq!(nested[0]["type"], "code");
    assert_eq!(nested[0]["text"], "```mermaid\nA-->B\n```\n");
    assert!(nested.iter().all(|node| node["type"] != "content"));
    assert_eq!(messages[6]["source_revision"], 7);
    let inline = &messages[6]["root"]["children"][0];
    assert_eq!(inline["inlines"][0]["source"], "E=mc^2");
    assert_eq!(inline["inlines"][0]["language"], "latex");
    assert_eq!(inline["inlines"].as_array().unwrap().len(), 1);
    assert!(
        inline["html"]
            .as_str()
            .unwrap()
            .contains("<strong><span data-content=\"inline-0\">$E=mc^2$</span></strong>")
    );
    assert_eq!(messages[7]["session_dir"], "/session-two");
    assert_eq!(messages[7]["root"]["children"][0]["html"], "<p>Заново</p>");
}
