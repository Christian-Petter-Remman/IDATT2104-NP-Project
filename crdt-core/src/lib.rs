//! Core CRDT traits and types.
//!
//! This crate intentionally has no I/O and no async. Concrete CRDTs and the
//! `CanvasDocument` composite live here; the gossip transport lives in
//! `crdt-net`, which is generic over any type implementing [`Crdt`].

/// State-based CRDT (CvRDT).
///
/// `merge` must be commutative, associative and idempotent:
///   - `a.merge(&b) == b.merge(&a)`
///   - `a.merge(&b).merge(&c) == a.merge(&b.merge(&c))`
///   - `a.merge(&a) == a`
pub trait Crdt: Clone {
    type Value;
    fn value(&self) -> Self::Value;
    fn merge(&self, other: &Self) -> Self;
}
