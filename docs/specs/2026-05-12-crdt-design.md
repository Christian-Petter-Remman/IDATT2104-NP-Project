# IDATT2104 CRDT Project — Design Specification

## Context

Course project for IDATT2104 Nettverksprogrammering. Goal: grade A. Requirements:
- Implement CRDTs from scratch in Rust (no external CRDT libraries)
- Client-server or P2P architecture
- README with CI link, docs, install instructions, etc.
- Deadline: 2026-05-26 23:59

**Decisions:**
- Language: Rust (Cargo workspace with 2 crates)
- Architecture: Peer-to-peer with gossip protocol
- Demo: Collaborative todo list
- Backend API: Axum (HTTP + WebSocket)
- Frontend: Vue 3 + Vite
- CI: GitHub Actions

---

## File Structure

```
crdt-rs/
├── Cargo.toml                  # workspace manifest
├── Cargo.lock
├── README.md
├── .github/workflows/ci.yml
├── docs/specs/
├── crates/
│   ├── crdt-core/              # pure CRDT library (no I/O, no async)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── traits.rs
│   │       ├── counters/       # GCounter, PNCounter
│   │       ├── sets/           # GSet, TwoPSet, ORSet
│   │       ├── registers/      # LWWRegister, MVRegister
│   │       └── todo/           # TodoDocument CRDT
│   └── crdt-node/              # P2P node binary
│       └── src/
│           ├── main.rs         # CLI args (clap), startup
│           ├── node.rs         # shared state Arc<RwLock<NodeState>>
│           ├── gossip.rs       # TCP gossip loop
│           └── api.rs          # Axum routes + WebSocket hub
└── frontend/                   # Vue 3 + Vite
    └── src/
        ├── App.vue
        ├── components/         # TodoList, TodoItem, NodeInfo
        └── stores/todo.js      # Pinia store + WebSocket client
```

---

## CRDT Library (`crdt-core`)

### Core trait

```rust
pub trait Crdt: Clone {
    type Value;
    fn value(&self) -> Self::Value;
    fn merge(&self, other: &Self) -> Self;  // commutative, associative, idempotent
}
```

### Implementations

| Type | Description | Merge rule |
|---|---|---|
| `GCounter` | `HashMap<NodeId, u64>` | element-wise max; value = sum |
| `PNCounter` | two `GCounter`s (inc/dec) | merge each; value = inc - dec |
| `GSet<T>` | `HashSet<T>` | union |
| `TwoPSet<T>` | added + removed `GSet` | merge each; in set iff in added ∧ not in removed |
| `ORSet<T>` | `HashMap<T, HashSet<Tag>>` + tombstones | union element-tag maps; union tombstones |
| `LWWRegister<T>` | value + timestamp + node_id | higher timestamp wins; node_id breaks ties |
| `MVRegister<T>` | `Vec<(VectorClock, T)>` | keep values with incomparable clocks |

### `TodoDocument` CRDT

```rust
pub struct TodoDocument {
    pub items: ORSet<Uuid>,
    pub text:  HashMap<Uuid, LWWRegister<String>>,
    pub done:  HashMap<Uuid, LWWRegister<bool>>,
}
// merge: merge ORSet + merge all registers for items in union
```

---

## P2P Node (`crdt-node`)

### Node state

```rust
pub struct NodeState {
    pub id:    Uuid,
    pub todo:  TodoDocument,
    pub peers: HashSet<SocketAddr>,
}
// shared as Arc<RwLock<NodeState>>
```

### Gossip protocol

- Every 5 seconds: pick up to 2 random peers
- TCP: send `GossipMessage::Sync(snapshot)`, receive peer snapshot, merge
- Also listen for incoming connections
- Serialization: `serde_json`

### HTTP/WebSocket API

```
GET    /api/todos            → TodoDocument JSON
POST   /api/todos            → { text } → add item
PUT    /api/todos/:id/text   → { text } → update text
PATCH  /api/todos/:id/done   → { done } → set done
DELETE /api/todos/:id        → remove item
GET    /api/peers            → list peers
POST   /api/peers            → { addr } → add peer
GET    /api/node             → { id, addr }
WS     /ws                   → push on every state change
```

### CLI

```
crdt-node --port 8080 --gossip-port 9090 --peers 127.0.0.1:9091,127.0.0.1:9092
```

---

## Testing

### Unit tests (`crdt-core`)

Each CRDT verifies:
- Commutativity: `a.merge(&b) == b.merge(&a)`
- Associativity: `a.merge(&b).merge(&c) == a.merge(&b.merge(&c))`
- Idempotency: `a.merge(&a) == a`

Use `proptest` for property-based tests.

### Integration test (`crdt-node`)

3 in-process nodes, conflicting operations, gossip rounds, assert convergence.

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

### `crdt-core`
| Crate | Use |
|---|---|
| `serde` + `serde_json` | CRDT state serialization |
| `uuid` | tags, item IDs, node IDs |
| `proptest` (dev) | property-based tests |

### `crdt-node`
| Crate | Use |
|---|---|
| `tokio` | async runtime |
| `axum` | HTTP + WebSocket server |
| `serde` + `serde_json` | gossip serialization |
| `uuid` | node ID |
| `clap` | CLI args |
| `tracing` + `tracing-subscriber` | structured logging |

### Frontend
| Package | Use |
|---|---|
| `vue` 3 | UI framework |
| `vite` | build tool |
| `pinia` | state management |

---

## Work Division

| Student | Area |
|---|---|
| 1 | `crdt-core`: all CRDTs + `TodoDocument` + unit/property tests |
| 2 | `crdt-node`: gossip + Axum API + integration tests |
| 3 | Vue frontend + GitHub Actions CI + README |

---

## Timeline

| Date | Milestone |
|---|---|
| May 12–15 | Workspace scaffold + `crdt-core` stubs + Vue scaffold |
| May 15–19 | All CRDTs implemented + tested; gossip working between 2 nodes |
| May 19–22 | Axum API + WebSocket hub; Vue connected |
| May 22–24 | Frontend polish, CI green, integration tests |
| May 24–26 | README complete, final testing, submission |

---

## Verification

1. `cargo test --workspace` — all tests pass
2. `cargo clippy --workspace -- -D warnings` — zero warnings
3. Start 3 nodes on ports 8080/8081/8082 (gossip 9080/9081/9082)
4. Open Vue app on each, add/edit/delete on different nodes
5. All three frontends converge to same state within ~10 seconds
6. Kill a node, make changes, restart — verify gossip re-sync
