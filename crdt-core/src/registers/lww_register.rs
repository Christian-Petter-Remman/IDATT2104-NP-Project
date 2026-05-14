use serde::{Deserialize, Serialize};

use crate::traits::{Crdt, NodeId};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct LWWRegister<T> {
    value: T,
    timestamp: u64,
    node_id: NodeId,
}

impl<T: Clone + PartialEq> LWWRegister<T> {
    pub fn new(value: T, timestamp: u64, node_id: NodeId) -> Self {
        Self { value, timestamp, node_id }
    }

    /// Returns true if `other` would replace `self` in a merge.
    fn is_superseded_by(&self, other: &Self) -> bool {
        other.timestamp > self.timestamp
            || (other.timestamp == self.timestamp && other.node_id > self.node_id)
    }

    /// Updates the register value if the new write wins.
    /// Returns `true` if the value was updated, `false` if
    /// the existing value has a higher or equal timestamp.
    pub fn set(&mut self, value: T, timestamp: u64, node_id: NodeId) -> bool {
        let incoming = LWWRegister::new(value, timestamp, node_id);
        if self.is_superseded_by(&incoming) {
            *self = incoming;
            return true;
        }
        false
    }

    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }
}

impl<T: Clone + PartialEq> Crdt for LWWRegister<T> {
    type Value = T;

    fn value(&self) -> T {
        self.value.clone()
    }

    /// Higher timestamp wins; equal timestamp -> higher `node_id` wins.
    fn merge(&mut self, other: Self) {
        if self.is_superseded_by(&other) {
            *self = other;
        }
    }

    /// Returns `true` if `self` is dominated by or equal to `other`.
    ///
    /// When `true`, merging `self` into `other` would not change `other`.
    /// For LWWRegister this means `other` has a higher timestamp,
    /// or equal timestamp with a higher or equal node_id.
    fn compare(&self, other: &Self) -> bool {
        !other.is_superseded_by(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use uuid::Uuid;

    fn n(id: u128) -> NodeId { Uuid::from_u128(id) }

    fn arb_lww() -> impl Strategy<Value = LWWRegister<u32>> {
        (0u32..=100u32, 0u64..=10u64, 0usize..3).prop_map(|(value, timestamp, idx)| {
            LWWRegister::new(value, timestamp, [n(1), n(2), n(3)][idx])
        })
    }

    #[test]
    fn lww_value_returns_initial() {
        let r = LWWRegister::new(42u32, 0, n(1));
        assert_eq!(r.value(), 42);
    }

    #[test]
    fn lww_merge_higher_timestamp_wins() {
        let a = LWWRegister::new(1u32, 10, n(1));
        let b = LWWRegister::new(2u32, 5, n(2));
        let mut r1 = a.clone();
        r1.merge(b.clone());
        assert_eq!(r1.value(), 1);
        let mut r2 = b.clone();
        r2.merge(a.clone());
        assert_eq!(r2.value(), 1);
    }

    #[test]
    fn lww_merge_equal_timestamp_higher_node_wins() {
        let a = LWWRegister::new(1u32, 5, n(2));
        let b = LWWRegister::new(2u32, 5, n(1));
        let mut a1 = a.clone();
        a1.merge(b.clone());
        let mut b1 = b.clone();
        b1.merge(a.clone());
        assert_eq!(a1, b1);
    }

    #[test]
    fn lww_set_updates_when_newer() {
        let mut r = LWWRegister::new(1u32, 1, n(1));
        r.set(99, 5, n(1));
        assert_eq!(r.value(), 99);
    }

    #[test]
    fn lww_set_ignores_older_write() {
        let mut r = LWWRegister::new(1u32, 10, n(1));
        r.set(99, 3, n(1));
        assert_eq!(r.value(), 1);
    }

    #[test]
    fn lww_compare_other_dominates() {
        let a = LWWRegister::new(1u32, 5, n(1));
        let b = LWWRegister::new(2u32, 10, n(1));
        assert!(a.compare(&b));
        assert!(!b.compare(&a));
    }

    proptest! {
        #[test]
        fn lww_commutative(a in arb_lww(), b in arb_lww()) {
            prop_assume!(
                a.node_id != b.node_id || a.timestamp != b.timestamp || a.value == b.value
            );
            let mut a1 = a.clone();
            a1.merge(b.clone());
            let mut b1 = b.clone();
            b1.merge(a.clone());
            prop_assert_eq!(a1, b1);
        }

        #[test]
        fn lww_associative(a in arb_lww(), b in arb_lww(), c in arb_lww()) {
            let mut ab = a.clone();
            ab.merge(b.clone());
            ab.merge(c.clone());
            let mut bc = b.clone();
            bc.merge(c.clone());
            let mut a2 = a.clone();
            a2.merge(bc);
            prop_assert_eq!(ab, a2);
        }

        #[test]
        fn lww_idempotent(a in arb_lww()) {
            let mut a1 = a.clone();
            a1.merge(a.clone());
            prop_assert_eq!(a1, a);
        }
    }
}
