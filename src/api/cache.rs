use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A generic in-memory cache with TTL-based expiry.
///
/// Each entry is stored with a timestamp and expires after the configured TTL.
/// Expired entries are treated as missing (returning `None` from `get`), but the
/// stale value can still be retrieved via `get_stale` for background-refresh
/// patterns.
#[derive(Debug)]
pub struct ResponseCache<V> {
    entries: HashMap<String, CacheEntry<V>>,
    ttl: Duration,
}

#[derive(Debug, Clone)]
struct CacheEntry<V> {
    value: V,
    inserted_at: Instant,
}

#[allow(dead_code)]
impl<V: Clone> ResponseCache<V> {
    /// Create a new cache with the given time-to-live for entries.
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    /// Insert a value into the cache, replacing any existing entry for the key.
    pub fn insert(&mut self, key: impl Into<String>, value: V) {
        self.entries.insert(
            key.into(),
            CacheEntry {
                value,
                inserted_at: Instant::now(),
            },
        );
    }

    /// Get a cached value if it exists and has not expired.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.get(key).and_then(|entry| {
            if entry.inserted_at.elapsed() < self.ttl {
                Some(&entry.value)
            } else {
                None
            }
        })
    }

    /// Get a cached value regardless of expiry. Returns `None` only if the key
    /// was never inserted.
    pub fn get_stale(&self, key: &str) -> Option<&V> {
        self.entries.get(key).map(|entry| &entry.value)
    }

    /// Returns `true` if the key exists and has not expired.
    pub fn is_fresh(&self, key: &str) -> bool {
        self.entries
            .get(key)
            .is_some_and(|entry| entry.inserted_at.elapsed() < self.ttl)
    }

    /// Remove a specific entry from the cache.
    pub fn invalidate(&mut self, key: &str) {
        self.entries.remove(key);
    }

    /// Remove all entries from the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn insert_and_get_returns_value() {
        let mut cache = ResponseCache::new(Duration::from_secs(60));
        cache.insert("key1", vec![1, 2, 3]);
        assert_eq!(cache.get("key1"), Some(&vec![1, 2, 3]));
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let cache: ResponseCache<String> = ResponseCache::new(Duration::from_secs(60));
        assert!(cache.get("missing").is_none());
    }

    #[test]
    fn expired_entry_returns_none() {
        let mut cache = ResponseCache::new(Duration::from_millis(10));
        cache.insert("key", "value".to_string());
        thread::sleep(Duration::from_millis(20));
        assert!(cache.get("key").is_none());
    }

    #[test]
    fn get_stale_returns_expired_entry() {
        let mut cache = ResponseCache::new(Duration::from_millis(10));
        cache.insert("key", "value".to_string());
        thread::sleep(Duration::from_millis(20));
        assert!(cache.get("key").is_none());
        assert_eq!(cache.get_stale("key"), Some(&"value".to_string()));
    }

    #[test]
    fn get_stale_returns_none_for_missing_key() {
        let cache: ResponseCache<String> = ResponseCache::new(Duration::from_secs(60));
        assert!(cache.get_stale("missing").is_none());
    }

    #[test]
    fn is_fresh_returns_true_for_valid_entry() {
        let mut cache = ResponseCache::new(Duration::from_secs(60));
        cache.insert("key", 42);
        assert!(cache.is_fresh("key"));
    }

    #[test]
    fn is_fresh_returns_false_for_expired_entry() {
        let mut cache = ResponseCache::new(Duration::from_millis(10));
        cache.insert("key", 42);
        thread::sleep(Duration::from_millis(20));
        assert!(!cache.is_fresh("key"));
    }

    #[test]
    fn is_fresh_returns_false_for_missing_key() {
        let cache: ResponseCache<i32> = ResponseCache::new(Duration::from_secs(60));
        assert!(!cache.is_fresh("missing"));
    }

    #[test]
    fn insert_replaces_existing_entry() {
        let mut cache = ResponseCache::new(Duration::from_secs(60));
        cache.insert("key", "old".to_string());
        cache.insert("key", "new".to_string());
        assert_eq!(cache.get("key"), Some(&"new".to_string()));
    }

    #[test]
    fn invalidate_removes_entry() {
        let mut cache = ResponseCache::new(Duration::from_secs(60));
        cache.insert("key", 1);
        cache.invalidate("key");
        assert!(cache.get("key").is_none());
        assert!(cache.get_stale("key").is_none());
    }

    #[test]
    fn invalidate_nonexistent_key_is_no_op() {
        let mut cache: ResponseCache<i32> = ResponseCache::new(Duration::from_secs(60));
        cache.invalidate("missing"); // should not panic
    }

    #[test]
    fn clear_removes_all_entries() {
        let mut cache = ResponseCache::new(Duration::from_secs(60));
        cache.insert("a", 1);
        cache.insert("b", 2);
        cache.clear();
        assert!(cache.get("a").is_none());
        assert!(cache.get("b").is_none());
    }

    #[test]
    fn multiple_keys_independent() {
        let mut cache = ResponseCache::new(Duration::from_secs(60));
        cache.insert("a", 1);
        cache.insert("b", 2);
        assert_eq!(cache.get("a"), Some(&1));
        assert_eq!(cache.get("b"), Some(&2));
        cache.invalidate("a");
        assert!(cache.get("a").is_none());
        assert_eq!(cache.get("b"), Some(&2));
    }

    #[test]
    fn insert_refreshes_ttl() {
        let mut cache = ResponseCache::new(Duration::from_millis(50));
        cache.insert("key", "v1".to_string());
        thread::sleep(Duration::from_millis(30));
        // Re-insert before expiry to refresh TTL
        cache.insert("key", "v2".to_string());
        thread::sleep(Duration::from_millis(30));
        // Should still be fresh because we re-inserted
        assert_eq!(cache.get("key"), Some(&"v2".to_string()));
    }
}
