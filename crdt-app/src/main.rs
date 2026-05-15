mod api;
mod canvas;
mod state;

use crdt_net::{GossipConfig, GossipEngine};
use state::AppState;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(clap::Parser)]
struct Args {
    #[arg(long, default_value_t = 8080)]
    port: u16,
    #[arg(long, default_value_t = 9090)]
    gossip_port: u16,
    /// Comma-separated bootstrap peers, e.g. 127.0.0.1:9091,127.0.0.1:9092
    #[arg(long, default_value = "")]
    peers: String,
    /// Gossip tick interval in milliseconds. Lower = snappier
    /// convergence, more network chatter. 200ms gives near-real-time
    /// updates between peers on localhost.
    #[arg(long, default_value_t = 200)]
    gossip_interval_ms: u64,
}

#[tokio::main]
async fn main() {
    // Default filter: app + gossip at INFO, mDNS silenced. The mdns-sd
    // crate emits ERRORs for every network interface it can't use
    // (WSL virtual adapters, IPv6-only NICs) which is normal and not
    // actionable. Override with RUST_LOG when debugging discovery.
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,mdns_sd=off"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let args = <Args as clap::Parser>::parse();
    let node_id = Uuid::new_v4();
    let http_addr = format!("0.0.0.0:{}", args.port);

    let bootstrap: Vec<std::net::SocketAddr> = args
        .peers
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            s.parse()
                .map_err(|_| tracing::warn!("invalid peer address ignored: {s}"))
                .ok()
        })
        .collect();

    let (state, local_rx) = AppState::new(node_id, http_addr.clone());
    let (merged_tx, _) = broadcast::channel::<canvas::CanvasDocument>(64);

    let gossip_addr: std::net::SocketAddr =
        format!("0.0.0.0:{}", args.gossip_port).parse().unwrap();
    let config = GossipConfig::new(node_id, gossip_addr)
        .with_peers(bootstrap)
        .with_interval(Duration::from_millis(args.gossip_interval_ms))
        .with_mdns(true);

    let engine = Arc::new(
        GossipEngine::run(config, local_rx, merged_tx.clone())
            .await
            .expect("gossip engine failed to start"),
    );
    state.set_engine(Arc::clone(&engine));

    let state_clone = Arc::clone(&state);
    let mut merged_rx = merged_tx.subscribe();
    tokio::spawn(async move {
        while let Ok(incoming) = merged_rx.recv().await {
            state_clone.apply_gossip(incoming);
        }
        tracing::warn!("gossip listener exited");
    });

    // On ctrl-c, send a `Goodbye` so surviving peers learn we left
    // immediately instead of waiting ~3s for failure-detection to fire.
    // Without this, every disconnect goes through the slow path that
    // logs `gossip send failed` bursts on the surviving terminals.
    let engine_for_signal = Arc::clone(&engine);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("ctrl-c received, sending Goodbye");
            engine_for_signal.graceful_shutdown().await;
            // `process::exit(0)` skips Axum's graceful drain — any
            // in-flight HTTP/WS request is severed mid-response.
            // Acceptable for this demo binary; a production deployment
            // would wire `axum::serve(...).with_graceful_shutdown(...)`
            // to the same signal and wait for Axum to settle here.
            std::process::exit(0);
        }
    });

    tracing::info!("node {} http={} gossip={}", node_id, http_addr, gossip_addr);

    let listener = tokio::net::TcpListener::bind(&http_addr)
        .await
        .expect("failed to bind");
    axum::serve(listener, api::router(state))
        .await
        .expect("server error");
    // `engine` and its tasks tear down via `Arc` drop on process exit.
    // The Ctrl+C path above already called `graceful_shutdown` before
    // `process::exit`, so reaching this point means `axum::serve`
    // returned on its own — unusual for this binary.
}
