//! Borrowed wire envelopes for owned roots and immutable shared widget trees.
use std::sync::Arc;

use serde::{Serialize, Serializer, ser::SerializeSeq};
use serde_json::Value;

use super::Host;
use crate::ui::SharedColumn;

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum Shown {
    Owned(Value),
    Shared(Arc<Value>),
    Column(SharedColumn),
    Fill(Fill<Vec<Arc<Value>>>),
}

#[derive(Serialize)]
struct View<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    session_dir: Option<&'a str>,
    view: &'a str,
    root: &'a Shown,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
    sequence: u64,
    source_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    keys: Option<Value>,
}

#[derive(Serialize)]
struct Failure<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    session_dir: Option<&'a str>,
    view: &'a str,
    sequence: u64,
    keys: Value,
    root: Fill<ErrorChildren<'a>>,
}

#[derive(Serialize)]
pub(super) struct Fill<T> {
    #[serde(rename = "type")]
    kind: &'static str,
    children: T,
}

impl<T> Fill<T> {
    pub(super) fn new(children: T) -> Self {
        Self {
            kind: "fill",
            children,
        }
    }
}

struct ErrorChildren<'a> {
    notice: &'a Value,
    previous: Option<&'a Shown>,
}

impl Serialize for ErrorChildren<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut children =
            serializer.serialize_seq(Some(1 + usize::from(self.previous.is_some())))?;
        children.serialize_element(self.notice)?;
        if let Some(previous) = self.previous {
            children.serialize_element(previous)?;
        }
        children.end()
    }
}

impl Host {
    pub(super) fn show_root(
        &mut self,
        view: &str,
        root: Shown,
        title: Option<&str>,
        keys: Option<Value>,
    ) {
        self.send_ref(&View {
            kind: "view",
            session_dir: self.session.as_deref(),
            view,
            root: &root,
            title,
            sequence: self.sequence(view),
            source_revision: self
                .content(view)
                .and_then(|content| content.source_revision),
            keys,
        });
        self.shown.insert(view.to_owned(), root);
    }

    pub(super) fn send_failure(&self, view: &str, notice: &Value) {
        self.send_ref(&Failure {
            kind: "view",
            session_dir: self.session.as_deref(),
            view,
            sequence: self.sequence(view),
            keys: serde_json::json!({}),
            root: Fill::new(ErrorChildren {
                notice,
                previous: self.shown.get(view),
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Surface, json, ui};
    use std::sync::Arc;

    #[test]
    fn shared_root_preserves_wire_and_lives_until_replaced() {
        let mut host = Host::default();
        let root = Arc::new(ui::svg("<svg><!-- shared 🧪 --></svg>", 100.0, 200.0));
        let retained = Arc::downgrade(&root);
        host.views.insert("figure".into(), (Surface::Page, None));
        host.show("figure", root.as_ref().clone());
        host.show_shared_root("figure", root);
        let frames = host.sent.borrow();
        assert_eq!(frames[0].to_string(), frames[1].to_string());
        drop(frames);

        host.failed("figure", "fallback");
        let frames = host.sent.borrow();
        assert_eq!(frames[2]["root"]["children"][1], frames[0]["root"]);
        drop(frames);
        assert_eq!(retained.strong_count(), 1);

        host.show("figure", ui::text("replacement"));
        assert!(retained.upgrade().is_none());
    }

    #[test]
    fn shared_fill_is_the_same_view_and_error_fallback() {
        let mut host = Host::default();
        let content = Arc::new(ui::code("content", "shared 🧪", "text"));
        let header = Arc::new(ui::text("header"));
        let expected = ui::fill(vec![(*header).clone(), (*content).clone()]);
        host.views.insert("file".into(), (Surface::Document, None));
        host.show_shared_fill("file", vec![header, Arc::clone(&content)]);
        assert_eq!(
            host.sent.borrow().last().unwrap()["root"].to_string(),
            expected.to_string()
        );
        assert_eq!(Arc::strong_count(&content), 2);
        for _ in 0..2 {
            host.failed("file", "failure");
            assert_eq!(
                host.sent.borrow().last().unwrap()["root"]["children"][1],
                expected
            );
            assert_eq!(Arc::strong_count(&content), 2);
        }
        host.show("file", expected);
        assert_eq!(Arc::strong_count(&content), 1);
    }

    #[test]
    fn shown_column_shares_one_allocation_until_replaced() {
        let mut host = Host::default();
        let widget = Arc::new(ui::text("shared 🧪"));
        let cached = Arc::downgrade(&widget);
        host.show_shared_column("figure", SharedColumn::new("medium", vec![widget.clone()]));
        assert_eq!(Arc::strong_count(&widget), 2);
        drop(widget);

        host.views.insert("figure".into(), (Surface::Content, None));
        host.failed("figure", "old revision");
        assert_eq!(host.sent.borrow().last().unwrap()["type"], "content_error");
        assert_eq!(cached.strong_count(), 1);

        host.views.insert("figure".into(), (Surface::Page, None));
        for _ in 0..2 {
            host.failed("figure", "fallback");
            let sent = host.sent.borrow();
            let previous = &sent.last().unwrap()["root"]["children"][1];
            assert_eq!(previous["children"], json!([ui::text("shared 🧪")]));
            assert_eq!(cached.strong_count(), 1);
        }

        host.show("figure", ui::text("replacement"));
        assert!(cached.upgrade().is_none());
    }
}
