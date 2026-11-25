use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::Arc;
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
    value: Arc<Value>,
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
        self.session_for(host, false).map(str::to_owned)
    }

    fn session_for<'a>(&'a self, host: &'a Host, active: bool) -> Option<&'a str> {
        if active {
            host.session.as_deref()
        } else {
            self.chosen
                .as_deref()
                .filter(|chosen| !chosen.is_empty())
                .or(host.session.as_deref())
        }
    }

    /// Cached GET; `active` reads the active chat instead of the chosen session.
    pub fn fetch(
        &mut self,
        host: &Host,
        path: &str,
        query: &[(&str, &str)],
        active: bool,
    ) -> Result<Value> {
        self.fetch_shared(host, path, query, active)
            .map(Arc::unwrap_or_clone)
    }

    /// Read-only handle to a cached answer. The handle remains valid across
    /// later fetches or eviction; the cache still owns only its budgeted entries.
    pub fn fetch_shared(
        &mut self,
        host: &Host,
        path: &str,
        query: &[(&str, &str)],
        active: bool,
    ) -> Result<Arc<Value>> {
        let query_key = if query.is_empty() {
            Cow::Borrowed("[]")
        } else {
            Cow::Owned(format!("{query:?}"))
        };
        let index = self.cache_index(path, self.session_for(host, active), &query_key);
        if let Some(value) = index.and_then(|index| self.touch_cached(index)) {
            return Ok(value);
        }

        // The lookup may reorder or expire an entry, but never changes the
        // selected session. Own a key only when there is an answer to retain.
        let session = self.session_for(host, active);
        let value = host.api_for(path, query, session)?;
        let key = (
            path.to_owned(),
            session.map(str::to_owned),
            query_key.into_owned(),
        );
        Ok(self.remember(key, value))
    }

    fn cache_index(&self, path: &str, session: Option<&str>, query: &str) -> Option<usize> {
        self.cached.iter().position(|entry| {
            let (key_path, key_session, key_query) = &entry.key;
            key_path.as_str() == path
                && key_session.as_deref() == session
                && key_query.as_str() == query
        })
    }

    #[cfg(test)]
    fn cached_value(&mut self, key: &Key) -> Option<Arc<Value>> {
        let index = self.cache_index(&key.0, key.1.as_deref(), &key.2)?;
        self.touch_cached(index)
    }

    fn touch_cached(&mut self, index: usize) -> Option<Arc<Value>> {
        let entry = self.cached.remove(index)?;
        if self.ttl.is_some_and(|ttl| entry.fetched.elapsed() >= ttl) {
            self.bytes -= entry.bytes;
            return None;
        }
        let value = Arc::clone(&entry.value);
        self.cached.push_back(entry);
        Some(value)
    }

    fn remember(&mut self, key: Key, value: Value) -> Arc<Value> {
        let value = Arc::new(value);
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
            value: Arc::clone(&value),
            bytes,
        });
        value
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
        let sessions = self.fetch_shared(host, "/sessions", &[], false)?;
        let choices: Vec<(Value, String)> = sessions
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|session| {
                let preview = session_preview(session);
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

fn session_preview(session: &Value) -> &str {
    let text = session
        .opt_str("preview")
        .filter(|text| !text.is_empty())
        .unwrap_or("Новая сессия");
    match text.char_indices().nth(59) {
        Some((start, scalar)) => &text[..start + scalar.len_utf8()],
        None => text,
    }
}

#[cfg(test)]
mod session_preview_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preview_prefix_borrows_exact_sixty_scalar_prefix() {
        for session in [
            json!({}),
            json!({"preview":null}),
            json!({"preview":false}),
            json!({"preview":""}),
        ] {
            assert_eq!(session_preview(&session), "Новая сессия");
        }
        for text in [
            "  \n".to_owned(),
            "λ".repeat(59),
            "𐐷".repeat(60),
            format!("{}🧪\u{200d}e\u{301}", "λ".repeat(59)),
            format!("{}e\u{301}", "λ".repeat(59)),
        ] {
            let session = json!({"preview":text});
            let original = session.str("preview");
            let preview = session_preview(&session);
            assert_eq!(preview, original.chars().take(60).collect::<String>());
            assert_eq!(preview.as_ptr(), original.as_ptr());
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
