//! Shared test helpers.
//!
//! Cargo treats every `tests/*.rs` file as its own test binary, so utility
//! code that several tests need must live in a *subdirectory* (here:
//! `tests/common/mod.rs`) and be brought in via `mod common;` at the top
//! of each test file. Cargo does not compile this file as a standalone
//! test binary.
//!
//! Each test binary includes the whole module, but only references the
//! helpers it actually uses. `#[allow(dead_code)]` on the inherent
//! methods keeps the per-binary dead-code lint quiet without forcing
//! every test file to use every method.

#![allow(dead_code)]

use std::collections::BTreeMap;

use crdt_core::{Crdt, DeltaCrdt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Element-wise-max counter map, keyed by node UUID. Trivially
/// commutative, associative, and idempotent.
///
/// Used as a stand-in for `CanvasDocument` in transport-level tests. The
/// real composite CRDT lives in `crdt-core`; here we only need *any* type
/// that implements `Crdt`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MockCrdt {
    pub counts: BTreeMap<Uuid, u64>,
}

impl MockCrdt {
    pub fn bump(&mut self, who: Uuid) {
        *self.counts.entry(who).or_default() += 1;
    }

    pub fn total(&self) -> u64 {
        self.counts.values().sum()
    }
}

impl Crdt for MockCrdt {
    type Value = u64;

    fn value(&self) -> u64 {
        self.total()
    }

    fn merge(&mut self, other: Self) {
        for (k, v) in other.counts {
            let slot = self.counts.entry(k).or_default();
            if v > *slot {
                *slot = v;
            }
        }
    }

    fn compare(&self, other: &Self) -> bool {
        self.counts
            .iter()
            .all(|(k, v)| other.counts.get(k).is_some_and(|ov| v <= ov))
    }
}

impl DeltaCrdt for MockCrdt {
    /// Sparse map of entries whose counter exceeds the receiver's known
    /// value for that node.
    type Delta = BTreeMap<Uuid, u64>;
    type Version = BTreeMap<Uuid, u64>;

    fn version(&self) -> Self::Version {
        self.counts.clone()
    }

    fn delta_since(&self, since: &Self::Version) -> Self::Delta {
        self.counts
            .iter()
            .filter_map(|(k, &v)| {
                let known = since.get(k).copied().unwrap_or(0);
                (v > known).then_some((*k, v))
            })
            .collect()
    }

    fn merge_delta(&mut self, delta: Self::Delta) {
        for (k, v) in delta {
            let slot = self.counts.entry(k).or_default();
            if v > *slot {
                *slot = v;
            }
        }
    }

    fn is_empty_delta(delta: &Self::Delta) -> bool {
        delta.is_empty()
    }
}
