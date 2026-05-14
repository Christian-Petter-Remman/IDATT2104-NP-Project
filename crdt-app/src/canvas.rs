//! The composite CRDT that represents the shared canvas state.
//!
//! [`CanvasDocument`] is the single value gossiped between peers. It
//! composes several independent CRDTs, each field handles a different
//! aspect of the shared canvas, and a [`VectorClock`] that ties them
//! together.
//!
//! # How the VectorClock fits in
//!
//! The clock serves two roles:
//!
//! 1. **LWW timestamp source.** Pixels and cursors use
//!    [`LWWRegister`], which needs a monotonic timestamp to decide
//!    which write wins. The clock's [`increment`](VectorClock::increment)
//!    method returns a value that is strictly greater than any
//!    component in the clock, so a paint that happens after observing 
//!    remote state always gets a higher timestamp.
//!
//! 2. **Causality tracking.** After two documents merge, their clocks
//!    merge (element-wise max), so each peer knows what the other has
//!    seen. This is what makes the Lamport timestamps safe — without
//!    merge, a peer could fall behind and generate losing timestamps
//!    indefinitely.
//!
//! Mutations that don't need an LWW timestamp (e.g. [`add_user`],
//! which goes through [`ORSet`] with its own internal tagging) still
//! increment the clock for document-level causality, so a peer can
//! tell whether it has seen a particular mutation.

use crdt_core::clocks::VectorClock;
use crdt_core::registers::lww_register::LWWRegister;
use crdt_core::sets::ORSet;
use crdt_core::traits::{Crdt, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;


pub type Rgba = (u8, u8, u8, u8);
/// Canvas is bounded to 256×256 by u8 coordinates.
pub type PixelCoord = (u8, u8);
pub const DEFAULT_PIXEL: Rgba = (255, 255, 255, 255);

/// The shared state gossiped between peers.
///
/// Every field is a CRDT with its own merge semantics:
/// - `pixels`: per-coordinate [`LWWRegister`], last writer wins.
/// - `users`: [`ORSet`] of active peer UUIDs, add-wins on concurrent
///   add/remove.
/// - `cursors`: per-user [`LWWRegister`] of cursor position, last
///   writer wins.
/// - `clock`: [`VectorClock`] tracking across all of the
///   above. Merged alongside the other fields so timestamps stay
///   consistent across peers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanvasDocument {
    pub pixels: HashMap<PixelCoord, LWWRegister<Rgba>>,
    users: ORSet<Uuid>,
    pub cursors: HashMap<Uuid, LWWRegister<PixelCoord>>,
    clock: VectorClock,
}
 
impl Default for CanvasDocument {
    fn default() -> Self {
        Self {
            pixels: HashMap::new(),
            users: ORSet::new(),
            cursors: HashMap::new(),
            clock: VectorClock::new(),
        }
    }
}


impl CanvasDocument {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a pixel's color.
    ///
    /// Increments the document clock and uses the resulting timestamp
    /// for the LWW register, ensuring this write beats any previously
    /// observed state.
    pub fn paint(&mut self, x: u8, y: u8, color: Rgba, node_id: NodeId) {
        let ts = self.clock.increment(node_id);
        self.pixels
            .entry((x, y))
            .or_insert_with(|| LWWRegister::new(DEFAULT_PIXEL, 0, node_id))
            .set(color, ts, node_id);
    }


    // TODO: why increment clock? And should only increment by one or max?
    // and return a bool, and for other methods as well?

    /// Register a peer as active. Uses ORSet add-wins semantics: a
    /// concurrent add and remove resolves in favour of the add.
    pub fn add_user(&mut self, user: Uuid, node_id: &NodeId) {
        self.clock.increment(*node_id);
        self.users.insert(user, node_id);
    }


    /// Remove a peer from the active set.
    pub fn remove_user(&mut self, user: &Uuid) -> bool {
        self.users.remove(user)
    }

    /// Returns the set of currently active peer UUIDs.
    pub fn active_users(&self) -> HashSet<Uuid> {
        self.users.value()
    }

    pub fn max_pixel_timestamp(&self) -> u64 {
        self.pixels
            .values()
            .map(|r| r.timestamp())
            .max()
            .unwrap_or(0)
    }

    // TODO: wire cursor updates via API once crdt-net gossip is integrated. Done maybe?
     /// Update a peer's cursor position.
    pub fn update_cursor(&mut self, user: Uuid, x: u8, y: u8, node_id: NodeId) {
        let ts = self.clock.increment(node_id);
        self.cursors
            .entry(user)
            .or_insert_with(|| LWWRegister::new((0, 0), 0, node_id))
            .set((x, y), ts, node_id);
    }

}

impl Crdt for CanvasDocument {
    type Value = Self;

    fn value(&self) -> Self {
        self.clone()
    }

    /// Merge another replica into this one.
    ///
    /// Each field merges independently. Pixels and cursors by
    /// LWW per key, users by ORSet add-wins, clock by element-wise
    /// max. After merge, the clock reflects all events both replicas
    /// have seen.
    fn merge(&mut self, other: Self) {
        self.clock.merge(other.clock);
 
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
        self.clock.compare(&other.clock)
            && self
                .pixels
                .iter()
                .all(|(k, r)| other.pixels.get(k).is_some_and(|o| r.compare(o)))
            && self.users.compare(&other.users)
            && self
                .cursors
                .iter()
                .all(|(k, r)| other.cursors.get(k).is_some_and(|o| r.compare(o)))
    }

}
 
/// Client-facing view. Strips CRDT metadata (timestamps, node ids,
/// vector clock) so the browser only sees pixel colors and active users.
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
        d.paint(1, 2, (255, 0, 0, 255), node(1));
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
        // a paints twice (ts=2), b paints once (ts=1)
        a.paint(0, 0, (255, 0, 0, 255), node(1));
        a.paint(0, 0, (255, 0, 0, 255), node(1));
        b.paint(0, 0, (0, 0, 255, 255), node(2));
        a.merge(b);
        assert_eq!(get_pixel(&a, 0, 0), (255, 0, 0, 255));
    }
 
    /// The critical test for VectorClock + LWW interaction.
    ///
    /// B paints many times, A merges B's state, then A paints once.
    /// A's single paint happened *after* observing B, so it must win.
    /// This fails if `increment` uses naive `+= 1` instead of the
    /// Lamport rule (`max(own, max_all) + 1`).
    #[test]
    fn paint_after_merge_beats_higher_remote_count() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();
 
        // B paints 5 times on the same pixel.
        for _ in 0..5 {
            b.paint(0, 0, (255, 0, 0, 255), node(2));
        }
 
        // A merges B's state, then paints once.
        a.merge(b);
        a.paint(0, 0, (0, 0, 255, 255), node(1));
 
        // A painted after seeing B — A must win.
        assert_eq!(get_pixel(&a, 0, 0), (0, 0, 255, 255));
    }
 
    #[test]
    fn merge_commutative() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();
        a.paint(0, 0, (255, 0, 0, 255), node(1));
        a.paint(0, 0, (255, 0, 0, 255), node(1));
        b.paint(0, 0, (0, 0, 255, 255), node(2));
 
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
        a.paint(0, 0, (1, 2, 3, 4), node(1));
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
