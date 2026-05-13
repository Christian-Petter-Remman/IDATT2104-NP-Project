use std::collections::HashMap;
use crate::traits::{Crdt, NodeId};

/// Grow-only counter.
///
/// Each node tracks its own increment count. Value = sum of all nodes.
/// Merge = element-wise max. Never decreases.
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
