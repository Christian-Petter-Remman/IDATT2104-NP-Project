use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;
use crdt_core::Crdt;
use crate::canvas::{CanvasDocument, Rgba};
use crate::gossip::GossipBackend;

pub struct AppState<G: GossipBackend> {
    pub node_id: Uuid,
    pub canvas: RwLock<CanvasDocument>,
    pub gossip: G,
    pub ws_tx: broadcast::Sender<CanvasDocument>,
}

impl<G: GossipBackend> AppState<G> {
    pub fn new(node_id: Uuid, gossip: G) -> Arc<Self> {
        let (ws_tx, _) = broadcast::channel(64);
        Arc::new(Self {
            node_id,
            canvas: RwLock::new(CanvasDocument::new()),
            gossip,
            ws_tx,
        })
    }

    pub async fn paint(&self, x: u8, y: u8, color: Rgba, timestamp: u64) {
        let mut canvas = self.canvas.write().await;
        canvas.paint(x, y, color, self.node_id, timestamp);
        let snapshot = canvas.clone();
        drop(canvas);
        let _ = self.ws_tx.send(snapshot.clone());
        self.gossip.publish(snapshot);
    }

    pub async fn apply_gossip(&self, incoming: CanvasDocument) {
        let mut canvas = self.canvas.write().await;
        canvas.merge(incoming);
        let snapshot = canvas.clone();
        drop(canvas);
        let _ = self.ws_tx.send(snapshot);
    }
}
