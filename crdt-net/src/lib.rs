//! Peer-to-peer gossip transport for state-based CRDTs.
//!
//! The crate exposes a generic [`GossipEngine`] over any type implementing
//! [`crdt_core::Crdt`] plus serde traits. The engine periodically pushes the
//! latest local state to up to two random peers over TCP and merges incoming
//! states into the local snapshot, publishing the merged result on a
//! broadcast channel.

pub mod config;
pub mod engine;
pub mod message;

pub use config::GossipConfig;
pub use engine::GossipEngine;
pub use message::{GossipMessage, MAX_FRAME};
