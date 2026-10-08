//! A small in-memory cache for the playback path (docs/design.md §2: `delivery` is stateless and
//! "caches in memory", ADR-0034). It remembers what is expensive to look up and cheap to be a
//! little stale on: where a recording's ladder lives and what its stored playlists say. Signed
//! URLs and tokens are never cached; they are made fresh for every request.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Mutex, PoisonError};

/// Entries live this long. Short, because a ladder can be rebuilt (a retry) and the new playlists
/// should be seen within a moment.
pub const TTL_S: i64 = 30;

/// Most entries held; when full, what has expired goes first, then everything (a cache that is
/// bounded and simple beats one that is clever).
pub const CAPACITY: usize = 4096;

pub struct TtlCache<K, V> {
    entries: Mutex<HashMap<K, (V, i64)>>,
    ttl_s: i64,
    capacity: usize,
}

impl<K: Eq + Hash, V: Clone> TtlCache<K, V> {
    pub fn new(ttl_s: i64, capacity: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl_s,
            capacity,
        }
    }

    /// The value stored for `key`, if it has not expired at `now` (unix seconds).
    pub fn get(&self, key: &K, now: i64) -> Option<V> {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries
            .get(key)
            .filter(|(_, expires_at)| *expires_at > now)
            .map(|(value, _)| value.clone())
    }

    pub fn insert(&self, key: K, value: V, now: i64) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() >= self.capacity && !entries.contains_key(&key) {
            entries.retain(|_, (_, expires_at)| *expires_at > now);
            if entries.len() >= self.capacity {
                entries.clear();
            }
        }
        entries.insert(key, (value, now + self.ttl_s));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_served_until_its_ttl_then_gone() {
        let cache = TtlCache::new(30, 8);
        cache.insert("k", "v".to_string(), 100);
        assert_eq!(cache.get(&"k", 100), Some("v".to_string()));
        assert_eq!(cache.get(&"k", 129), Some("v".to_string()));
        assert_eq!(cache.get(&"k", 130), None);
        assert_eq!(cache.get(&"other", 100), None);
    }

    #[test]
    fn inserting_again_renews_the_ttl() {
        let cache = TtlCache::new(30, 8);
        cache.insert("k", 1, 100);
        cache.insert("k", 2, 125);
        assert_eq!(cache.get(&"k", 140), Some(2));
    }

    #[test]
    fn it_never_holds_more_than_its_capacity() {
        let cache = TtlCache::new(30, 4);
        for i in 0..50 {
            cache.insert(i, i, 100);
            assert!(cache.len() <= 4, "{} entries", cache.len());
        }
        // The newest is always there.
        assert_eq!(cache.get(&49, 100), Some(49));
    }

    #[test]
    fn a_full_cache_drops_what_expired_before_anything_live() {
        let cache = TtlCache::new(30, 2);
        cache.insert("old", 1, 100);
        cache.insert("new", 2, 125);
        cache.insert("newer", 3, 131);
        assert_eq!(cache.get(&"old", 131), None);
        assert_eq!(cache.get(&"new", 131), Some(2));
        assert_eq!(cache.get(&"newer", 131), Some(3));
    }
}
