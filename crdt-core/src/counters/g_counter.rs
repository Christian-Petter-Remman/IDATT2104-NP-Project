use std::collections::HashMap;
use crate::traits::{Crdt, NodeId};

/// Grow-only counter.
///
/// Each node tracks its own increment count. Value = sum of all nodes.
/// Merge = element-wise max. Never decreases.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct GCounter {
    counts: HashMap<NodeId, u64>,
}

impl GCounter {
    pub fn new() -> Self {
        Self { counts: HashMap::new() }
    }

    /// Increment this node's count by 1.
    pub fn increment(&mut self, node_id: NodeId) {
        *self.counts.entry(node_id).or_insert(0) += 1;
    }

    pub fn get(&self, node_id: &NodeId) -> u64 {
        *self.counts.get(node_id).unwrap_or(&0)
    }
}

impl Default for GCounter {
    fn default() -> Self { Self::new() }
}

impl Crdt for GCounter {
    type Value = u64;

    /// Sum of all node counts.
    fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Element-wise max of each node's count.
    fn merge(&mut self, other: Self) {
        for (node, count) in other.counts {
            let e = self.counts.entry(node).or_insert(0);
            *e = (*e).max(count);
        }
    }

    /// True if every component of self ≤ other.
    fn compare(&self, other: &Self) -> bool {
        self.counts.iter().all(|(k, v)| *v <= other.get(k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn n(id: u128) -> NodeId { Uuid::from_u128(id) }

    #[test]
    fn new_starts_at_zero() {
        assert_eq!(GCounter::new().value(), 0);
    }

    #[test]
    fn get_unknown_node_returns_zero() {
        assert_eq!(GCounter::new().get(&n(1)), 0);
    }

    #[test]
    fn increment_increases_count() {
        let mut c = GCounter::new();
        c.increment(n(1));
        c.increment(n(1));
        assert_eq!(c.get(&n(1)), 2);
    }

    #[test]
    fn value_sums_all_nodes() {
        let mut c = GCounter::new();
        c.increment(n(1));
        c.increment(n(1));
        c.increment(n(2));
        assert_eq!(c.value(), 3);
    }

    #[test]
    fn merge_takes_element_wise_max() {
        let mut a = GCounter::new();
        a.increment(n(1));
        a.increment(n(1)); // n1=2
        let mut b = GCounter::new();
        b.increment(n(1)); // n1=1
        b.increment(n(2)); // n2=1
        a.merge(b);
        assert_eq!(a.get(&n(1)), 2);
        assert_eq!(a.get(&n(2)), 1);
        assert_eq!(a.value(), 3);
    }

    #[test]
    fn merge_commutativity() {
        let mut a = GCounter::new();
        a.increment(n(1));
        let mut b = GCounter::new();
        b.increment(n(2));
        let mut ab = a.clone(); ab.merge(b.clone());
        let mut ba = b.clone(); ba.merge(a.clone());
        assert_eq!(ab.value(), ba.value());
    }

    #[test]
    fn merge_idempotency() {
        let mut a = GCounter::new();
        a.increment(n(1));
        let before = a.value();
        a.merge(a.clone());
        assert_eq!(a.value(), before);
    }

    #[test]
    fn compare_subset() {
        let mut a = GCounter::new();
        a.increment(n(1));
        let mut b = GCounter::new();
        b.increment(n(1));
        b.increment(n(1));
        assert!(a.compare(&b));
        assert!(!b.compare(&a));
    }
}
