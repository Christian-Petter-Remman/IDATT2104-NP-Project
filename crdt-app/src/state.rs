//! Application state — the single source of truth for the shared canvas.
//!
//! [`AppState`] sits between the network layer (`crdt-net`) and the API
//! layer (`api.rs`). All mutations — local or from gossip — flow through here.
//!
//! Timestamps live on the [`CanvasDocument`]'s own `VectorClock` and
//! travel with it during gossip; no manual syncing needed.
//!
//! All mutation methods are synchronous. State lives inside a
//! [`watch::Sender`] whose `send_modify` runs the closure atomically
//! without holding a lock across an await point.

use crate::canvas::{CanvasDocument, Rgba};
use crdt_core::Crdt;
use std::sync::Arc;
use tokio::sync::watch;
use uuid::Uuid;

/// Shared application state, wrapped in `Arc` and passed to all tasks.
///
/// The canvas is stored inside a [`watch::Sender`] which is the single
/// source of truth. Readers obtain a [`watch::Receiver`] via
/// [`subscribe`](Self::subscribe) and are notified on every change.
pub struct AppState {
    node_id: Uuid,
    addr: String,
    canvas: watch::Sender<CanvasDocument>,
}

impl AppState {
    pub fn new(node_id: Uuid, addr: String) -> (Arc<Self>, watch::Receiver<CanvasDocument>) {
        let (tx, rx) = watch::channel(CanvasDocument::new());
        let state = Arc::new(Self {
            node_id,
            addr,
            canvas: tx,
        });
        (state, rx)
    }

    pub fn node_id(&self) -> Uuid {
        self.node_id
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Apply an arbitrary mutation to the canvas.
    ///
    /// The closure receives `&mut CanvasDocument` and this node's ID.
    /// Timestamp handling is internal to the document's `VectorClock`.
    pub fn mutate<R>(&self, f: impl FnOnce(&mut CanvasDocument, Uuid) -> R) -> R {
        let mut result = None;
        self.canvas.send_modify(|doc| {
            result = Some(f(doc, self.node_id));
        });
        // send_modify calls the closure exactly once synchronously before returning
        result.expect("send_modify did not invoke closure")
    }

    /// Merge a remotely-received document into local state.
    ///
    /// The document's `VectorClock` merges as part of `Crdt::merge`,
    /// so subsequent local writes automatically have higher timestamps
    /// than anything observed from the remote peer.
    pub fn apply_gossip(&self, incoming: CanvasDocument) {
        self.canvas.send_modify(|doc| doc.merge(incoming));
    }

    /// Borrow the current canvas state.
    ///
    /// Hold the returned guard only briefly; while it is alive, `mutate`
    /// and `apply_gossip` block. For longer-lived access use [`snapshot`](Self::snapshot).
    pub fn canvas(&self) -> watch::Ref<'_, CanvasDocument> {
        self.canvas.borrow()
    }

    /// Clone the current canvas state.
    pub fn snapshot(&self) -> CanvasDocument {
        self.canvas.borrow().clone()
    }

    /// Obtain a receiver that is notified on every state change.
    pub fn subscribe(&self) -> watch::Receiver<CanvasDocument> {
        self.canvas.subscribe()
    }

    // Convenience wrappers used by api.rs

    pub fn paint(&self, x: u8, y: u8, color: Rgba) {
        self.mutate(|doc, id| doc.paint(x, y, color, id));
    }

    pub fn add_user(&self, user: Uuid) {
        self.mutate(|doc, id| doc.add_user(user, &id));
    }

    pub fn remove_user(&self, user: &Uuid) {
        let user = *user;
        self.mutate(|doc, _| { doc.remove_user(&user); });
    }

    pub fn add_palette_color(&self, color: Rgba) {
        self.mutate(|doc, id| doc.add_palette_color(color, &id));
    }

    pub fn remove_palette_color(&self, color: Rgba) -> bool {
        self.mutate(|doc, _| doc.remove_palette_color(&color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> (Arc<AppState>, watch::Receiver<CanvasDocument>) {
        AppState::new(Uuid::from_u128(1), "0.0.0.0:8080".to_string())
    }

    #[test]
    fn paint_via_mutate() {
        let (state, rx) = make();
        state.mutate(|doc, node_id| {
            doc.paint(1, 2, (255, 0, 0, 255), node_id);
        });
        let pixel = rx.borrow().pixels.get(&(1, 2)).map(|r| r.value());
        assert_eq!(pixel, Some((255, 0, 0, 255)));
    }

    #[test]
    fn mutate_returns_value() {
        let (state, _rx) = make();
        let result = state.mutate(|_doc, _id| 42);
        assert_eq!(result, 42);
    }

    #[test]
    fn apply_gossip_merges() {
        let (state, rx) = make();
        let mut incoming = CanvasDocument::new();
        incoming.paint(5, 5, (0, 255, 0, 255), Uuid::from_u128(2));
        state.apply_gossip(incoming);
        let pixel = rx.borrow().pixels.get(&(5, 5)).map(|r| r.value());
        assert_eq!(pixel, Some((0, 255, 0, 255)));
    }

    #[test]
    fn gossip_merge_advances_clock() {
        let (state, _rx) = make();

        let mut incoming = CanvasDocument::new();
        let remote_id = Uuid::from_u128(2);
        incoming.paint(0, 0, (255, 0, 0, 255), remote_id);

        state.apply_gossip(incoming);

        // Local paint after merging remote state must win.
        state.mutate(|doc, node_id| {
            doc.paint(0, 0, (0, 0, 255, 255), node_id);
        });

        let pixel = state.canvas().pixels.get(&(0, 0)).map(|r| r.value());
        assert_eq!(pixel, Some((0, 0, 255, 255)));
    }

    #[test]
    fn subscribe_sees_changes() {
        let (state, _rx) = make();
        let mut watcher = state.subscribe();
        state.mutate(|doc, id| doc.paint(0, 0, (1, 2, 3, 4), id));
        assert!(watcher.has_changed().unwrap());
    }

    #[test]
    fn paint_convenience_wrapper() {
        let (state, rx) = make();
        state.paint(3, 4, (10, 20, 30, 40));
        let pixel = rx.borrow().pixels.get(&(3, 4)).map(|r| r.value());
        assert_eq!(pixel, Some((10, 20, 30, 40)));
    }
}
