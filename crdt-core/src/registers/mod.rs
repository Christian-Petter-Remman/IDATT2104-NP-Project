use serde::{Deserialize, Serialize};
use crate::traits::{Crdt, NodeId};
use crate::clocks::{ClockOrder, VectorClock};

// ---------------------------------------------------------------------------
// LWWRegister
// ---------------------------------------------------------------------------

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

    /// Replace the stored value if the new (timestamp, node_id) wins.
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

// ---------------------------------------------------------------------------
// MVRegister
// ---------------------------------------------------------------------------

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

    /// Record a write. Removes entries causally dominated by `clock`.
    pub fn write(&mut self, value: T, clock: VectorClock) {
        // Drop entries that the new clock strictly dominates
        self.entries
            .retain(|(vc, _)| clock.partial_order(vc) != ClockOrder::After);
        // Only add if not itself dominated by a surviving entry
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
        // Union of entries from both sides, deduplicated
        let mut all: Vec<(VectorClock, T)> = Vec::new();
        for e in self.entries.iter().chain(other.entries.iter()) {
            if !all.contains(e) {
                all.push(e.clone());
            }
        }
        // Keep only entries not strictly dominated by any other entry
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use uuid::Uuid;

    fn n(id: u128) -> NodeId {
        Uuid::from_u128(id)
    }

    // -----------------------------------------------------------------------
    // LWWRegister helpers
    // -----------------------------------------------------------------------

    fn arb_node() -> impl Strategy<Value = NodeId> {
        prop::sample::select(vec![n(1), n(2), n(3)])
    }

    fn arb_lww() -> impl Strategy<Value = LWWRegister<u32>> {
        (0u32..=100u32, 0u64..=10u64, 0usize..3).prop_map(|(value, timestamp, idx)| {
            LWWRegister::new(value, timestamp, [n(1), n(2), n(3)][idx])
        })
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

    // -----------------------------------------------------------------------
    // LWWRegister unit tests
    // -----------------------------------------------------------------------

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
        let a = LWWRegister::new(1u32, 5, n(2)); // n(2) > n(1) in Uuid byte order
        let b = LWWRegister::new(2u32, 5, n(1));
        // Both should produce the same winner
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
            // Same (node, ts) must have same value — an invariant of correct usage.
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

    // -----------------------------------------------------------------------
    // MVRegister unit tests
    // -----------------------------------------------------------------------

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
        vc2.increment(n(1)); // vc2 > vc1

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
        vc_b.increment(n(2)); // concurrent with vc_a

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
        vc2.increment(n(1)); // vc2 > vc1

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
