//! A workspace file or its diff in a document tab.

use proteus_extension::ui::{Node, button, code, content, muted, row, text};
use proteus_extension::{Host, Result, Value, ValueExt, json};

use crate::{Files, paths};
use std::{sync::Arc, time::Instant};

pub(super) struct SavedDocument {
    body: Arc<Value>,
    checked: Instant,
}

fn preview_body(mut document: Value, path: &str, diff: bool) -> Value {
    let source = document
        .get_mut(if diff { "patch" } else { "text" })
        .map(Value::take);
    match source {
        Some(Value::String(source)) => {
            let language = if diff { "diff" } else { paths::language(path) };
            if language == "markdown" {
                return content("content", "text/markdown", source);
            }
            let mut widget = code("content", "", language);
            widget["text"] = Value::String(String::from(source.into_boxed_str()));
            widget
        }
        _ => muted(match document.str("kind") {
            "binary" => "Двоичный файл",
            "too_large" => "Файл превышает предел предпросмотра 512 КБ",
            "unavailable" => "Изменений нет",
            _ => "Нет текстового содержимого",
        }),
    }
}

impl Files {
    pub(super) fn track_document_view(&mut self, view: &str, path: Option<&str>) {
        if self.document_views.get(view).map(String::as_str) == path {
            return;
        }
        if let Some(previous) = self.document_views.remove(view)
            && !self.document_views.values().any(|path| path == &previous)
        {
            // Keep both file/diff variants while another view owns this path.
            // Closing a window must not invalidate an active sibling preview.
            self.documents.retain(|(path, _), _| path != &previous);
        }
        if let Some(path) = path {
            self.document_views.insert(view.to_owned(), path.to_owned());
        }
    }

    pub fn document(&mut self, host: &Host, path: &str) -> Vec<Arc<Value>> {
        // Documents can stay visible while the file tree is closed.
        self.refresh(host);
        let mode = self
            .modes
            .get(path)
            .cloned()
            .unwrap_or_else(|| "file".to_owned());
        let body = self
            .body(host, path, &mode)
            .unwrap_or_else(|error| Arc::new(text(error).with("tone", "danger")));
        let diff = mode == "diff";
        let mut header = vec![text(path)];
        if diff || self.git.row(path, false).1 {
            let (label, next) = if diff {
                ("К файлу", "file")
            } else {
                ("Изменения", "diff")
            };
            header.push(
                button(label, "document-mode")
                    .with("value", json!([path, next]))
                    .with("variant", "ghost"),
            );
        }
        vec![Arc::new(row(header)), body]
    }

    fn body(&mut self, host: &Host, path: &str, mode: &str) -> Result<Arc<Value>> {
        let diff = mode == "diff";
        let key = (path.to_owned(), mode.to_owned());
        // The server bounds each preview. Recheck only the active document on
        // the package's existing poll interval; hidden retained views keep their
        // snapshot until activation, and sibling views share this one read.
        let active = host.active_document.as_ref().and_then(Value::as_str) == Some(path);
        let stale = self
            .documents
            .get(&key)
            .is_none_or(|saved| active && saved.checked.elapsed() >= crate::POLL);
        if stale {
            let document = host.api(
                if diff {
                    "/workspace/diff"
                } else {
                    "/workspace/file"
                },
                &[("path", path)],
            )?;
            self.documents.insert(
                key.clone(),
                SavedDocument {
                    body: Arc::new(preview_body(document, path, diff)),
                    checked: Instant::now(),
                },
            );
        }
        Ok(Arc::clone(&self.documents[&key].body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_extension::{Extension, Surface};

    fn host() -> Host {
        let mut host = Host::default();
        host.session = Some("/fixture/chat".into());
        host
    }

    fn seed(files: &mut Files, path: &str, mode: &str) {
        files.documents.insert(
            (path.into(), mode.into()),
            SavedDocument {
                body: Arc::new(preview_body(
                    json!({"text": format!("file: {path}"), "patch": format!("diff: {path}")}),
                    path,
                    mode == "diff",
                )),
                checked: Instant::now(),
            },
        );
    }

    fn contains(files: &Files, path: &str, mode: &str) -> bool {
        files.documents.contains_key(&(path.into(), mode.into()))
    }

    #[test]
    fn markdown_uses_the_document_renderer_and_keeps_its_diff_as_a_patch() {
        for path in ["README.md", "docs/manual.MARKDOWN"] {
            let file = json!({"text": "# Heading", "patch": "+# Heading"});
            let preview = preview_body(file.clone(), path, false);
            assert_eq!(preview["type"], "content");
            assert_eq!(preview["language"], "text/markdown");
            assert_eq!(preview["source"], "# Heading");
            let patch = preview_body(file, path, true);
            assert_eq!(patch["type"], "code");
            assert_eq!(patch["language"], "diff");
            assert_eq!(patch["text"], "+# Heading");
        }
    }

    #[test]
    fn toolbar_offers_only_available_actions_and_can_return_from_a_diff() {
        let mut files = Files::default();
        files.deadline = Some(Instant::now() + crate::POLL);
        files.git = crate::git::Git::parse(&json!({
            "repository": true,
            "entries": [{"path": "dirty.md", "status": "modified"}],
        }));
        seed(&mut files, "clean.md", "file");
        seed(&mut files, "dirty.md", "file");
        seed(&mut files, "dirty.md", "diff");
        let mut host = host();

        let clean = files.document(&host, "clean.md");
        assert_eq!(clean[0]["children"], json!([text("clean.md")]));

        let changed = files.document(&host, "dirty.md");
        assert_eq!(changed[0]["children"].as_array().unwrap().len(), 2);
        assert_eq!(changed[0]["children"][1]["label"], "Изменения");
        let action = &changed[0]["children"][1]["value"];
        files.operation(&mut host, "document-mode", action);
        let changes = files.document(&host, "dirty.md");
        assert_eq!(changes[0]["children"].as_array().unwrap().len(), 2);
        assert_eq!(changes[0]["children"][1]["label"], "К файлу");
        assert_eq!(
            changes[0]["children"][1]["value"],
            json!(["dirty.md", "file"])
        );
    }

    #[test]
    fn closing_one_owner_keeps_both_modes_until_the_last_owner_leaves() {
        let (mut files, mut host) = (Files::default(), host());
        for mode in ["file", "diff"] {
            seed(&mut files, "shared.rs", mode);
        }
        seed(&mut files, "other.rs", "file");
        files.modes.insert("shared.rs".into(), "diff".into());
        for (view, path) in [
            ("window-a", "shared.rs"),
            ("window-b", "shared.rs"),
            ("other", "other.rs"),
        ] {
            files
                .render(&mut host, view, Surface::Document, Some(path))
                .unwrap();
        }
        files.closed(&mut host, "window-a");
        files.closed(&mut host, "untracked-panel");
        assert!(contains(&files, "shared.rs", "file"));
        assert!(contains(&files, "shared.rs", "diff"));
        assert_eq!(
            files.document_views.get("window-b").map(String::as_str),
            Some("shared.rs")
        );
        files.closed(&mut host, "window-b");
        assert!(!contains(&files, "shared.rs", "file"));
        assert!(!contains(&files, "shared.rs", "diff"));
        assert!(contains(&files, "other.rs", "file"));
        assert_eq!(
            files.modes.get("shared.rs").map(String::as_str),
            Some("diff")
        );
    }

    #[test]
    fn replacing_a_view_argument_releases_only_unowned_paths() {
        let (mut files, mut host) = (Files::default(), host());
        for path in ["a.rs", "b.rs"] {
            seed(&mut files, path, "file");
        }
        for view in ["preview", "retained"] {
            files
                .render(&mut host, view, Surface::Document, Some("a.rs"))
                .unwrap();
        }
        let key = ("a.rs".into(), "file".into());
        let original = files.documents[&key].body["text"]
            .as_str()
            .unwrap()
            .as_ptr();
        files
            .render(&mut host, "preview", Surface::Document, Some("a.rs"))
            .unwrap();
        assert_eq!(
            files.documents[&key].body["text"]
                .as_str()
                .unwrap()
                .as_ptr(),
            original
        );
        files
            .render(&mut host, "preview", Surface::Document, Some("b.rs"))
            .unwrap();
        assert!(contains(&files, "a.rs", "file"));
        assert!(contains(&files, "b.rs", "file"));
        files.closed(&mut host, "retained");
        assert!(!contains(&files, "a.rs", "file"));
        assert!(contains(&files, "b.rs", "file"));
        seed(&mut files, "c.rs", "file");
        files
            .render(&mut host, "preview", Surface::Document, Some("c.rs"))
            .unwrap();
        assert!(!contains(&files, "b.rs", "file"));
        assert!(contains(&files, "c.rs", "file"));
        files
            .render(&mut host, "preview", Surface::Page, None)
            .unwrap();
        assert!(files.documents.is_empty());
        assert!(files.document_views.is_empty());
    }

    #[test]
    fn toggling_modes_keeps_the_other_cached_variant_and_shared_path_selection() {
        let (mut files, mut host) = (Files::default(), host());
        for mode in ["file", "diff"] {
            seed(&mut files, "shared.rs", mode);
        }
        for view in ["one", "two"] {
            files
                .render(&mut host, view, Surface::Document, Some("shared.rs"))
                .unwrap();
        }
        for mode in ["diff", "file", "diff"] {
            files.operation(&mut host, "document-mode", &json!(["shared.rs", mode]));
            for view in ["one", "two"] {
                files
                    .render(&mut host, view, Surface::Document, Some("shared.rs"))
                    .unwrap();
            }
            assert!(contains(&files, "shared.rs", "file"));
            assert!(contains(&files, "shared.rs", "diff"));
            assert_eq!(files.modes.get("shared.rs").map(String::as_str), Some(mode));
            assert_eq!(files.document_views.len(), 2);
        }
    }

    #[test]
    fn failed_fetch_retains_only_the_view_owner_and_close_is_idempotent() {
        // No server is configured: Host::api returns its existing error before
        // any network request. The document renders the ordinary error node.
        let (mut files, mut host) = (Files::default(), host());
        files
            .render(&mut host, "failed", Surface::Document, Some("missing.rs"))
            .unwrap();
        assert!(files.documents.is_empty());
        assert_eq!(
            files.document_views.get("failed").map(String::as_str),
            Some("missing.rs")
        );
        files.closed(&mut host, "failed");
        files.closed(&mut host, "failed");
        assert!(files.documents.is_empty());
        assert!(files.document_views.is_empty());
    }

    #[test]
    fn invalidation_keeps_live_owners_and_session_reset_clears_both_maps() {
        let (mut files, mut host) = (Files::default(), host());
        seed(&mut files, "active.rs", "file");
        files
            .render(&mut host, "active", Surface::Document, Some("active.rs"))
            .unwrap();
        files.event(&mut host, "run_settled");
        assert!(files.documents.is_empty());
        assert_eq!(
            files.document_views.get("active").map(String::as_str),
            Some("active.rs")
        );
        seed(&mut files, "active.rs", "file");
        files.closed(&mut host, "active");
        assert!(files.documents.is_empty());
        seed(&mut files, "next.rs", "file");
        files
            .render(&mut host, "next", Surface::Document, Some("next.rs"))
            .unwrap();
        files.modes.insert("next.rs".into(), "file".into());
        files.session_changed(&mut host);
        assert!(files.documents.is_empty());
        assert!(files.document_views.is_empty());
        assert!(files.modes.is_empty());
    }

    #[test]
    fn losing_a_session_releases_the_addressed_owner_not_other_live_paths() {
        let (mut files, mut host) = (Files::default(), host());
        for path in ["one.rs", "two.rs"] {
            seed(&mut files, path, "file");
            files
                .render(&mut host, path, Surface::Document, Some(path))
                .unwrap();
        }
        host.session = None;
        files
            .render(&mut host, "one.rs", Surface::Document, Some("one.rs"))
            .unwrap();
        assert!(!contains(&files, "one.rs", "file"));
        assert!(contains(&files, "two.rs", "file"));
        assert_eq!(files.document_views.len(), 1);
    }

    #[test]
    fn active_file_and_diff_recheck_after_poll_but_hidden_cached_views_do_not_read() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;
        for mode in ["file", "diff"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let calls = Arc::new(AtomicUsize::new(0));
            let count = calls.clone();
            let server = std::thread::spawn(move || {
                for content in ["OLD!", "NEW!"] {
                    let deadline = Instant::now() + Duration::from_secs(3);
                    let mut socket = loop {
                        if let Ok((socket, _)) = listener.accept() {
                            break socket;
                        }
                        assert!(Instant::now() < deadline, "missing active document read");
                        std::thread::sleep(Duration::from_millis(5));
                    };
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = BufReader::new(socket.try_clone().unwrap());
                    let mut line = String::new();
                    request.read_line(&mut line).unwrap();
                    assert!(line.starts_with(if mode == "diff" {
                        "GET /workspace/diff?"
                    } else {
                        "GET /workspace/file?"
                    }));
                    assert!(line.contains("path=shared.rs"));
                    loop {
                        line.clear();
                        request.read_line(&mut line).unwrap();
                        assert!(!line.is_empty());
                        if line == "\r\n" {
                            break;
                        }
                    }
                    count.fetch_add(1, Ordering::SeqCst);
                    let body = json!({"text":content,"patch":content}).to_string();
                    write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                }
            });
            let (mut files, mut host) = (Files::default(), host());
            host.server = Some(proteus_extension::Server {
                origin,
                token: "synthetic".into(),
            });
            host.active_document = Some(json!("shared.rs"));
            assert_eq!(
                files.body(&host, "shared.rs", mode).unwrap()["text"],
                "OLD!"
            );
            let key = ("shared.rs".into(), mode.into());
            files.documents.get_mut(&key).unwrap().checked = Instant::now() - crate::POLL;
            host.active_document = Some(json!("another.rs"));
            assert_eq!(
                files.body(&host, "shared.rs", mode).unwrap()["text"],
                "OLD!"
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "hidden snapshot must be lazy"
            );
            host.active_document = Some(json!("shared.rs"));
            assert_eq!(
                files.body(&host, "shared.rs", mode).unwrap()["text"],
                "NEW!"
            );
            assert_eq!(
                files.body(&host, "shared.rs", mode).unwrap()["text"],
                "NEW!"
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                2,
                "sibling reads share a fresh snapshot"
            );
            server.join().unwrap();
        }
    }
}
