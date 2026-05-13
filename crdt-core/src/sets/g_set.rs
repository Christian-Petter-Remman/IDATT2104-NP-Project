use std::collections::HashSet;
use crate::traits::Crdt;

/// Grow-only set. Items can only be added, never removed.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GSet<T: Clone + Eq + std::hash::Hash> {
    items: HashSet<T>,
}

impl<T: Clone + Eq + std::hash::Hash> GSet<T> {
    pub fn new() -> Self {
        Self { items: HashSet::new() }
    }

    pub fn insert(&mut self, item: T) {
        self.items.insert(item);
    }

    pub fn contains(&self, item: &T) -> bool {
        self.items.contains(item)
    }
}

impl<T: Clone + Eq + std::hash::Hash> Default for GSet<T> {
    fn default() -> Self { Self::new() }
}

impl<T: Clone + Eq + std::hash::Hash> Crdt for GSet<T> {
    type Value = HashSet<T>;

    fn value(&self) -> HashSet<T> {
        self.items.clone()
    }

    /// Merge = union.
    fn merge(&mut self, other: Self) {
        for item in other.items {
            self.items.insert(item);
        }
    }

    /// True if self ⊆ other.
    fn compare(&self, other: &Self) -> bool {
        self.items.iter().all(|i| other.items.contains(i))
    }
}
