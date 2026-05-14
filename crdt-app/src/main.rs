mod api;
mod canvas;
mod state;

use crdt_net::{GossipConfig, GossipEngine};
use state::AppState;
use std::sync::Arc;
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
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
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
        .with_mdns(true);

    let _engine = GossipEngine::run(config, local_rx, merged_tx.clone())
        .await
        .expect("gossip engine failed to start");

    let state_clone = Arc::clone(&state);
    let mut merged_rx = merged_tx.subscribe();
    tokio::spawn(async move {
        while let Ok(incoming) = merged_rx.recv().await {
            state_clone.apply_gossip(incoming);
        }
        tracing::warn!("gossip listener exited");
    });

    tracing::info!("node {} http={} gossip={}", node_id, http_addr, gossip_addr);

    let listener = tokio::net::TcpListener::bind(&http_addr)
        .await
        .expect("failed to bind");
    axum::serve(listener, api::router(state))
        .await
        .expect("server error");
}
