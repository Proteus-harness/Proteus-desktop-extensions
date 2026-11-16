//! Presentation writes are correlated to a native-window context and revision,
//! never to the session shared by sibling windows.

use std::collections::VecDeque;

use super::*;
use crate::Map;
use crate::preferences::{Preferences, check_bytes, check_pending};

const FEATURE: &str = "preferences.visual.v1";
const MAX_PENDING: usize = 32;

#[derive(Default)]
pub(super) struct VisualState {
    initialized: bool,
    revision: u64,
    preferences: Preferences,
    pending: VecDeque<Write>,
    inflight: Option<(String, u64)>,
    error: Option<String>,
}

struct Write {
    id: String,
    view: String,
    key: String,
    value: Option<Value>,
}

impl Host {
    pub fn preferences(&self) -> &Preferences {
        self.current_context
            .as_ref()
            .and_then(|id| self.contexts.get(id))
            .map_or(&self.empty_preferences, |scope| &scope.visual.preferences)
    }

    pub fn visual_preferences_error(&self) -> Option<&str> {
        self.current_context
            .as_ref()
            .and_then(|id| self.contexts.get(id))
            .and_then(|scope| scope.visual.error.as_deref())
    }

    /// Queue a package-local presentation edit. Acceptance is asynchronous;
    /// the owning window returns the authoritative values and revision.
    pub fn set_visual_preference(&mut self, key: &str, value: Option<Value>) -> Result {
        if !self.has_feature(FEATURE) {
            bail!("The client does not support {FEATURE}");
        }
        if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
            bail!("invalid visual preference key");
        }
        let context = self
            .current_context
            .clone()
            .ok_or_else(|| anyhow!("visual preferences require a live context"))?;
        let view = self
            .current_view
            .clone()
            .ok_or_else(|| anyhow!("visual preferences require an addressed view"))?;
        if self.view_contexts.get(&view) != Some(&context) {
            bail!("visual preference view no longer belongs to this context");
        }
        if self
            .contexts
            .values()
            .map(|scope| scope.visual.pending.len())
            .sum::<usize>()
            >= MAX_PENDING
        {
            bail!("too many pending visual preference writes");
        }
        let queued: Vec<_> = self
            .contexts
            .values()
            .flat_map(|scope| scope.visual.pending.iter())
            .map(|write| (write.key.as_str(), write.value.as_ref()))
            .chain(std::iter::once((key, value.as_ref())))
            .collect();
        check_pending(&queued)?;
        let state = &mut self
            .contexts
            .get_mut(&context)
            .ok_or_else(|| anyhow!("visual context is closed"))?
            .visual;
        if !state.initialized {
            bail!("window visual preferences are not ready");
        }
        let mut desired = state.preferences.values().clone();
        for pending in &state.pending {
            apply(&mut desired, &pending.key, pending.value.as_ref());
        }
        apply(&mut desired, key, value.as_ref());
        check_bytes(&desired)?;
        self.counter += 1;
        let id = format!("visual-{}", self.counter);
        state.pending.push_back(Write {
            id,
            view,
            key: key.into(),
            value,
        });
        state.error = None;
        self.visual_action = true;
        self.send_next_visual(&context);
        Ok(())
    }

    pub(super) fn update_visual_context(&mut self, message: &mut Value) -> Result<bool> {
        let context = message.str("context").to_owned();
        if !self.has_feature(FEATURE) {
            if let Some(scope) = self.contexts.get_mut(&context) {
                scope.visual = VisualState::default();
            }
            return Ok(false);
        }
        let mut visual = message
            .get_mut("window_visual")
            .map(Value::take)
            .ok_or_else(|| anyhow!("context.window_visual is required for {FEATURE}"))?;
        let revision = visual
            .get("revision")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("window_visual.revision is required"))?;
        let values = take_values(&mut visual, "preferences")?;
        let preferences = Preferences::from_values(values)?;
        let state = &mut self
            .contexts
            .get_mut(&context)
            .ok_or_else(|| anyhow!("visual context is closed"))?
            .visual;
        if state.initialized && revision < state.revision {
            return Ok(false);
        }
        if state.initialized && revision == state.revision {
            if preferences != state.preferences {
                bail!("visual preference values changed without a revision");
            }
            return Ok(false);
        }
        *state = VisualState {
            initialized: true,
            revision,
            preferences,
            ..VisualState::default()
        };
        Ok(true)
    }

    pub(super) fn visual_preferences_result(
        &mut self,
        extension: &mut dyn Extension,
        mut message: Value,
    ) -> Result {
        let context = message.str("context").to_owned();
        let id = message.str("id");
        let Some(scope) = self.contexts.get(&context) else {
            return Ok(());
        };
        let Some((pending_id, expected)) = &scope.visual.inflight else {
            return Ok(());
        };
        if pending_id != id {
            return Ok(());
        }
        let Some(revision) = message.get("revision").and_then(Value::as_u64) else {
            bail!("visual result revision is required");
        };
        if revision < *expected || scope.visual.revision != *expected {
            return Ok(());
        }
        let error = message.opt_str("error").map(str::to_owned);
        let values = take_values(&mut message, "values")?;
        let preferences = Preferences::from_values(values)?;
        let state = &mut self.contexts.get_mut(&context).unwrap().visual;
        state.preferences = preferences;
        state.revision = revision;
        state.inflight = None;
        state.pending.pop_front();
        state.error = error;
        if state.error.is_some() {
            state.pending.clear();
        }
        self.send_next_visual(&context);
        self.render_visual_context(extension, &context, None);
        Ok(())
    }

    fn send_next_visual(&mut self, context: &str) {
        let Some(scope) = self.contexts.get_mut(context) else {
            return;
        };
        let state = &mut scope.visual;
        if state.inflight.is_some() {
            return;
        }
        while state.pending.front().is_some_and(|write| {
            self.view_contexts.get(&write.view).map(String::as_str) != Some(context)
        }) {
            state.pending.pop_front();
        }
        let Some(write) = state.pending.front() else {
            return;
        };
        let mut values = state.preferences.values().clone();
        apply(&mut values, &write.key, write.value.as_ref());
        let message = json!({"type":"set_visual_preferences", "context":context,
            "view":write.view, "id":write.id, "expected_revision":state.revision, "values":values});
        state.inflight = Some((write.id.clone(), state.revision));
        self.send(message);
    }

    pub(super) fn forget_visual_view(&mut self, context: &str, view: &str) {
        if let Some(scope) = self.contexts.get_mut(context) {
            let state = &mut scope.visual;
            let invalidates = state
                .pending
                .front()
                .is_some_and(|write| write.view == view);
            state.pending.retain(|write| write.view != view);
            if invalidates {
                state.inflight = None;
            }
        }
        self.send_next_visual(context);
    }

    pub(super) fn render_visual_context(
        &mut self,
        extension: &mut dyn Extension,
        context: &str,
        first: Option<&str>,
    ) {
        let mut views: Vec<_> = self
            .views
            .keys()
            .filter(|view| self.view_contexts.get(*view).map(String::as_str) == Some(context))
            .cloned()
            .collect();
        if let Some(at) = first.and_then(|first| views.iter().position(|view| view == first)) {
            views[..=at].rotate_right(1);
        }
        for view in views {
            self.render_view(extension, &view);
        }
    }
}

fn apply(values: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        values.insert(key.into(), value.clone());
    } else {
        values.remove(key);
    }
}

fn take_values(message: &mut Value, field: &str) -> Result<Map<String, Value>> {
    match message.get_mut(field).map(Value::take) {
        Some(Value::Object(values)) => Ok(values),
        _ => bail!("visual preference {field} object is required"),
    }
}

#[cfg(test)]
mod tests;
