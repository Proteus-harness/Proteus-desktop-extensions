//! Package state belongs to a session, not to whichever window rendered
//! last. One process retains one instance per live session, shared by that
//! session's surfaces, including surfaces moved to a second window.

use super::*;

pub(super) struct Instances<E> {
    initial: Option<E>,
    sessions: HashMap<Option<String>, E>,
}

impl<E: Extension + Default> Instances<E> {
    pub(super) fn new(extension: E) -> Self {
        Self {
            initial: Some(extension),
            sessions: HashMap::new(),
        }
    }
    fn for_host(&mut self, host: &Host) -> &mut E {
        let initial = &mut self.initial;
        self.sessions
            .entry(host.session.clone())
            .or_insert_with(|| initial.take().unwrap_or_default())
    }
    pub(super) fn prune(&mut self, host: &Host) {
        let live = host.live_sessions();
        self.sessions.retain(|session, _| live.contains(session));
    }
    #[cfg(test)]
    pub(super) fn count(&self) -> usize {
        self.sessions.len()
    }
}

impl<E: Extension + Default> Extension for Instances<E> {
    fn poll(&self) -> Option<Duration> {
        self.initial
            .iter()
            .chain(self.sessions.values())
            .filter_map(Extension::poll)
            .min()
    }
    fn render(
        &mut self,
        host: &mut Host,
        view: &str,
        surface: Surface,
        argument: Option<&str>,
    ) -> Result {
        self.for_host(host).render(host, view, surface, argument)
    }
    fn action(&mut self, host: &mut Host, view: &str, action: &str, value: &Value) -> Result {
        self.for_host(host).action(host, view, action, value)
    }
    fn event(&mut self, host: &mut Host, event: &str) {
        self.for_host(host).event(host, event);
    }
    fn session_changed(&mut self, host: &mut Host) {
        self.for_host(host).session_changed(host);
    }
    fn closed(&mut self, host: &mut Host, view: &str) {
        self.for_host(host).closed(host, view);
    }
}
