//! Exact document-argument changes preserve the client's tab/pane ownership.
use super::*;

impl Host {
    /// Retarget this package's documents in the addressed session, including
    /// sibling windows. Arguments are opaque; file/folder semantics belong to
    /// the package, which sends one exact pair for each affected argument.
    pub fn retarget_document(&mut self, from: &str, to: &str) -> Result {
        if !self.has_feature("documents.retarget.v1") {
            bail!("host does not support document retargeting");
        }
        let context = self
            .current_context
            .as_deref()
            .ok_or_else(|| anyhow!("document retarget requires an addressed context"))?;
        if from == to {
            return Ok(());
        }
        for (view, (surface, argument)) in &mut self.views {
            if *surface != Surface::Document || argument.as_deref() != Some(from) {
                continue;
            }
            let session = self
                .view_contexts
                .get(view)
                .and_then(|context| self.contexts.get(context))
                .and_then(|scope| scope.session.as_deref());
            if session == self.session.as_deref() {
                *argument = Some(to.to_owned());
            }
        }
        if self.active_document.as_ref().and_then(Value::as_str) == Some(from) {
            self.active_document = Some(Value::String(to.to_owned()));
        }
        for scope in self.contexts.values_mut() {
            if scope.session == self.session
                && scope.active_document.as_ref().and_then(Value::as_str) == Some(from)
            {
                scope.active_document = Some(Value::String(to.to_owned()));
            }
        }
        self.send(json!({"type":"retarget_document", "context":context, "from":from, "to":to}));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retarget_preserves_surfaces_and_sequences_and_excludes_other_sessions() {
        let mut host = Host::default();
        host.features.insert("documents.retarget.v1".into());
        host.current_context = Some("first".into());
        host.session = Some("/one".into());
        host.active_document = Some(json!("old"));
        for (context, session) in [("first", "/one"), ("sibling", "/one"), ("other", "/two")] {
            host.contexts.insert(
                context.into(),
                Scope {
                    session: Some(session.into()),
                    active_document: Some(json!("old")),
                    ..Scope::default()
                },
            );
            host.view_contexts.insert(context.into(), context.into());
            host.views
                .insert(context.into(), (Surface::Document, Some("old".into())));
            host.sequences.insert(context.into(), 17);
        }
        host.retarget_document("old", "new").unwrap();
        for view in ["first", "sibling"] {
            assert_eq!(host.argument(view), Some("new"));
            assert_eq!(host.surface(view), Some(Surface::Document));
            assert_eq!(host.sequence(view), 17);
        }
        assert_eq!(host.argument("other"), Some("old"));
        assert_eq!(host.contexts["other"].active_document, Some(json!("old")));
        assert_eq!(host.active_document, Some(json!("new")));
        assert_eq!(
            host.sent.borrow().last().unwrap(),
            &json!({
                "type":"retarget_document", "context":"first", "from":"old", "to":"new"
            })
        );
    }

    #[test]
    fn unsupported_retarget_does_not_change_arguments() {
        let mut host = Host::default();
        host.views
            .insert("doc".into(), (Surface::Document, Some("old".into())));
        assert!(host.retarget_document("old", "new").is_err());
        assert_eq!(host.argument("doc"), Some("old"));
        assert!(host.sent.borrow().is_empty());
    }
}
