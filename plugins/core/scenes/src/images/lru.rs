//! Least-recently-used map with a small fixed capacity.

use std::collections::VecDeque;

#[derive(Debug)]
pub struct Lru<V> {
    capacity: usize,
    entries: VecDeque<(usize, V)>,
}

impl<V> Lru<V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
        }
    }

    pub fn contains(&self, key: usize) -> bool {
        self.entries.iter().any(|(entry_key, _)| *entry_key == key)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Look up and mark as most recently used.
    pub fn get(&mut self, key: usize) -> Option<&V> {
        let position = self
            .entries
            .iter()
            .position(|(entry_key, _)| *entry_key == key)?;
        let entry = self.entries.remove(position)?;
        self.entries.push_back(entry);
        self.entries.back().map(|(_, value)| value)
    }

    /// Insert as most recently used, evicting the oldest entry when full.
    pub fn insert(&mut self, key: usize, value: V) {
        self.entries.retain(|(entry_key, _)| *entry_key != key);
        self.entries.push_back((key, value));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oldest_entry_goes_first_and_a_read_refreshes() {
        let mut lru = Lru::new(2);
        lru.insert(1, "a");
        lru.insert(2, "b");
        assert_eq!(lru.get(1), Some(&"a"));
        lru.insert(3, "c");
        assert!(lru.contains(1) && lru.contains(3));
        assert!(!lru.contains(2));
        assert_eq!(lru.len(), 2);
    }

    #[test]
    fn inserting_a_key_again_replaces_it_and_clear_empties() {
        let mut lru = Lru::new(3);
        lru.insert(1, "a");
        lru.insert(1, "b");
        assert_eq!(lru.len(), 1);
        assert_eq!(lru.get(1), Some(&"b"));
        lru.clear();
        assert_eq!(lru.len(), 0);
        assert_eq!(lru.get(1), None);
        assert_eq!(Lru::<u8>::new(0).capacity, 1);
    }
}
