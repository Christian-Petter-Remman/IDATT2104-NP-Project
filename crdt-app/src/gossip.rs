use std::sync::Arc;
use tokio::sync::broadcast;
use crate::canvas::CanvasDocument;

/// Interface between crdt-app and the gossip layer.
/// crdt-net implements this trait for its GossipEngine.
pub trait GossipBackend: Send + Sync + 'static {
    fn subscribe(&self) -> broadcast::Receiver<CanvasDocument>;
    fn publish(&self, doc: CanvasDocument);
}

const GOSSIP_CHANNEL_CAPACITY: usize = 64;

/// No-op backend for running without a gossip layer (dev / testing).
/// Holds the sender so receivers don't get Closed immediately.
#[derive(Clone)]
pub struct NoopGossip {
    tx: Arc<broadcast::Sender<CanvasDocument>>,
}

impl NoopGossip {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(GOSSIP_CHANNEL_CAPACITY);
        Self { tx: Arc::new(tx) }
    }
}

impl GossipBackend for NoopGossip {
    fn subscribe(&self) -> broadcast::Receiver<CanvasDocument> {
        self.tx.subscribe()
    }
    fn publish(&self, _doc: CanvasDocument) {}
}
