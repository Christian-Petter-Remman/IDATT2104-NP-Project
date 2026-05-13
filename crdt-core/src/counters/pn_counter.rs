use crate::traits::{Crdt, NodeId};
use super::g_counter::GCounter;

/// Positive-Negative counter.
///
/// Two GCounters: one for increments, one for decrements.
/// Value = increments.value() - decrements.value() (may be negative).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone)]
pub struct PNCounter {
    increments: GCounter,
    decrements: GCounter,
}

impl PNCounter {
    pub fn new() -> Self {
        Self {
            increments: GCounter::new(),
            decrements: GCounter::new(),
        }
    }

    pub fn increment(&mut self, node_id: NodeId) {
        self.increments.increment(node_id);
    }

    pub fn decrement(&mut self, node_id: NodeId) {
        self.decrements.increment(node_id);
    }
}

impl Default for PNCounter {
    fn default() -> Self { Self::new() }
}

impl Crdt for PNCounter {
    type Value = i64;

    fn value(&self) -> i64 {
        self.increments.value() as i64 - self.decrements.value() as i64
    }

    fn merge(&mut self, other: Self) {
        self.increments.merge(other.increments);
        self.decrements.merge(other.decrements);
    }

    fn compare(&self, other: &Self) -> bool {
        self.increments.compare(&other.increments)
            && self.decrements.compare(&other.decrements)
    }
}
