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

## Running

From the repo root:

```
npm run setup   # first time only — installs all dependencies
npm run dev     # starts backend and frontend together
```

Open http://localhost:5173. The canvas loads automatically once the backend is ready (first compile takes ~30–60s).

To run processes separately:

```
npm run dev:backend    # cargo run -p crdt-app (port 8080, gossip port 9090)
npm run dev:frontend   # Vite dev server (port 5173)
```

## LAN multiplayer

Nodes on the same network discover each other automatically via mDNS — no configuration needed. Each machine runs `npm run dev` independently and peers connect within ~5 seconds.

If mDNS is unavailable (e.g. on a university network), specify peers manually:

```
npm run dev:backend -- --peers 192.168.x.x:9090
```

## Two nodes on one machine

```
npm run dev:backend -- --port 8080 --gossip-port 9090 --peers 127.0.0.1:9091
npm run dev:backend -- --port 8081 --gossip-port 9091 --peers 127.0.0.1:9090
npm run dev:frontend
```

Open http://localhost:5173 in two browser tabs. The UI auto-connects to port 8080; change the port field to `8081` in the second tab and click Connect.

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
