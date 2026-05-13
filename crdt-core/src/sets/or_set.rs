use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use crate::traits::Crdt;

/// Observed-Remove Set.
///
/// Supports add and remove with concurrent-add-wins semantics:
/// if node A adds an item while node B removes it concurrently,
/// after merge the item is present (A's new tag is not tombstoned).
///
/// Internal state:
/// - `tags: HashMap<T, HashSet<Uuid>>` — each item maps to its unique add-tags
/// - `tombstones: HashSet<Uuid>`       — removed tags
///
/// `contains(item)` = item has at least one tag NOT in tombstones.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone)]
pub struct ORSet<T: Clone + Eq + std::hash::Hash> {
    tags: HashMap<T, HashSet<Uuid>>,
    tombstones: HashSet<Uuid>,
}

impl<T: Clone + Eq + std::hash::Hash> ORSet<T> {
    pub fn new() -> Self {
        Self {
            tags: HashMap::new(),
            tombstones: HashSet::new(),
        }
    }

    /// Assign a fresh unique tag to item and record it.
    pub fn insert(&mut self, item: T) {
        let tag = Uuid::new_v4();
        self.tags.entry(item).or_default().insert(tag);
    }

    /// Move all current tags for item into tombstones.
    pub fn remove(&mut self, item: &T) {
        if let Some(item_tags) = self.tags.get(item) {
            for tag in item_tags {
                self.tombstones.insert(*tag);
            }
        }
    }

    /// Item is present if it has at least one tag not in tombstones.
    pub fn contains(&self, item: &T) -> bool {
        self.tags
            .get(item)
            .map_or(false, |ts| ts.iter().any(|t| !self.tombstones.contains(t)))
    }
}

impl<T: Clone + Eq + std::hash::Hash> Default for ORSet<T> {
    fn default() -> Self { Self::new() }
}

impl<T: Clone + Eq + std::hash::Hash> Crdt for ORSet<T> {
    type Value = Vec<T>;

    fn value(&self) -> Vec<T> {
        self.tags
            .keys()
            .filter(|item| self.contains(item))
            .cloned()
            .collect()
    }

    /// Union tag maps + union tombstone sets.
    fn merge(&mut self, other: Self) {
        for (item, other_tags) in other.tags {
            self.tags.entry(item).or_default().extend(other_tags);
        }
        self.tombstones.extend(other.tombstones);
    }

    /// True if every live tag in self is also in other's tags or tombstoned.
    fn compare(&self, other: &Self) -> bool {
        self.tags.iter().all(|(item, tags)| {
            tags.iter().all(|tag| {
                other.tombstones.contains(tag)
                    || other.tags.get(item).map_or(false, |ot| ot.contains(tag))
            })
        })
    }
}
