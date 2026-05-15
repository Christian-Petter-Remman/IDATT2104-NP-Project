# IDATT2104 Network Programming Project

A distributed collaborative pixel canvas using state-based CRDTs and peer-to-peer gossip.

Multiple nodes sync automatically — paint on one, see it appear on another after the gossip interval (~5 seconds).

## What it is

- **crdt-core** — CRDT implementations (LWW register, ORSet, GCounter, etc.)
- **crdt-net** — Gossip engine over TCP. Nodes exchange state periodically; merge is CRDT-safe.
- **crdt-app** — Axum HTTP + WebSocket server. Serves the frontend and exposes a REST API.
- **frontend** — Vue 3 single-page app. Pixel canvas, color picker, peer list, leaderboard.

## Quick start (pre-built binary)

No Rust or Node.js required. Download the binary for your platform from [GitHub Releases](../../releases/latest):

| Platform | File |
|---|---|
| Linux x86_64 | `crdt-node-linux-x86_64` |
| macOS Apple Silicon | `crdt-node-macos-arm64` |
| macOS Intel | `crdt-node-macos-x86_64` |
| Windows | `crdt-node-windows-x86_64.exe` |

Run it:

```
./crdt-node
```

Open http://localhost:8080. The canvas loads automatically.

On a LAN, each peer runs their own copy. Nodes discover each other via mDNS with no configuration. If mDNS is unavailable (e.g. university network), specify a peer manually:

```
./crdt-node --peers 192.168.x.x:9090
```

## Development

### Prerequisites

- Rust (stable) — https://rustup.rs
- Node.js 18+ and npm

### Running

```
npm run setup   # first time only — installs dependencies
npm run dev     # starts backend (port 8080) and frontend dev server (port 3000) together
```

Open http://localhost:3000. The canvas connects automatically. First compile takes ~30–60s.

To run processes separately:

```
npm run dev:backend    # cargo run -p crdt-app (port 8080, gossip port 9090)
npm run dev:frontend   # Vite dev server (port 3000, proxies /api and /ws to :8080)
```

### Two nodes on one machine (binary)

```
./crdt-node --port 8080 --gossip-port 9090 --peers 127.0.0.1:9091
./crdt-node --port 8081 --gossip-port 9091 --peers 127.0.0.1:9090
```

Open http://localhost:8080 and http://localhost:8081 in separate tabs. Both canvases sync.

### Building a release binary

```
npm run build
```

Builds the frontend, then compiles the Rust binary with the frontend embedded. Output: `target/release/crdt-app`.

## Releasing

Push a version tag to trigger the CI release workflow, which builds binaries for all platforms and uploads them to GitHub Releases:

```
git tag v1.0.0
git push --tags
```

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

## Testing

```
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
