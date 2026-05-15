use crdt_core::clocks::VectorClock;
use crdt_core::counters::GCounter;
use crdt_core::registers::lww_register::LWWRegister;
use crdt_core::sets::{ORSet, ORSetDelta};
use crdt_core::traits::{Crdt, DeltaCrdt, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub type Rgba = (u8, u8, u8, u8);
/// Canvas is bounded to 256×256 by u8 coordinates.
pub type PixelCoord = (u8, u8);
pub const DEFAULT_PIXEL: Rgba = (255, 255, 255, 255);

mod pixel_map_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(
        map: &HashMap<PixelCoord, LWWRegister<Rgba>>,
        s: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = s.serialize_seq(Some(map.len()))?;
        for pair in map.iter() {
            seq.serialize_element(&pair)?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D>(d: D) -> Result<HashMap<PixelCoord, LWWRegister<Rgba>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<(PixelCoord, LWWRegister<Rgba>)>::deserialize(d).map(|v| v.into_iter().collect())
    }
}

/// The shared state gossiped between peers.
///
/// Every field is a CRDT with its own merge semantics:
/// - `pixels`: per-coordinate [`LWWRegister`], last writer wins.
/// - `users`: [`ORSet`] of active peer UUIDs, add-wins on concurrent add/remove.
/// - `cursors`: per-user [`LWWRegister`] of cursor position, last writer wins.
/// - `palette`: [`ORSet`] of active palette colors, add-wins.
/// - `paint_counts`: [`GCounter`] tracking total paints per node.
/// - `clock`: [`VectorClock`] tracking causality across all of the above.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanvasDocument {
    #[serde(with = "pixel_map_serde")]
    pub pixels: HashMap<PixelCoord, LWWRegister<Rgba>>,
    users: ORSet<Uuid>,
    pub cursors: HashMap<Uuid, LWWRegister<PixelCoord>>,
    pub palette: ORSet<Rgba>,
    pub paint_counts: GCounter,
    clock: VectorClock,
}

impl Default for CanvasDocument {
    fn default() -> Self {
        Self {
            pixels: HashMap::new(),
            users: ORSet::new(),
            cursors: HashMap::new(),
            palette: ORSet::new(),
            paint_counts: GCounter::new(),
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
        self.paint_counts.increment(node_id);
    }

    // TODO: why increment clock? And should only increment by one or max?
    // and return a bool, and for other methods as well?

    /// Register a peer as active. Uses ORSet add-wins semantics.
    pub fn add_user(&mut self, user: Uuid, node_id: &NodeId) {
        let seq = self.clock.increment(*node_id);
        self.users.insert(user, node_id, seq);
    }

    /// Remove a peer from the active set.
    pub fn remove_user(&mut self, user: &Uuid) -> bool {
        self.users.remove(user)
    }

    /// Returns the set of currently active peer UUIDs.
    pub fn active_users(&self) -> HashSet<Uuid> {
        self.users.value()
    }

    pub fn add_palette_color(&mut self, color: Rgba, node_id: &NodeId) {
        let seq = self.clock.increment(*node_id);
        self.palette.insert(color, node_id, seq);
    }

    pub fn remove_palette_color(&mut self, color: &Rgba) -> bool {
        self.palette.remove(color)
    }

    pub fn palette_colors(&self) -> Vec<Rgba> {
        let mut colors: Vec<Rgba> = self.palette.value().into_iter().collect();
        colors.sort();
        colors
    }

    pub fn ownership_leaderboard(&self) -> Vec<(NodeId, u64)> {
        let mut counts: HashMap<NodeId, u64> = HashMap::new();
        for reg in self.pixels.values() {
            *counts.entry(reg.node_id()).or_insert(0) += 1;
        }
        let mut result: Vec<(NodeId, u64)> = counts.into_iter().collect();
        result.sort_by_key(|b| std::cmp::Reverse(b.1));
        result
    }

    /// Update a peer's cursor position.
    ///
    /// Not wired through the REST API yet; the `cursors` field exists so
    /// CRDT merge/delta logic stays correct once the cursor endpoint
    /// lands. Marked `#[allow(dead_code)]` so the field's merge path
    /// keeps compiling without forcing CI failures in the interim.
    #[allow(dead_code)]
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

        self.palette.merge(other.palette);
        self.paint_counts.merge(other.paint_counts);
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
            && self.palette.compare(&other.palette)
            && self.paint_counts.compare(&other.paint_counts)
    }
}

mod pixel_vec_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(v: &Vec<(PixelCoord, LWWRegister<Rgba>)>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = s.serialize_seq(Some(v.len()))?;
        for pair in v {
            seq.serialize_element(pair)?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Vec<(PixelCoord, LWWRegister<Rgba>)>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<(PixelCoord, LWWRegister<Rgba>)>::deserialize(d)
    }
}

/// Delta payload for a [`CanvasDocument`].
///
/// Each field carries the corresponding CRDT's delta — empty / `None`
/// when nothing changed there. The receiver applies a `CanvasDelta` via
/// [`CanvasDocument::merge_delta`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanvasDelta {
    /// VectorClock entries that advanced. Receivers absorb this into their
    /// own clock so subsequent local writes carry strictly higher
    /// timestamps.
    pub clock: <VectorClock as DeltaCrdt>::Delta,
    /// Pixels whose LWWRegister timestamp exceeds the receiver's view for
    /// the writing node. Carried as `(coord, LWWRegister)` pairs.
    #[serde(with = "pixel_vec_serde")]
    pub pixels: Vec<(PixelCoord, LWWRegister<Rgba>)>,
    pub users: ORSetDelta<Uuid>,
    pub cursors: Vec<(Uuid, LWWRegister<PixelCoord>)>,
    pub palette: ORSetDelta<Rgba>,
    pub paint_counts: <GCounter as DeltaCrdt>::Delta,
}

impl DeltaCrdt for CanvasDocument {
    type Delta = CanvasDelta;
    type Version = VectorClock;

    fn version(&self) -> Self::Version {
        self.clock.clone()
    }

    fn delta_since(&self, since: &Self::Version) -> Self::Delta {
        let pixels: Vec<(PixelCoord, LWWRegister<Rgba>)> = self
            .pixels
            .iter()
            .filter_map(|(coord, reg)| {
                let known = since.get(&reg.node_id());
                (reg.timestamp() > known).then(|| (*coord, reg.clone()))
            })
            .collect();

        let cursors: Vec<(Uuid, LWWRegister<PixelCoord>)> = self
            .cursors
            .iter()
            .filter_map(|(uid, reg)| {
                let known = since.get(&reg.node_id());
                (reg.timestamp() > known).then(|| (*uid, reg.clone()))
            })
            .collect();

        // ORSet tag seqs are sourced from the same VectorClock the document
        // tracks, so its frontier-as-HashMap is `since.clock`.
        let or_set_version: std::collections::HashMap<NodeId, u64> = since.value();

        CanvasDelta {
            clock: self.clock.delta_since(since),
            pixels,
            users: self.users.delta_since(&or_set_version),
            cursors,
            palette: self.palette.delta_since(&or_set_version),
            paint_counts: self.paint_counts.delta_since(&or_set_version),
        }
    }

    fn merge_delta(&mut self, delta: Self::Delta) {
        self.clock.merge_delta(delta.clock);

        for ((x, y), reg) in delta.pixels {
            match self.pixels.entry((x, y)) {
                Entry::Occupied(mut e) => e.get_mut().merge(reg),
                Entry::Vacant(e) => {
                    e.insert(reg);
                }
            }
        }

        self.users.merge_delta(delta.users);

        for (uid, reg) in delta.cursors {
            match self.cursors.entry(uid) {
                Entry::Occupied(mut e) => e.get_mut().merge(reg),
                Entry::Vacant(e) => {
                    e.insert(reg);
                }
            }
        }

        self.palette.merge_delta(delta.palette);
        self.paint_counts.merge_delta(delta.paint_counts);
    }

    fn is_empty_delta(delta: &Self::Delta) -> bool {
        // Clock progression is the canonical "did anything happen" signal.
        // Every state-changing operation increments the clock, so an
        // empty clock delta implies no per-field changes worth shipping.
        VectorClock::is_empty_delta(&delta.clock)
            && delta.pixels.is_empty()
            && delta.cursors.is_empty()
            && ORSet::<Uuid>::is_empty_delta(&delta.users)
            && ORSet::<Rgba>::is_empty_delta(&delta.palette)
            && GCounter::is_empty_delta(&delta.paint_counts)
    }

    fn version_includes(current: &Self::Version, other: &Self::Version) -> bool {
        current.dominates(other)
    }
}

/// Client-facing view. Strips CRDT metadata (timestamps, node ids, vector clock).
#[derive(Serialize)]
pub struct CanvasView {
    pub pixels: HashMap<String, [u8; 4]>,
    pub active_peers: Vec<String>,
    pub palette: Vec<[u8; 4]>,
    pub paint_total: u64,
    pub leaderboard: Vec<LeaderboardEntry>,
}

#[derive(Serialize)]
pub struct LeaderboardEntry {
    pub peer_id: String,
    pub pixels: u64,
}

impl From<&CanvasDocument> for CanvasView {
    fn from(doc: &CanvasDocument) -> Self {
        let mut active_peers: Vec<String> =
            doc.active_users().iter().map(|u| u.to_string()).collect();
        active_peers.sort();
        Self {
            pixels: doc
                .pixels
                .iter()
                .map(|((x, y), r)| {
                    let (a, b, c, d) = r.value();
                    (format!("{x},{y}"), [a, b, c, d])
                })
                .collect(),
            active_peers,
            palette: doc
                .palette_colors()
                .into_iter()
                .map(|(r, g, b, a)| [r, g, b, a])
                .collect(),
            paint_total: doc.paint_counts.value(),
            leaderboard: doc
                .ownership_leaderboard()
                .into_iter()
                .map(|(id, n)| LeaderboardEntry {
                    peer_id: id.to_string(),
                    pixels: n,
                })
                .collect(),
        }
    }
}

/// Sparse client-facing view of a [`CanvasDelta`].
///
/// `pixels` lists only the coordinates whose colour changed since the
/// receiver's last update. Each `Option` field is `Some` only when the
/// corresponding derived view actually changed; the frontend should
/// treat `None` as "no change, keep what you had".
#[derive(Serialize)]
pub struct CanvasDeltaView {
    pub pixels: HashMap<String, [u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_peers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<Vec<[u8; 4]>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paint_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leaderboard: Option<Vec<LeaderboardEntry>>,
}

impl CanvasDeltaView {
    /// Project a CRDT-level `CanvasDelta` against the new authoritative
    /// document state, recomputing each derived view only when its
    /// underlying CRDT actually changed.
    pub fn project(delta: &CanvasDelta, doc: &CanvasDocument) -> Self {
        let pixels = delta
            .pixels
            .iter()
            .map(|((x, y), r)| {
                let (a, b, c, d) = r.value();
                (format!("{x},{y}"), [a, b, c, d])
            })
            .collect::<HashMap<_, _>>();

        let active_peers = if ORSet::<Uuid>::is_empty_delta(&delta.users) {
            None
        } else {
            let mut peers: Vec<String> = doc.active_users().iter().map(|u| u.to_string()).collect();
            peers.sort();
            Some(peers)
        };

        let palette = if ORSet::<Rgba>::is_empty_delta(&delta.palette) {
            None
        } else {
            Some(
                doc.palette_colors()
                    .into_iter()
                    .map(|(r, g, b, a)| [r, g, b, a])
                    .collect(),
            )
        };

        let paint_total = if GCounter::is_empty_delta(&delta.paint_counts) {
            None
        } else {
            Some(doc.paint_counts.value())
        };

        // Leaderboard is derived from per-pixel ownership; any pixel change
        // can shift it. Recompute and ship when pixels changed.
        let leaderboard = if pixels.is_empty() {
            None
        } else {
            Some(
                doc.ownership_leaderboard()
                    .into_iter()
                    .map(|(id, n)| LeaderboardEntry {
                        peer_id: id.to_string(),
                        pixels: n,
                    })
                    .collect(),
            )
        };

        Self {
            pixels,
            active_peers,
            palette,
            paint_total,
            leaderboard,
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

        for _ in 0..5 {
            b.paint(0, 0, (255, 0, 0, 255), node(2));
        }

        a.merge(b);
        a.paint(0, 0, (0, 0, 255, 255), node(1));

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

    #[test]
    fn palette_add_and_colors() {
        let mut d = CanvasDocument::new();
        d.add_palette_color((255, 0, 0, 255), &node(1));
        d.add_palette_color((0, 255, 0, 255), &node(1));
        let colors = d.palette_colors();
        assert_eq!(colors.len(), 2);
        assert!(colors.contains(&(255, 0, 0, 255)));
        assert!(colors.contains(&(0, 255, 0, 255)));
    }

    #[test]
    fn paint_increments_paint_total() {
        let mut d = CanvasDocument::new();
        d.paint(0, 0, (1, 2, 3, 4), node(1));
        d.paint(1, 1, (5, 6, 7, 8), node(1));
        assert_eq!(d.paint_counts.value(), 2);
    }

    #[test]
    fn delta_since_empty_replays_full_state() {
        let mut a = CanvasDocument::new();
        a.paint(1, 2, (10, 20, 30, 40), node(1));
        a.add_palette_color((255, 0, 0, 255), &node(1));

        let delta = a.delta_since(&VectorClock::new());
        let mut b = CanvasDocument::new();
        b.merge_delta(delta);

        assert_eq!(get_pixel(&b, 1, 2), (10, 20, 30, 40));
        assert!(b.palette_colors().contains(&(255, 0, 0, 255)));
    }

    #[test]
    fn delta_since_current_version_is_empty() {
        let mut a = CanvasDocument::new();
        a.paint(0, 0, (1, 2, 3, 4), node(1));
        a.add_palette_color((10, 20, 30, 40), &node(1));

        let delta = a.delta_since(&a.version());
        assert!(
            CanvasDocument::is_empty_delta(&delta),
            "delta to self should be empty"
        );
    }

    /// Apply a delta computed against B's version; B should converge to A.
    #[test]
    fn delta_catches_up_lagging_replica() {
        let mut a = CanvasDocument::new();
        let mut b = CanvasDocument::new();

        a.paint(0, 0, (255, 0, 0, 255), node(1));
        b.merge(a.clone()); // bring B current

        // Now A pulls ahead.
        a.paint(1, 1, (0, 255, 0, 255), node(1));
        a.add_palette_color((0, 0, 255, 255), &node(1));

        let delta = a.delta_since(&b.version());
        b.merge_delta(delta);

        assert_eq!(get_pixel(&b, 0, 0), (255, 0, 0, 255));
        assert_eq!(get_pixel(&b, 1, 1), (0, 255, 0, 255));
        assert!(b.palette_colors().contains(&(0, 0, 255, 255)));
    }

    #[test]
    fn delta_is_idempotent() {
        let mut a = CanvasDocument::new();
        a.paint(3, 4, (5, 6, 7, 8), node(1));
        let delta = a.delta_since(&VectorClock::new());

        let mut b = CanvasDocument::new();
        b.merge_delta(delta.clone());
        let snapshot = b.clone();
        b.merge_delta(delta);

        // Re-applying the same delta leaves the state unchanged.
        assert_eq!(get_pixel(&b, 3, 4), get_pixel(&snapshot, 3, 4));
        assert_eq!(b.paint_counts.value(), snapshot.paint_counts.value());
    }

    #[test]
    fn delta_propagates_tombstones() {
        let mut a = CanvasDocument::new();
        a.add_palette_color((1, 2, 3, 4), &node(1));
        a.add_palette_color((5, 6, 7, 8), &node(1));
        a.remove_palette_color(&(1, 2, 3, 4));

        let mut b = CanvasDocument::new();
        b.merge_delta(a.delta_since(&VectorClock::new()));

        assert!(b.palette_colors().contains(&(5, 6, 7, 8)));
        assert!(!b.palette_colors().contains(&(1, 2, 3, 4)));
    }

    /// Regression: after any palette removal, `is_empty_delta` was
    /// permanently false because `ORSet::delta_since` shipped the full
    /// tombstone set every time. That silently disabled the WS skip in
    /// `api.rs::handle_ws`. Verify the skip is active again.
    #[test]
    fn idle_delta_after_palette_removal_is_empty() {
        let mut a = CanvasDocument::new();
        a.add_palette_color((1, 2, 3, 4), &node(1));
        a.remove_palette_color(&(1, 2, 3, 4));

        // Re-querying at the post-removal version must produce an empty
        // delta — nothing has happened since.
        let delta = a.delta_since(&a.version());
        assert!(
            CanvasDocument::is_empty_delta(&delta),
            "idle delta after tombstone must be empty so WS skip fires"
        );
    }

    /// `version_includes` powers the partition-heal drop in the gossip
    /// engine: a delta is only safe to apply when the receiver's state
    /// already knows everything the sender's `since` baseline asserts.
    #[test]
    fn version_includes_detects_lagging_receiver() {
        let mut sender = CanvasDocument::new();
        sender.paint(0, 0, (1, 2, 3, 4), node(1));
        sender.paint(1, 1, (5, 6, 7, 8), node(1));

        let fresh_receiver_version = CanvasDocument::new().version();
        assert!(
            !CanvasDocument::version_includes(&fresh_receiver_version, &sender.version()),
            "fresh receiver must NOT include sender's advanced baseline"
        );

        let caught_up_receiver = sender.clone();
        assert!(
            CanvasDocument::version_includes(&caught_up_receiver.version(), &sender.version()),
            "caught-up receiver MUST include sender's baseline"
        );
    }
}
