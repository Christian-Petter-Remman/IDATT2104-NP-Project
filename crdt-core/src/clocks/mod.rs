use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::traits::{Crdt, NodeId};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    pub(crate) clock: HashMap<NodeId, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockOrder {
    Before,
    After,
    Equal,
    Concurrent,
}

impl VectorClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn increment(&mut self, node: NodeId) {
        *self.clock.entry(node).or_insert(0) += 1;
    }

    pub fn get(&self, node: &NodeId) -> u64 {
        *self.clock.get(node).unwrap_or(&0)
    }

    pub fn lamport_timestamp(&self) -> u64 {
        self.clock.values().copied().max().unwrap_or(0)
    }

    pub fn partial_order(&self, other: &Self) -> ClockOrder {
        let self_dom = self.dominates(other);
        let other_dom = other.dominates(self);
        match (self_dom, other_dom) {
            (true, true) => ClockOrder::Equal,
            (true, false) => ClockOrder::After,
            (false, true) => ClockOrder::Before,
            (false, false) => ClockOrder::Concurrent,
        }
    }

    // self >= other component-wise
    fn dominates(&self, other: &Self) -> bool {
        other.clock.iter().all(|(k, v)| self.get(k) >= *v)
    }
}

impl Crdt for VectorClock {
    type Value = HashMap<NodeId, u64>;

    fn value(&self) -> Self::Value {
        self.clock.clone()
    }

    fn merge(&self, other: &Self) -> Self {
        let mut clock = self.clock.clone();
        for (k, v) in &other.clock {
            let e = clock.entry(*k).or_insert(0);
            *e = (*e).max(*v);
        }
        VectorClock { clock }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use uuid::Uuid;

    fn n(id: u128) -> NodeId {
        Uuid::from_u128(id)
    }

    fn arb_node() -> impl Strategy<Value = NodeId> {
        prop::sample::select(vec![n(1), n(2), n(3)])
    }

    fn arb_clock() -> impl Strategy<Value = VectorClock> {
        proptest::collection::hash_map(arb_node(), 1u64..=10u64, 0..=3)
            .prop_map(|clock| VectorClock { clock })
    }

    #[test]
    fn new_returns_zero_for_unknown_node() {
        let vc = VectorClock::new();
        assert_eq!(vc.get(&n(1)), 0);
    }

    #[test]
    fn increment_increases_own_component() {
        let mut vc = VectorClock::new();
        vc.increment(n(1));
        vc.increment(n(1));
        assert_eq!(vc.get(&n(1)), 2);
        assert_eq!(vc.get(&n(2)), 0);
    }

    #[test]
    fn merge_takes_element_wise_max() {
        let mut a = VectorClock::new();
        let mut b = VectorClock::new();
        a.increment(n(1));
        a.increment(n(1)); // n1=2
        b.increment(n(1)); // n1=1
        b.increment(n(2)); // n2=1
        let m = a.merge(&b);
        assert_eq!(m.get(&n(1)), 2);
        assert_eq!(m.get(&n(2)), 1);
    }

    #[test]
    fn partial_order_before() {
        let mut a = VectorClock::new();
        let mut b = VectorClock::new();
        a.increment(n(1));
        b.increment(n(1));
        b.increment(n(1));
        assert_eq!(a.partial_order(&b), ClockOrder::Before);
    }

    #[test]
    fn partial_order_after() {
        let mut a = VectorClock::new();
        let mut b = VectorClock::new();
        a.increment(n(1));
        a.increment(n(1));
        b.increment(n(1));
        assert_eq!(a.partial_order(&b), ClockOrder::After);
    }

    #[test]
    fn partial_order_equal() {
        let mut a = VectorClock::new();
        let mut b = VectorClock::new();
        a.increment(n(1));
        b.increment(n(1));
        assert_eq!(a.partial_order(&b), ClockOrder::Equal);
    }

    #[test]
    fn partial_order_concurrent() {
        let mut a = VectorClock::new();
        let mut b = VectorClock::new();
        a.increment(n(1));
        b.increment(n(2));
        assert_eq!(a.partial_order(&b), ClockOrder::Concurrent);
    }

    #[test]
    fn lamport_timestamp_is_max_component() {
        let mut vc = VectorClock::new();
        vc.increment(n(1));
        vc.increment(n(1)); // 2
        vc.increment(n(2)); // 1
        assert_eq!(vc.lamport_timestamp(), 2);
    }

    proptest! {
        #[test]
        fn commutative(a in arb_clock(), b in arb_clock()) {
            prop_assert_eq!(a.merge(&b), b.merge(&a));
        }

        #[test]
        fn associative(a in arb_clock(), b in arb_clock(), c in arb_clock()) {
            prop_assert_eq!(a.merge(&b).merge(&c), a.merge(&b.merge(&c)));
        }

        #[test]
        fn idempotent(a in arb_clock()) {
            prop_assert_eq!(a.merge(&a.clone()), a);
        }
    }
}
