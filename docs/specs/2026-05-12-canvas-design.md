# IDATT2104 CRDT Project — Collaborative Pixel Canvas

## Context

Course project for IDATT2104 Nettverksprogrammering. Goal: grade A. Requirements:
- Implement CRDTs from scratch in Rust (no external CRDT libraries)
- P2P architecture with gossip protocol
- README with CI link, docs, install instructions
- Deadline: 2026-05-26 23:59

**Decisions:**
- Language: Rust (Cargo workspace, 3 crates)
- Architecture: Peer-to-peer with gossip protocol
- Demo: Collaborative pixel art canvas
- Backend API: Axum (HTTP + WebSocket)
- Frontend: Vue 3 + Vite
- CI: GitHub Actions

---

## File Structure

```
IDATT2104-NP-Project/
├── Cargo.toml                      # workspace: crdt-lib, crdt-net, crdt-app
├── Cargo.lock
├── README.md
├── .github/workflows/ci.yml
├── docs/specs/
├── crates/
│   ├── crdt-lib/                   # pure CRDT library (no I/O, no async)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── traits.rs           # Crdt trait
│   │       ├── counters/           # GCounter, PNCounter
│   │       ├── sets/               # GSet, TwoPSet, ORSet
│   │       ├── registers/          # LWWRegister, MVRegister
│   │       └── canvas/             # CanvasDocument composite CRDT
│   ├── crdt-net/                   # P2P gossip transport
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs           # GossipConfig
│   │       └── engine.rs           # GossipEngine (async, TCP)
│   └── crdt-app/                   # binary — HTTP + WebSocket API
│       └── src/
│           ├── main.rs             # CLI args, startup
│           ├── state.rs            # Arc<RwLock<AppState>>
│           └── api.rs              # Axum routes + WebSocket hub
└── frontend/                       # Vue 3 + Vite
    └── src/
        ├── App.vue
        ├── components/
        │   ├── PixelCanvas.vue     # 64×64 canvas element
        │   └── ColorPicker.vue
        └── stores/
            └── canvas.js           # Pinia store + WebSocket client
```

---

## CRDT Library (`crdt-lib`)

### Core trait

```rust
pub trait Crdt: Clone {
    type Value;
    fn value(&self) -> Self::Value;
    fn merge(&self, other: &Self) -> Self;  // commutative, associative, idempotent
}
```

### Implementations

| Type | Internal | Merge rule |
|---|---|---|
| `GCounter` | `HashMap<NodeId, u64>` | element-wise max; value = sum |
| `PNCounter` | two `GCounter`s (inc/dec) | merge each; value = inc - dec |
| `GSet<T>` | `HashSet<T>` | union |
| `TwoPSet<T>` | added + tombstone `GSet` | merge each; in set iff added ∧ ¬tombstoned |
| `ORSet<T>` | `HashMap<T, HashSet<Uuid>>` + tombstones | union tag maps + tombstones |
| `LWWRegister<T>` | value + timestamp + node_id | higher timestamp wins; node_id breaks ties |
| `MVRegister<T>` | `Vec<(VectorClock, T)>` | keep values with incomparable clocks |

### `CanvasDocument` composite CRDT

```rust
pub struct CanvasDocument {
    pub pixels:  HashMap<(u8, u8), LWWRegister<Rgba>>,
    pub users:   ORSet<Uuid>,
    pub cursors: HashMap<Uuid, LWWRegister<(u8, u8)>>,
}
// Rgba = (u8, u8, u8, u8)
// merge: merge each pixel register, merge users ORSet, merge each cursor register
```

---

## P2P Network Layer (`crdt-net`)

### GossipConfig

```rust
pub struct GossipConfig {
    pub node_id:      Uuid,
    pub gossip_addr:  SocketAddr,
    pub peers:        Vec<SocketAddr>,
    pub interval_secs: u64,  // default: 5
}
```

### GossipEngine

- Binds TCP listener on `gossip_addr`
- Every `interval_secs`: picks up to 2 random peers, sends `GossipMessage::Sync(CanvasDocument)` as JSON
- On receive: merges incoming `CanvasDocument`, notifies subscribers via broadcast channel
- Unreachable peer: warn + skip, retry next interval
- Malformed message: discard silently

---

## Application Binary (`crdt-app`)

### Shared state

```rust
pub struct AppState {
    pub node_id: Uuid,
    pub canvas:  CanvasDocument,
    pub peers:   HashSet<SocketAddr>,
}
// shared as Arc<RwLock<AppState>>
```

### HTTP API

```
GET    /api/canvas              → full CanvasDocument JSON
POST   /api/canvas/paint        → { x, y, color: [r,g,b,a] } → updated pixel
GET    /api/peers               → [addr, ...]
POST   /api/peers               → { addr } → 201
GET    /api/node                → { id, addr }
WS     /ws                      → push channel
```

### WebSocket push

- On connect: send full `CanvasDocument` snapshot
- On state change (paint or gossip merge): broadcast `{ type: "diff", pixels: [{x,y,color}], cursors: [{user_id,x,y}] }`

### CLI

```
crdt-app --port 8080 --gossip-port 9090 --peers 127.0.0.1:9091,127.0.0.1:9092
```

---

## Frontend (`frontend/`)

| Component | Responsibility |
|---|---|
| `stores/canvas.js` | Pinia store: `pixels Map<"x,y", Rgba>`, `cursors Map<userId, {x,y}>`, WS connect/reconnect |
| `PixelCanvas.vue` | `<canvas>` 64×64 grid, click/drag paint, cursor overlay |
| `ColorPicker.vue` | Active color selection |
| `App.vue` | Assembles components, handles WS diff messages |

Auto-reconnect every 3s on disconnect. Show "Reconnecting…" banner while offline.

---

## Testing

### Unit tests (`crdt-lib`)

Each CRDT must pass property-based tests (proptest):
- Commutativity: `a.merge(&b) == b.merge(&a)`
- Associativity: `a.merge(&b).merge(&c) == a.merge(&b.merge(&c))`
- Idempotency: `a.merge(&a) == a`

Additional targeted tests:
- `LWWRegister`: tie-break on equal timestamps (node_id decides winner)
- `MVRegister`: concurrent writes → 2 values; sequential → 1 value
- `ORSet`: concurrent add+remove → add wins (new tag not tombstoned)
- `CanvasDocument`: same pixel painted on 2 nodes → deterministic merge result

### Integration tests (`crdt-net` / `crdt-app`)

- 3 in-process nodes: paint on node 1 → converges to nodes 2+3 after 2 gossip intervals
- Network partition: diverge + reconnect → LWW winner survives
- HTTP paint → WebSocket diff received within 100ms

---

## CI

```yaml
name: CI
on: [push, pull_request]
jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: "clippy, rustfmt" }
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings
      - run: cargo fmt --check
  frontend:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: "20" }
      - run: cd frontend && npm ci && npm run build
```

---

## Dependencies

### `crdt-lib`
| Crate | Use |
|---|---|
| `serde` + `serde_json` | serialization |
| `uuid` | tags, node IDs |
| `proptest` (dev) | property-based tests |

### `crdt-net`
| Crate | Use |
|---|---|
| `crdt-lib` | CanvasDocument + Crdt trait |
| `tokio` | async TCP |
| `serde_json` | gossip serialization |
| `tracing` | logging |

### `crdt-app`
| Crate | Use |
|---|---|
| `crdt-lib` + `crdt-net` | local deps |
| `tokio` | async runtime |
| `axum` | HTTP + WebSocket |
| `clap` | CLI args |
| `tracing` + `tracing-subscriber` | structured logging |

---

## Work Division

| Student | Crate(s) | Key files |
|---|---|---|
| 1 | `crdt-lib` | traits.rs, all CRDTs, CanvasDocument, tests |
| 2 | `crdt-net` + `crdt-app` | engine.rs, state.rs, api.rs, main.rs, integration tests |
| 3 | `frontend/` + CI + README | canvas.js, PixelCanvas.vue, ColorPicker.vue, ci.yml |

See `docs/specs/` for full acceptance criteria per AC code.

---

## Verification

1. `cargo test --workspace` — all tests pass
2. `cargo clippy --workspace -- -D warnings` — zero warnings
3. Start 3 nodes: ports 8080/8081/8082, gossip 9090/9091/9092
4. Open Vue app, paint on one node → other nodes update within ~10s
5. Kill a node, paint on others, restart → canvas converges
6. CI green on push
