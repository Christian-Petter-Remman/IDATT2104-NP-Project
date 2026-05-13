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
    let mut bind: String = "0.0.0.0".to_string();
    let mut port: Option<u16> = None;
    let mut peers = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bind" => bind = it.next().expect("--bind needs value"),
            "--port" => port = Some(it.next().expect("--port needs value").parse().unwrap()),
            "--peer" => peers.push(it.next().expect("--peer needs value").parse().unwrap()),
            "-h" | "--help" => {
                eprintln!(
                    "usage: two_node_demo --port <P> [--bind <IP>] [--peer addr]...\n\
                     defaults: --bind 0.0.0.0 (listen on all interfaces)"
                );
                std::process::exit(0);
            }
            other => panic!("unknown arg: {other}"),
        }
    }
    Args {
        bind,
        port: port.expect("--port is required"),
        peers,
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
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

    // Print every state change.
    {
        let mut rx = state_rx.clone();
        tokio::spawn(async move {
            loop {
                if rx.changed().await.is_err() {
                    return;
                }
                let s = rx.borrow().clone();
                println!("STATE total={} counts={:?}", s.total(), s.counts);
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
