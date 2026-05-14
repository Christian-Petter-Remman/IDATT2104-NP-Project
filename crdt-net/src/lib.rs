//! Peer-to-peer gossip transport for state-based CRDTs.
//!
//! The crate exposes a generic [`GossipEngine`] over any type implementing
//! [`crdt_core::Crdt`] plus serde traits. The engine periodically pushes the
//! latest local state to up to two random peers over TCP, merges incoming
//! states into the local snapshot, publishes the merged result on a
//! broadcast channel, and (optionally) auto-discovers peers on the local
//! network via mDNS.

// Internal modules — types are re-exported at the crate root below. Keeping
// the modules `pub(crate)` prevents external callers from depending on
// internal items (e.g. `PeerRegistry`, raw codec helpers) via the module
// path. If a new public surface item is needed, re-export it explicitly.
pub(crate) mod config;
pub(crate) mod discovery;
pub(crate) mod engine;
pub(crate) mod message;

pub use config::GossipConfig;
pub use engine::GossipEngine;
pub use message::{GossipMessage, MAX_FRAME, PeerEntry};
