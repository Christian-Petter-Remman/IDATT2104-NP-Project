mod api;
mod canvas;
mod gossip;
mod state;

use gossip::{GossipBackend, NoopGossip};
use state::AppState;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let node_id = Uuid::new_v4();
    let state = AppState::new(node_id, NoopGossip::new());

    // TODO: replace NoopGossip with crdt-net's GossipEngine once available.
    // GossipEngine must implement GossipBackend (see gossip.rs).

    let state_clone = Arc::clone(&state);
    let _gossip_handle = tokio::spawn(async move {
        let mut rx = state_clone.gossip.subscribe();
        while let Ok(incoming) = rx.recv().await {
            state_clone.apply_gossip(incoming).await;
        }
        tracing::warn!("gossip listener exited");
    });

    let port: u16 = std::env::args()
        .position(|a| a == "--port")
        .and_then(|i| std::env::args().nth(i + 1))
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("node {} listening on {}", node_id, addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind to address");
    axum::serve(listener, api::router(state))
        .await
        .expect("server error");
}
