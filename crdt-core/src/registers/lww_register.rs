use serde::{Deserialize, Serialize};
use crate::traits::{Crdt, NodeId};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LWWRegister<T> {
    value: T,
    timestamp: u64,
    node_id: NodeId,
}

impl<T: Clone + PartialEq + Serialize + for<'de> Deserialize<'de>> LWWRegister<T> {
    pub fn new(value: T, timestamp: u64, node_id: NodeId) -> Self {
        Self { value, timestamp, node_id }
    }

    pub fn set(&mut self, value: T, timestamp: u64, node_id: NodeId) {
        *self = self.clone().merge(&LWWRegister::new(value, timestamp, node_id));
    }
}

impl<T: Clone + PartialEq + Serialize + for<'de> Deserialize<'de>> Crdt for LWWRegister<T> {
    type Value = T;

    fn value(&self) -> T {
        self.value.clone()
    }

    fn merge(&self, other: &Self) -> Self {
        if self.timestamp > other.timestamp {
            self.clone()
        } else if other.timestamp > self.timestamp {
            other.clone()
        } else if self.node_id >= other.node_id {
            self.clone()
        } else {
            other.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use uuid::Uuid;

    fn n(id: u128) -> NodeId { Uuid::from_u128(id) }

    fn arb_node() -> impl Strategy<Value = NodeId> {
        prop::sample::select(vec![n(1), n(2), n(3)])
    }

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
        assert_eq!(a.merge(&b).value(), 1);
        assert_eq!(b.merge(&a).value(), 1);
    }

    #[test]
    fn lww_merge_equal_timestamp_higher_node_wins() {
        let a = LWWRegister::new(1u32, 5, n(2));
        let b = LWWRegister::new(2u32, 5, n(1));
        assert_eq!(a.merge(&b), b.merge(&a));
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

    proptest! {
        #[test]
        fn lww_commutative(a in arb_lww(), b in arb_lww()) {
            prop_assume!(
                a.node_id != b.node_id || a.timestamp != b.timestamp || a.value == b.value
            );
            prop_assert_eq!(a.merge(&b), b.merge(&a));
        }

        #[test]
        fn lww_associative(a in arb_lww(), b in arb_lww(), c in arb_lww()) {
            prop_assert_eq!(a.merge(&b).merge(&c), a.merge(&b.merge(&c)));
        }

        #[test]
        fn lww_idempotent(a in arb_lww()) {
            prop_assert_eq!(a.merge(&a.clone()), a);
        }
    }
}
