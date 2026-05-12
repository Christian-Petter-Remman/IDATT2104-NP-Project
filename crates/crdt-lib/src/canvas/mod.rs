use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use uuid::Uuid;

pub type Rgba = (u8, u8, u8, u8);
pub type Pixel = (u8, u8);
pub type NodeId = Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LWWRegister<T> {
    pub value: T,
    pub timestamp: u64,
    pub node_id: NodeId,
}

impl<T> LWWRegister<T> {
    pub fn new(value: T, timestamp: u64, node_id: NodeId) -> Self {
        Self {
            value,
            timestamp,
            node_id,
        }
    }

    pub fn value(&self) -> &T {
        &self.value
    }
}

impl<T: Clone> LWWRegister<T> {
    #[inline]
    pub fn merge(&self, other: &Self) -> Self {
        if (other.timestamp, other.node_id) > (self.timestamp, self.node_id) {
            other.clone()
        } else {
            self.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "T: Eq + Hash + Serialize",
    deserialize = "T: Eq + Hash + Deserialize<'de>"
))]
pub struct ORSet<T>
where
    T: Eq + Hash,
{
    pub entries: HashMap<T, HashSet<Uuid>>,
    pub tombstones: HashSet<Uuid>,
}

impl<T> Default for ORSet<T>
where
    T: Eq + Hash,
{
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            tombstones: HashSet::new(),
        }
    }
}

impl<T> ORSet<T>
where
    T: Eq + Hash + Clone,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, value: T, tag: Uuid) {
        self.entries.entry(value).or_default().insert(tag);
    }

    pub fn remove(&mut self, value: &T) {
        if let Some(tags) = self.entries.get(value) {
            self.tombstones.extend(tags.iter().copied());
        }
    }

    pub fn contains(&self, value: &T) -> bool {
        self.entries
            .get(value)
            .is_some_and(|tags| tags.iter().any(|tag| !self.tombstones.contains(tag)))
    }

    pub fn values(&self) -> Vec<T> {
        self.entries
            .keys()
            .filter(|value| self.contains(value))
            .cloned()
            .collect()
    }

    pub fn merge(&self, other: &Self) -> Self {
        let mut entries = self.entries.clone();
        for (value, tags) in &other.entries {
            entries
                .entry(value.clone())
                .or_default()
                .extend(tags.iter().copied());
        }

        let mut tombstones = self.tombstones.clone();
        tombstones.extend(other.tombstones.iter().copied());

        Self {
            entries,
            tombstones,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanvasDocument {
    pub pixels: HashMap<Pixel, LWWRegister<Rgba>>,
    pub users: ORSet<Uuid>,
    pub cursors: HashMap<Uuid, LWWRegister<Pixel>>,
}

impl CanvasDocument {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_pixel(&mut self, x: u8, y: u8, color: Rgba, timestamp: u64, node_id: NodeId) {
        self.pixels
            .insert((x, y), LWWRegister::new(color, timestamp, node_id));
    }

    pub fn pixel(&self, x: u8, y: u8) -> Option<Rgba> {
        self.pixels.get(&(x, y)).map(|register| register.value)
    }

    pub fn add_user(&mut self, user_id: Uuid, tag: Uuid) {
        self.users.insert(user_id, tag);
    }

    pub fn remove_user(&mut self, user_id: &Uuid) {
        self.users.remove(user_id);
        self.cursors.remove(user_id);
    }

    pub fn contains_user(&self, user_id: &Uuid) -> bool {
        self.users.contains(user_id)
    }

    pub fn set_cursor(&mut self, user_id: Uuid, x: u8, y: u8, timestamp: u64, node_id: NodeId) {
        self.cursors
            .insert(user_id, LWWRegister::new((x, y), timestamp, node_id));
    }

    pub fn cursor(&self, user_id: &Uuid) -> Option<Pixel> {
        self.cursors.get(user_id).map(|register| register.value)
    }

    pub fn merge(&self, other: &Self) -> Self {
        let mut pixels = self.pixels.clone();
        for (coord, register) in &other.pixels {
            pixels
                .entry(*coord)
                .and_modify(|existing| *existing = existing.merge(register))
                .or_insert_with(|| register.clone());
        }

        let mut cursors = self.cursors.clone();
        for (user_id, register) in &other.cursors {
            cursors
                .entry(*user_id)
                .and_modify(|existing| *existing = existing.merge(register))
                .or_insert_with(|| register.clone());
        }

        Self {
            pixels,
            users: self.users.merge(&other.users),
            cursors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_same_pixel_is_deterministic() {
        let node_a = Uuid::from_u128(1);
        let node_b = Uuid::from_u128(2);
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();

        a.set_pixel(4, 8, (255, 0, 0, 255), 10, node_a);
        b.set_pixel(4, 8, (0, 0, 255, 255), 10, node_b);

        assert_eq!(a.merge(&b), b.merge(&a));
        assert_eq!(a.merge(&b).pixel(4, 8), Some((0, 0, 255, 255)));
    }

    #[test]
    fn merge_combines_users_and_cursors() {
        let node = Uuid::from_u128(1);
        let user = Uuid::from_u128(2);
        let tag = Uuid::from_u128(3);
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();

        a.add_user(user, tag);
        b.set_cursor(user, 7, 9, 1, node);

        let merged = a.merge(&b);

        assert!(merged.contains_user(&user));
        assert_eq!(merged.cursor(&user), Some((7, 9)));
    }
}
