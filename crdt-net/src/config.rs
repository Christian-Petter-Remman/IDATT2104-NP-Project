use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use uuid::Uuid;

const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct GossipConfig {
    pub node_id: Uuid,
    pub gossip_addr: SocketAddr,
    /// Address others should use to reach this node (what we put in `from.addr`
    /// of outgoing `Sync` messages and announce over mDNS). If `None`, derived
    /// from `gossip_addr` at engine startup — when `gossip_addr` is a wildcard
    /// (`0.0.0.0`/`::`) the engine falls back to the first non-loopback local
    /// IPv4.
    pub advertise_addr: Option<SocketAddr>,
    /// Bootstrap peers whose `node_id` isn't yet known. The engine tries
    /// gossiping to these addresses each tick until one responds, after which
    /// the peer is migrated into the resolved peer map.
    pub peers: Vec<SocketAddr>,
    pub interval: Duration,
    pub enable_mdns: bool,
}

impl GossipConfig {
    pub fn new(node_id: Uuid, gossip_addr: SocketAddr) -> Self {
        Self {
            node_id,
            gossip_addr,
            advertise_addr: None,
            peers: Vec::new(),
            interval: DEFAULT_INTERVAL,
            enable_mdns: true,
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

    pub fn with_advertise_addr(mut self, addr: SocketAddr) -> Self {
        self.advertise_addr = Some(addr);
        self
    }

    pub fn with_mdns(mut self, enable: bool) -> Self {
        self.enable_mdns = enable;
        self
    }

    /// Resolve the address that should be advertised to peers, given the
    /// configured `advertise_addr` and the bound `gossip_addr`.
    pub(crate) fn resolved_advertise_addr(&self) -> std::io::Result<SocketAddr> {
        if let Some(a) = self.advertise_addr {
            return Ok(a);
        }
        let ip = self.gossip_addr.ip();
        if !ip.is_unspecified() {
            return Ok(self.gossip_addr);
        }
        // Bind is wildcard — pick a non-loopback IPv4 from the OS.
        match local_ip_address::local_ip() {
            Ok(IpAddr::V4(v4)) if !v4.is_loopback() => {
                Ok(SocketAddr::new(IpAddr::V4(v4), self.gossip_addr.port()))
            }
            Ok(other) => Ok(SocketAddr::new(other, self.gossip_addr.port())),
            Err(_) => Ok(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                self.gossip_addr.port(),
            )),
        }
    }
}
