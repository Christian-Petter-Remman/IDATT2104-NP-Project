use std::io;
use std::net::SocketAddr;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

/// Maximum allowed frame size on the wire.
///
/// Sized for ~4× the worst-case `CanvasDocument` payload (full 64×64 LWW
/// pixel map + palette + active_peers + cursors, JSON-encoded, settles
/// well under 1 MiB). Keeping this tight reduces the worst-case
/// allocation a malicious peer can drive: `read_frame` allocates up to
/// `MAX_FRAME` bytes per inbound connection.
pub const MAX_FRAME: usize = 4 * 1024 * 1024;

/// A peer's identity and reachable address.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerEntry {
    pub node_id: Uuid,
    pub addr: SocketAddr,
}

/// Wire-level gossip message.
///
/// `Sync` carries the sender's full CRDT state plus membership/tombstone
/// hints. `Goodbye` is a state-free farewell that just propagates
/// tombstones — it lets a peer leave the mesh cleanly without forcing
/// callers to also serialize a final state snapshot.
///
/// The `T` parameter on `Goodbye` is phantom: the variant has no field
/// that uses it, so a sender can construct
/// `GossipMessage::<()>::Goodbye {...}` and any receiver will decode the
/// same bytes regardless of which `T` they parameterize with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage<T> {
    Sync {
        from: PeerEntry,
        state: T,
        known_peers: Vec<PeerEntry>,
        departed: Vec<Uuid>,
    },
    Goodbye {
        from: PeerEntry,
        departed: Vec<Uuid>,
        known_peers: Vec<PeerEntry>,
    },
}

pub async fn write_frame<W, T>(w: &mut W, msg: &GossipMessage<T>) -> io::Result<()>
where
    W: AsyncWriteExt + Unpin,
    T: Serialize,
{
    let bytes = serde_json::to_vec(msg).map_err(io::Error::other)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "outgoing frame exceeds MAX_FRAME",
        ));
    }
    let len = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_frame<R, T>(r: &mut R) -> io::Result<GossipMessage<T>>
where
    R: AsyncReadExt + Unpin,
    T: DeserializeOwned,
{
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incoming frame exceeds MAX_FRAME",
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}
