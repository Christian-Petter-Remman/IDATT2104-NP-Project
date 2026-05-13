use std::collections::HashSet;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crdt_core::Crdt;
use rand::seq::IteratorRandom;
use serde::{Serialize, de::DeserializeOwned};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, broadcast, watch};
use tokio::time;
use tracing::{debug, trace, warn};

use crate::config::GossipConfig;
use crate::message::{GossipMessage, read_frame, write_frame};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const FANOUT: usize = 2;

type PeerSet = Arc<Mutex<HashSet<SocketAddr>>>;

/// Handle to a running gossip engine.
///
/// The engine spawns its listener and tick loop tasks on construction. The
/// handle is used to mutate the peer set at runtime and to request shutdown.
///
/// # State contract
///
/// * `local` — the engine reads the current local state from this `watch`
///   each time it gossips. The owner of the [`watch::Sender`] must overwrite
///   this value after every local edit.
/// * `merged` — every time the engine receives a remote `Sync` and merges it
///   into the local snapshot, the resulting value is published on this
///   broadcast.
///
/// Consumers of `merged` MUST install the value by **merging** it into their
/// own state (e.g. `watch_tx.send_modify(|s| *s = s.merge(&incoming))`), not
/// by replacing. Multiple peers can deliver concurrently, and each broadcast
/// frame was computed against the watch value at the moment of receive —
/// merging is what guarantees no concurrent delivery is lost. Replacing would
/// drop whichever delivery lost the race to be installed last.
pub struct GossipEngine {
    peers: PeerSet,
    local_addr: SocketAddr,
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

        let peers: PeerSet = Arc::new(Mutex::new(config.peers.iter().copied().collect()));
        let shutdown = Arc::new(Notify::new());

        spawn_listener::<T>(listener, local.clone(), merged, shutdown.clone());
        spawn_ticker::<T>(local, peers.clone(), config.interval, shutdown.clone());

        Ok(Self {
            peers,
            local_addr,
            shutdown,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn add_peer(&self, addr: SocketAddr) {
        self.peers.lock().unwrap().insert(addr);
    }

    pub fn remove_peer(&self, addr: SocketAddr) {
        self.peers.lock().unwrap().remove(&addr);
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
                            tokio::spawn(handle_connection::<T>(stream, peer, local, merged));
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
) where
    T: Crdt + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    match read_frame::<_, T>(&mut stream).await {
        Ok(GossipMessage::Sync(remote)) => {
            debug!(%peer, "received Sync, merging");
            let merged_value = local.borrow().merge(&remote);
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
    peers: PeerSet,
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
                    let targets: Vec<SocketAddr> = {
                        let guard = peers.lock().unwrap();
                        let mut rng = rand::thread_rng();
                        guard.iter().copied().choose_multiple(&mut rng, FANOUT)
                    };
                    for addr in targets {
                        let payload = snapshot.clone();
                        tokio::spawn(async move {
                            match send_sync::<T>(addr, &payload).await {
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

async fn send_sync<T>(addr: SocketAddr, payload: &T) -> io::Result<()>
where
    T: Serialize + Send + Sync,
{
    let mut stream = time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timeout"))??;
    write_frame(&mut stream, &GossipMessage::Sync(payload)).await
}
