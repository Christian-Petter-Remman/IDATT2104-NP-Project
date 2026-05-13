use tokio::sync::broadcast;
use crate::canvas::CanvasDocument;

/// Interface between crdt-app and the gossip layer.
/// crdt-net implements this trait for its GossipEngine.
pub trait GossipBackend: Send + Sync + 'static {
    fn subscribe(&self) -> broadcast::Receiver<CanvasDocument>;
    fn publish(&self, doc: CanvasDocument);
}

/// No-op backend for running without a gossip layer (dev / testing).
#[derive(Clone)]
pub struct NoopGossip;

impl GossipBackend for NoopGossip {
    fn subscribe(&self) -> broadcast::Receiver<CanvasDocument> {
        broadcast::channel(1).1
    }
    fn publish(&self, _doc: CanvasDocument) {}
}
