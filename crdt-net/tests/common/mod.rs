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

use crdt_core::Crdt;
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

    fn merge(&self, other: &Self) -> Self {
        let mut out = self.counts.clone();
        for (k, v) in &other.counts {
            let slot = out.entry(*k).or_default();
            if *v > *slot {
                *slot = *v;
            }
        }
        Self { counts: out }
    }
}
