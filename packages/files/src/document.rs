//! A workspace file or its diff in a document tab.

use proteus_extension::ui::{Node, button, code, fill, muted, row, segments, text};
use proteus_extension::{Host, Result, Value, ValueExt, json};

use crate::{Files, paths};

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

    pub fn document(&mut self, host: &Host, path: &str) -> Value {
        let mode = self
            .modes
            .get(path)
            .cloned()
            .unwrap_or_else(|| "file".to_owned());
        let body = self
            .body(host, path, &mode)
            .unwrap_or_else(|error| text(error).with("tone", "danger"));
        let choices = [
            (json!([path, "file"]), "Файл".to_owned()),
            (json!([path, "diff"]), "Changes".to_owned()),
        ];
        let header = row(vec![
            text(path),
            segments("document-mode", &choices, &json!([path, mode])),
            button("", "copy")
                .with("value", self.absolute(path))
                .with("variant", "ghost")
                .with("icon", "copy")
                .with("hint", "Копировать путь"),
        ]);
        fill(vec![header, body])
    }

    fn body(&mut self, host: &Host, path: &str, mode: &str) -> Result<Value> {
        let diff = mode == "diff";
        let key = (path.to_owned(), mode.to_owned());
        if !self.documents.contains_key(&key) {
            let document = host.api(
                if diff {
                    "/workspace/diff"
                } else {
                    "/workspace/file"
                },
                &[("path", path)],
            )?;
            self.documents.insert(key.clone(), document);
        }
        let document = &self.documents[&key];
        Ok(
            match document.opt_str(if diff { "patch" } else { "text" }) {
                Some(content) => code(
                    "content",
                    content,
                    if diff { "diff" } else { paths::language(path) },
                ),
                None => muted(match document.str("kind") {
                    "binary" => "Двоичный файл",
                    "too_large" => "Файл превышает предел предпросмотра 512 КБ",
                    "unavailable" => "Изменений нет",
                    _ => "Нет текстового содержимого",
                }),
            },
        )
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
            json!({"text": format!("file: {path}"), "patch": format!("diff: {path}")}),
        );
    }

    fn contains(files: &Files, path: &str, mode: &str) -> bool {
        files.documents.contains_key(&(path.into(), mode.into()))
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
        let original = files.documents[&key]["text"].as_str().unwrap().as_ptr();
        files
            .render(&mut host, "preview", Surface::Document, Some("a.rs"))
            .unwrap();
        assert_eq!(
            files.documents[&key]["text"].as_str().unwrap().as_ptr(),
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
}
