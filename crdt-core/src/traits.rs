use uuid::Uuid;

/// Unique identifier for a node in the P2P network.
pub type NodeId = Uuid;

/// RGBA color as four bytes: (red, green, blue, alpha).
pub type Rgba = (u8, u8, u8, u8);

/// Core trait for all CRDT types.
///
/// Every implementation must satisfy three mathematical laws:
/// - **Commutativity**: `a.merge(&b) == b.merge(&a)`
/// - **Associativity**: `a.merge(&b).merge(&c) == a.merge(&b.merge(&c))`
/// - **Idempotency**: `a.merge(&a) == a`
///
/// These laws guarantee that nodes converge to the same state regardless of
/// the order or number of times gossip messages are received.
pub trait Crdt: Clone {
    /// The read-only view of this CRDT's current state.
    type Value;

    /// Returns the current logical value of this CRDT.
    fn value(&self) -> Self::Value;

    /// Merges `other` into `self`, returning the least upper bound of both
    /// states. Must not mutate either operand.
    fn merge(&self, other: &Self) -> Self;
}
