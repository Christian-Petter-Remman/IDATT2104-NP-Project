# IDATT2104 Network Programming Project

A distributed collaborative pixel canvas using state-based CRDTs and peer-to-peer gossip.

Multiple nodes sync automatically — paint on one, see it appear on another after the gossip interval (~5 seconds).

## What it is

- **crdt-core** — CRDT implementations (LWW register, ORSet, GCounter, etc.)
- **crdt-net** — Gossip engine over TCP. Nodes exchange state periodically; merge is CRDT-safe.
- **crdt-app** — Axum HTTP + WebSocket server. Exposes a REST API and pushes canvas snapshots to connected browsers.
- **frontend** — Vue 3 single-page app. Pixel canvas, color picker, peer list, leaderboard.

## Prerequisites

- Rust (stable, 2021 edition) — https://rustup.rs
- Node.js 18+ and npm

## Running a single node

**Terminal 1 — backend:**
```
cargo run -p crdt-app -- --port 8080 --gossip-port 9090
```

**Terminal 2 — frontend:**
```
cd frontend
npm install   # only needed the first time
npm run dev
```

Open http://localhost:5173 in a browser. The UI asks for an API port on first load — enter `8080` and click Connect.

## Running two nodes (gossip demo)

**Terminal 1:**
```
cargo run -p crdt-app -- --port 8080 --gossip-port 9090 --peers 127.0.0.1:9091
```

**Terminal 2:**
```
cargo run -p crdt-app -- --port 8081 --gossip-port 9091 --peers 127.0.0.1:9090
```

**Terminal 3 — frontend:**
```
cd frontend && npm run dev
```

Open http://localhost:5173 in **two browser tabs**. In tab 1 enter port `8080`, in tab 2 enter port `8081`. Paint on one — it propagates to the other within ~5 seconds.

## CLI flags

| Flag | Default | Description |
|---|---|---|
| `--port` | 8080 | HTTP/WebSocket port |
| `--gossip-port` | 9090 | TCP port for peer-to-peer gossip |
| `--peers` | _(empty)_ | Comma-separated bootstrap peers, e.g. `127.0.0.1:9091` |

## REST API

| Method | Path | Description |
|---|---|---|
| GET | `/api/canvas` | Full canvas snapshot |
| POST | `/api/canvas/paint` | Paint a pixel `{"x":0,"y":0,"color":[r,g,b,a]}` |
| GET | `/api/node` | Node ID and address |
| GET | `/api/palette` | Current shared palette |
| POST | `/api/palette` | Add color `{"color":[r,g,b,a]}` |
| DELETE | `/api/palette` | Remove color `{"color":[r,g,b,a]}` |
| GET | `/api/leaderboard` | Pixel ownership counts per node |
| GET | `/ws` | WebSocket — streams canvas snapshots on every change |

## Building and testing

```
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
