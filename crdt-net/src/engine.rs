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
/// Number of consecutive failed sends to a peer address before we evict it.
/// With the default 1s tick interval this is ~10 seconds of unreachability.
const FAILURE_THRESHOLD: u32 = 10;
/// Maximum number of peers we attempt to notify during graceful shutdown.
const GOODBYE_FANOUT: usize = 4;
const GOODBYE_TIMEOUT: Duration = Duration::from_millis(500);

/// Tracks the set of peers known to this engine.
///
/// `resolved` is keyed by node UUID: one definitive address per remote node.
/// `bootstraps` is a side-set of addresses we were told about (via
/// `config.peers` or `add_bootstrap`) but haven't yet exchanged messages
/// with — we don't know their UUIDs until they reply. Once a bootstrap
/// address gossips with us, it migrates into `resolved`.
///
/// `tombstones` records UUIDs that have departed (gracefully or by
/// repeated failure). 2P-Set semantics: once a UUID is tombstoned it can
/// never be re-added to `resolved`. Tombstones propagate to peers via the
/// `departed` field of every outgoing `Sync` and `Goodbye`.
///
/// `failure_counts` powers the K-consecutive-failure eviction heuristic.
/// Each successful send to an address resets it to zero; each failure
/// increments. When it hits `FAILURE_THRESHOLD` the corresponding UUID (if
/// any) is tombstoned; an unresolved bootstrap is just dropped.
pub(crate) struct PeerRegistry {
    self_id: Uuid,
    self_addr: SocketAddr,
    resolved: Mutex<HashMap<Uuid, SocketAddr>>,
    bootstraps: Mutex<HashSet<SocketAddr>>,
    tombstones: Mutex<HashSet<Uuid>>,
    failure_counts: Mutex<HashMap<SocketAddr, u32>>,
}

impl PeerRegistry {
    fn new(self_id: Uuid, self_addr: SocketAddr) -> Self {
        Self {
            self_id,
            self_addr,
            resolved: Mutex::new(HashMap::new()),
            bootstraps: Mutex::new(HashSet::new()),
            tombstones: Mutex::new(HashSet::new()),
            failure_counts: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn add_resolved(&self, id: Uuid, addr: SocketAddr) {
        if id == self.self_id || addr == self.self_addr {
            return;
        }
        // 2P-Set rule: once tombstoned, never re-added.
        if self.tombstones.lock().unwrap().contains(&id) {
            return;
        }
        self.bootstraps.lock().unwrap().remove(&addr);
        self.resolved.lock().unwrap().insert(id, addr);
    }

    pub(crate) fn add_bootstrap(&self, addr: SocketAddr) {
        if addr == self.self_addr {
            return;
        }
        if self.resolved.lock().unwrap().values().any(|a| *a == addr) {
            return;
        }
        self.bootstraps.lock().unwrap().insert(addr);
    }

    /// Manually drop a peer by UUID. Does **not** add a tombstone — callers
    /// that want propagation should use `tombstone(id)`.
    pub(crate) fn remove(&self, id: Uuid) {
        self.resolved.lock().unwrap().remove(&id);
    }

    /// Tombstone a peer UUID: drop it from `resolved` and add to
    /// `tombstones` so it propagates and can't be re-added.
    pub(crate) fn tombstone(&self, id: Uuid) {
        if id == self.self_id {
            return;
        }
        let mut resolved = self.resolved.lock().unwrap();
        if let Some(addr) = resolved.remove(&id) {
            self.failure_counts.lock().unwrap().remove(&addr);
        }
        drop(resolved);
        self.tombstones.lock().unwrap().insert(id);
    }

    /// Absorb a batch of tombstones learned via gossip.
    pub(crate) fn absorb_tombstones(&self, incoming: &[Uuid]) {
        let mut ts = self.tombstones.lock().unwrap();
        for id in incoming {
            if *id == self.self_id {
                continue; // never tombstone ourselves
            }
            ts.insert(*id);
        }
        // Drop any resolved entries whose UUID is now tombstoned.
        let mut resolved = self.resolved.lock().unwrap();
        let mut failures = self.failure_counts.lock().unwrap();
        resolved.retain(|id, addr| {
            if ts.contains(id) {
                failures.remove(addr);
                false
            } else {
                true
            }
        });
    }

    /// Record a successful send. Resets the failure counter for this
    /// address.
    pub(crate) fn mark_success(&self, addr: SocketAddr) {
        self.failure_counts.lock().unwrap().remove(&addr);
    }

    /// Record a failed send. Returns `Some(id)` if the threshold was hit
    /// and a UUID was tombstoned, `None` otherwise (also `None` for a
    /// bootstrap which is silently dropped on threshold).
    pub(crate) fn mark_failure(&self, addr: SocketAddr) -> Option<Uuid> {
        let mut failures = self.failure_counts.lock().unwrap();
        let count = failures.entry(addr).or_insert(0);
        *count += 1;
        if *count < FAILURE_THRESHOLD {
            return None;
        }
        // Threshold hit: clear the counter for this addr and decide what to evict.
        failures.remove(&addr);
        drop(failures);

        // If the address corresponds to a resolved peer, tombstone its UUID.
        let resolved = self.resolved.lock().unwrap();
        let dead = resolved
            .iter()
            .find(|(_, a)| **a == addr)
            .map(|(id, _)| *id);
        drop(resolved);

        if let Some(id) = dead {
            self.tombstone(id);
            return Some(id);
        }

        // Otherwise it's an unresolved bootstrap — drop it silently.
        self.bootstraps.lock().unwrap().remove(&addr);
        None
    }

    /// Snapshot used by each gossip tick: returns the targets to attempt
    /// this tick (resolved peers + unresolved bootstraps), the `known_peers`
    /// to include in outgoing `Sync` messages, and the current tombstone
    /// set as the `departed` field.
    fn gossip_snapshot(&self) -> (Vec<SocketAddr>, Vec<PeerEntry>, Vec<Uuid>) {
        let resolved = self.resolved.lock().unwrap();
        let bootstraps = self.bootstraps.lock().unwrap();
        let tombstones = self.tombstones.lock().unwrap();

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

        let departed: Vec<Uuid> = tombstones.iter().copied().collect();

        (targets, known, departed)
    }

    /// Snapshot used by graceful shutdown — just the addresses we should
    /// notify, plus the same `known_peers` and `departed` payload.
    fn farewell_snapshot(&self) -> (Vec<SocketAddr>, Vec<PeerEntry>, Vec<Uuid>) {
        // Identical to gossip_snapshot today, but factored separately so
        // we can tune it independently (e.g., prefer resolved over
        // bootstraps for the goodbye).
        self.gossip_snapshot()
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

    pub(crate) fn known_tombstones(&self) -> Vec<Uuid> {
        self.tombstones.lock().unwrap().iter().copied().collect()
    }
}

/// Handle to a running gossip engine.
///
/// The engine spawns its listener, tick loop, and (optionally) mDNS
/// announce and browse tasks on construction. The handle is used to mutate
/// the peer registry at runtime and to request shutdown.
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
///
/// # Graceful shutdown
///
/// Callers who want to leave the mesh cleanly should `await
/// engine.graceful_shutdown()` before dropping. This sends a `Goodbye`
/// message to a few peers so the rest of the mesh learns immediately that
/// this node has departed. Plain `Drop` only stops the engine's tasks; it
/// does not send the farewell (no async in `Drop`).
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
            merged,
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

        if config.enable_mdns
            && let Err(e) =
                discovery::spawn_mdns(self_id, advertise_addr, registry.clone(), shutdown.clone())
        {
            warn!(error = %e, "mDNS init failed; continuing without auto-discovery");
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

    /// Remove a peer from the resolved map by UUID without tombstoning.
    /// The peer may come back via mDNS, bootstrap, or peer-list gossip.
    pub fn remove_peer(&self, node_id: Uuid) {
        self.registry.remove(node_id);
    }

    /// Tombstone a peer by UUID. The tombstone propagates to other peers
    /// via the next gossip tick, and the UUID can no longer be re-added.
    pub fn tombstone_peer(&self, node_id: Uuid) {
        self.registry.tombstone(node_id);
    }

    pub fn known_peers(&self) -> Vec<PeerEntry> {
        self.registry.known_peers()
    }

    pub fn known_tombstones(&self) -> Vec<Uuid> {
        self.registry.known_tombstones()
    }

    /// Notify peers that this node is leaving and stop the background tasks.
    ///
    /// Sends a `Goodbye` message (with this node's UUID in `departed`) to
    /// up to [`GOODBYE_FANOUT`] random peers in parallel, each with a
    /// [`GOODBYE_TIMEOUT`] connect/write deadline. Then triggers the
    /// engine's tasks to exit.
    pub async fn graceful_shutdown(&self) {
        let (targets, known_peers, mut departed) = self.registry.farewell_snapshot();
        // Include ourselves in the goodbye's departed set.
        if !departed.contains(&self.self_id) {
            departed.push(self.self_id);
        }
        let goodbye = GossipMessage::<()>::Goodbye {
            from: PeerEntry {
                node_id: self.self_id,
                addr: self.advertise_addr,
            },
            departed,
            known_peers,
        };

        let mut handles = Vec::new();
        for addr in targets.into_iter().take(GOODBYE_FANOUT) {
            let g = goodbye.clone();
            handles.push(tokio::spawn(async move {
                let _ = time::timeout(GOODBYE_TIMEOUT, send_goodbye(addr, &g)).await;
            }));
        }
        for h in handles {
            let _ = h.await;
        }

        self.shutdown.notify_waiters();
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
            departed,
        }) => {
            debug!(%peer, sender = %from.node_id, "received Sync, merging");
            // Absorb tombstones FIRST so a freshly-tombstoned UUID in
            // `known_peers` can't be re-added by the same message.
            registry.absorb_tombstones(&departed);
            registry.add_resolved(from.node_id, from.addr);
            for entry in known_peers {
                registry.add_resolved(entry.node_id, entry.addr);
            }
            let merged_value = local.borrow().merge(&state);
            let _ = merged.send(merged_value);
        }
        Ok(GossipMessage::Goodbye {
            from,
            departed,
            known_peers,
        }) => {
            debug!(%peer, sender = %from.node_id, "received Goodbye");
            registry.absorb_tombstones(&departed);
            for entry in known_peers {
                registry.add_resolved(entry.node_id, entry.addr);
            }
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
                    let (all_targets, known_peers, departed) = registry.gossip_snapshot();
                    let chosen: Vec<SocketAddr> = {
                        let mut rng = rand::thread_rng();
                        all_targets.into_iter().choose_multiple(&mut rng, FANOUT)
                    };
                    let from = PeerEntry { node_id: self_id, addr: advertise_addr };
                    for addr in chosen {
                        let payload = snapshot.clone();
                        let from = from.clone();
                        let known = known_peers.clone();
                        let dep = departed.clone();
                        let registry = registry.clone();
                        tokio::spawn(async move {
                            match send_sync::<T>(addr, from, payload, known, dep).await {
                                Ok(()) => {
                                    registry.mark_success(addr);
                                    debug!(%addr, "gossip send ok");
                                }
                                Err(e) => {
                                    let evicted = registry.mark_failure(addr);
                                    if let Some(id) = evicted {
                                        debug!(%addr, %id, "peer evicted after repeated failures");
                                    }
                                    warn!(%addr, error = %e, "gossip send failed");
                                }
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
    departed: Vec<Uuid>,
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
        departed,
    };
    write_frame(&mut stream, &msg).await
}

async fn send_goodbye(addr: SocketAddr, msg: &GossipMessage<()>) -> io::Result<()> {
    let mut stream = time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timeout"))??;
    write_frame::<_, ()>(&mut stream, msg).await
}
