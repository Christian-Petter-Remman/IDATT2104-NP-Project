use uuid::Uuid;

/// Unique identifier for a node in the P2P network.
pub type NodeId = Uuid;

/// Trait for state-based Conflict-free Replicated Data Types
///
/// All CRDTs should have four operations, where three of them is
/// implemented the same way for all (part of this trait).
/// - value: reads the current state.
/// - compare: check if self is a part of other replica.
/// - merge: merge another replica to self.
/// - update: type specific, and not part of this trait.
pub trait Crdt {
    type Value;

    /// Query the current value of this CRDT.
    ///
    /// Clones internal state so the caller gets an independent copy.
    fn value(&self) -> Self::Value;

    /// Merge another replica into this one.
    ///
    /// Guarantees convergence — result is the same regardless of
    /// merge order, grouping, or repetition.
    fn merge(&mut self, other: Self);

    /// Returns true if self is a subset of other.
    ///
    /// If `self.compare(other)` is true, then `self.merge(other)`
    /// would result in other (merging is not required for the other side).
    fn compare(&self, other: &Self) -> bool;
}
