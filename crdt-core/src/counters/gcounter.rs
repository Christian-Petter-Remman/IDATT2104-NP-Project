//! Grow-only Counter (GCounter) CRDT.
//!
//! Each peer increments its own entry. The total count
//! is the sum across all peers. 
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use crate::traits::{Crdt, NodeId};

/// A counter that can only increase.
///
/// Peer A increments 3 times ->{A: 3}
/// Peer B increments 2 times → {B: 2}
/// After merge: {A: 3, B: 2} → value() = 5
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GCounter {
    counts: HashMap<NodeId, u64>,
}

impl GCounter {
    pub fn new() -> Self {
        Self {
            counts: HashMap::new(),
        }
    }

    /// Increments this peer's count by 1.
    pub fn increment(&mut self, node_id: &NodeId) {
        *self.counts.entry(*node_id).or_default() += 1;
    }
}

impl Crdt for GCounter {
    type Value = u64;

    /// Returns the total count across all peers.
    fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Returns `true` if every peer's count in `self`
    /// is less than or equal to that peer's count in `other`.
    fn compare(&self, other: &Self) -> bool {
        self.counts.iter().all(|(node, count)| {
            *count <= *other.counts.get(node).unwrap_or(&0)
        })
    }

    /// Merges by taking the max count per peer.
    fn merge(&mut self, other: Self) {
        for (node, count) in other.counts {
            let local = self.counts.entry(node).or_default();
            *local = (*local).max(count);
        }
    }
}