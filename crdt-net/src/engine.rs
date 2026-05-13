use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crdt_core::Crdt;
use rand::seq::IteratorRandom;
use serde::{Serialize, de::DeserializeOwned};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, broadcast, watch};
use tokio::time;
use tracing::{debug, trace, warn};
use uuid::Uuid;

use crate::config::GossipConfig;
use crate::discovery;
use crate::message::{GossipMessage, PeerEntry, read_frame, write_frame};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const FANOUT: usize = 2;
const KNOWN_PEERS_CAP: usize = 64;

/// Tracks the set of peers known to this engine.
///
/// `resolved` is keyed by node UUID: one definitive address per remote node.
/// `bootstraps` is a side-set of addresses we were told about (via
/// `config.peers` or `add_bootstrap`) but haven't yet exchanged messages
/// with — we don't know their UUIDs until they reply. Once a bootstrap
/// address gossips with us, it migrates into `resolved`.
pub(crate) struct PeerRegistry {
    self_id: Uuid,
    self_addr: SocketAddr,
    resolved: Mutex<HashMap<Uuid, SocketAddr>>,
    bootstraps: Mutex<HashSet<SocketAddr>>,
}

impl PeerRegistry {
    fn new(self_id: Uuid, self_addr: SocketAddr) -> Self {
        Self {
            self_id,
            self_addr,
            resolved: Mutex::new(HashMap::new()),
            bootstraps: Mutex::new(HashSet::new()),
        }
    }

    pub(crate) fn add_resolved(&self, id: Uuid, addr: SocketAddr) {
        if id == self.self_id || addr == self.self_addr {
            return;
        }
        // Migrate any matching bootstrap entry over to resolved.
        self.bootstraps.lock().unwrap().remove(&addr);
        self.resolved.lock().unwrap().insert(id, addr);
    }

    pub(crate) fn add_bootstrap(&self, addr: SocketAddr) {
        if addr == self.self_addr {
            return;
        }
        // If we already have a resolved entry at this address, no point adding.
        if self
            .resolved
            .lock()
            .unwrap()
            .values()
            .any(|a| *a == addr)
        {
            return;
        }
        self.bootstraps.lock().unwrap().insert(addr);
    }

    pub(crate) fn remove(&self, id: Uuid) {
        self.resolved.lock().unwrap().remove(&id);
    }

    /// Snapshot of (gossip targets this tick, known_peers payload).
    fn snapshot(&self) -> (Vec<SocketAddr>, Vec<PeerEntry>) {
        let resolved = self.resolved.lock().unwrap();
        let bootstraps = self.bootstraps.lock().unwrap();

        let mut targets: Vec<SocketAddr> = resolved.values().copied().collect();
        targets.extend(bootstraps.iter().copied());

        let known: Vec<PeerEntry> = resolved
            .iter()
            .take(KNOWN_PEERS_CAP)
            .map(|(id, addr)| PeerEntry {
                node_id: *id,
                addr: *addr,
            })
            .collect();

        (targets, known)
    }

    pub(crate) fn known_peers(&self) -> Vec<PeerEntry> {
        self.resolved
            .lock()
            .unwrap()
            .iter()
            .map(|(id, addr)| PeerEntry {
                node_id: *id,
                addr: *addr,
            })
            .collect()
    }
}

/// Handle to a running gossip engine.
///
/// The engine spawns its listener, tick loop, and (optionally) mDNS announce
/// + browse tasks on construction. The handle is used to mutate the peer
/// registry at runtime and to request shutdown.
///
/// # State contract
///
/// * `local` — the engine reads the current local state from this `watch`
///   each time it gossips.
/// * `merged` — every time the engine receives a remote `Sync` and merges it
///   into the local snapshot, the resulting value is published on this
///   broadcast.
///
/// Consumers of `merged` MUST install the value by **merging** it into their
/// own state (e.g. `watch_tx.send_modify(|s| *s = s.merge(&incoming))`), not
/// by replacing. See `docs/crdt-net.md` §7.
pub struct GossipEngine {
    registry: Arc<PeerRegistry>,
    self_id: Uuid,
    local_addr: SocketAddr,
    advertise_addr: SocketAddr,
    shutdown: Arc<Notify>,
}

impl GossipEngine {
    pub async fn run<T>(
        config: GossipConfig,
        local: watch::Receiver<T>,
        merged: broadcast::Sender<T>,
    ) -> io::Result<Self>
    where
        T: Crdt + Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        let listener = TcpListener::bind(config.gossip_addr).await?;
        let local_addr = listener.local_addr()?;
        let advertise_addr = resolve_advertise_addr(&config, local_addr);
        let self_id = config.node_id;

        let registry = Arc::new(PeerRegistry::new(self_id, advertise_addr));
        for boot in &config.peers {
            registry.add_bootstrap(*boot);
        }

        let shutdown = Arc::new(Notify::new());

        spawn_listener::<T>(
            listener,
            local.clone(),
            merged.clone(),
            registry.clone(),
            shutdown.clone(),
        );
        spawn_ticker::<T>(
            local,
            registry.clone(),
            self_id,
            advertise_addr,
            config.interval,
            shutdown.clone(),
        );

        if config.enable_mdns {
            if let Err(e) = discovery::spawn_mdns(
                self_id,
                advertise_addr,
                registry.clone(),
                shutdown.clone(),
            ) {
                warn!(error = %e, "mDNS init failed; continuing without auto-discovery");
            }
        }

        Ok(Self {
            registry,
            self_id,
            local_addr,
            advertise_addr,
            shutdown,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn advertise_addr(&self) -> SocketAddr {
        self.advertise_addr
    }

    pub fn node_id(&self) -> Uuid {
        self.self_id
    }

    /// Add a peer we know the UUID of (e.g. from mDNS resolution).
    pub fn add_peer(&self, node_id: Uuid, addr: SocketAddr) {
        self.registry.add_resolved(node_id, addr);
    }

    /// Add a peer address whose UUID we don't yet know. The engine will
    /// attempt to gossip to it until it responds, at which point it migrates
    /// to the resolved peer map.
    pub fn add_bootstrap(&self, addr: SocketAddr) {
        self.registry.add_bootstrap(addr);
    }

    pub fn remove_peer(&self, node_id: Uuid) {
        self.registry.remove(node_id);
    }

    pub fn known_peers(&self) -> Vec<PeerEntry> {
        self.registry.known_peers()
    }

    pub fn shutdown(&self) {
        self.shutdown.notify_waiters();
    }
}

impl Drop for GossipEngine {
    fn drop(&mut self) {
        self.shutdown.notify_waiters();
    }
}

fn spawn_listener<T>(
    listener: TcpListener,
    local: watch::Receiver<T>,
    merged: broadcast::Sender<T>,
    registry: Arc<PeerRegistry>,
    shutdown: Arc<Notify>,
) where
    T: Crdt + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown.notified() => {
                    debug!("listener shutdown");
                    return;
                }
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, peer)) => {
                            debug!(%peer, "accepted gossip connection");
                            let local = local.clone();
                            let merged = merged.clone();
                            let registry = registry.clone();
                            tokio::spawn(handle_connection::<T>(stream, peer, local, merged, registry));
                        }
                        Err(e) => {
                            warn!(error = %e, "accept failed");
                        }
                    }
                }
            }
        }
    });
}

async fn handle_connection<T>(
    mut stream: TcpStream,
    peer: SocketAddr,
    local: watch::Receiver<T>,
    merged: broadcast::Sender<T>,
    registry: Arc<PeerRegistry>,
) where
    T: Crdt + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    match read_frame::<_, T>(&mut stream).await {
        Ok(GossipMessage::Sync {
            from,
            state,
            known_peers,
        }) => {
            debug!(%peer, sender = %from.node_id, "received Sync, merging");
            registry.add_resolved(from.node_id, from.addr);
            for entry in known_peers {
                registry.add_resolved(entry.node_id, entry.addr);
            }
            let merged_value = local.borrow().merge(&state);
            // Receivers may not exist yet; that's fine.
            let _ = merged.send(merged_value);
        }
        Err(e) => {
            trace!(error = %e, %peer, "discarding malformed frame");
        }
    }
}

fn spawn_ticker<T>(
    local: watch::Receiver<T>,
    registry: Arc<PeerRegistry>,
    self_id: Uuid,
    advertise_addr: SocketAddr,
    interval: Duration,
    shutdown: Arc<Notify>,
) where
    T: Crdt + Serialize + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut ticker = time::interval(interval);
        ticker.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        // Skip the immediate first tick so callers can set up subscribers.
        ticker.tick().await;
        loop {
            tokio::select! {
                _ = shutdown.notified() => {
                    debug!("ticker shutdown");
                    return;
                }
                _ = ticker.tick() => {
                    let snapshot = local.borrow().clone();
                    let (all_targets, known_peers) = registry.snapshot();
                    let chosen: Vec<SocketAddr> = {
                        let mut rng = rand::thread_rng();
                        all_targets.into_iter().choose_multiple(&mut rng, FANOUT)
                    };
                    let from = PeerEntry { node_id: self_id, addr: advertise_addr };
                    for addr in chosen {
                        let payload = snapshot.clone();
                        let from = from.clone();
                        let known = known_peers.clone();
                        tokio::spawn(async move {
                            match send_sync::<T>(addr, from, payload, known).await {
                                Ok(()) => debug!(%addr, "gossip send ok"),
                                Err(e) => warn!(%addr, error = %e, "gossip send failed"),
                            }
                        });
                    }
                }
            }
        }
    });
}

fn resolve_advertise_addr(config: &GossipConfig, bound: SocketAddr) -> SocketAddr {
    if let Some(a) = config.advertise_addr {
        return a;
    }
    let port = bound.port();
    let ip = config.gossip_addr.ip();
    if !ip.is_unspecified() {
        return SocketAddr::new(ip, port);
    }
    // Wildcard bind — pick a non-loopback local IP. Fall back to loopback if
    // OS interrogation fails.
    let ip = match local_ip_address::local_ip() {
        Ok(IpAddr::V4(v4)) if !v4.is_loopback() => IpAddr::V4(v4),
        Ok(other) => other,
        Err(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
    };
    SocketAddr::new(ip, port)
}

async fn send_sync<T>(
    addr: SocketAddr,
    from: PeerEntry,
    state: T,
    known_peers: Vec<PeerEntry>,
) -> io::Result<()>
where
    T: Serialize + Send + Sync,
{
    let mut stream = time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timeout"))??;
    let msg = GossipMessage::Sync {
        from,
        state,
        known_peers,
    };
    write_frame(&mut stream, &msg).await
}
