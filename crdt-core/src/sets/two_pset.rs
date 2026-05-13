//! Two-Phase Set (2PSet) CRDT.
//!
//! Extends [`GSet`] by allowing removal. Once removed, an element
//! can never be re-added. This limitation is solved by [`ORSet`].
use std::collections::HashSet;
use std::hash::Hash;
use serde::{Deserialize, Serialize};

use crate::traits::Crdt;
use super::gset::GSet;

/// A set that supports both add and remove, but removal is permanent.
///
/// Internally composed of two [`GSet`]s: one tracking additions,
/// one tracking removals. An element is in the set
/// if it has been added and not removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TwoPSet<T> 
where 
    T: Eq + Hash + Clone,
{
    added: GSet<T>,
    removed: GSet<T>,
}

impl<T> TwoPSet<T>
where  
    T: Eq + Hash + Clone,
{
    /// Creates an empty TwoPSet.
    pub fn new() -> Self {
        Self {
            added: GSet::new(),
            removed: GSet::new(),
        }
    }

    /// Adds an element to the set.
    ///
    /// If the element has been previously removed, the add is ignored.
    /// Removal is permanent in a TwoPSet.
    pub fn insert(&mut self, element: T) -> bool {
        if self.removed.contains(&element) {
            return false;
        }
        self.added.insert(element)
    }
        
    /// Removes an element from the set permanently.
    ///
    /// Returns `true` if the element was present before removal.
    /// Once removed, the element can never be re-added.
    pub fn remove(&mut self, element: T) -> bool {
        if self.added.contains(&element) {
            self.removed.insert(element);
            return true;
        } 
        return false;
    }

    /// Returns `true` if the element is in the set. 
    /// Meaning in added and not removed set.
    pub fn contains(&self, element: &T) -> bool {
        self.added.contains(element) && !self.removed.contains(element)
    }
}


impl<T> Crdt for TwoPSet<T>
where
    T: Eq + Hash + Clone,
{
    type Value = HashSet<T>;

    /// Returns the set of elements that have been added
    /// but not removed.
    fn value(&self) -> Self::Value {
        let added = self.added.value();
        let removed = self.removed.value();
        added.difference(&removed).cloned().collect()
    }

    /// Returns `true` if both the added and removed sets
    /// of `self` are subsets of `other`'s two sets.
    fn compare(&self, other: &Self) -> bool {
        self.added.compare(&other.added) && self.removed.compare(&other.removed)
    }

    /// Merges by merging both inner GSets independently.
    fn merge(&mut self, other: Self) {
        self.added.merge(other.added);
        self.removed.merge(other.removed);
    }
}