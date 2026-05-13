use serde::{Deserialize, Serialize};
use crate::traits::Crdt;
use crate::clocks::{ClockOrder, VectorClock};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MVRegister<T> {
    entries: Vec<(VectorClock, T)>,
}

impl<T: Clone + PartialEq + Serialize + for<'de> Deserialize<'de>> PartialEq for MVRegister<T> {
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self.entries.iter().all(|e| other.entries.contains(e))
    }
}

impl<T: Clone + PartialEq + Serialize + for<'de> Deserialize<'de>> Default for MVRegister<T> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

impl<T: Clone + PartialEq + Serialize + for<'de> Deserialize<'de>> MVRegister<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write(&mut self, value: T, clock: VectorClock) {
        self.entries
            .retain(|(vc, _)| clock.partial_order(vc) != ClockOrder::After);
        let dominated = self
            .entries
            .iter()
            .any(|(vc, _)| vc.partial_order(&clock) == ClockOrder::After);
        if !dominated {
            self.entries.push((clock, value));
        }
    }
}

impl<T: Clone + PartialEq + Serialize + for<'de> Deserialize<'de>> Crdt for MVRegister<T> {
    type Value = Vec<T>;

    fn value(&self) -> Vec<T> {
        self.entries.iter().map(|(_, v)| v.clone()).collect()
    }

    fn merge(&self, other: &Self) -> Self {
        let mut all: Vec<(VectorClock, T)> = Vec::new();
        for e in self.entries.iter().chain(other.entries.iter()) {
            if !all.contains(e) {
                all.push(e.clone());
            }
        }
        let entries = all
            .iter()
            .filter(|(vc, _)| {
                !all.iter()
                    .any(|(ovc, _)| ovc != vc && ovc.partial_order(vc) == ClockOrder::After)
            })
            .cloned()
            .collect();
        MVRegister { entries }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use uuid::Uuid;
    use crate::traits::NodeId;

    fn n(id: u128) -> NodeId { Uuid::from_u128(id) }

    fn arb_node() -> impl Strategy<Value = NodeId> {
        prop::sample::select(vec![n(1), n(2), n(3)])
    }

    fn arb_clock() -> impl Strategy<Value = VectorClock> {
        proptest::collection::hash_map(arb_node(), 1u64..=10u64, 0..=3)
            .prop_map(|clock| VectorClock { clock })
    }

    fn arb_mv() -> impl Strategy<Value = MVRegister<u32>> {
        proptest::collection::vec((arb_clock(), 0u32..=100u32), 0..=4).prop_map(|writes| {
            let mut reg = MVRegister::new();
            for (clock, val) in writes {
                reg.write(val, clock);
            }
            reg
        })
    }

    #[test]
    fn mv_new_is_empty() {
        let r: MVRegister<u32> = MVRegister::new();
        assert!(r.value().is_empty());
    }

    #[test]
    fn mv_single_write_returns_one_value() {
        let mut r = MVRegister::new();
        let mut vc = VectorClock::new();
        vc.increment(n(1));
        r.write(42u32, vc);
        assert_eq!(r.value(), vec![42]);
    }

    #[test]
    fn mv_sequential_write_keeps_latest() {
        let mut r = MVRegister::new();
        let mut vc1 = VectorClock::new();
        vc1.increment(n(1));
        let mut vc2 = vc1.clone();
        vc2.increment(n(1));
        r.write(1u32, vc1);
        r.write(2u32, vc2);
        assert_eq!(r.value(), vec![2]);
    }

    #[test]
    fn mv_concurrent_writes_keeps_both() {
        let mut r = MVRegister::new();
        let mut vc_a = VectorClock::new();
        vc_a.increment(n(1));
        let mut vc_b = VectorClock::new();
        vc_b.increment(n(2));
        r.write(1u32, vc_a);
        r.write(2u32, vc_b);
        let mut vals = r.value();
        vals.sort();
        assert_eq!(vals, vec![1, 2]);
    }

    #[test]
    fn mv_merge_unions_concurrent_entries() {
        let mut vc_a = VectorClock::new();
        vc_a.increment(n(1));
        let mut vc_b = VectorClock::new();
        vc_b.increment(n(2));
        let mut ra = MVRegister::new();
        ra.write(1u32, vc_a.clone());
        let mut rb = MVRegister::new();
        rb.write(2u32, vc_b.clone());
        let merged = ra.merge(&rb);
        let mut vals = merged.value();
        vals.sort();
        assert_eq!(vals, vec![1, 2]);
    }

    #[test]
    fn mv_merge_discards_dominated_entries() {
        let mut vc1 = VectorClock::new();
        vc1.increment(n(1));
        let mut vc2 = vc1.clone();
        vc2.increment(n(1));
        let mut ra = MVRegister::new();
        ra.write(1u32, vc1);
        let mut rb = MVRegister::new();
        rb.write(2u32, vc2);
        let merged = ra.merge(&rb);
        assert_eq!(merged.value(), vec![2]);
    }

    proptest! {
        #[test]
        fn mv_commutative(a in arb_mv(), b in arb_mv()) {
            prop_assert_eq!(a.merge(&b), b.merge(&a));
        }

        #[test]
        fn mv_associative(a in arb_mv(), b in arb_mv(), c in arb_mv()) {
            prop_assert_eq!(a.merge(&b).merge(&c), a.merge(&b.merge(&c)));
        }

        #[test]
        fn mv_idempotent(a in arb_mv()) {
            prop_assert_eq!(a.merge(&a.clone()), a);
        }
    }
}
