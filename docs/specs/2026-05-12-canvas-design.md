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
│   │       ├── clocks/             # VectorClock (causality), LamportClock (LWW timestamps)
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
        │   ├── ColorPicker.vue     # palette ORSet view + add/remove
        │   ├── PeerList.vue        # active_peers + global paintTotal
        │   └── Leaderboard.vue     # ranked owned-tile counts (derived from pixels)
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
| `VectorClock` | `HashMap<NodeId, u64>` | element-wise max; partial order via component-wise ≤ |
| `GCounter` | `HashMap<NodeId, u64>` | element-wise max; value = sum |
| `PNCounter` | two `GCounter`s (inc/dec) | merge each; value = inc - dec |
| `GSet<T>` | `HashSet<T>` | union |
| `TwoPSet<T>` | added + tombstone `GSet` | merge each; in set iff added ∧ ¬tombstoned |
| `ORSet<T>` | `HashMap<T, HashSet<Uuid>>` + tombstones | union tag maps + tombstones; add-wins on concurrent add+remove |
| `LWWRegister<T>` | value + Lamport timestamp + node_id | higher timestamp wins; node_id breaks ties (Lamport clock derived from local `VectorClock`) |
| `MVRegister<T>` | `Vec<(VectorClock, T)>` | keep values with incomparable clocks |

**`VectorClock` is the causality primitive** underpinning the document. Every local mutation increments the writer's component; gossip merges take the element-wise max. `MVRegister` uses it directly; `LWWRegister` derives a Lamport timestamp (`max` of all components) from it so writes have a total order with `node_id` tiebreaks.

### `CanvasDocument` composite CRDT

```rust
pub struct CanvasDocument {
    pub clock:        VectorClock,                          // causality, advanced on every local op
    pub pixels:       HashMap<(u8, u8), LWWRegister<Rgba>>, // pixel colour state (per-cell LWW)
    pub palette:      ORSet<Rgba>,                          // shared color palette (add-wins)
    pub active_peers: ORSet<Uuid>,                          // peers currently in the session (add-wins)
    pub paint_counts: GCounter,                             // value() = global lifetime paint ops (all peers)
    pub cursors:      HashMap<Uuid, LWWRegister<(u8, u8)>>, // cursor position per peer
}
// Rgba = (u8, u8, u8, u8)
//
// merge: merge clock (component-wise max), merge each pixel register,
//        merge palette + active_peers ORSets, merge paint_counts GCounter,
//        merge each cursor register.
//
// Local mutations (paint, palette add/remove, peer join/leave) advance
// `clock[self.node_id]` first, then update the relevant sub-CRDT using
// the new tick as the Lamport timestamp / ORSet tag seed.

// Derived view (not stored, computed on demand from `pixels`):
//   fn ownership_leaderboard(&self) -> HashMap<NodeId, u64> {
//       self.pixels.values().fold(HashMap::new(), |mut acc, lww| {
//           *acc.entry(lww.node_id).or_default() += 1; acc
//       })
//   }
// Convergence: pure function of the merged LWW state, so all peers see the
// same ranking. Overwrites naturally decrement the previous owner because
// the LWW register's node_id changes to the new writer.
```

| Course-spec CRDT | Field | Type | Why |
|---|---|---|---|
| Pixel color state | `pixels[(x,y)]` | `LWWRegister<Rgba>` | last writer wins per cell; ties broken by node_id |
| Color palette | `palette` | `ORSet<Rgba>` | add-wins so a peer's colour survives a concurrent removal |
| Active peer tracking | `active_peers` | `ORSet<Uuid>` | a rejoining peer can be re-added (2P-Set can't) |
| Paint operation count | `paint_counts` | `GCounter` | monotonic counter; `value()` = **global lifetime paint ops** across all peers |
| Leaderboard (owned tiles) | derived from `pixels` | — | per-peer count of LWW registers where `node_id == peer`; decreases when overwritten |
| Causality tracking | `clock` | `VectorClock` | underpins LWW timestamps + concurrent-op detection |

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
POST   /api/canvas/paint        → { x, y, color: [r,g,b,a] } → updated pixel (increments paint_counts[self])
GET    /api/palette             → [[r,g,b,a], ...]
POST   /api/palette             → { color: [r,g,b,a] } → 201           (ORSet add)
DELETE /api/palette             → { color: [r,g,b,a] } → 204           (ORSet remove)
GET    /api/stats               → { paint_total, active_peers: [uuid, ...] }   (paint_total = GCounter.value(), global lifetime)
GET    /api/leaderboard         → [{ peer_id, pixels }, ...] sorted desc, derived from pixels LWW (currently-owned tiles)
GET    /api/peers               → [addr, ...]
POST   /api/peers               → { addr } → 201
GET    /api/node                → { id, addr }
WS     /ws                      → push channel
```

### WebSocket push

- On connect: send full `CanvasDocument` snapshot (includes clock, palette, active_peers, paint_counts)
- On state change (paint, palette mutation, peer join/leave, or gossip merge): broadcast
  ```
  { type: "diff",
    pixels:       [{x, y, color}],
    palette_add:  [[r,g,b,a]],
    palette_rm:   [[r,g,b,a]],
    cursors:      [{user_id, x, y}],
    active_peers: [uuid, ...],
    paint_total:  n,                            // global lifetime ops (GCounter.value())
    leaderboard:  [{peer_id, pixels}, ...] }    // currently-owned tiles, derived from pixels LWW
  ```

### CLI

```
crdt-app --port 8080 --gossip-port 9090 --peers 127.0.0.1:9091,127.0.0.1:9092
```

---

## Frontend (`frontend/`)

| Component | Responsibility |
|---|---|
| `stores/canvas.js` | Pinia store: `pixels Map<"x,y", Rgba>`, `palette Set<Rgba>`, `cursors Map<userId, {x,y}>`, `activePeers Set<Uuid>`, `paintTotal number`, `leaderboard [{peer_id, pixels}]`, WS connect/reconnect |
| `PixelCanvas.vue` | `<canvas>` 64×64 grid, click/drag paint, cursor overlay |
| `ColorPicker.vue` | Renders ORSet `palette`; add/remove colours via `/api/palette` |
| `PeerList.vue` | Shows `activePeers` and `paintTotal` (live counters from gossip merges) |
| `Leaderboard.vue` | Ranked list of peers by **currently-owned tiles** (decreases when overwritten); also shows global `paintTotal` from the GCounter; highlights self |
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
- `VectorClock`: partial order — `a < b`, `a > b`, `a == b`, `a || b` (concurrent) all detected correctly
- `GCounter`: per-peer increments + merge → `value()` equals sum across peers, never decreases
- `LWWRegister`: tie-break on equal Lamport timestamps (node_id decides winner)
- `MVRegister`: concurrent writes → 2 values; sequential → 1 value
- `ORSet` (palette + active_peers): concurrent add+remove → add wins (new tag not tombstoned); rejoining peer re-appears in `active_peers`
- `CanvasDocument`:
  - same pixel painted on 2 nodes → deterministic merge result
  - palette add on node A + remove on node B (concurrent) → colour remains
  - paint on 2 nodes → `paint_counts.value()` (global lifetime total) equals total ops after merge, never decreases on overwrite
  - leaderboard convergence: derived ownership counts from `pixels` are identical on every node after gossip (deterministic LWW winner)
  - leaderboard decrement: peer A paints (3,4), peer B overwrites (3,4) → after merge, A's owned count drops by 1 on every node
  - concurrent overwrite: A and C both paint over B's tile → exactly one wins via LWW tiebreak; B loses exactly one, winner gains one, no double-counting
  - peer leaves then rejoins → present in `active_peers`

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
| 3 | `frontend/` + CI + README | canvas.js, PixelCanvas.vue, ColorPicker.vue, PeerList.vue, Leaderboard.vue, ci.yml |

See `docs/specs/` for full acceptance criteria per AC code.

---

## Verification

1. `cargo test --workspace` — all tests pass
2. `cargo clippy --workspace -- -D warnings` — zero warnings
3. Start 3 nodes: ports 8080/8081/8082, gossip 9090/9091/9092
4. Open Vue app, paint on one node → other nodes update within ~10s
5. Kill a node, paint on others, restart → canvas converges
6. CI green on push
