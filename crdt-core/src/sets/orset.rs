//! Observed-Remove Set (ORSet) CRDT.
//!
//! Solves the permanent removal limitation of [`TwoPSet`] by
//! tagging each add operation with a unique identifier.
//! Concurrent add and remove of the same element results in
//! the element being present.
use crate::traits::{Crdt, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Unique identifier for a single add operation.
///
/// The (node_id, seq) pair is guaranteed unique across all peers.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    node_id: NodeId,
    seq: u64,
}

/// An observed-remove set with add-wins.
///
/// Each add creates a unique [`Tag`]. Remove only tombstones tags
/// that are currently visible. A concurrent add (with an unseen tag)
/// survives a concurrent remove.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct ORSet<T>
where
    T: Eq + Hash + Clone,
{
    /// Map of active tags keeping it alive
    entries: HashMap<T, HashSet<Tag>>,
    /// Tags that have been removed, the tombstones
    /// Grows unbounded. ideally should use a GC strategy.
    removed_tags: HashSet<Tag>,
    /// Incremented on each insert to generate unique tags
    counter: u64,
}

impl<T> Default for ORSet<T>
where
    T: Eq + Hash + Clone,
{
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            removed_tags: HashSet::new(),
            counter: 0,
        }
    }
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
        Self::default()
    }

    pub fn insert(&mut self, element: T, node_id: &NodeId) {
        self.counter += 1;
        let tag = Tag {
            node_id: *node_id,
            seq: self.counter,
        };
        self.entries.entry(element).or_default().insert(tag);
    }

    pub fn remove(&mut self, element: &T) -> bool {
        if let Some(tags) = self.entries.remove(element) {
            self.removed_tags.extend(tags);
            return true;
        }
        false
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
            let local = self.entries.entry(element).or_default();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{Crdt, NodeId};
    use proptest::collection::vec as prop_vec;
    use proptest::prelude::*;
    use uuid::Uuid;

    fn node(n: u128) -> NodeId {
        Uuid::from_u128(n)
    }

    fn arb_orset() -> impl Strategy<Value = ORSet<u8>> {
        prop_vec((0u8..=3u8, 0usize..3usize, proptest::bool::ANY), 0..10).prop_map(|ops| {
            let nodes = [node(1), node(2), node(3)];
            let mut set = ORSet::new();
            for (elem, node_idx, is_remove) in ops {
                if is_remove {
                    set.remove(&elem);
                } else {
                    set.insert(elem, &nodes[node_idx]);
                }
            }
            set
        })
    }

    #[test]
    fn insert_and_contains() {
        let mut set = ORSet::new();
        let a = node(1);
        assert!(!set.contains(&"milk"));
        set.insert("milk", &a);
        assert!(set.contains(&"milk"));
    }

    #[test]
    fn remove_and_read() {
        let mut set = ORSet::new();
        let a = node(1);
        set.insert("milk", &a);
        set.remove(&"milk");
        assert!(!set.contains(&"milk"));

        // re-add works unlike in TwoPSet
        set.insert("milk", &a);
        assert!(set.contains(&"milk"));
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let mut set: ORSet<&str> = ORSet::new();
        assert!(!set.remove(&"milk"));
    }

    #[test]
    fn value_returns_active_elements() {
        let mut set = ORSet::new();
        let a = node(1);
        set.insert("milk", &a);
        set.insert("eggs", &a);
        set.remove(&"milk");
        assert_eq!(set.value(), HashSet::from(["eggs"]));
    }

    #[test]
    fn concurrent_add_and_remove_add_wins() {
        // Peer A adds "milk", peer B independently adds then removes "milk"
        // After merge: "milk" survives because A's tag was never tombstoned
        let a = node(1);
        let b = node(2);

        let mut peer_a = ORSet::new();
        peer_a.insert("milk", &a);

        let mut peer_b = ORSet::new();
        peer_b.insert("milk", &b);
        peer_b.remove(&"milk");

        peer_a.merge(peer_b);
        assert!(peer_a.contains(&"milk")); // add wins, not removed like in 2P
    }

    #[test]
    fn merge_commutativity() {
        let a = node(1);
        let b = node(2);

        let mut peer_a = ORSet::new();
        peer_a.insert("milk", &a);
        peer_a.insert("bread", &a);

        let mut peer_b = ORSet::new();
        peer_b.insert("eggs", &b);
        peer_b.insert("milk", &b);
        peer_b.remove(&"milk");

        let mut ab = peer_a.clone();
        ab.merge(peer_b.clone());
        let mut ba = peer_b.clone();
        ba.merge(peer_a.clone());

        assert_eq!(ab.value(), ba.value());
    }

    #[test]
    fn merge_idempotency() {
        let mut set = ORSet::new();
        let a = node(1);
        set.insert("milk", &a);
        set.insert("eggs", &a);
        set.remove(&"milk");
        let before = set.value();
        set.merge(set.clone());
        assert_eq!(set.value(), before);
    }

    #[test]
    fn merge_associativity() {
        let a = node(1);
        let b = node(2);
        let c = node(3);

        let mut peer_a = ORSet::new();
        peer_a.insert("milk", &a);
        let mut peer_b = ORSet::new();
        peer_b.insert("eggs", &b);
        let mut peer_c = ORSet::new();
        peer_c.insert("bread", &c);

        // (A merge B) merge C
        let mut ab_c = peer_a.clone();
        ab_c.merge(peer_b.clone());
        ab_c.merge(peer_c.clone());

        // A merge (B merge C)
        let mut a_bc = peer_a.clone();
        let mut bc = peer_b.clone();
        bc.merge(peer_c.clone());
        a_bc.merge(bc);

        assert_eq!(ab_c.value(), a_bc.value());
    }

    #[test]
    fn compare_subset() {
        let a = node(1);
        let b = node(2);

        let mut small = ORSet::new();
        small.insert("milk", &a);

        let mut big = ORSet::new();
        big.insert("milk", &a);
        big.insert("eggs", &b);

        assert!(small.compare(&big));
        assert!(!big.compare(&small));
    }
    proptest! {
        #[test]
        fn orset_commutative(a in arb_orset(), b in arb_orset()) {
            let mut ab = a.clone();
            ab.merge(b.clone());
            let mut ba = b.clone();
            ba.merge(a.clone());
            prop_assert_eq!(ab.value(), ba.value());
        }

        #[test]
        fn orset_idempotent(a in arb_orset()) {
            let mut a1 = a.clone();
            a1.merge(a.clone());
            prop_assert_eq!(a1.value(), a.value());
        }

        #[test]
        fn orset_associative(a in arb_orset(), b in arb_orset(), c in arb_orset()) {
            let mut ab_c = a.clone();
            ab_c.merge(b.clone());
            ab_c.merge(c.clone());

            let mut bc = b.clone();
            bc.merge(c.clone());
            let mut a_bc = a.clone();
            a_bc.merge(bc);

            prop_assert_eq!(ab_c.value(), a_bc.value());
        }
    }
}
