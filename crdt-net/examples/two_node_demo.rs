//! Run two (or more) nodes in separate terminals and watch them converge.
//!
//! Each node owns a `GCounter`-like map keyed by node UUID with element-wise
//! max merge (a valid state-based CRDT). Type `bump` on stdin to increment
//! this node's counter. Every state change — local or merged — prints.
//!
//! Example (3 terminals):
//!   cargo run -p crdt-net --example two_node_demo -- --port 9090 --peer 127.0.0.1:9091 --peer 127.0.0.1:9092
//!   cargo run -p crdt-net --example two_node_demo -- --port 9091 --peer 127.0.0.1:9090 --peer 127.0.0.1:9092
//!   cargo run -p crdt-net --example two_node_demo -- --port 9092 --peer 127.0.0.1:9090 --peer 127.0.0.1:9091
//!
//! Commands on stdin:
//!   bump      — increment this node's counter
//!   peers     — list known peers
//!   add  ADDR — add a peer at runtime
//!   rm   ADDR — remove a peer at runtime
//!   quit      — exit

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use crdt_core::Crdt;
use crdt_net::{GossipConfig, GossipEngine};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Counter {
    counts: BTreeMap<Uuid, u64>,
}

impl Counter {
    fn bump(&mut self, who: Uuid) {
        *self.counts.entry(who).or_default() += 1;
    }
    fn total(&self) -> u64 {
        self.counts.values().sum()
    }
}

impl Crdt for Counter {
    type Value = u64;
    fn value(&self) -> u64 {
        self.total()
    }
    fn merge(&self, other: &Self) -> Self {
        let mut out = self.counts.clone();
        for (k, v) in &other.counts {
            let slot = out.entry(*k).or_default();
            if *v > *slot {
                *slot = *v;
            }
        }
        Self { counts: out }
    }
}

struct Args {
    bind: String,
    port: u16,
    peers: Vec<SocketAddr>,
}

fn parse_args() -> Args {
    fn die(msg: impl std::fmt::Display) -> ! {
        eprintln!("error: {msg}");
        eprintln!(
            "usage: two_node_demo --port <P> [--bind <IP>] [--peer IP:PORT]...\n\
             example: two_node_demo --port 9090 --peer 192.168.1.57:9090"
        );
        std::process::exit(2);
    }

    let mut bind: String = "0.0.0.0".to_string();
    let mut port: Option<u16> = None;
    let mut peers = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bind" => bind = it.next().unwrap_or_else(|| die("--bind needs value")),
            "--port" => {
                let raw = it.next().unwrap_or_else(|| die("--port needs value"));
                port = Some(
                    raw.parse()
                        .unwrap_or_else(|e| die(format!("--port {raw}: {e}"))),
                );
            }
            "--peer" => {
                let raw = it.next().unwrap_or_else(|| die("--peer needs value"));
                let addr: SocketAddr = raw
                    .parse()
                    .unwrap_or_else(|e| die(format!("--peer {raw}: {e} (expected IP:PORT)")));
                peers.push(addr);
            }
            "-h" | "--help" => {
                eprintln!(
                    "usage: two_node_demo --port <P> [--bind <IP>] [--peer IP:PORT]...\n\
                     defaults: --bind 0.0.0.0 (listen on all interfaces)\n\
                     example: two_node_demo --port 9090 --peer 192.168.1.57:9090"
                );
                std::process::exit(0);
            }
            other => die(format!("unknown arg: {other}")),
        }
    }
    Args {
        bind,
        port: port.unwrap_or_else(|| die("--port is required")),
        peers,
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Initialize tracing so engine WARN/DEBUG/TRACE lines actually print.
    // Override level with e.g.  $env:RUST_LOG="crdt_net=debug"
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("crdt_net=info,warn")),
        )
        .with_target(false)
        .init();

    let args = parse_args();
    let node_id = Uuid::new_v4();
    let gossip_addr: SocketAddr = format!("{}:{}", args.bind, args.port).parse().unwrap();

    println!("node {} listening on {}", node_id, gossip_addr);
    println!("initial peers: {:?}", args.peers);
    println!("commands: bump | peers | add ADDR | rm ADDR | quit");

    let (state_tx, state_rx) = watch::channel(Counter::default());
    let (merged_tx, _) = broadcast::channel::<Counter>(64);

    let engine = GossipEngine::run(
        GossipConfig::new(node_id, gossip_addr)
            .with_peers(args.peers)
            .with_interval(Duration::from_secs(1)),
        state_rx.clone(),
        merged_tx.clone(),
    )
    .await?;

    // Forwarder: fold merges back into the watch source.
    {
        let state_tx = state_tx.clone();
        let mut rx = merged_tx.subscribe();
        tokio::spawn(async move {
            while let Ok(incoming) = rx.recv().await {
                state_tx.send_modify(|s| *s = s.merge(&incoming));
            }
        });
    }

    // Print only when the state *really* changes.
    // (watch::Sender::send_modify fires `changed` even when the value is
    // identical, so we dedupe against the last value we printed.)
    {
        let mut rx = state_rx.clone();
        let mut last = rx.borrow().clone();
        println!("STATE total={} counts={:?}", last.total(), last.counts);
        tokio::spawn(async move {
            loop {
                if rx.changed().await.is_err() {
                    return;
                }
                let s = rx.borrow().clone();
                if s != last {
                    println!("STATE total={} counts={:?}", s.total(), s.counts);
                    last = s;
                }
            }
        });
    }

    // stdin loop.
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        match cmd {
            "bump" => state_tx.send_modify(|s| s.bump(node_id)),
            "peers" => println!("(peers are managed in the engine; no read API exposed)"),
            "add" => match rest.parse::<SocketAddr>() {
                Ok(addr) => {
                    engine.add_peer(addr);
                    println!("added {addr}");
                }
                Err(e) => println!("bad addr: {e}"),
            },
            "rm" => match rest.parse::<SocketAddr>() {
                Ok(addr) => {
                    engine.remove_peer(addr);
                    println!("removed {addr}");
                }
                Err(e) => println!("bad addr: {e}"),
            },
            "quit" | "exit" => break,
            _ => println!("unknown command: {cmd}"),
        }
    }
    Ok(())
}
