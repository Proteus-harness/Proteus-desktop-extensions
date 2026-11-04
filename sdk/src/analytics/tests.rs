use super::*;
use crate::json;

fn key(turn: usize) -> Key {
    (
        "/analysis".into(),
        Some("/saved".into()),
        format!("turn_id={turn}"),
    )
}

#[test]
fn many_saved_turns_share_entry_and_exact_encoded_byte_budgets() {
    let mut analytics = Analytics::new(None);
    analytics.entry_limit = 4;
    analytics.byte_limit = 1500;
    for turn in 0..500 {
        let value = json!({"turn": turn, "text": "синтетика\\\"\n".repeat(12)});
        assert_eq!(analytics.remember(key(turn), value.clone()), value);
        assert!(analytics.cached.len() <= 4);
        assert!(analytics.bytes <= 1500);
        assert_eq!(
            analytics.bytes,
            analytics
                .cached
                .iter()
                .map(|entry| entry.bytes)
                .sum::<usize>()
        );
    }
    assert!(analytics.cached_value(&key(0)).is_none());
    assert!(analytics.cached_value(&key(499)).is_some());
    analytics.forget("/analysis");
    assert!(analytics.cached.is_empty());
    assert_eq!(analytics.bytes, 0);
}

#[test]
fn recent_hits_survive_eviction_and_oversized_answers_are_not_copied_into_cache() {
    let mut analytics = Analytics::new(None);
    analytics.entry_limit = 2;
    analytics.byte_limit = 512;
    analytics.remember(key(1), json!({"text":"one"}));
    analytics.remember(key(2), json!({"text":"two"}));
    analytics.cached_value(&key(1));
    analytics.remember(key(3), json!({"text":"three"}));
    assert!(analytics.cached_value(&key(2)).is_none());
    assert!(analytics.cached_value(&key(1)).is_some());
    let huge = json!({"text":"large synthetic payload".repeat(1000)});
    let pointer = huge["text"].as_str().unwrap().as_ptr();
    let returned = analytics.remember(key(4), huge);
    assert_eq!(returned["text"].as_str().unwrap().as_ptr(), pointer);
    assert!(analytics.cached_value(&key(4)).is_none());
    assert!(analytics.cached_value(&key(1)).is_some());
    assert_eq!(analytics.cached.len(), 2);
}

#[test]
fn ttl_expiration_and_clear_release_retention_and_encoded_size_counts_utf8() {
    let mut analytics = Analytics::new(Some(Duration::from_secs(5)));
    let value = json!({"text":"☕\\\"\n"});
    let target = key(1);
    let expected = target.0.len()
        + target.1.as_ref().unwrap().len()
        + target.2.len()
        + serde_json::to_vec(&value).unwrap().len();
    assert_eq!(encoded_size(&target, &value, expected), Some(expected));
    assert_eq!(encoded_size(&target, &value, expected - 1), None);
    analytics.remember(target.clone(), value);
    analytics.cached.front_mut().unwrap().fetched = Instant::now() - Duration::from_secs(6);
    assert!(analytics.cached_value(&target).is_none());
    assert_eq!(analytics.bytes, 0);
    analytics.remember(key(2), json!({"text":"another"}));
    analytics.clear();
    assert!(analytics.cached.is_empty());
    assert_eq!(analytics.bytes, 0);
}
