use std::collections::{HashSet, VecDeque};
use std::io::{self, Write};

use serde_json::Value;

const MAX_ENTRIES: usize = 8;
const MAX_BYTES: usize = 4 * 1024 * 1024;

// Count the compact JSON bytes without keeping a second copy of the rendered root.
struct ByteCounter(usize);

impl Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("cache byte count overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Entry {
    key: String,
    root: Value,
    owners: HashSet<String>,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct Cache {
    entries: VecDeque<Entry>,
}

impl Cache {
    pub fn get(&mut self, key: &str, view: &str) -> Option<Value> {
        for entry in &mut self.entries {
            if entry.key != key {
                entry.owners.remove(view);
            }
        }
        self.entries.retain(|entry| !entry.owners.is_empty());
        let position = self.entries.iter().position(|entry| entry.key == key)?;
        let mut entry = self.entries.remove(position)?;
        entry.owners.insert(view.to_owned());
        let root = entry.root.clone();
        self.entries.push_back(entry);
        Some(root)
    }

    pub fn insert(&mut self, key: String, view: &str, root: Value) {
        self.closed(view);
        let mut counter = ByteCounter(key.len());
        if serde_json::to_writer(&mut counter, &root).is_err() {
            return;
        }
        let bytes = counter.0;
        if bytes > MAX_BYTES {
            return;
        }
        self.entries.push_back(Entry {
            key,
            root,
            owners: HashSet::from([view.to_owned()]),
            bytes,
        });
        while self.entries.len() > MAX_ENTRIES
            || self.entries.iter().map(|entry| entry.bytes).sum::<usize>() > MAX_BYTES
        {
            self.entries.pop_front();
        }
    }

    pub fn closed(&mut self, view: &str) {
        for entry in &mut self.entries {
            entry.owners.remove(view);
        }
        self.entries.retain(|entry| !entry.owners.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn byte_accounting_matches_compact_json_for_escaped_and_utf8_values() {
        for root in [
            Value::Null,
            json!(true),
            json!(i64::MIN),
            json!(u64::MAX),
            json!(-0.125),
            json!(""),
            json!("Кириллица 😀 \"\\\n\r\t\u{0000}\u{001f}"),
            json!({"quote\"\\\n": ["😀", null, false, 1.5, {"nested": "\u{0008}"}]}),
        ] {
            let key = "ключ\n";
            let expected = key.len() + root.to_string().len();
            let mut cache = Cache::default();
            cache.insert(key.into(), "view", root.clone());
            assert_eq!(cache.entries.len(), 1);
            assert_eq!(cache.entries[0].bytes, expected);
            assert_eq!(cache.entries[0].root, root);
        }
    }

    #[test]
    fn byte_limit_accepts_exact_fit_and_rejects_one_extra_byte() {
        for payload_bytes in [MAX_BYTES - 4, MAX_BYTES - 3, MAX_BYTES - 2] {
            let mut cache = Cache::default();
            // One key byte and two JSON quote bytes count against the quota.
            cache.insert("k".into(), "view", json!("x".repeat(payload_bytes)));
            if payload_bytes + 3 <= MAX_BYTES {
                assert_eq!(cache.entries.len(), 1);
                assert_eq!(cache.entries[0].bytes, payload_bytes + 3);
            } else {
                assert!(cache.entries.is_empty());
            }
        }
    }

    #[test]
    fn eviction_counts_json_escape_bytes_not_unescaped_text() {
        let mut cache = Cache::default();
        cache.insert("old".into(), "first", json!("x".repeat(MAX_BYTES / 2)));
        let escaped = json!("\u{0000}".repeat(MAX_BYTES / 12));
        let expected = "new".len() + escaped.to_string().len();
        cache.insert("new".into(), "second", escaped);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.entries[0].key, "new");
        assert_eq!(cache.entries[0].bytes, expected);
    }

    #[test]
    fn rejected_replacement_releases_only_the_replacing_owner() {
        let mut cache = Cache::default();
        cache.insert("old".into(), "first", json!("old"));
        assert!(cache.get("old", "second").is_some());
        cache.insert("new".into(), "first", json!("x".repeat(MAX_BYTES)));
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.entries[0].key, "old");
        assert_eq!(cache.entries[0].owners, HashSet::from(["second".into()]));
        cache.closed("second");
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn counter_propagates_overflow_without_wrapping_or_partial_accounting() {
        let mut counter = ByteCounter(usize::MAX - 1);
        assert_eq!(counter.write(b"x").unwrap(), 1);
        assert_eq!(counter.0, usize::MAX);
        assert!(serde_json::to_writer(&mut counter, &Value::Null).is_err());
        assert_eq!(counter.0, usize::MAX);
        assert_eq!(counter.write(b"").unwrap(), 0);
        assert!(counter.flush().is_ok());
    }

    #[test]
    fn shared_cache_releases_closed_views_and_stays_bounded() {
        let mut cache = Cache::default();
        cache.insert("same".into(), "first", json!({"svg":"original"}));
        assert!(cache.get("same", "second").is_some());
        cache.closed("first");
        assert!(cache.get("same", "second").is_some());
        cache.closed("second");
        assert!(cache.entries.is_empty());
        for index in 0..20 {
            cache.insert(
                index.to_string(),
                &format!("v{index}"),
                json!("x".repeat(700_000)),
            );
        }
        assert!(cache.entries.len() <= MAX_ENTRIES);
        assert!(cache.entries.iter().map(|entry| entry.bytes).sum::<usize>() <= MAX_BYTES);
        assert!(cache.get("0", "old").is_none());
        assert!(cache.get("19", "latest").is_some());
    }

    #[test]
    fn changed_source_or_theme_drops_the_previous_owner() {
        let mut cache = Cache::default();
        cache.insert("old-theme".into(), "view", json!("old"));
        assert!(cache.get("new-theme", "view").is_none());
        assert!(cache.entries.is_empty());
    }
}
