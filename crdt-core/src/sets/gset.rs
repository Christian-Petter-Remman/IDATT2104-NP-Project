use std::collections::HashSet;
use std::hash::Hash;
use serde::{Serialize, Deserialize};

use crate::traits::Crdt; 

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub fn new() -> Self {
        Self {
            elements: HashSet::new(),
        }
    }

    pub fn insert(&mut self, element: T) {
        self.elements.insert(element);
    }

    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains(element)
    }
}

impl<T> Crdt for GSet<T>
where
    T: Eq + Hash + Clone,
{
    type Value = HashSet<T>;

    fn value(&self) -> Self::Value {
        self.elements.clone()
    }

    fn merge(&mut self, other: Self) {
        for element in other.elements {
            self.elements.insert(element);
        }
    }
}