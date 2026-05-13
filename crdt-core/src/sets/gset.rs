//! Grow-only Set (GSet) CRDT.
//!
//! The simplest set CRDT. Elements can be added but never removed.
//! Merge is set union. Serves as a building block for [`TwoPSet`].
use std::collections::HashSet;
use std::hash::Hash;
use serde::{Serialize, Deserialize};

use crate::traits::Crdt; 

/// A grow-only set where elements can be added but never removed.
/// Is a set union, guaranteeing convergence across peers
/// 
/// Peer A: {"milk", "bread"}
/// Peer B: {"milk", "eggs"}
/// will give us
/// Union:  {"milk", "bread", "eggs"}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GSet<T>
where
    T: Eq + Hash + Clone,
{
    elements: HashSet<T>,
}

impl<T> GSet<T>
where
    T: Eq + Hash + Clone,
{
    /// Creates an empty GSet.
    pub fn new() -> Self {
        Self {
            elements: HashSet::new(),
        }
    }

    /// Adds an element to the set. Duplicate inserts are ignored.
    pub fn insert(&mut self, element: T) -> bool {
        self.elements.insert(element)
    }

    /// Returns `true` if the set contains the element.
    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains(element)
    }
}

impl<T> Crdt for GSet<T>
where
    T: Eq + Hash + Clone,
{
    type Value = HashSet<T>;

    /// Returns the current set of elements.
    fn value(&self) -> Self::Value {
        self.elements.clone()
    }

    /// Merges another GSet into this one by taking the union.
    fn merge(&mut self, other: Self) {
        for element in other.elements {
            self.elements.insert(element);
        }
    }

    /// Returns `true` if every element in `self` is also in `other`.
    fn compare(&self, other: &Self) -> bool {
        self.elements.is_subset(&other.elements)
    }
}