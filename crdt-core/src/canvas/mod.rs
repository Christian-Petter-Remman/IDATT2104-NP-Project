use std::collections::HashMap;
use std::collections::hash_map::Entry;
use uuid::Uuid;
use crate::traits::{Crdt, NodeId};
use crate::registers::lww_register::LWWRegister;

/// RGBA color as four bytes: (red, green, blue, alpha).
pub type Rgba = (u8, u8, u8, u8);

pub const DEFAULT_PIXEL: Rgba = (255, 255, 255, 255);

// PLACEHOLDER: `UserSet` will be replaced with `ORSet<Uuid>` once the sets module is merged.
//
// `ORSet` (Observed-Remove Set) is required here because user membership has
// concurrent-add-wins semantics: if node A adds a user while node B removes the
// same user concurrently, after merge the user should be present (A's add wins).
// A plain `HashSet` cannot express this — it has no tombstone mechanism.
//
// When ORSet is available:
//   1. Replace this type alias with: `use crate::sets::or_set::ORSet;`
//   2. Change the field type to `users: ORSet<Uuid>`
//   3. Implement `add_user`, `remove_user`, and users merge using ORSet API
type UserSet = std::collections::HashSet<Uuid>;

/// Composite CRDT representing the shared pixel canvas state.
///
/// Assembles individual CRDTs into one document that can be gossiped between
/// nodes. `crdt-app` holds this in `AppState`; `crdt-net` serializes and merges
/// it on gossip. The app layer calls `paint`, `get_pixel`, `merge` — it never
/// touches `LWWRegister` directly.
#[derive(Clone)]
pub struct CanvasDocument {
    /// One LWWRegister per pixel coordinate. LWW rule: highest timestamp wins.
    /// Primary shared state gossiped between nodes.
    pub pixels: HashMap<(u8, u8), LWWRegister<Rgba>>,

    /// Active users. PLACEHOLDER — see UserSet comment above.
    /// Will become ORSet<Uuid> for concurrent-add-wins semantics.
    users: UserSet,

    /// Last known cursor position per user, tracked as LWWRegister<(x, y)>.
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Paint pixel at (x, y) with color. Higher timestamp wins on conflict.
    pub fn paint(&mut self, x: u8, y: u8, color: Rgba, node_id: NodeId, timestamp: u64) {
        self.pixels
            .entry((x, y))
            .or_insert_with(|| LWWRegister::new(DEFAULT_PIXEL, 0, node_id))
            .set(color, timestamp, node_id);
    }

    /// Read pixel color. Returns white if never painted.
    pub fn get_pixel(&self, x: u8, y: u8) -> Rgba {
        self.pixels
            .get(&(x, y))
            .map(|r| r.value())
            .unwrap_or(DEFAULT_PIXEL)
    }

    /// Update cursor position for a user.
    pub fn update_cursor(&mut self, user_id: Uuid, pos: (u8, u8), timestamp: u64) {
        self.cursors
            .entry(user_id)
            .or_insert_with(|| LWWRegister::new((0, 0), 0, user_id))
            .set(pos, timestamp, user_id);
    }

    /// Add user to active set. TODO: replace with ORSet::insert once sets module merges.
    pub fn add_user(&mut self, user_id: Uuid) {
        self.users.insert(user_id);
    }

    /// Remove user from active set. TODO: replace with ORSet::remove (tombstone) once sets module merges.
    pub fn remove_user(&mut self, user_id: Uuid) {
        self.users.remove(&user_id);
    }

    pub fn active_users(&self) -> Vec<Uuid> {
        self.users.iter().copied().collect()
    }
}

impl Crdt for CanvasDocument {
    type Value = Self;

    fn value(&self) -> Self {
        self.clone()
    }

    /// Merge pixels, users, and cursors independently.
    /// Each field uses its own CRDT merge rule.
    fn merge(&mut self, other: Self) {
        for ((x, y), other_reg) in other.pixels {
            match self.pixels.entry((x, y)) {
                Entry::Occupied(mut e) => e.get_mut().merge(other_reg),
                Entry::Vacant(e) => { e.insert(other_reg); }
            }
        }
        // TODO: replace with ORSet::merge once sets module merges
        for user in other.users {
            self.users.insert(user);
        }
        for (user_id, other_reg) in other.cursors {
            match self.cursors.entry(user_id) {
                Entry::Occupied(mut e) => e.get_mut().merge(other_reg),
                Entry::Vacant(e) => { e.insert(other_reg); }
            }
        }
    }

    /// Returns true if every pixel and cursor in self is dominated by other.
    fn compare(&self, other: &Self) -> bool {
        self.pixels.iter().all(|(k, reg)| {
            other.pixels.get(k).map_or(false, |o| reg.compare(o))
        }) && self.cursors.iter().all(|(k, reg)| {
            other.cursors.get(k).map_or(false, |o| reg.compare(o))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u128) -> NodeId { Uuid::from_u128(id) }

    #[test]
    fn paint_and_get_pixel() {
        let mut doc = CanvasDocument::new();
        doc.paint(5, 10, (255, 0, 0, 255), node(1), 1);
        assert_eq!(doc.get_pixel(5, 10), (255, 0, 0, 255));
    }

    #[test]
    fn unpainted_pixel_returns_default() {
        let doc = CanvasDocument::new();
        assert_eq!(doc.get_pixel(0, 0), DEFAULT_PIXEL);
    }

    #[test]
    fn higher_timestamp_wins() {
        let mut doc = CanvasDocument::new();
        doc.paint(0, 0, (255, 0, 0, 255), node(1), 10);
        doc.paint(0, 0, (0, 0, 255, 255), node(1), 5);
        assert_eq!(doc.get_pixel(0, 0), (255, 0, 0, 255));
    }

    #[test]
    fn two_nodes_paint_same_pixel_lww_wins() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();
        a.paint(0, 0, (255, 0, 0, 255), node(1), 10);
        b.paint(0, 0, (0, 0, 255, 255), node(2), 5);
        a.merge(b);
        assert_eq!(a.get_pixel(0, 0), (255, 0, 0, 255));
    }

    #[test]
    fn merge_unions_pixels_from_both_nodes() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();
        a.paint(0, 0, (255, 0, 0, 255), node(1), 1);
        b.paint(1, 1, (0, 255, 0, 255), node(2), 1);
        a.merge(b);
        assert_eq!(a.get_pixel(0, 0), (255, 0, 0, 255));
        assert_eq!(a.get_pixel(1, 1), (0, 255, 0, 255));
    }

    #[test]
    fn merge_is_commutative() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();
        a.paint(0, 0, (255, 0, 0, 255), node(1), 10);
        b.paint(0, 0, (0, 0, 255, 255), node(2), 5);
        let mut a1 = a.clone();
        let mut b1 = b.clone();
        a1.merge(b);
        b1.merge(a);
        assert_eq!(a1.get_pixel(0, 0), b1.get_pixel(0, 0));
    }

    #[test]
    fn merge_is_idempotent() {
        let mut a = CanvasDocument::new();
        a.paint(0, 0, (255, 0, 0, 255), node(1), 1);
        let b = a.clone();
        a.merge(b);
        assert_eq!(a.get_pixel(0, 0), (255, 0, 0, 255));
    }

    #[test]
    fn cursor_update_and_merge() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();
        a.update_cursor(node(1), (10, 20), 1);
        b.update_cursor(node(2), (30, 40), 1);
        a.merge(b);
        assert_eq!(a.cursors[&node(1)].value(), (10, 20));
        assert_eq!(a.cursors[&node(2)].value(), (30, 40));
    }

    #[test]
    fn add_remove_user() {
        let mut doc = CanvasDocument::new();
        doc.add_user(node(1));
        assert!(doc.active_users().contains(&node(1)));
        doc.remove_user(node(1));
        assert!(!doc.active_users().contains(&node(1)));
    }
}
