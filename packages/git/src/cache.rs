//! Bounded immutable history snapshots, shared by all views of one session.
//!
//! Sixteen records retain roughly eight neighbouring commits and their selected
//! file patches. The combined 8 MiB encoded-payload budget also bounds unusually
//! large commit messages/patches. This is a logical wire-byte budget, not RSS:
//! serde's tree and active view roots have their own allocations. Closing a
//! preview does not invalidate an immutable snapshot needed by a sibling view.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::Arc;

use proteus_extension::{Value, ValueExt};

const ENTRIES: usize = 16;
const BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Commit,
    Diff(String),
}

struct Entry {
    kind: Kind,
    /// One canonical record, regardless of which full/short alias read it.
    aliases: Vec<String>,
    value: Arc<Value>,
    bytes: usize,
}

pub(crate) struct HistoryCache {
    entries: VecDeque<Entry>,
    bytes: usize,
    entry_limit: usize,
    byte_limit: usize,
}

impl Default for HistoryCache {
    fn default() -> Self {
        Self::with_limits(ENTRIES, BYTES)
    }
}

/// Count exactly serde_json's UTF-8 encoding, without a second payload string.
struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("JSON size overflow"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn encoded_bytes(value: &Value) -> Option<usize> {
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.0)
}

pub(crate) fn encoded_text_bytes(value: &str) -> Option<usize> {
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.0)
}

/// Mutable worktree snapshots have their own shared per-session retention
/// budget; immutable commit records cannot crowd them out (or vice versa).
#[derive(Default)]
pub(crate) struct WorkingCache {
    entries: VecDeque<(String, Value, usize)>,
    bytes: usize,
}

impl WorkingCache {
    pub(crate) fn get(&mut self, path: &str) -> Option<&Value> {
        let index = self.entries.iter().position(|(key, _, _)| key == path)?;
        let entry = self.entries.remove(index)?;
        self.entries.push_back(entry);
        self.entries.back().map(|(_, value, _)| value)
    }

    pub(crate) fn insert(&mut self, path: String, value: Value) {
        self.remove(&path);
        let Some(bytes) = encoded_bytes(&value).and_then(|size| size.checked_add(path.len()))
        else {
            return;
        };
        if bytes > BYTES {
            return;
        }
        while self.entries.len() >= ENTRIES || bytes > BYTES - self.bytes {
            let (_, _, bytes) = self.entries.pop_front().unwrap();
            self.bytes -= bytes;
        }
        self.bytes += bytes;
        self.entries.push_back((path, value, bytes));
    }

    pub(crate) fn remove(&mut self, path: &str) {
        self.retain(|key, _| key != path);
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&str, &Value) -> bool) {
        self.entries.retain(|(key, value, bytes)| {
            if keep(key, value) {
                true
            } else {
                self.bytes -= bytes;
                false
            }
        });
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
    #[cfg(test)]
    pub(crate) fn contains_key(&self, path: &str) -> bool {
        self.entries.iter().any(|(key, _, _)| key == path)
    }
}

impl HistoryCache {
    fn with_limits(entry_limit: usize, byte_limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            entry_limit,
            byte_limit,
        }
    }

    fn get(&mut self, kind: &Kind, hash: &str) -> Option<Arc<Value>> {
        let index = self.entries.iter().position(|entry| {
            entry.kind == *kind && entry.aliases.iter().any(|alias| alias == hash)
        })?;
        let entry = self.entries.remove(index)?;
        let value = Arc::clone(&entry.value);
        self.entries.push_back(entry);
        Some(value)
    }

    pub(crate) fn commit(&mut self, hash: &str) -> Option<Arc<Value>> {
        self.get(&Kind::Commit, hash)
    }

    pub(crate) fn diff(&mut self, hash: &str, path: &str) -> Option<Arc<Value>> {
        self.get(&Kind::Diff(path.into()), hash)
    }

    fn insert(&mut self, kind: Kind, aliases: Vec<String>, value: Arc<Value>) {
        let mut counter = Counter(0);
        if serde_json::to_writer(&mut counter, value.as_ref()).is_err() {
            return;
        }
        let Some(bytes) = aliases
            .iter()
            .try_fold(counter.0, |bytes, alias| bytes.checked_add(alias.len()))
            .and_then(|bytes| {
                bytes.checked_add(match &kind {
                    Kind::Commit => 0,
                    Kind::Diff(path) => path.len(),
                })
            })
        else {
            return;
        };
        // Oversized values are still returned/rendered, but never cached or
        // allowed to evict useful small records.
        if self.entry_limit == 0 || bytes > self.byte_limit || aliases.is_empty() {
            return;
        }
        self.entries.retain(|entry| {
            let duplicate =
                entry.kind == kind && aliases.iter().any(|alias| entry.aliases.contains(alias));
            if duplicate {
                self.bytes -= entry.bytes;
            }
            !duplicate
        });
        while self.entries.len() >= self.entry_limit || bytes > self.byte_limit - self.bytes {
            if let Some(entry) = self.entries.pop_front() {
                self.bytes -= entry.bytes;
            }
        }
        self.bytes += bytes;
        self.entries.push_back(Entry {
            kind,
            aliases,
            value,
            bytes,
        });
    }

    pub(crate) fn insert_commit(&mut self, requested: &str, value: Arc<Value>) {
        let mut aliases = Vec::new();
        for hash in [value.str("hash"), value.str("short"), requested] {
            if !hash.is_empty() && !aliases.iter().any(|alias| alias == hash) {
                aliases.push(hash.to_owned());
            }
        }
        self.insert(Kind::Commit, aliases, value);
    }

    pub(crate) fn insert_diff(&mut self, hash: &str, path: &str, value: Arc<Value>) {
        let aliases = self
            .entries
            .iter()
            .find(|entry| {
                entry.kind == Kind::Commit && entry.aliases.iter().any(|alias| alias == hash)
            })
            .map(|entry| entry.aliases.clone())
            .unwrap_or_else(|| vec![hash.to_owned()]);
        self.insert(Kind::Diff(path.into()), aliases, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_extension::json;

    #[test]
    fn working_diffs_have_an_independent_lru_byte_budget() {
        let mut cache = WorkingCache::default();
        for n in 0..100 {
            cache.insert(
                n.to_string(),
                json!({"patch":"\n\\\"Ж".repeat(30_000), "revision":"r"}),
            );
            assert!(cache.bytes <= BYTES);
            assert!(cache.len() <= ENTRIES);
        }
        assert!(cache.get("0").is_none());
        assert!(cache.get("99").is_some());
        // Touching an entry promotes it; byte-heavy insertion evicts older ones.
        cache.insert("large".into(), json!({"patch":"x".repeat(BYTES)}));
        assert!(!cache.contains_key("large"));
        assert!(cache.contains_key("99"));
        cache.retain(|_, _| false);
        assert_eq!(cache.bytes, 0);
    }

    fn commit(hash: &str, short: &str) -> Arc<Value> {
        Arc::new(json!({"hash": hash, "short": short, "patch": "synthetic"}))
    }

    #[test]
    fn aliases_share_identity_and_evict_together() {
        let mut cache = HistoryCache::with_limits(2, usize::MAX);
        let first = commit("first-full", "first");
        cache.insert_commit("first", first.clone());
        assert!(Arc::ptr_eq(&first, &cache.commit("first-full").unwrap()));
        assert!(Arc::ptr_eq(&first, &cache.commit("first").unwrap()));
        cache.insert_commit("second", commit("second-full", "second"));
        cache.commit("first"); // recent hit protects first.
        cache.insert_commit("third", commit("third-full", "third"));
        assert!(cache.commit("second").is_none());
        assert!(cache.commit("second-full").is_none());
        assert!(cache.commit("first").is_some());
        assert_eq!(cache.entries.len(), 2);
    }

    #[test]
    fn commit_and_diff_share_entry_and_byte_budgets() {
        let mut cache = HistoryCache::with_limits(3, 200);
        for n in 0..100 {
            cache.insert_commit(
                &format!("{n}"),
                commit(&format!("full-{n}"), &format!("{n}")),
            );
            cache.insert_diff(
                &format!("full-{n}"),
                "file",
                Arc::new(json!({"patch":"synthetic\\\"\n☕"})),
            );
            assert!(cache.entries.len() <= 3);
            assert!(cache.bytes <= 200);
            assert_eq!(
                cache.bytes,
                cache.entries.iter().map(|entry| entry.bytes).sum::<usize>()
            );
        }
    }

    #[test]
    fn byte_pressure_evicts_lru_even_below_the_entry_limit() {
        let first = commit("first-full", "first");
        let mut counter = Counter(0);
        serde_json::to_writer(&mut counter, first.as_ref()).unwrap();
        let bytes = counter.0 + "first-full".len() + "first".len();
        let mut cache = HistoryCache::with_limits(16, bytes + 1);
        cache.insert_commit("first", first);
        cache.insert_commit("other", commit("other-full", "other"));
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.commit("first").is_none());
        assert!(cache.commit("first-full").is_none());
        assert!(cache.commit("other").is_some());
        assert!(cache.bytes <= bytes + 1);
    }

    #[test]
    fn diff_aliases_survive_commit_eviction() {
        let mut cache = HistoryCache::with_limits(2, usize::MAX);
        cache.insert_commit("first", commit("first-full", "first"));
        let diff = Arc::new(json!({"patch":"synthetic"}));
        cache.insert_diff("first-full", "file", diff.clone());
        cache.insert_commit("second", commit("second-full", "second"));
        assert!(cache.commit("first").is_none());
        assert!(Arc::ptr_eq(&diff, &cache.diff("first", "file").unwrap()));
        assert!(Arc::ptr_eq(
            &diff,
            &cache.diff("first-full", "file").unwrap()
        ));
    }

    #[test]
    fn counts_encoded_utf8_without_a_payload_clone_and_rejects_oversize() {
        let value = commit("full", "short");
        let mut counter = Counter(0);
        serde_json::to_writer(&mut counter, &json!({"patch":"☕\\\"\n"})).unwrap();
        assert_eq!(
            counter.0,
            serde_json::to_vec(&json!({"patch":"☕\\\"\n"}))
                .unwrap()
                .len()
        );
        let mut cache = HistoryCache::with_limits(2, 80);
        cache.insert_commit("short", value.clone());
        assert_eq!(cache.entries.len(), 1);
        let large = Arc::new(json!({"hash":"large", "short":"large", "patch":"x".repeat(100)}));
        cache.insert_commit("large", large);
        assert!(cache.commit("large").is_none());
        assert!(Arc::ptr_eq(&value, &cache.commit("short").unwrap()));
        let mut counter = Counter(usize::MAX);
        assert!(counter.write(b"x").is_err());
    }
}
