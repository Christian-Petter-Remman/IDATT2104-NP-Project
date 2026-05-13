//! Observed-Remove Set (ORSet) CRDT.
//!
//! Solves the permanent removal limitation of [`TwoPSet`] by
//! tagging each add operation with a unique identifier.
//! Concurrent add and remove of the same element results in
//! the element being present.
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use serde::{Serialize, Deserialize};
use crate::traits::{Crdt, NodeId};

/// Unique identifier for a single add operation.
/// 
/// The (node_id, seq) pair is guaranteed unique across all peers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tag {
    node_id: NodeId,
    seq: u64,
}

/// An observed-remove set with add-wins.
///
/// Each add creates a unique [`Tag`]. Remove only tombstones tags
/// that are currently visible. A concurrent add (with an unseen tag)
/// survives a concurrent remove.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ORSet<T>
where
    T: Eq + Hash + Clone,
{
    /// Map of active tags keeping it alive
    entries: HashMap<T, HashSet<Tag>>,
    /// Tags that have been removed, the tombstones
    removed_tags: HashSet<Tag>,
    /// Incremented on each insert to generate unique tags
    counter: u64,
}

/// Implementation with specific logic for re-adding items
/// 
/// Functions like this:
/// Peer A: adds "milk" -> tag (A,1)
/// Peer B: adds "milk" -> tag (B,1), then removes "milk" -> `removed_tags` {(B,1)}
///
/// After merge:
///   tag (A,1) was never tombstoned -> "milk" survives
///   tag (B,1) was tombstoned       -> dead
///   Result: {"milk"} with one active tag
impl<T> ORSet<T>
where
    T: Eq + Hash + Clone,
{
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            removed_tags: HashSet::new(),
            counter: 0
        }
    }

    pub fn insert(&mut self, element: T, node_id: &NodeId) {
        self.counter += 1;
        let tag = Tag {
            node_id: *node_id,
            seq: self.counter,
        };
        self.entries
            .entry(element)
            .or_insert_with(HashSet::new)
            .insert(tag);
    }

    pub fn remove(&mut self, element: &T) -> bool {
        if let Some(tags) = self.entries.remove(element) {
            self.removed_tags.extend(tags);
            return true;
        }
        return false;
    }

    pub fn contains(&self, element: &T) -> bool {
        self.entries.contains_key(element)
    }
}


/// Merge combines two replicas by keeping every tag that
/// neither side has tombstoned:
///
/// Self:   entries{"milk": {(A,1)}}  removed_tags: {}
/// Other:  entries{"milk": {(B,1)}}  removed_tags: {(A,1)}
///
/// Result: entries{"milk": {(B,1)}}  removed_tags: {(A,1)}
/// - (A,1) was tombstoned by other, removed
/// - (B,1) was not tombstoned, survives
/// - "milk" still in the set with one active tag
impl<T> Crdt for ORSet<T>
where
    T: Eq + Hash + Clone,
{
    type Value = HashSet<T>;

    /// Returns the set of elements that have at least one active tag.
    fn value(&self) -> Self::Value {
        self.entries.keys().cloned().collect()
    }

    /// Returns `true` if all active tags in `self` exist in `other`,
    /// and all tombstones in `self` exist in `other`.
    fn compare(&self, other: &Self) -> bool {
        for (element, tags) in &self.entries {
            match other.entries.get(element) {
                None => return false,
                Some(other_tags) => {
                    if !tags.is_subset(other_tags) {
                        return false;
                    }
                }
            }
        }
        self.removed_tags.is_subset(&other.removed_tags)
    }

    /// Merges another replica into this one.
    ///
    /// A tag survives if it exists in either replica and
    /// is not tombstoned (in `removed_tags`) by either replica.
    /// 
    /// Consist of five steps:
    /// 1: Combine both sides' removal knowledge.
    ///     We do this FIRST because we need the full picture
    ///     of what's been removed before deciding what survives.
    /// 2: Add tags from the other replica (to the `entries`), but only
    ///     if they haven't been removed by either side.
    /// 3: Clean our own tags against the newly learned removals from step 1.
    /// 4:  If an element has zero surviving tags, it's fully removed,
    ///     and we drop it from the map.
    /// 5: Take the higher counter so future inserts on this replica
    ///     don't accidentally reuse a seq number.
    fn merge(&mut self, other: Self) {
        self.removed_tags.extend(other.removed_tags.iter().cloned());

        for (element, other_tags) in other.entries {
            let local = self.entries
                .entry(element)
                .or_insert_with(HashSet::new);
            for tag in other_tags {
                if !self.removed_tags.contains(&tag) {
                    local.insert(tag);
                }
            }
        }

        for tags in self.entries.values_mut() {
            tags.retain(|tag| !self.removed_tags.contains(tag));
        }

        self.entries.retain(|_, tags| !tags.is_empty());

        self.counter = self.counter.max(other.counter);
    }
}