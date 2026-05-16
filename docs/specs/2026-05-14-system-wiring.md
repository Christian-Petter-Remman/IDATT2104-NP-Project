# System Wiring Plan

**Date:** 2026-05-14  
**Objective:** Connect crdt-core, crdt-net, crdt-app, and frontend into a working end-to-end system.

---

## Current state

| Crate/Layer | Status |
|---|---|
| `crdt-core` | Complete — all CRDTs implemented |
| `crdt-net` | Complete — `GossipEngine` fully implemented, generic over `T: Crdt` |
| `crdt-app` | Partial — still uses `NoopGossip`, missing fields and routes |
| Frontend | Partial — components exist, but WS snapshot fields are missing from backend |

---

## Architecture decision: Option B — direct channels

`crdt-net`'s `GossipEngine::run` uses idiomatic Rust async channels:
- `watch::Receiver<T>` — engine reads latest local state each gossip tick
- `broadcast::Sender<T>` — engine publishes merged results after receiving a peer sync

The `GossipBackend` trait in `crdt-app/gossip.rs` was a placeholder written before `crdt-net` existed. It is now dead weight. Remove it. `AppState` holds channels directly — no generic, no trait, no adapter.

**Dependency chain:** `crdt-core ← crdt-net ← crdt-app`

---

## What frontend components need from the WS snapshot

| Component | Fields needed | Currently served |
|---|---|---|
| PixelCanvas | `pixels: {"x,y": [r,g,b,a]}` | ✓ works |
| PeerList | `active_peers: [uuid]`, `paint_total: n` | ✗ missing |
| Leaderboard | `leaderboard: [{peer_id, pixels}]`, `paint_total` | ✗ missing |
| ColorPicker | `palette: [[r,g,b,a]]` | ✗ missing |

---

## Known blockers

### 1. GossipBackend trait creates a dead abstraction
`AppState<G: GossipBackend>` propagates through every handler in `api.rs`. Remove the trait, remove the generic. `AppState` becomes a plain struct holding `watch::Sender<CanvasDocument>`.

### 2. serde_json rejects tuple map keys
`HashMap<(u8,u8), LWWRegister<Rgba>>` (pixels) and `ORSet<Rgba>`'s internal `HashMap<(u8,u8,u8,u8), HashSet<Tag>>` (palette) both fail `serde_json` at runtime — JSON object keys must be strings. Currently hidden because `NoopGossip` never serializes. Wire the real engine → immediate panic.

Fix for pixels: inline `serde(with = "...")` module in `canvas.rs`.  
Fix for ORSet: manual `Serialize`/`Deserialize` impl in `crdt-core/src/sets/orset.rs` that serializes `entries` as `Vec<(T, Vec<Tag>)>` instead of a map.

### 3. CanvasDocument missing fields
`palette: ORSet<Rgba>` and `paint_counts: GCounter` are not in the struct. Frontend expects `palette`, `paint_total`, and `leaderboard` in the WS snapshot.

### 4. No `--gossip-port` / `--peers` CLI args
`main.rs` uses manual `--port` parsing only. Cannot start `GossipEngine` without a gossip address.

---

## Files to change (in order)

| # | File | Change |
|---|---|---|
| 1 | `crdt-core/src/registers/lww_register.rs` | Add `pub fn node_id() -> NodeId` getter |
| 2 | `crdt-core/src/sets/orset.rs` | Replace derived serde with manual impl (Vec-of-pairs for entries) |
| 3 | `crdt-app/Cargo.toml` | Add `crdt-net`, `clap`, `tower-http` |
| 4 | `crdt-app/src/canvas.rs` | Fix pixels serde, add `palette`+`paint_counts`, expand `CanvasView` |
| 5 | `crdt-app/src/gossip.rs` | Delete |
| 6 | `crdt-app/src/state.rs` | Remove `G: GossipBackend`, add `local_tx: watch::Sender`, add palette methods |
| 7 | `crdt-app/src/api.rs` | Remove all `G: GossipBackend` bounds, add palette + leaderboard routes |
| 8 | `crdt-app/src/main.rs` | clap args, create channels, start `GossipEngine::run`, wire listener |

---

## Step 1 — `crdt-core/src/registers/lww_register.rs`

Add alongside `timestamp()`:

```rust
pub fn node_id(&self) -> NodeId {
    self.node_id
}
```

Required by `CanvasDocument::ownership_leaderboard()`.

---

## Step 2 — `crdt-core/src/sets/orset.rs`

Remove the `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]` line from `ORSet`. Replace with manual impls:

```rust
#[cfg(feature = "serde")]
impl<T> serde::Serialize for ORSet<T>
where
    T: Eq + std::hash::Hash + Clone + serde::Serialize,
{
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("ORSet", 3)?;
        let entries_vec: Vec<(&T, Vec<&Tag>)> = self
            .entries.iter()
            .map(|(k, v)| (k, v.iter().collect()))
            .collect();
        st.serialize_field("entries", &entries_vec)?;
        st.serialize_field("removed_tags", &self.removed_tags.iter().collect::<Vec<_>>())?;
        st.serialize_field("counter", &self.counter)?;
        st.end()
    }
}

#[cfg(feature = "serde")]
impl<'de, T> serde::Deserialize<'de> for ORSet<T>
where
    T: Eq + std::hash::Hash + Clone + serde::Deserialize<'de>,
{
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Helper<T> {
            entries: Vec<(T, Vec<Tag>)>,
            removed_tags: Vec<Tag>,
            counter: u64,
        }
        let h = Helper::<T>::deserialize(d)?;
        Ok(ORSet {
            entries: h.entries.into_iter()
                .map(|(k, v)| (k, v.into_iter().collect()))
                .collect(),
            removed_tags: h.removed_tags.into_iter().collect(),
            counter: h.counter,
        })
    }
}
```

`Tag` keeps its derived serde.

---

## Step 3 — `crdt-app/Cargo.toml`

Add dependencies:

```toml
crdt-net   = { path = "../crdt-net" }
clap       = { version = "4", features = ["derive"] }
tower-http = { version = "0.5", features = ["cors"] }
```

---

## Step 4 — `crdt-app/src/canvas.rs`

### 4a — Fix pixels serde

Annotate the `pixels` field with `#[serde(with = "pixel_map_serde")]` and add the module:

```rust
mod pixel_map_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(
        map: &HashMap<PixelCoord, LWWRegister<Rgba>>,
        s: S,
    ) -> Result<S::Ok, S::Error>
    where S: Serializer {
        map.iter().collect::<Vec<_>>().serialize(s)
    }

    pub fn deserialize<'de, D>(
        d: D,
    ) -> Result<HashMap<PixelCoord, LWWRegister<Rgba>>, D::Error>
    where D: Deserializer<'de> {
        Vec::<(PixelCoord, LWWRegister<Rgba>)>::deserialize(d)
            .map(|v| v.into_iter().collect())
    }
}
```

Serializes pixels as `[[[x,y], {...lww...}], ...]` — valid JSON, lossless round-trip.  
`cursors: HashMap<Uuid, LWWRegister<PixelCoord>>` is fine — Uuid keys serialize as strings.

### 4b — Add fields to CanvasDocument

```rust
use crdt_core::counters::GCounter;

#[derive(Clone, Serialize, Deserialize)]
pub struct CanvasDocument {
    #[serde(with = "pixel_map_serde")]
    pub pixels:       HashMap<PixelCoord, LWWRegister<Rgba>>,
    users:            ORSet<Uuid>,
    pub cursors:      HashMap<Uuid, LWWRegister<PixelCoord>>,
    pub palette:      ORSet<Rgba>,
    pub paint_counts: GCounter,
}
```

Update `Default` / `new()` to include `palette: ORSet::new(), paint_counts: GCounter::new()`.

### 4c — Update methods

`paint()` — add after setting pixel:
```rust
self.paint_counts.increment(node_id);
```

New methods:
```rust
pub fn add_palette_color(&mut self, color: Rgba, node_id: &NodeId) {
    self.palette.insert(color, node_id);
}

pub fn remove_palette_color(&mut self, color: &Rgba) -> bool {
    self.palette.remove(color)
}

pub fn palette_colors(&self) -> Vec<Rgba> {
    let mut colors: Vec<Rgba> = self.palette.value().into_iter().collect();
    colors.sort();
    colors
}

pub fn ownership_leaderboard(&self) -> Vec<(NodeId, u64)> {
    let mut counts: HashMap<NodeId, u64> = HashMap::new();
    for reg in self.pixels.values() {
        *counts.entry(reg.node_id()).or_insert(0) += 1;
    }
    let mut result: Vec<(NodeId, u64)> = counts.into_iter().collect();
    result.sort_by(|a, b| b.1.cmp(&a.1));
    result
}
```

Update `Crdt::merge` to include:
```rust
self.palette.merge(other.palette);
self.paint_counts.merge(other.paint_counts);
```

Update `Crdt::compare` to include palette and paint_counts.

### 4d — Expand CanvasView

```rust
#[derive(Serialize)]
pub struct CanvasView {
    pub pixels:       HashMap<String, [u8; 4]>,  // "x,y" → [r,g,b,a]
    pub active_peers: Vec<String>,
    pub palette:      Vec<[u8; 4]>,
    pub paint_total:  u64,
    pub leaderboard:  Vec<LeaderboardEntry>,
}

#[derive(Serialize)]
pub struct LeaderboardEntry {
    pub peer_id: String,
    pub pixels:  u64,
}
```

Update `From<&CanvasDocument> for CanvasView`:
- `pixels` key: `"{x},{y}"` (comma — matches frontend `canvas.js`)
- `active_peers`: `doc.active_users()` sorted
- `palette`: `doc.palette_colors()` as `Vec<[u8;4]>`
- `paint_total`: `doc.paint_counts.value()`
- `leaderboard`: `doc.ownership_leaderboard()` → `Vec<LeaderboardEntry>`

---

## Step 5 — Delete `crdt-app/src/gossip.rs`

Remove file. Remove `mod gossip;` from `main.rs`.

---

## Step 6 — Rewrite `crdt-app/src/state.rs`

Remove the `G: GossipBackend` generic. Replace `gossip: G` with `local_tx: watch::Sender<CanvasDocument>`.

New struct:
```rust
pub struct AppState {
    pub node_id:  Uuid,
    pub addr:     String,
    pub canvas:   RwLock<CanvasDocument>,
    pub ws_tx:    broadcast::Sender<CanvasDocument>,
    local_tx:     watch::Sender<CanvasDocument>,
    timestamp:    AtomicU64,
}
```

New constructor:
```rust
pub fn new(node_id: Uuid, addr: String, local_tx: watch::Sender<CanvasDocument>) -> Arc<Self>
```

`paint()` replaces `self.gossip.publish(snapshot)` with `let _ = self.local_tx.send(snapshot.clone())`.

`apply_gossip()` adds `let _ = self.local_tx.send(snapshot.clone())` after merge.

New palette methods:
```rust
pub async fn add_palette_color(&self, color: Rgba) {
    let mut canvas = self.canvas.write().await;
    canvas.add_palette_color(color, &self.node_id);
    let snap = canvas.clone(); drop(canvas);
    let _ = self.local_tx.send(snap.clone());
    let _ = self.ws_tx.send(snap);
}

pub async fn remove_palette_color(&self, color: Rgba) {
    let mut canvas = self.canvas.write().await;
    canvas.remove_palette_color(&color);
    let snap = canvas.clone(); drop(canvas);
    let _ = self.local_tx.send(snap.clone());
    let _ = self.ws_tx.send(snap);
}
```

Update tests: replace `NoopGossip::new()` with:
```rust
let (tx, _) = tokio::sync::watch::channel(CanvasDocument::new());
AppState::new(node_id(), "0.0.0.0:8080".into(), tx)
```

---

## Step 7 — Update `crdt-app/src/api.rs`

Remove all `G: GossipBackend` type parameter bounds. `AppState` is no longer generic.

```rust
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/canvas",       get(get_canvas))
        .route("/api/canvas/paint", post(paint))
        .route("/api/node",         get(node_info))
        .route("/api/palette",      get(get_palette).post(add_palette).delete(remove_palette))
        .route("/api/leaderboard",  get(get_leaderboard))
        .route("/ws",               get(ws_handler))
        .with_state(state)
        .layer(CorsLayer::permissive())
}
```

New request struct:
```rust
#[derive(Deserialize)]
pub struct PaletteRequest { pub color: [u8; 4] }
```

New handlers:
```rust
async fn get_palette(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let canvas = s.canvas.read().await;
    Json(canvas.palette_colors().into_iter().map(|(r,g,b,a)| [r,g,b,a]).collect::<Vec<_>>())
}

async fn add_palette(State(s): State<Arc<AppState>>, Json(req): Json<PaletteRequest>)
    -> impl IntoResponse
{
    s.add_palette_color((req.color[0], req.color[1], req.color[2], req.color[3])).await;
    StatusCode::CREATED
}

async fn remove_palette(State(s): State<Arc<AppState>>, Json(req): Json<PaletteRequest>)
    -> impl IntoResponse
{
    s.remove_palette_color((req.color[0], req.color[1], req.color[2], req.color[3])).await;
    StatusCode::NO_CONTENT
}

async fn get_leaderboard(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let canvas = s.canvas.read().await;
    let board: Vec<LeaderboardEntry> = canvas.ownership_leaderboard()
        .into_iter()
        .map(|(id, n)| LeaderboardEntry { peer_id: id.to_string(), pixels: n })
        .collect();
    Json(board)
}
```

---

## Step 8 — Rewrite `crdt-app/src/main.rs`

```rust
mod api;
mod canvas;
mod state;

use canvas::CanvasDocument;
use crdt_net::{GossipConfig, GossipEngine};
use state::AppState;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

#[derive(clap::Parser)]
struct Args {
    #[arg(long, default_value_t = 8080)]  port: u16,
    #[arg(long, default_value_t = 9090)]  gossip_port: u16,
    /// Comma-separated bootstrap peers, e.g. 127.0.0.1:9091,127.0.0.1:9092
    #[arg(long, default_value = "")]      peers: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args = <Args as clap::Parser>::parse();
    let node_id = Uuid::new_v4();
    let http_addr = format!("0.0.0.0:{}", args.port);

    let bootstrap: Vec<std::net::SocketAddr> = args.peers
        .split(',').filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok()).collect();

    let (local_tx, local_rx) = watch::channel(CanvasDocument::new());
    let (merged_tx, _)       = broadcast::channel::<CanvasDocument>(64);

    let gossip_addr: std::net::SocketAddr =
        format!("0.0.0.0:{}", args.gossip_port).parse().unwrap();
    let config = GossipConfig::new(node_id, gossip_addr)
        .with_peers(bootstrap)
        .with_mdns(false);

    let _engine = GossipEngine::run(config, local_rx, merged_tx.clone())
        .await
        .expect("gossip engine failed to start");

    let state = AppState::new(node_id, http_addr.clone(), local_tx);

    let state_clone = Arc::clone(&state);
    let mut merged_rx = merged_tx.subscribe();
    tokio::spawn(async move {
        while let Ok(incoming) = merged_rx.recv().await {
            state_clone.apply_gossip(incoming).await;
        }
        tracing::warn!("gossip listener exited");
    });

    tracing::info!("node {} http={} gossip={}", node_id, http_addr, gossip_addr);

    let listener = tokio::net::TcpListener::bind(&http_addr).await
        .expect("failed to bind");
    axum::serve(listener, api::router(state)).await.expect("server error");
}
```

---

## Verification

```powershell
# Build in dependency order
cargo build -p crdt-core
cargo build -p crdt-net
cargo build -p crdt-app

# All tests
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Two-node gossip smoke test
# Terminal 1:
cargo run -p crdt-app -- --port 8080 --gossip-port 9090 --peers 127.0.0.1:9091
# Terminal 2:
cargo run -p crdt-app -- --port 8081 --gossip-port 9091 --peers 127.0.0.1:9090

# Paint on node 1
Invoke-RestMethod -Method POST -Uri http://localhost:8080/api/canvas/paint `
  -ContentType application/json -Body '{"x":5,"y":5,"color":[255,0,0,255]}'

# After ~10s: node 2 should have the pixel
Invoke-RestMethod http://localhost:8081/api/canvas   # pixels."5,5" = [255,0,0,255]

# Frontend
cd frontend && npm run dev
# Tab 1 → port 8080, Tab 2 → port 8081
# Paint on one → appears on the other after gossip interval
```
