use std::collections::VecDeque;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::Value;

use crate::{Host, ValueExt, ui};

// A process may inspect many saved turns. Freshness does not limit retention:
// all analytics paths/queries share these entry and encoded-payload budgets.
const ENTRIES: usize = 16;
const BYTES: usize = 8 * 1024 * 1024;
type Key = (String, Option<String>, String);

struct Cached {
    key: Key,
    fetched: Instant,
    value: Value,
    bytes: usize,
}

/// Pages that read the server API for an active or a chosen saved session.
pub struct Analytics {
    /// Saved session picked in the page; `None` follows the active chat.
    pub chosen: Option<String>,
    /// How long a fetched answer stays fresh; `None` keeps it until refresh.
    pub ttl: Option<Duration>,
    cached: VecDeque<Cached>,
    bytes: usize,
    entry_limit: usize,
    byte_limit: usize,
}

impl Analytics {
    pub fn new(ttl: Option<Duration>) -> Self {
        Self {
            chosen: None,
            ttl,
            cached: VecDeque::new(),
            bytes: 0,
            entry_limit: ENTRIES,
            byte_limit: BYTES,
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
        if let Some(value) = self.cached_value(&key) {
            return Ok(value);
        }
        let value = host.api_for(path, query, session.as_deref())?;
        Ok(self.remember(key, value))
    }

    fn cached_value(&mut self, key: &Key) -> Option<Value> {
        let index = self.cached.iter().position(|entry| entry.key == *key)?;
        let entry = self.cached.remove(index)?;
        if self.ttl.is_some_and(|ttl| entry.fetched.elapsed() >= ttl) {
            self.bytes -= entry.bytes;
            return None;
        }
        let value = entry.value.clone();
        self.cached.push_back(entry);
        Some(value)
    }

    fn remember(&mut self, key: Key, value: Value) -> Value {
        let Some(bytes) = encoded_size(&key, &value, self.byte_limit) else {
            // Keep an oversized answer only in its active view, without an
            // extra cache copy or eviction of useful small answers.
            return value;
        };
        if self.entry_limit == 0 {
            return value;
        }
        while self.cached.len() >= self.entry_limit || bytes > self.byte_limit - self.bytes {
            let removed = self.cached.pop_front().expect("occupied cache budget");
            self.bytes -= removed.bytes;
        }
        self.bytes += bytes;
        self.cached.push_back(Cached {
            key,
            fetched: Instant::now(),
            value,
            bytes,
        });
        self.cached.back().unwrap().value.clone()
    }

    /// Drops cached answers of one path, e.g. after `run_settled`.
    pub fn forget(&mut self, path: &str) {
        self.cached.retain(|entry| {
            if entry.key.0 == path {
                self.bytes -= entry.bytes;
                false
            } else {
                true
            }
        });
    }

    pub fn clear(&mut self) {
        self.cached.clear();
        self.bytes = 0;
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
                self.clear();
                true
            }
            "refresh" => {
                self.clear();
                false
            }
            _ => false,
        }
    }
}

/// Measure serde's exact UTF-8 encoding without allocating a second JSON body.
/// Stop walking a large answer as soon as retaining it would exceed the budget.
fn encoded_size(key: &Key, value: &Value, limit: usize) -> Option<usize> {
    let bytes = key
        .0
        .len()
        .checked_add(key.1.as_ref().map_or(0, String::len))?
        .checked_add(key.2.len())?;
    struct Size {
        bytes: usize,
        limit: usize,
    }
    impl Write for Size {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            self.bytes = self
                .bytes
                .checked_add(input.len())
                .filter(|bytes| *bytes <= self.limit)
                .ok_or_else(|| io::Error::other("analytics cache budget exceeded"))?;
            Ok(input.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut size = Size { bytes, limit };
    serde_json::to_writer(&mut size, value).ok()?;
    Some(size.bytes)
}

#[cfg(test)]
mod tests;
