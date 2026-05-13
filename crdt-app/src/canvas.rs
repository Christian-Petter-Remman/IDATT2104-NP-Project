use std::collections::HashMap;
use std::collections::hash_map::Entry;
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use crdt_core::traits::{Crdt, NodeId};
use crdt_core::registers::lww_register::LWWRegister;

pub type Rgba = (u8, u8, u8, u8);
pub const DEFAULT_PIXEL: Rgba = (255, 255, 255, 255);

// PLACEHOLDER: replace with ORSet<Uuid> once sets module is merged.
// ORSet required for concurrent-add-wins semantics on user membership.
// Swap: use crdt_core::sets::or_set::ORSet; + change field type + update
// add_user/remove_user/merge/compare to use ORSet API.
type UserSet = std::collections::HashSet<Uuid>;

/// Composite CRDT — the shared state gossiped between nodes.
///
/// Pixels + cursors use LWWRegister (last-writer-wins).
/// Users use ORSet (concurrent-add-wins) — placeholder until sets merge.
#[derive(Clone, Serialize, Deserialize)]
pub struct CanvasDocument {
    pub pixels: HashMap<(u8, u8), LWWRegister<Rgba>>,
    users: UserSet,
    pub cursors: HashMap<Uuid, LWWRegister<(u8, u8)>>,
}

impl Default for CanvasDocument {
    fn default() -> Self {
        Self {
            pixels: HashMap::new(),
            users: UserSet::new(),
            cursors: HashMap::new(),
        }
    }
}

impl CanvasDocument {
    pub fn new() -> Self { Self::default() }

    pub fn paint(&mut self, x: u8, y: u8, color: Rgba, node_id: NodeId, timestamp: u64) {
        self.pixels
            .entry((x, y))
            .or_insert_with(|| LWWRegister::new(DEFAULT_PIXEL, 0, node_id))
            .set(color, timestamp, node_id);
    }

    pub fn get_pixel(&self, x: u8, y: u8) -> Rgba {
        self.pixels.get(&(x, y)).map(|r| r.value()).unwrap_or(DEFAULT_PIXEL)
    }

    pub fn update_cursor(&mut self, user_id: Uuid, pos: (u8, u8), timestamp: u64) {
        self.cursors
            .entry(user_id)
            .or_insert_with(|| LWWRegister::new((0, 0), 0, user_id))
            .set(pos, timestamp, user_id);
    }

    pub fn add_user(&mut self, user_id: Uuid) { self.users.insert(user_id); }
    pub fn remove_user(&mut self, user_id: &Uuid) { self.users.remove(user_id); }
    pub fn active_users(&self) -> Vec<Uuid> { self.users.iter().copied().collect() }
}

impl Crdt for CanvasDocument {
    type Value = Self;

    fn value(&self) -> Self { self.clone() }

    fn merge(&mut self, other: Self) {
        for ((x, y), reg) in other.pixels {
            match self.pixels.entry((x, y)) {
                Entry::Occupied(mut e) => e.get_mut().merge(reg),
                Entry::Vacant(e) => { e.insert(reg); }
            }
        }
        // TODO: replace with ORSet::merge once sets module merges
        for user in other.users {
            self.users.insert(user);
        }
        for (uid, reg) in other.cursors {
            match self.cursors.entry(uid) {
                Entry::Occupied(mut e) => e.get_mut().merge(reg),
                Entry::Vacant(e) => { e.insert(reg); }
            }
        }
    }

    fn compare(&self, other: &Self) -> bool {
        self.pixels.iter().all(|(k, r)| other.pixels.get(k).map_or(false, |o| r.compare(o)))
            && self.cursors.iter().all(|(k, r)| other.cursors.get(k).map_or(false, |o| r.compare(o)))
            // TODO: replace with ORSet::compare once sets module merges
            && self.users.iter().all(|u| other.users.contains(u))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u128) -> NodeId { Uuid::from_u128(id) }

    #[test]
    fn paint_and_get() {
        let mut d = CanvasDocument::new();
        d.paint(1, 2, (255, 0, 0, 255), node(1), 1);
        assert_eq!(d.get_pixel(1, 2), (255, 0, 0, 255));
    }

    #[test]
    fn default_pixel_white() {
        assert_eq!(CanvasDocument::new().get_pixel(0, 0), DEFAULT_PIXEL);
    }

    #[test]
    fn lww_higher_ts_wins() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();
        a.paint(0, 0, (255, 0, 0, 255), node(1), 10);
        b.paint(0, 0, (0, 0, 255, 255), node(2), 5);
        a.merge(b);
        assert_eq!(a.get_pixel(0, 0), (255, 0, 0, 255));
    }

    #[test]
    fn merge_commutative() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();
        a.paint(0, 0, (255, 0, 0, 255), node(1), 10);
        b.paint(0, 0, (0, 0, 255, 255), node(2), 5);
        let mut a1 = a.clone(); a1.merge(b.clone());
        let mut b1 = b.clone(); b1.merge(a.clone());
        assert_eq!(a1.get_pixel(0, 0), b1.get_pixel(0, 0));
    }

    #[test]
    fn add_remove_user() {
        let mut doc = CanvasDocument::new();
        doc.add_user(node(1));
        assert!(doc.active_users().contains(&node(1)));
        doc.remove_user(&node(1));
        assert!(!doc.active_users().contains(&node(1)));
    }
    // TODO: add concurrent-add-wins test once ORSet is merged in
}
