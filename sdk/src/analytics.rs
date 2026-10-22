use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::Value;

use crate::{Host, ValueExt, ui};

/// Pages that read the server API for an active or a chosen saved session.
pub struct Analytics {
    /// Saved session picked in the page; `None` follows the active chat.
    pub chosen: Option<String>,
    /// How long a fetched answer stays fresh; `None` keeps it until refresh.
    pub ttl: Option<Duration>,
    cached: HashMap<(String, Option<String>, String), (Instant, Value)>,
}

impl Analytics {
    pub fn new(ttl: Option<Duration>) -> Self {
        Self {
            chosen: None,
            ttl,
            cached: HashMap::new(),
        }
    }

    pub fn selected_session(&self, host: &Host) -> Option<String> {
        self.chosen
            .clone()
            .filter(|chosen| !chosen.is_empty())
            .or_else(|| host.session.clone())
    }

    /// Cached GET; `active` reads the active chat instead of the chosen session.
    pub fn fetch(
        &mut self,
        host: &Host,
        path: &str,
        query: &[(&str, &str)],
        active: bool,
    ) -> Result<Value> {
        let session = if active {
            host.session.clone()
        } else {
            self.selected_session(host)
        };
        let key = (path.to_owned(), session.clone(), format!("{query:?}"));
        let fresh = self
            .cached
            .get(&key)
            .is_some_and(|(at, _)| self.ttl.is_none_or(|ttl| at.elapsed() < ttl));
        if !fresh {
            let value = host.api_for(path, query, session.as_deref())?;
            self.cached.insert(key.clone(), (Instant::now(), value));
        }
        Ok(self.cached[&key].1.clone())
    }

    /// Drops cached answers of one path, e.g. after `run_settled`.
    pub fn forget(&mut self, path: &str) {
        self.cached.retain(|(cached, _, _), _| cached != path);
    }

    pub fn clear(&mut self) {
        self.cached.clear();
    }

    /// Select of saved sessions; sends action `session`.
    pub fn session_picker(&mut self, host: &Host) -> Result<Value> {
        let sessions = self.fetch(host, "/sessions", &[], false)?;
        let choices: Vec<(Value, String)> = sessions
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|session| {
                let preview: String = session
                    .opt_str("preview")
                    .filter(|text| !text.is_empty())
                    .unwrap_or("Новая сессия")
                    .chars()
                    .take(60)
                    .collect();
                (
                    session.at("session_dir").clone(),
                    format!(
                        "{preview} · {} сообщ.",
                        session.i64("message_count").unwrap_or(0)
                    ),
                )
            })
            .collect();
        let selected = self
            .selected_session(host)
            .map(Value::from)
            .unwrap_or(Value::Null);
        let title = choices
            .iter()
            .find(|(value, _)| *value == selected)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| "Сохранённая сессия".to_owned());
        Ok(ui::select(
            "session", &title, &choices, &selected, "session",
        ))
    }

    /// Handles `session` and `refresh`; returns true when the inspected session changed.
    pub fn action(&mut self, action: &str, value: &Value) -> bool {
        match action {
            "session" => {
                self.chosen = value.as_str().map(str::to_owned);
                self.cached.clear();
                true
            }
            "refresh" => {
                self.cached.clear();
                false
            }
            _ => false,
        }
    }
}
