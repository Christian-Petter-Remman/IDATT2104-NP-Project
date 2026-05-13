use std::net::SocketAddr;
use std::time::Duration;

use uuid::Uuid;

const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct GossipConfig {
    pub node_id: Uuid,
    pub gossip_addr: SocketAddr,
    pub peers: Vec<SocketAddr>,
    pub interval: Duration,
}

impl GossipConfig {
    pub fn new(node_id: Uuid, gossip_addr: SocketAddr) -> Self {
        Self {
            node_id,
            gossip_addr,
            peers: Vec::new(),
            interval: DEFAULT_INTERVAL,
        }
    }

    pub fn with_peers(mut self, peers: Vec<SocketAddr>) -> Self {
        self.peers = peers;
        self
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    pub fn with_interval_secs(self, secs: u64) -> Self {
        self.with_interval(Duration::from_secs(secs))
    }
}
