//! Bounded most-recently-used key ordering for fixed-size caches (the per-PR review
//! cache, pull-requests.md §11). Pure: it holds only the key order, never the cached
//! values — the owning cache drops whatever entry a `touch` eviction names.

/// Most-recently-used ordering over up to `capacity` keys. `touch` records a key as
/// most-recent; inserting a *new* key past `capacity` returns the least-recently-used
/// key the caller must evict from its store.
pub struct LruOrder<K> {
    capacity: usize,
    /// Least-recently used first, most-recently used last.
    order: Vec<K>,
}

impl<K: PartialEq + Clone> LruOrder<K> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: Vec::new(),
        }
    }

    /// Mark `key` as most-recently used. Re-touching a known key only reorders it
    /// (never evicts); inserting a new key past `capacity` returns the evicted
    /// least-recently-used key so the caller can drop its cached value.
    pub fn touch(&mut self, key: K) -> Option<K> {
        if let Some(pos) = self.order.iter().position(|k| *k == key) {
            let key = self.order.remove(pos);
            self.order.push(key);
            return None;
        }
        self.order.push(key);
        (self.order.len() > self.capacity).then(|| self.order.remove(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_does_not_evict_until_capacity_is_exceeded() {
        let mut lru = LruOrder::new(3);
        assert_eq!(lru.touch(1), None);
        assert_eq!(lru.touch(2), None);
        assert_eq!(lru.touch(3), None);
    }

    #[test]
    fn touch_evicts_the_least_recently_used_key() {
        let mut lru = LruOrder::new(3);
        lru.touch(1);
        lru.touch(2);
        lru.touch(3);
        // 1 is the oldest, so a fourth key evicts it.
        assert_eq!(lru.touch(4), Some(1));
        // Now 2 is the oldest.
        assert_eq!(lru.touch(5), Some(2));
    }

    #[test]
    fn re_touching_a_key_moves_it_to_most_recent_and_never_evicts() {
        let mut lru = LruOrder::new(3);
        lru.touch(1);
        lru.touch(2);
        lru.touch(3);
        // Re-touch the oldest: no eviction, and it is now the most recent.
        assert_eq!(lru.touch(1), None);
        // So the next insert evicts 2 (the new oldest), not 1.
        assert_eq!(lru.touch(4), Some(2));
    }

    #[test]
    fn capacity_is_clamped_to_at_least_one() {
        let mut lru = LruOrder::new(0);
        assert_eq!(lru.touch("a"), None);
        assert_eq!(lru.touch("b"), Some("a"));
    }
}
