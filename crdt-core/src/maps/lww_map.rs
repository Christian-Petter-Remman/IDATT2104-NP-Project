//! Last-Writer-Wins Map (LWWMap) CRDT.
//!
//! A key-value map where each key holds an [`LWWRegister`].
//! Concurrent writes to the same key are resolved by highest
//! timestamp, with node_id as tiebreaker.
//! Internally composed of [`LWWRegister`]s, same pattern
//! as [`TwoPSet`] composing two [`GSet`]s.
use std::collections::HashMap;
use std::hash::Hash;
use serde::{Serialize, Deserialize};
use crate::traits::{Crdt, NodeId};
use crate::registers::LWWRegister;

/// A map where each key independently resolves conflicts
/// using last-writer-wins.
///
/// Peer A: sets "x" = 5 at time 3
/// Peer B: sets "x" = 9 at time 7
/// After merge: "x" = 9 (higher timestamp wins)
///
/// Keys are created on first write and never removed.
/// Key removal would require ORSet-style tracking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LWWMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone + PartialEq,
{
    entries: HashMap<K, LWWRegister<V>>,
}

impl<K, V> Default for LWWMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone + PartialEq,
{
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

/// Update operations for LWWMap.
///
/// The map itself has no concept of time or identity, it
/// forwards the timestamp and node_id to the underlying
/// [`LWWRegister`]` for each key. This means the caller
/// is responsible for generating timestamps (can be done with [`VectorClock`])
///
/// Keys are created on first write and never removed.
impl<K, V> LWWMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone + PartialEq,
{
    pub fn new() -> Self {
        Self::default()
    }


    /// Sets a key to a value at the given timestamp.
    ///
    /// If the key already exists, the write is forwarded to its
    /// LWWRegister, which only accepts it if the timestamp is
    /// higher (or equal with a higher node_id). Stale writes
    /// are silently ignored.
    ///
    /// If the key is new, a fresh LWWRegister is created.
    pub fn set(&mut self, key: K, value: V, timestamp: u64, node_id: NodeId) -> bool {
        match self.entries.get_mut(&key) {
            Some(register) => register.set(value, timestamp, node_id),
            None => {
                self.entries.insert(key, LWWRegister::new(value, timestamp, node_id));
                true
            }
        }
    }

    /// Returns the current value for a key, if it exists.
    pub fn get(&self, key: &K) -> Option<V> {
        self.entries.get(key).map(|r| r.value())
    }

    /// Returns true if the key exists in the map.
    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }
}

impl<K, V> Crdt for LWWMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone + PartialEq,
{
    type Value = HashMap<K, V>;

    /// Returns a snapshot of all keys and their current values.
    fn value(&self) -> Self::Value {
        self.entries.iter()
            .map(|(k, r)| (k.clone(), r.value()))
            .collect()
    }

    /// Returns `true` if every key in `self` exists in `other`
    /// and each register is dominated by or equal to other's.
    fn compare(&self, other: &Self) -> bool {
        self.entries.iter().all(|(key, reg)| {
            match other.entries.get(key) {
                Some(other_reg) => reg.compare(other_reg),
                None => false,
            }
        })
    }

    /// Merges by merging each register independently.
    /// Keys only in `other` are added. Keys only in `self` stay.
    fn merge(&mut self, other: Self) {
        for (key, other_reg) in other.entries {
            match self.entries.get_mut(&key) {
                Some(local_reg) => local_reg.merge(other_reg),
                None => { self.entries.insert(key, other_reg); }
            }
        }
    }
}