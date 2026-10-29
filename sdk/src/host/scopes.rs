//! Window metadata is small and per context. Session projections have one
//! owner per session, mounted into Host with moves rather than transcript
//! clones when callbacks switch windows.

use super::*;

#[derive(Default)]
pub(super) struct Scope {
    pub(super) workspace: Option<String>,
    pub(super) session: Option<String>,
    pub(super) active_document: Option<Value>,
}

#[derive(Default)]
pub(super) struct SessionContext {
    state: Option<Value>,
    entries: Vec<Value>,
    awaiting_snapshot: bool,
    command_error: Option<String>,
    pending: Vec<(String, String, Value)>,
}

impl SessionContext {
    fn take(host: &mut Host) -> Self {
        Self {
            state: host.state.take(),
            entries: std::mem::take(&mut host.entries),
            awaiting_snapshot: std::mem::take(&mut host.awaiting_snapshot),
            command_error: host.command_error.take(),
            pending: std::mem::take(&mut host.pending),
        }
    }
    fn mount(self, host: &mut Host) {
        host.state = self.state;
        host.entries = self.entries;
        host.awaiting_snapshot = self.awaiting_snapshot;
        host.command_error = self.command_error;
        host.pending = self.pending;
    }
}

impl Host {
    pub(super) fn activate_session(&mut self, session: Option<String>) -> bool {
        if self.session == session {
            return false;
        }
        let had_state = self.state.is_some();
        let old = SessionContext::take(self);
        self.sessions.insert(self.session.clone(), old);
        let target = self.sessions.remove(&session);
        let fresh = target.is_none();
        let mut target = target.unwrap_or_default();
        if fresh {
            target.awaiting_snapshot = had_state;
        }
        self.session = session;
        target.mount(self);
        fresh
    }

    pub(super) fn activate_context(&mut self, context: &str) {
        if self.current_context.as_deref() == Some(context) {
            return;
        }
        if let Some(previous) = self
            .current_context
            .as_ref()
            .and_then(|id| self.contexts.get_mut(id))
        {
            previous.workspace = self.workspace.take();
            previous.active_document = self.active_document.take();
            previous.session = self.session.clone();
        }
        let Some(scope) = self.contexts.get_mut(context) else {
            return;
        };
        self.workspace = scope.workspace.take();
        self.active_document = scope.active_document.take();
        let session = scope.session.clone();
        self.current_context = Some(context.into());
        self.activate_session(session);
    }

    pub(super) fn route_message(&mut self, message: &Value) -> Result {
        let kind = message.str("type");
        if matches!(
            kind,
            "context"
                | "session_snapshot"
                | "session_update"
                | "event"
                | "render"
                | "command_result"
        ) {
            let context = message
                .opt_str("context")
                .filter(|id| !id.is_empty())
                .ok_or_else(|| anyhow!("{kind}.context is required"))?;
            if kind == "command_result" && !self.contexts.contains_key(context) {
                // A command can finish after its originating surface moved
                // and the source context closed. Reuse the live session route;
                // never resurrect a window just to acknowledge an old command.
                let target = self
                    .contexts
                    .iter()
                    .find(|(_, scope)| scope.session.as_deref() == message.opt_str("session_dir"))
                    .map(|(id, _)| id.clone());
                if let Some(target) = target {
                    self.activate_context(&target);
                }
                return Ok(());
            }
            if !self.contexts.contains_key(context) {
                self.contexts.insert(
                    context.into(),
                    Scope {
                        workspace: self.workspace.clone(),
                        session: self.session.clone(),
                        active_document: None,
                    },
                );
                if self.current_context.is_none() {
                    self.current_context = Some(context.into());
                }
            }
            self.activate_context(context);
        } else if matches!(kind, "action" | "close") {
            let view = message.str("view");
            let context = self
                .view_contexts
                .get(view)
                .cloned()
                .ok_or_else(|| anyhow!("{kind} requires a live addressed view"))?;
            self.activate_context(&context);
        }
        Ok(())
    }

    pub(super) fn view_session(&self, view: &str) -> Option<&str> {
        self.view_contexts
            .get(view)
            .and_then(|context| self.contexts.get(context))
            .map_or(self.session.as_deref(), |scope| scope.session.as_deref())
    }

    pub(super) fn acknowledge(&mut self, session: Option<&str>, id: &str) {
        if self.session.as_deref() == session {
            self.pending.retain(|(pending, _, _)| pending != id);
        } else if let Some(state) = self.sessions.get_mut(&session.map(str::to_owned)) {
            state.pending.retain(|(pending, _, _)| pending != id);
        }
    }

    pub(super) fn close_context(&mut self, context: &str) {
        if self.view_contexts.values().any(|id| id == context) {
            return;
        }
        self.contexts.remove(context);
        if self.current_context.as_deref() == Some(context) {
            self.current_context = None;
            if let Some(next) = self.contexts.keys().next().cloned() {
                self.activate_context(&next);
            }
        }
        self.prune_sessions();
    }

    pub(super) fn live_sessions(&self) -> HashSet<Option<String>> {
        self.contexts
            .values()
            .map(|scope| scope.session.clone())
            .collect()
    }

    pub(super) fn prune_sessions(&mut self) {
        // Contexts may arrive just before their first render, so keep their
        // mounted session until close_context removes the route explicitly.
        let live: HashSet<_> = self
            .contexts
            .values()
            .map(|scope| scope.session.clone())
            .collect();
        self.sessions.retain(|session, _| live.contains(session));
        if self.current_context.is_none() && self.contexts.is_empty() {
            self.session = None;
            SessionContext::default().mount(self);
        }
    }

    pub(super) fn render_every_context(&mut self, extension: &mut dyn Extension) {
        let views: Vec<_> = self.views.keys().cloned().collect();
        for view in views {
            self.render_view(extension, &view);
        }
    }
}
