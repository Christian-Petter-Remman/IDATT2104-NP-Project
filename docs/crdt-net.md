# `crdt-net` — Implementation Walkthrough

This document explains how the `crdt-net` crate works end-to-end: what each
file contains, what each method does, how the pieces fit together, and why
certain design decisions look the way they do. It assumes you have read
[specs/2026-05-12-canvas-design.md](specs/2026-05-12-canvas-design.md).

> If the spec tells you **what** to build, this doc tells you **how the code
> in this repo actually does it**.

---

## 1. What `crdt-net` is, in one paragraph

`crdt-net` is a small peer-to-peer gossip layer that synchronises state-based
CRDTs across a fixed-ish set of TCP peers. Each node periodically picks up to
two random peers, dumps its current CRDT state to them as a length-prefixed
JSON frame, and merges any incoming state into its own. The crate is
**generic over the CRDT type** — it knows nothing about `CanvasDocument`. It
only needs `T: Crdt + Serialize + DeserializeOwned + Send + Sync + 'static`.

It is **not** an application. It does not own application state, does not
expose HTTP/WebSocket endpoints, and does not parse CLI args. That all lives
in `crdt-app`. `crdt-net`'s only job is: take a CRDT-shaped value, push it to
peers, accept values from peers, merge.

---

## 2. The mental model

Think of three things flowing through the system:

```
┌─────────────────────────────────────────────────────────────────┐
│                                                                 │
│   App code                                                      │
│   ────────                                                      │
│                                                                 │
│   watch::Sender<T>  ─────────►   watch::Receiver<T>  ─────┐     │
│        ▲                                                   │     │
│        │ "I edited locally,                                │     │
│        │  here's my new state"                             │     │
│        │                                                   ▼     │
│        │                                          ┌──────────────┐│
│   forwarder                                       │              ││
│   (merge-not-replace)                             │ GossipEngine ││
│        ▲                                          │              ││
│        │                                          │  - listener  ││
│        │                                          │  - ticker    ││
│        │                                          │              ││
│   broadcast::Receiver<T>  ◄───  broadcast::Sender<T>  ◄───┘     │
│                                                                 │
│                            └────────── TCP listener ─── peers   │
│                            └────────── TCP dialer   ─── peers   │
└─────────────────────────────────────────────────────────────────┘
```

There are two **channels** between the app and the engine:

| Channel | Direction | Carries |
|---|---|---|
| `watch::Receiver<T>` | app → engine | the latest local snapshot the engine should gossip |
| `broadcast::Sender<T>` | engine → app | every state the engine produced by merging a remote message |

The engine **owns neither side's `Sender`** — the app owns both. That is why
the engine has no notion of "the canvas". It just reads-and-sends, then
receives-and-merges-and-publishes.

The **forwarder** in the diagram is a small piece of glue the app must
provide: it subscribes to the broadcast and pushes every value back into the
watch (merging, not overwriting — section 7 explains why). Without the
forwarder, the engine's outgoing gossip would never reflect incoming merges.

---

## 3. File-by-file tour

```
crdt-net/
├── Cargo.toml          # dependencies
└── src/
    ├── lib.rs          # module declarations + public re-exports
    ├── config.rs       # GossipConfig struct
    ├── message.rs      # wire format: GossipMessage<T>, write_frame, read_frame
    └── engine.rs       # GossipEngine + the two async tasks (listener, ticker)

crdt-net/tests/
└── gossip.rs           # 3 integration tests with a MockCrdt
```

Four source files, three integration tests, one workspace dep — that's the
whole crate. Everything below explains those files in detail.

### 3.1 `lib.rs`

Just module declarations and a small re-export surface so consumers can
`use crdt_net::{GossipEngine, GossipConfig, GossipMessage, MAX_FRAME}`
without reaching into modules.

### 3.2 `config.rs` — `GossipConfig`

A plain data struct with a small builder, exactly mirroring the fields the
spec asks for plus a default 5-second interval.

```rust
pub struct GossipConfig {
    pub node_id: Uuid,             // who am I
    pub gossip_addr: SocketAddr,   // where I listen
    pub peers: Vec<SocketAddr>,    // initial peer set (mutable at runtime)
    pub interval: Duration,        // gossip tick period
}
```

Builder methods:

| Method | Purpose |
|---|---|
| `new(node_id, gossip_addr)` | Defaults: `peers = []`, `interval = 5s` |
| `with_peers(peers)` | Replace the initial peer list |
| `with_interval(duration)` | Set the gossip tick period |
| `with_interval_secs(secs)` | Convenience over `with_interval` |

`interval` is stored as `Duration` (not `u64 secs` like the spec) so
tests can use millisecond-scale intervals without surprises.

### 3.3 `message.rs` — wire format

Two types and two functions. This is the entirety of the on-the-wire
protocol.

```rust
pub const MAX_FRAME: usize = 16 * 1024 * 1024;   // 16 MiB

pub enum GossipMessage<T> {
    Sync(T),     // the only variant — full-state CRDT push
}

pub async fn write_frame<W, T>(w: &mut W, msg: &GossipMessage<T>) -> io::Result<()>
where W: AsyncWriteExt + Unpin, T: Serialize;

pub async fn read_frame<R, T>(r: &mut R) -> io::Result<GossipMessage<T>>
where R: AsyncReadExt + Unpin, T: DeserializeOwned;
```

**Wire layout of one frame:**

```
┌────────────┬──────────────────────────┐
│  u32 BE    │   JSON bytes (UTF-8)     │
│  length    │   serde_json::to_vec     │
│  (4 bytes) │   of GossipMessage<T>    │
└────────────┴──────────────────────────┘
```

- **`write_frame`** — serialise the message to JSON via `serde_json::to_vec`,
  refuse if larger than `MAX_FRAME`, write the 4-byte length, write the
  body, flush. Returns `io::Error::other` on serialisation failure (which
  shouldn't happen for any valid `T: Serialize`).
- **`read_frame`** — read 4 bytes, decode the length, refuse if larger than
  `MAX_FRAME` (which protects the listener from an attacker claiming a 4 GiB
  message and OOM-ing us), allocate exactly that many bytes, read them,
  deserialize.

The `MAX_FRAME` cap is deliberately generous — the full `CanvasDocument`
will be well under a megabyte even at full saturation — but small enough
that a single garbage peer can't allocate gigabytes on our heap.

### 3.4 `engine.rs` — the engine

This is the only file with real logic. It is structured as:

- The public **`GossipEngine`** struct + its methods.
- A `Drop` impl that signals shutdown.
- Two private free functions: **`spawn_listener`** and **`spawn_ticker`**,
  each of which `tokio::spawn`s exactly one task.
- A private helper **`handle_connection`** that processes one inbound TCP
  connection.
- A private helper **`send_sync`** that opens one outbound TCP connection
  and writes one `Sync` frame.

The struct fields:

```rust
pub struct GossipEngine {
    peers: Arc<Mutex<HashSet<SocketAddr>>>,   // mutable peer set
    local_addr: SocketAddr,                   // what the OS gave us (for tests using port 0)
    shutdown: Arc<Notify>,                    // cooperative cancellation
}
```

`peers` is shared with the ticker. `shutdown` is shared with both spawned
tasks. `local_addr` is reported back by the kernel after `bind()`, so when
config used port 0 (common in tests) the caller can find out what real port
to tell other nodes about.

---

## 4. The public API, method by method

### `GossipEngine::run`

```rust
pub async fn run<T>(
    config: GossipConfig,
    local: watch::Receiver<T>,
    merged: broadcast::Sender<T>,
) -> io::Result<Self>
where T: Crdt + Serialize + DeserializeOwned + Send + Sync + 'static
```

**What it does**, line by line:

1. **`TcpListener::bind(config.gossip_addr).await?`** — bind the TCP
   listener. If this fails (port in use, permission denied), `run` returns
   `Err` without spawning anything. This is the *only* failure path of
   `run`; once it returns `Ok`, nothing the engine does can fail to the
   caller — runtime errors are logged via `tracing` and swallowed.
2. **`listener.local_addr()?`** — capture the actual address (important
   when `config.gossip_addr` had port 0).
3. Build the shared peer set from `config.peers`.
4. Build the shutdown `Notify`.
5. **`spawn_listener::<T>(...)`** — spawn the accept loop.
6. **`spawn_ticker::<T>(...)`** — spawn the periodic gossip loop.
7. Return the handle.

After `run` returns, two tokio tasks are alive and running. The engine
struct is essentially a small bag of `Arc`s used to talk to them.

### `GossipEngine::local_addr`

Returns the actual bound socket address. Used by tests that bind to port 0
and need to tell other engines where to connect.

### `GossipEngine::add_peer` / `remove_peer`

Take the mutex on `peers`, insert/remove the address, drop the lock. That's
it. Both are non-async and non-fallible — there's nothing to wait on.
Mutex contention is trivial (one ticker reader, occasional add/remove).

This is what backs the spec's `POST /api/peers` and `DELETE /api/peers`
endpoints in `crdt-app`.

### `GossipEngine::shutdown`

Calls `Notify::notify_waiters()`. Both spawned tasks check the shutdown
notify on every iteration via `tokio::select!`, so they exit at the next
loop turn. Dropping `GossipEngine` also calls `shutdown` (via the `Drop`
impl) so leaking the handle never leaks tasks indefinitely.

> **Caveat:** `Notify::notify_waiters` only wakes futures that are
> *already polling* `notified()`. In practice the spawned tasks enter their
> `select!` essentially immediately after `run` returns, so calling
> `shutdown` after at least one tick interval is always safe. The 100ms or
> so right after `run` returns is a theoretical race window we don't bother
> closing.

---

## 5. The two async tasks

Two tokio tasks run inside the engine. Both are spawned from `run` and live
until `shutdown` fires.

### 5.1 The listener task (`spawn_listener` → accept loop)

```
loop {
    select {
        shutdown.notified() => return
        listener.accept()    => spawn(handle_connection(stream, ...))
    }
}
```

Pure accept loop. Each accepted TCP connection is handed off to a
short-lived task running `handle_connection`. The accept loop never blocks
on a slow peer — the slow peer just keeps its own dedicated handler task
busy.

If `accept()` itself errors (rare — usually a fd-table problem), it logs a
warning and continues. The listener never dies on its own.

### 5.2 `handle_connection`

Per-connection task. Reads exactly one frame, processes it, exits.

```rust
match read_frame::<_, T>(&mut stream).await {
    Ok(GossipMessage::Sync(remote)) => {
        let merged_value = local.borrow().merge(&remote);
        let _ = merged.send(merged_value);
    }
    Err(e) => {
        trace!(...);  // malformed: discard silently per spec
    }
}
```

The flow:

1. Read one length-prefixed JSON frame. If the prefix is bogus, the body
   is truncated, the JSON is malformed, or the frame exceeds `MAX_FRAME`,
   `read_frame` returns an `io::Error`. We drop the frame and let the
   connection close.
2. Take `local.borrow()` — a read-locked view of the latest watch value.
3. Call `merge(&remote)`. Because `Crdt::merge` produces a new `T`,
   nothing is mutated in place — `local` still holds the pre-merge value.
4. `merged.send(merged_value)` publishes the result on the broadcast. If
   nobody is subscribed yet, `send` returns `Err` and we ignore it; the
   engine is not responsible for guaranteeing delivery.

**Why does the engine not write the merged value back to the watch?**
Because the engine doesn't own the watch `Sender` — the app does. The app
must do that itself via the forwarder pattern. The reason for that split
is the merge-vs-replace problem, explained in section 7.

### 5.3 The ticker task (`spawn_ticker`)

```
let mut ticker = time::interval(interval);
ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
ticker.tick().await;  // throw away the immediate first tick

loop {
    select {
        shutdown.notified() => return
        ticker.tick() => {
            let snapshot = local.borrow().clone();
            let targets = peers.lock().choose_multiple(rng, FANOUT);
            for addr in targets {
                spawn(send_sync(addr, snapshot));
            }
        }
    }
}
```

The first `tick().await` outside the loop discards tokio's immediate-fire
on tick interval creation. Without it the engine would gossip as soon as
`run` returned, before the caller had a chance to add peers, leading to
flaky tests.

**Each interval:**

1. Take a snapshot of the local state. `local.borrow().clone()` releases
   the read guard immediately — we hold no locks across the network I/O.
2. Snapshot the peer set, choose up to 2 with `IteratorRandom::choose_multiple`.
   `FANOUT = 2` matches the spec.
3. For each chosen peer, spawn a fire-and-forget task that runs
   `send_sync`. Spawning is the simplest way to ensure one slow/unreachable
   peer doesn't delay sending to the other one in the same tick.

**`MissedTickBehavior::Delay`** means: if the runtime is so loaded that a
tick deadline passes before we wake up, *don't* immediately fire a
catch-up tick. Just wait the full interval from now. This avoids gossip
storms after a temporary stall.

### 5.4 `send_sync`

```rust
async fn send_sync<T>(addr: SocketAddr, payload: &T) -> io::Result<()>
where T: Serialize + Send + Sync,
{
    let mut stream = time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timeout"))??;
    write_frame(&mut stream, &GossipMessage::Sync(payload)).await
}
```

- 2-second connect timeout. A node that's down doesn't stall the ticker for
  more than 2 seconds, and we don't pile up connection attempts.
- One frame, then we drop the stream (which closes the TCP connection).
- The ticker logs `warn!` on any error and moves on. The peer is retried
  next tick.

---

## 6. Concurrency layout

After `run()` returns, here is everything that exists in memory:

| Owner | Lives in |
|---|---|
| `Arc<Mutex<HashSet<SocketAddr>>>` peer set | shared between `GossipEngine` handle and the ticker task |
| `Arc<Notify>` shutdown | shared between handle, listener task, ticker task |
| `watch::Receiver<T>` | cloned into the listener task and the ticker task; each acceptor clones again per connection |
| `broadcast::Sender<T>` | cloned into the listener task; cloned again per connection |

**Tasks alive per engine:**

- 1 listener task (long-lived, until shutdown)
- 1 ticker task (long-lived, until shutdown)
- 0..N handler tasks (one per in-flight inbound connection, very short-lived)
- 0..2 sender tasks per tick (one per outbound peer this interval, ≤2s each)

No mutex is held across `await` points. The only mutex (`peers`) is locked
exclusively for the time it takes to copy 2 addresses out of a `HashSet`.

---

## 7. The state contract (the one tricky thing)

Spec literal: *"On receive: merges incoming `CanvasDocument`, notifies
subscribers via broadcast channel."*

That sentence is correct but underspecifies one crucial detail: **how do
subscribers reinstall the merged value into the watch source so the *next*
gossip tick reflects it?** Two answers, only one of which is safe:

1. **Replace the watch value** with each broadcast frame: ❌ broken.
2. **Merge** the broadcast frame into the watch value: ✅ correct.

### Why replacing is broken

Imagine node X has watch value `{X:1}` and two peers A and C both send at
the same time. Both inbound connections are handled in parallel:

```
listener for A: reads local {X:1}, computes {X:1, A:3}, broadcasts {X:1, A:3}
listener for C: reads local {X:1}, computes {X:1, C:2}, broadcasts {X:1, C:2}
```

The forwarder receives both broadcasts. If it does
`watch_tx.send(value)` (replace), then whichever broadcast it processes
second wins, and the loser's contribution is lost forever — until the
next gossip round, if it happens at all.

### Why merging works

The forwarder does:

```rust
watch_tx.send_modify(|s| *s = s.merge(&incoming));
```

Now:

```
incoming {X:1, A:3} arrives first  → watch becomes {X:1, A:3}
incoming {X:1, C:2} arrives second → watch becomes {X:1, A:3, C:2}
```

Both contributions land. This is correct because state-based CRDT merge is
commutative, associative, and idempotent — applying `merge` to an
already-merged value is exactly the right thing.

### So why doesn't the engine just merge into the watch directly?

It can't — it only has a `watch::Receiver`, not a `Sender`. That was a
deliberate split:

- Keeps the engine's API surface small (no `watch::Sender` parameter).
- Keeps the app fully in control of the watch source, which it also writes
  to on every local edit. Two writers to the same watch would have to
  coordinate anyway.

Cost: the app must implement the small "forwarder" task. Benefit: the
engine has zero opinions about how the app's state container is
structured. The tests show the forwarder pattern in
[crdt-net/tests/gossip.rs](../crdt-net/tests/gossip.rs#L65-L72).

---

## 8. Three end-to-end scenarios

### 8.1 Local edit

```
app code:                            engine:                          peers:
─────────                            ───────                          ─────
ctx.canvas.bump_pixel(x,y,color)
state_tx.send_modify(|s| ...)
                                     watch sees new state
                                     (ticker hasn't fired yet)

tick fires                           snapshot = local.borrow().clone()
                                     choose 2 peers
                                     spawn send_sync(p1, snapshot)  ────►  p1: read_frame, merge, broadcast
                                     spawn send_sync(p2, snapshot)  ────►  p2: read_frame, merge, broadcast
```

### 8.2 Inbound from a peer

```
peer Y:                              engine on node X:               forwarder on node X:
───────                              ─────────────────               ────────────────────
write_frame(Sync(Y_state)) ────►     accept(), spawn handler
                                     handler: read_frame
                                     local.borrow().merge(&Y_state)
                                     merged.send(new_state)  ─────►   forward_rx.recv()
                                                                      state_tx.send_modify(|s| *s = s.merge(...))
                                                                      ↑ now the watch reflects Y's contribution
```

### 8.3 Partition + heal

```
A and B are peered, both bump locally → converge.
A.remove_peer(B); B.remove_peer(A) → partition.
A bumps, B bumps independently → states diverge.
A.add_peer(B); B.add_peer(A) → next tick(s) gossip in both directions,
both sides merge, both reach the union of edits.
```

(This is one of the integration tests.)

---

## 9. Error handling philosophy

The crate has exactly one fallible function visible to callers: `run`. It
returns `Err` if and only if the TCP bind fails. Everything else is
log-and-continue:

| Situation | What happens |
|---|---|
| Peer is down / connect refused | `warn!`, skip, retry next tick |
| Peer connect times out (>2s) | `warn!`, skip, retry next tick |
| Inbound TCP frame is malformed | `trace!`, drop connection |
| Inbound frame claims length > 16 MiB | `trace!`, drop connection |
| Inbound JSON fails to deserialize | `trace!`, drop connection |
| Broadcast send fails (no subscribers) | silently ignored |
| Accept error | `warn!`, continue accept loop |

The spec phrasing for both failure modes ("warn + skip" / "discard
silently") is implemented literally.

---

## 10. Testing strategy

[crdt-net/tests/gossip.rs](../crdt-net/tests/gossip.rs) contains three
`#[tokio::test]` functions. They share:

- A **`MockCrdt`** — a `BTreeMap<Uuid, u64>` with element-wise-max merge.
  Trivially satisfies the CRDT laws. Used in lieu of `CanvasDocument`,
  which isn't built yet.
- A **`Node`** test fixture that spins up an engine, wires the forwarder,
  and exposes `bump()` / `addr()` / `current()` helpers.

The three tests:

1. **`converges_across_three_nodes`** — three nodes, full mesh, each
   bumps locally; assert all three converge to the union total within a
   few gossip intervals. Uses 80ms intervals to keep wall-clock short.
2. **`partition_then_heal`** — two nodes peer, mutate, converge,
   un-peer (partition), mutate independently, re-peer, assert
   convergence.
3. **`garbage_does_not_kill_listener`** — open raw TCP, send bogus
   length prefixes / truncated frames / half a length prefix; then open a
   real peer connection and verify the listener still processes it.

All three pass with `cargo test -p crdt-net`. The convergence test was
run 10× in a row to verify it isn't flaky.

---

## 11. How `crdt-net` plugs into the rest of the project

- **`crdt-core`** — provides the `Crdt` trait that `crdt-net` is generic
  over. Right now `crdt-core` only contains the trait; once student 1
  fills in `VectorClock`, `LWWRegister`, `ORSet`, ..., and the composite
  `CanvasDocument`, the latter will become the `T` that `crdt-app`
  instantiates the engine with. `crdt-net` itself doesn't need a single
  line of change.
- **`crdt-app`** — will own `Arc<RwLock<AppState>>`, the
  `watch::Sender<CanvasDocument>`, the `broadcast::Receiver<CanvasDocument>`,
  the forwarder task, the Axum HTTP/WS server, and CLI parsing. It calls
  `GossipEngine::run` once at startup, keeps the handle, and uses
  `add_peer` / `remove_peer` to back the `/api/peers` endpoints.

Roughly:

```rust
// In crdt-app startup, after parsing CLI args:
let (state_tx, state_rx) = watch::channel(CanvasDocument::default());
let (merged_tx, _) = broadcast::channel(32);

let engine = GossipEngine::run(
    GossipConfig::new(node_id, gossip_addr).with_peers(initial_peers),
    state_rx.clone(),
    merged_tx.clone(),
).await?;

// Forwarder
{
    let mut rx = merged_tx.subscribe();
    let tx = state_tx.clone();
    tokio::spawn(async move {
        while let Ok(incoming) = rx.recv().await {
            tx.send_modify(|s| *s = s.merge(&incoming));
        }
    });
}

// Hand `state_tx`, `merged_tx`, `engine` to the HTTP/WS layer.
```

That's the entire integration surface.

---

## 12. Quick reference — file → purpose

| File | Purpose |
|---|---|
| [Cargo.toml](../crdt-net/Cargo.toml) | Dependencies (tokio, serde, serde_json, uuid, tracing, rand, crdt-core) |
| [src/lib.rs](../crdt-net/src/lib.rs) | Module declarations + public re-exports |
| [src/config.rs](../crdt-net/src/config.rs) | `GossipConfig` struct + builders |
| [src/message.rs](../crdt-net/src/message.rs) | `GossipMessage<T>` + length-prefixed JSON frame codec |
| [src/engine.rs](../crdt-net/src/engine.rs) | `GossipEngine` + listener task + ticker task |
| [tests/gossip.rs](../crdt-net/tests/gossip.rs) | Convergence / partition / garbage tests with `MockCrdt` |

---

## 13. Glossary

- **State-based CRDT (CvRDT)** — a data type whose merge operation is
  commutative, associative, and idempotent. Two replicas exchanging full
  states and merging always converge, regardless of message order or
  duplication.
- **Gossip** — periodic, push-based, randomised peer-to-peer
  dissemination. Each tick a node sends its state to a small random subset
  of peers (fanout = 2 here). Anti-entropy: every node eventually sees
  every update.
- **`watch` channel** — single-producer, multi-consumer; consumers see
  only the latest value, not the history. Used here for "what is my
  current state".
- **`broadcast` channel** — multi-producer, multi-consumer; every
  subscriber receives every message (bounded buffer; if a subscriber
  lags, they get a `Lagged` error). Used here for "merged state event".
- **Forwarder** — small async task in `crdt-app` (and in the integration
  tests) that subscribes to the broadcast and merges values back into the
  watch source. Required for correctness; see section 7.
- **Fanout** — number of peers a node gossips to per tick. `FANOUT = 2`.
- **Anti-entropy interval** — `GossipConfig::interval`, default 5s.
