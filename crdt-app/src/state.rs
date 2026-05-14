use crate::canvas::{CanvasDocument, Rgba};
use crate::gossip::GossipBackend;
use crdt_core::Crdt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

pub struct AppState<G: GossipBackend> {
    pub node_id: Uuid,
    pub addr: String,
    pub canvas: RwLock<CanvasDocument>,
    pub gossip: G,
    pub ws_tx: broadcast::Sender<CanvasDocument>,
    timestamp: AtomicU64,
}

const WS_BROADCAST_CAPACITY: usize = 64;

impl<G: GossipBackend> AppState<G> {
    pub fn new(node_id: Uuid, addr: String, gossip: G) -> Arc<Self> {
        let (ws_tx, _) = broadcast::channel(WS_BROADCAST_CAPACITY);
        let now = std::time::SystemTime::UNIX_EPOCH
            .elapsed()
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Arc::new(Self {
            node_id,
            addr,
            canvas: RwLock::new(CanvasDocument::new()),
            gossip,
            ws_tx,
            timestamp: AtomicU64::new(now),
        })
    }

    /// Returns a strictly monotonically increasing timestamp.
    /// Tracks wall clock; never goes backwards within a session.
    pub fn next_ts(&self) -> u64 {
        let wall = std::time::SystemTime::UNIX_EPOCH
            .elapsed()
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        loop {
            let current = self.timestamp.load(Ordering::Relaxed);
            let next = wall.max(current) + 1;
            if self
                .timestamp
                .compare_exchange(current, next, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                return next;
            }
        }
    }

    pub async fn paint(&self, x: u8, y: u8, color: Rgba) {
        let ts = self.next_ts();
        let mut canvas = self.canvas.write().await;
        canvas.paint(x, y, color, self.node_id, ts);
        let snapshot = canvas.clone();
        drop(canvas);
        let _ = self.ws_tx.send(snapshot.clone());
        self.gossip.publish(snapshot);
    }

    pub async fn apply_gossip(&self, incoming: CanvasDocument) {
        let max_ts = incoming.max_pixel_timestamp();
        let mut canvas = self.canvas.write().await;
        canvas.merge(incoming);
        let snapshot = canvas.clone();
        self.advance_ts(max_ts);
        drop(canvas);
        let _ = self.ws_tx.send(snapshot);
    }

    /// Advances the timestamp counter to at least `seen`, so the next
    /// local write beats any timestamp we just observed from a remote peer.
    fn advance_ts(&self, seen: u64) {
        loop {
            let current = self.timestamp.load(Ordering::Relaxed);
            if seen <= current {
                break;
            }
            if self
                .timestamp
                .compare_exchange(current, seen, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gossip::NoopGossip;

    fn node_id() -> Uuid {
        Uuid::from_u128(1)
    }

    #[tokio::test]
    async fn paint_updates_canvas() {
        let state = AppState::new(node_id(), "0.0.0.0:8080".to_string(), NoopGossip::new());
        state.paint(1, 2, (255, 0, 0, 255)).await;
        let canvas = state.canvas.read().await;
        let pixel = canvas.pixels.get(&(1, 2)).map(|r| r.value());
        assert_eq!(pixel, Some((255, 0, 0, 255)));
    }

    #[tokio::test]
    async fn apply_gossip_merges_state() {
        let state = AppState::new(node_id(), "0.0.0.0:8080".to_string(), NoopGossip::new());
        let mut incoming = CanvasDocument::new();
        incoming.paint(5, 5, (0, 255, 0, 255), node_id(), 999);
        state.apply_gossip(incoming).await;
        let canvas = state.canvas.read().await;
        let pixel = canvas.pixels.get(&(5, 5)).map(|r| r.value());
        assert_eq!(pixel, Some((0, 255, 0, 255)));
    }

    #[tokio::test]
    async fn next_ts_is_monotonic() {
        let state = AppState::new(node_id(), "0.0.0.0:8080".to_string(), NoopGossip::new());
        let t1 = state.next_ts();
        let t2 = state.next_ts();
        let t3 = state.next_ts();
        assert!(t1 < t2);
        assert!(t2 < t3);
    }
}
