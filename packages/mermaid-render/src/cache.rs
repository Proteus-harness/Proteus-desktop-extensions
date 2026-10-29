use std::collections::{HashSet, VecDeque};

use serde_json::Value;

const MAX_ENTRIES: usize = 8;
const MAX_BYTES: usize = 4 * 1024 * 1024;

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
        let bytes = key.len() + root.to_string().len();
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
