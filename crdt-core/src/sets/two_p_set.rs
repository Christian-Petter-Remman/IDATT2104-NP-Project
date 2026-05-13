use crate::traits::Crdt;
use super::g_set::GSet;

/// Two-Phase Set. Items can be added and removed, but removal is permanent.
///
/// Internally: `added: GSet<T>` + `tombstoned: GSet<T>`.
/// `contains` = in added AND NOT in tombstoned.
/// Once tombstoned, an item cannot be re-added.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TwoPSet<T: Clone + Eq + std::hash::Hash> {
    added: GSet<T>,
    tombstoned: GSet<T>,
}

impl<T: Clone + Eq + std::hash::Hash> TwoPSet<T> {
    pub fn new() -> Self {
        Self {
            added: GSet::new(),
            tombstoned: GSet::new(),
        }
    }

    pub fn insert(&mut self, item: T) {
        self.added.insert(item);
    }

    pub fn remove(&mut self, item: T) {
        self.tombstoned.insert(item);
    }

    pub fn contains(&self, item: &T) -> bool {
        self.added.contains(item) && !self.tombstoned.contains(item)
    }
}

impl<T: Clone + Eq + std::hash::Hash> Default for TwoPSet<T> {
    fn default() -> Self { Self::new() }
}

impl<T: Clone + Eq + std::hash::Hash> Crdt for TwoPSet<T> {
    type Value = Vec<T>;

    fn value(&self) -> Vec<T> {
        self.added.value()
            .into_iter()
            .filter(|i| !self.tombstoned.contains(i))
            .collect()
    }

    fn merge(&mut self, other: Self) {
        self.added.merge(other.added);
        self.tombstoned.merge(other.tombstoned);
    }

    fn compare(&self, other: &Self) -> bool {
        self.added.compare(&other.added) && self.tombstoned.compare(&other.tombstoned)
    }
}
