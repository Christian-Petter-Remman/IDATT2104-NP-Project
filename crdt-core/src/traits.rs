use uuid::Uuid;

/// Unique identifier for a node in the P2P network.
pub type NodeId = Uuid;

/// Trait for state-based Conflict-free Replicated Data Types
///
/// All CRDTs should have four operations, where three of them is
/// implemented the same way for all (part of this trait).
/// - `value`: reads the current state.
/// - `compare`: check if self is a part of other replica.
/// - `merge`: merge another replica to self.
/// - `update`: type specific, and not part of this trait.
///
/// Requires `Clone` because `merge` consumes `other`, so callers
/// must be able to clone replicas to merge while keeping a copy.
pub trait Crdt: Clone {
    type Value;

    /// Query the current value of this CRDT
    ///
    /// This clones the internal state so the caller gets an
    /// independent copy. Modifying the returned value does not
    /// affect the CRDT.
    fn value(&self) -> Self::Value;

    /// Merge another replica into this one
    /// Guarantees convergence. The result is the same
    /// regardless of merge order, grouping, or repetition.
    ///
    /// Takes ownership of `other` because merge is a one-way absorption.
    /// After merging, `other`'s data lives inside `self` and `other` has
    /// no further purpose. Consuming it avoids unnecessary cloning and
    /// lets the compiler prevent accidental use of the stale replica.
    /// Callers who need `other` alive after merging can `.clone()` before calling.
    ///
    /// Merge order:
    /// `A.merge(B) == B.merge(A)`
    /// In P2P, you can't control which peer's state arrives first.
    ///
    /// Grouping:
    /// `A.merge(B).merge(C) == A.merge(B.merge(C))`
    /// It doesn't matter which two peers you merge first.
    ///
    /// Repetition
    /// `A.merge(A) == A`
    /// Network messages can be duplicated or retransmitted safely.
    fn merge(&mut self, other: Self);

    /// Returns `true` if self is a subset of other.
    /// Returns `true` if self is a subset of other.
    ///
    /// If `self.compare(other)` is `true`, then `self.merge(other)` would
    /// result in `other` (i.e. merging is not required for the other side).
    ///
    /// Borrows `other` because compare is a read-only check,
    /// both replicas remain unchanged and usable afterward.
    fn compare(&self, other: &Self) -> bool;
}
