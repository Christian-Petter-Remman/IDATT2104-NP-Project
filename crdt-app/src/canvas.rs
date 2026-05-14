use crdt_core::registers::lww_register::LWWRegister;
use crdt_core::sets::ORSet;
use crdt_core::traits::{Crdt, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use uuid::Uuid;

pub type Rgba = (u8, u8, u8, u8);
/// Canvas is bounded to 256×256 by u8 coordinates.
pub type PixelCoord = (u8, u8);
pub const DEFAULT_PIXEL: Rgba = (255, 255, 255, 255);

/// Composite CRDT — the shared state gossiped between nodes.
///
/// Pixels + cursors use LWWRegister (last-writer-wins).
/// Users use ORSet (concurrent-add-wins).
#[derive(Clone, Serialize, Deserialize)]
pub struct CanvasDocument {
    pub pixels: HashMap<PixelCoord, LWWRegister<Rgba>>,
    users: ORSet<Uuid>,
    pub cursors: HashMap<Uuid, LWWRegister<PixelCoord>>,
}

impl Default for CanvasDocument {
    fn default() -> Self {
        Self {
            pixels: HashMap::new(),
            users: ORSet::new(),
            cursors: HashMap::new(),
        }
    }
}

impl CanvasDocument {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn paint(&mut self, x: u8, y: u8, color: Rgba, node_id: NodeId, timestamp: u64) {
        self.pixels
            .entry((x, y))
            .or_insert_with(|| LWWRegister::new(DEFAULT_PIXEL, 0, node_id))
            .set(color, timestamp, node_id);
    }

    pub fn add_user(&mut self, user: Uuid, node_id: &NodeId) {
        self.users.insert(user, node_id);
    }

    pub fn remove_user(&mut self, user: &Uuid) -> bool {
        self.users.remove(user)
    }

    pub fn active_users(&self) -> std::collections::HashSet<Uuid> {
        self.users.value()
    }

    pub fn max_pixel_timestamp(&self) -> u64 {
        self.pixels
            .values()
            .map(|r| r.timestamp())
            .max()
            .unwrap_or(0)
    }

    // TODO: wire cursor updates via API once crdt-net gossip is integrated
}

impl Crdt for CanvasDocument {
    type Value = Self;

    fn value(&self) -> Self {
        self.clone()
    }

    fn merge(&mut self, other: Self) {
        for ((x, y), reg) in other.pixels {
            match self.pixels.entry((x, y)) {
                Entry::Occupied(mut e) => e.get_mut().merge(reg),
                Entry::Vacant(e) => {
                    e.insert(reg);
                }
            }
        }
        self.users.merge(other.users);
        for (uid, reg) in other.cursors {
            match self.cursors.entry(uid) {
                Entry::Occupied(mut e) => e.get_mut().merge(reg),
                Entry::Vacant(e) => {
                    e.insert(reg);
                }
            }
        }
    }

    fn compare(&self, other: &Self) -> bool {
        self.pixels
            .iter()
            .all(|(k, r)| other.pixels.get(k).is_some_and(|o| r.compare(o)))
            && self
                .cursors
                .iter()
                .all(|(k, r)| other.cursors.get(k).is_some_and(|o| r.compare(o)))
            && self.users.compare(&other.users)
    }
}

/// Client-facing view — strips CRDT metadata (timestamps, node ids).
#[derive(Serialize)]
pub struct CanvasView {
    pub pixels: HashMap<String, [u8; 4]>,
    pub users: Vec<String>,
}

impl From<&CanvasDocument> for CanvasView {
    fn from(doc: &CanvasDocument) -> Self {
        let mut users: Vec<String> = doc.active_users().iter().map(|u| u.to_string()).collect();
        users.sort();
        Self {
            pixels: doc
                .pixels
                .iter()
                .map(|((x, y), r)| {
                    let (a, b, c, d) = r.value();
                    (format!("{x},{y}"), [a, b, c, d])
                })
                .collect(),
            users,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u128) -> NodeId {
        Uuid::from_u128(id)
    }

    fn get_pixel(doc: &CanvasDocument, x: u8, y: u8) -> Rgba {
        doc.pixels
            .get(&(x, y))
            .map(|r| r.value())
            .unwrap_or(DEFAULT_PIXEL)
    }

    #[test]
    fn paint_and_get() {
        let mut d = CanvasDocument::new();
        d.paint(1, 2, (255, 0, 0, 255), node(1), 1);
        assert_eq!(get_pixel(&d, 1, 2), (255, 0, 0, 255));
    }

    #[test]
    fn default_pixel_white() {
        assert_eq!(get_pixel(&CanvasDocument::new(), 0, 0), DEFAULT_PIXEL);
    }

    #[test]
    fn lww_higher_ts_wins() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();
        a.paint(0, 0, (255, 0, 0, 255), node(1), 10);
        b.paint(0, 0, (0, 0, 255, 255), node(2), 5);
        a.merge(b);
        assert_eq!(get_pixel(&a, 0, 0), (255, 0, 0, 255));
    }

    #[test]
    fn merge_commutative() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();
        a.paint(0, 0, (255, 0, 0, 255), node(1), 10);
        b.paint(0, 0, (0, 0, 255, 255), node(2), 5);
        let mut a1 = a.clone();
        a1.merge(b.clone());
        let mut b1 = b.clone();
        b1.merge(a.clone());
        assert_eq!(get_pixel(&a1, 0, 0), get_pixel(&b1, 0, 0));
    }

    #[test]
    fn merge_idempotent() {
        let user = Uuid::from_u128(7);
        let mut a = CanvasDocument::new();
        a.paint(0, 0, (1, 2, 3, 4), node(1), 5);
        a.add_user(user, &node(1));
        let b = a.clone();
        a.merge(b);
        assert_eq!(get_pixel(&a, 0, 0), (1, 2, 3, 4));
        assert!(a.active_users().contains(&user));
    }

    #[test]
    fn user_add_wins_on_concurrent_remove() {
        let user = Uuid::from_u128(99);
        let mut peer_a = CanvasDocument::new();
        peer_a.add_user(user, &node(1));

        let mut peer_b = CanvasDocument::new();
        peer_b.add_user(user, &node(2));
        peer_b.remove_user(&user);

        let mut a1 = peer_a.clone();
        a1.merge(peer_b.clone());
        assert!(a1.active_users().contains(&user));

        let mut b1 = peer_b.clone();
        b1.merge(peer_a.clone());
        assert!(b1.active_users().contains(&user));
    }
}
