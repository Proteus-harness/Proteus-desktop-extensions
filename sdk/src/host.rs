use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::BufRead;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{anyhow, bail};
use serde_json::{Value, json};

use crate::ui::{self, Node as _};
use crate::{Extension, Result, Surface, ValueExt};

/// Адрес и токен HTTP API сервера из `hello`.
#[derive(Clone, Debug)]
pub struct Server {
    pub origin: String,
    pub token: String,
}

/// Состояние, которое клиент присылает процессу, и способы ему ответить.
pub struct Host {
    pub server: Option<Server>,
    pub features: HashSet<String>,
    pub workspace: Option<String>,
    /// Сессия активного чата.
    pub session: Option<String>,
    /// Последнее состояние сессии (`session_snapshot`/`session_update`).
    pub state: Option<Value>,
    /// Новый чат открыт, его снимок ещё не пришёл: значки ждут, а не мигают пустыми.
    pub awaiting_snapshot: bool,
    pub entries: Vec<Value>,
    pub active_document: Option<Value>,
    pub command_error: Option<String>,
    views: BTreeMap<String, (Surface, Option<String>)>,
    shown: HashMap<String, Value>,
    sequences: HashMap<String, u64>,
    pending: Vec<(String, String, Value)>,
    counter: u64,
    agent: ureq::Agent,
    #[cfg(test)]
    sent: std::cell::RefCell<Vec<Value>>,
}

impl Default for Host {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .proxy(None)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        Self {
            server: None,
            features: HashSet::new(),
            workspace: None,
            session: None,
            state: None,
            awaiting_snapshot: false,
            entries: Vec::new(),
            active_document: None,
            command_error: None,
            views: BTreeMap::new(),
            shown: HashMap::new(),
            sequences: HashMap::new(),
            pending: Vec::new(),
            counter: 0,
            agent,
            #[cfg(test)]
            sent: Default::default(),
        }
    }
}

impl Host {
    pub fn send(&self, message: Value) {
        #[cfg(test)]
        {
            self.sent.borrow_mut().push(message);
        }
        #[cfg(not(test))]
        {
            use std::io::Write as _;
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{message}");
            let _ = out.flush();
        }
    }

    /// GET к API сервера для активной сессии.
    pub fn api(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        self.request(path, query, None, self.session.as_deref(), None)
    }

    /// GET для выбранной сессии (`None` — без `session_dir`).
    pub fn api_for(
        &self,
        path: &str,
        query: &[(&str, &str)],
        session: Option<&str>,
    ) -> Result<Value> {
        self.request(path, query, None, session, None)
    }

    /// POST с JSON-телом для активной сессии.
    pub fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.request(path, &[], Some(body), self.session.as_deref(), None)
    }

    /// POST для долгих операций (например, git-хуков) со своим пределом вместо 15 с.
    pub fn post_with_timeout(&self, path: &str, body: Value, timeout: Duration) -> Result<Value> {
        self.request(
            path,
            &[],
            Some(body),
            self.session.as_deref(),
            Some(timeout),
        )
    }

    /// Ответ сервера как текст.
    pub fn api_text(&self, path: &str, query: &[(&str, &str)]) -> Result<String> {
        self.call(path, query, None, self.session.as_deref(), None)
    }

    fn request(
        &self,
        path: &str,
        query: &[(&str, &str)],
        body: Option<Value>,
        session: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        let text = self.call(path, query, body, session, timeout)?;
        Ok(serde_json::from_str(&text)?)
    }

    fn call(
        &self,
        path: &str,
        query: &[(&str, &str)],
        body: Option<Value>,
        session: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<String> {
        let server = self
            .server
            .as_ref()
            .ok_or_else(|| anyhow!("сервер ещё не подключён"))?;
        let mut url = server.origin.clone() + path;
        let pairs: Vec<(&str, &str)> = query
            .iter()
            .copied()
            .chain(session.map(|session| ("session_dir", session)))
            .collect();
        for (index, (key, value)) in pairs.iter().enumerate() {
            url.push(if index == 0 { '?' } else { '&' });
            url.push_str(&encode(key));
            url.push('=');
            url.push_str(&encode(value));
        }
        let authorization = format!("Bearer {}", server.token);
        let timeout = Some(timeout.unwrap_or(Duration::from_secs(15)));
        let mut response = match body {
            Some(body) => self
                .agent
                .post(&url)
                .config()
                .timeout_global(timeout)
                .build()
                .header("Authorization", &authorization)
                .header("Content-Type", "application/json")
                .send(body.to_string())?,
            None => self
                .agent
                .get(&url)
                .config()
                .timeout_global(timeout)
                .build()
                .header("Authorization", &authorization)
                .call()?,
        };
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string()?;
        if status >= 400 {
            // Сервер объясняет отказ как {"error": "..."}; показываем его, а не код ответа.
            let reason = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| value.opt_str("error").map(str::to_owned));
            bail!(reason.unwrap_or_else(|| format!("HTTP {status}")));
        }
        Ok(text)
    }

    pub fn badge(&self, view: &str, title: &str, lines: Vec<String>, face: Option<Value>) {
        self.send_badge(view, title, lines, face, false);
    }

    pub fn hide_badge(&self, view: &str, title: &str) {
        self.send_badge(view, title, Vec::new(), None, true);
    }

    /// Значок целиком; скрытый значок может сохранить строки и лицо до следующего показа.
    pub fn send_badge(
        &self,
        view: &str,
        title: &str,
        mut lines: Vec<String>,
        face: Option<Value>,
        hidden: bool,
    ) {
        lines.extend(self.command_error.clone());
        self.send(
            json!({"type": "badge", "view": view, "session_dir": self.session, "hidden": hidden,
                         "title": title, "lines": lines,
                         "face": face.unwrap_or_else(|| json!({"type": "icon", "tone": "muted"}))}),
        );
    }

    pub fn show(&mut self, view: &str, root: Value) {
        self.show_with_keys(view, root, json!({}));
    }

    /// Set a document's visible caption without changing its stable argument.
    pub fn show_with_title(&mut self, view: &str, root: Value, title: &str) {
        self.send(json!({"type":"view","session_dir":self.session,"view":view,"root":root,"title":title,"sequence":self.sequence(view)}));
        self.shown.insert(view.to_owned(), root);
    }

    pub fn show_with_keys(&mut self, view: &str, root: Value, keys: Value) {
        self.send(
            json!({"type": "view", "session_dir": self.session, "view": view, "root": root,
                         "sequence": self.sequence(view), "keys": keys}),
        );
        self.shown.insert(view.to_owned(), root);
    }

    /// Вид уже показан с открытия поверхности; новая поверхность — повод перечитать данные.
    pub fn is_shown(&self, view: &str) -> bool {
        self.shown.contains_key(view)
    }

    pub fn surface(&self, view: &str) -> Option<Surface> {
        self.views.get(view).map(|(surface, _)| *surface)
    }

    fn sequence(&self, view: &str) -> u64 {
        self.sequences.get(view).copied().unwrap_or(0)
    }

    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.contains(feature)
    }

    pub fn open_document(&self, argument: &str, preview: bool, focus: bool) {
        self.send(json!({"type": "open_document", "argument": argument, "preview": preview, "focus": focus}));
    }

    /// A single selection replaces the shared preview tab; double click or
    /// Enter (`open`) retains the document and moves focus into it.
    pub fn browse_document(&self, argument: &str, event: &str) {
        self.open_document(argument, event != "open", event == "open");
    }

    pub fn close_popup(&self) {
        self.send(json!({"type": "close_popup"}));
    }

    /// Фокус на контрол поверхности `view` (`None` — на саму поверхность).
    pub fn focus(&self, view: &str, control: Option<&str>) {
        self.send(json!({"type": "focus", "view": view, "control": control}));
    }

    pub fn clipboard(&self, text: &str) {
        self.send(json!({"type": "clipboard", "text": text}));
    }

    /// Меняет настройку сессии (`session.settings.v1`); ответ придёт как `command_result`.
    pub fn set_session(&mut self, kind: &str, value: Value) -> Result {
        if !self.has_feature("session.settings.v1") {
            bail!("The client does not support session.settings.v1");
        }
        let Some(session) = self.session.clone() else {
            return Ok(());
        };
        self.counter += 1;
        let id = format!("setting-{}", self.counter);
        self.pending
            .push((id.clone(), kind.to_owned(), value.clone()));
        self.command_error = None;
        self.send(
            json!({"type": "set_session", "id": id, "session_dir": session,
                         "setting": {"id": kind, "value": value}}),
        );
        Ok(())
    }

    /// Настройки-select из конфигурации сессии.
    pub fn session_settings(&self) -> Vec<&Value> {
        let config = self
            .state
            .as_ref()
            .map(|state| state.at("config"))
            .unwrap_or(&Value::Null);
        config
            .list("session_settings")
            .iter()
            .filter(|setting| setting.at("control").str("type") == "select")
            .collect()
    }

    pub fn setting(&self, id: &str) -> Option<&Value> {
        self.session_settings()
            .into_iter()
            .find(|setting| setting.str("id") == id)
            .map(|setting| setting.at("control"))
    }

    /// Значение настройки с учётом ещё не подтверждённых изменений.
    pub fn setting_value(&self, id: &str) -> Option<Value> {
        if let Some((_, _, value)) = self.pending.iter().rev().find(|(_, kind, _)| kind == id) {
            return Some(value.clone());
        }
        self.setting(id)
            .map(|control| control.at("value").clone())
            .filter(|value| !value.is_null())
    }

    fn failed(&self, view: &str, error: &str) {
        // Прежний вид остаётся под ошибкой, его элементы работают дальше.
        let notice = ui::row(vec![
            ui::text(error).with("tone", "danger"),
            ui::button("Повторить", "refresh").with("icon", "refresh-cw"),
        ]);
        let mut children = vec![notice];
        children.extend(self.shown.get(view).cloned());
        self.send(json!({"type": "view", "session_dir": self.session, "view": view, "sequence": self.sequence(view),
                         "keys": {}, "root": {"type": "fill", "children": children}}));
    }

    fn render_all(&mut self, extension: &mut dyn Extension) {
        let views: Vec<_> = self
            .views
            .iter()
            .map(|(view, (surface, argument))| (view.clone(), *surface, argument.clone()))
            .collect();
        for (view, surface, argument) in views {
            if surface == Surface::Badge && self.awaiting_snapshot {
                continue;
            }
            if let Err(error) =
                guard(|| extension.render(self, &view, surface, argument.as_deref()))
            {
                if surface == Surface::Badge {
                    self.badge(&view, "Ошибка расширения", vec![error], None);
                } else {
                    self.failed(&view, &error);
                }
            }
        }
    }

    fn handle(&mut self, extension: &mut dyn Extension, message: Value) -> Result {
        match message.str("type") {
            "hello" => {
                if message.at("protocol").as_u64() != Some(3) {
                    bail!("This extension requires protocol 3");
                }
                self.features = message
                    .list("features")
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                let server = message.at("server");
                self.server = Some(Server {
                    origin: server.str("origin").to_owned(),
                    token: server.str("token").to_owned(),
                });
                self.workspace = message.opt_str("workspace").map(str::to_owned);
            }
            "command_result" => {
                let id = message.str("id");
                self.pending.retain(|(pending, _, _)| pending != id);
                if message.opt_str("session_dir") != self.session.as_deref() {
                    return Ok(());
                }
                self.command_error = message.opt_str("error").map(str::to_owned);
                let config = message.at("config");
                if !config.is_null() {
                    let state = self.state.get_or_insert_with(|| json!({}));
                    state["config"] = config.clone();
                }
                self.render_all(extension);
            }
            "session_snapshot" => {
                self.session = message.opt_str("session_dir").map(str::to_owned);
                self.awaiting_snapshot = false;
                self.state = Some(message.at("state").clone());
                self.entries = message.list("entries").to_vec();
                self.render_all(extension);
            }
            "session_update" => {
                if message.opt_str("session_dir") != self.session.as_deref() {
                    return Ok(());
                }
                let update = message.at("update");
                match update.str("type") {
                    "state" => self.state = Some(update.at("state").clone()),
                    "entries" => {
                        let start = (update.at("start").as_u64().unwrap_or(0) as usize)
                            .min(self.entries.len());
                        let end = (start + update.at("removed").as_u64().unwrap_or(0) as usize)
                            .min(self.entries.len());
                        self.entries
                            .splice(start..end, update.list("entries").iter().cloned());
                    }
                    _ => {}
                }
                self.render_all(extension);
            }
            "render" => {
                self.session = message.opt_str("session_dir").map(str::to_owned);
                let surface = Surface::parse(message.str("surface"))
                    .ok_or_else(|| anyhow!("unknown surface"))?;
                let argument = message.opt_str("argument").map(str::to_owned);
                self.views
                    .insert(message.str("view").to_owned(), (surface, argument));
                self.render_all(extension);
            }
            "close" => {
                let view = message.str("view");
                self.views.remove(view);
                self.shown.remove(view);
            }
            "event" => {
                self.active_document =
                    Some(message.at("document").clone()).filter(|document| !document.is_null());
                let event = message.str("event");
                if event == "session_opened" {
                    let session = message.opt_str("session_dir").map(str::to_owned);
                    let changed = session != self.session;
                    if changed {
                        self.awaiting_snapshot = self.state.is_some();
                        self.state = None;
                        self.entries.clear();
                        self.pending.clear();
                        self.command_error = None;
                    }
                    self.session = session;
                    if changed {
                        extension.session_changed(self);
                    }
                }
                extension.event(self, event);
                self.render_all(extension);
            }
            "action" => {
                let view = message.str("view").to_owned();
                self.sequences
                    .insert(view.clone(), message.at("sequence").as_u64().unwrap_or(0));
                let value = message.at("value").clone();
                if let Err(error) =
                    guard(|| extension.action(self, &view, message.str("action"), &value))
                {
                    if self.surface(&view) != Some(Surface::Badge) {
                        self.failed(&view, &error);
                    }
                    return Ok(());
                }
                self.render_all(extension);
            }
            _ => {}
        }
        Ok(())
    }
}

/// Runs the protocol loop until the client closes stdin.
pub fn run(mut extension: impl Extension) {
    let (sender, inbox) = mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            match serde_json::from_str(&line) {
                Ok(message) => {
                    if sender.send(message).is_err() {
                        break;
                    }
                }
                Err(error) => eprintln!("extension: bad message: {error}"),
            }
        }
    });
    let mut host = Host::default();
    loop {
        let poll = extension.poll().filter(|_| !host.views.is_empty());
        let message = match poll {
            Some(timeout) => match inbox.recv_timeout(timeout) {
                Ok(message) => message,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    host.render_all(&mut extension);
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            },
            None => match inbox.recv() {
                Ok(message) => message,
                Err(_) => return,
            },
        };
        let hello = message.str("type") == "hello";
        if let Err(error) = host.handle(&mut extension, message) {
            if hello {
                eprintln!("extension: {error}");
                std::process::exit(1);
            }
            eprintln!("extension: {error}");
        }
    }
}

/// Turns an error or a panic of extension code into a message for the user.
fn guard(call: impl FnOnce() -> Result) -> std::result::Result<(), String> {
    match catch_unwind(AssertUnwindSafe(call)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error.to_string()),
        Err(panic) => Err(panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
            .unwrap_or_else(|| "внутренняя ошибка расширения".to_owned())),
    }
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode("/a b/ж"), "%2Fa%20b%2F%D0%B6");
    }

    #[derive(Default)]
    struct Flaky {
        broken: bool,
    }

    impl Extension for Flaky {
        fn render(&mut self, host: &mut Host, view: &str, _: Surface, _: Option<&str>) -> Result {
            if self.broken {
                bail!("fixture outage");
            }
            host.show(view, ui::text("ok"));
            Ok(())
        }

        fn action(&mut self, _: &mut Host, _: &str, action: &str, _: &Value) -> Result {
            if action == "panic" {
                panic!("fixture panic");
            }
            bail!("fixture action")
        }
    }

    fn last(host: &Host) -> Value {
        host.sent.borrow().last().cloned().unwrap_or_default()
    }

    #[test]
    fn failures_keep_the_last_view() {
        let (mut host, mut extension) = (Host::default(), Flaky::default());
        let render =
            json!({"type": "render", "view": "page", "surface": "page", "session_dir": "/one"});
        host.handle(&mut extension, render).unwrap();
        extension.broken = true;
        host.handle(
            &mut extension,
            json!({"type": "event", "event": "run_settled", "session_dir": "/one"}),
        )
        .unwrap();
        let children = last(&host)["root"]["children"].clone();
        assert!(children[0].to_string().contains("fixture outage"));
        assert_eq!(children[1], ui::text("ok"));
        for action in ["x", "panic"] {
            let message = json!({"type": "action", "view": "page", "sequence": 1, "action": action, "value": null});
            host.handle(&mut extension, message).unwrap();
            assert!(last(&host).to_string().contains("fixture"), "{action}");
        }
    }

    #[test]
    fn session_commands_acknowledge_errors_and_changes() {
        let (mut host, mut extension) = (Host::default(), Flaky::default());
        host.features.insert("session.settings.v1".into());
        host.session = Some("/one".into());
        host.state = Some(json!({"config": {"profile": "before"}}));
        host.set_session("reasoning_effort", json!("effort:high"))
            .unwrap();
        assert_eq!(
            host.setting_value("reasoning_effort"),
            Some(json!("effort:high"))
        );
        let id = last(&host)["id"].clone();
        let result = json!({"type": "command_result", "id": id, "session_dir": "/one", "config": null, "error": "fixture failure"});
        host.handle(&mut extension, result).unwrap();
        assert!(host.pending.is_empty());
        assert_eq!(host.command_error.as_deref(), Some("fixture failure"));
        host.set_session("mode", json!("plan")).unwrap();
        let id = last(&host)["id"].clone();
        let config = json!({"permission_mode": "plan"});
        let result = json!({"type": "command_result", "id": id, "session_dir": "/one", "config": config, "error": null});
        host.handle(&mut extension, result).unwrap();
        assert_eq!(
            host.state.as_ref().unwrap()["config"]["permission_mode"],
            "plan"
        );
        host.features.clear();
        assert!(host.set_session("mode", json!("auto")).is_err());
    }

    #[test]
    fn a_new_chat_holds_badges_until_its_snapshot() {
        let (mut host, mut extension) = (Host::default(), Flaky::default());
        host.handle(
            &mut extension,
            json!({"type": "session_snapshot", "session_dir": "/one", "state": {}, "entries": []}),
        )
        .unwrap();
        host.handle(
            &mut extension,
            json!({"type": "render", "view": "badge", "surface": "badge", "session_dir": "/one"}),
        )
        .unwrap();
        let count = host.sent.borrow().len();
        host.handle(
            &mut extension,
            json!({"type": "event", "event": "session_opened", "session_dir": "/two"}),
        )
        .unwrap();
        assert_eq!(host.sent.borrow().len(), count);
        host.handle(
            &mut extension,
            json!({"type": "session_snapshot", "session_dir": "/two", "state": {}, "entries": []}),
        )
        .unwrap();
        assert_eq!(last(&host)["session_dir"], "/two");
    }
}
