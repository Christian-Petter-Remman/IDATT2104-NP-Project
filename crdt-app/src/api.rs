use axum::{
    Router,
    routing::{get, post},
    extract::{State, ws::WebSocketUpgrade},
    Json,
    response::IntoResponse,
};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::canvas::Rgba;
use crate::state::AppState;
use crate::gossip::GossipBackend;

#[derive(Deserialize)]
pub struct PaintRequest {
    pub x: u8,
    pub y: u8,
    pub color: [u8; 4],
}

#[derive(Serialize)]
pub struct NodeInfo {
    pub id: String,
}

pub fn router<G: GossipBackend + Clone + 'static>(state: Arc<AppState<G>>) -> Router {
    Router::new()
        .route("/api/canvas", get(get_canvas::<G>))
        .route("/api/canvas/paint", post(paint::<G>))
        .route("/api/node", get(node_info::<G>))
        .route("/ws", get(ws_handler::<G>))
        .with_state(state)
}

async fn get_canvas<G: GossipBackend + Clone>(
    State(s): State<Arc<AppState<G>>>,
) -> impl IntoResponse {
    let canvas = s.canvas.read().await;
    Json(canvas.clone())
}

async fn paint<G: GossipBackend + Clone>(
    State(s): State<Arc<AppState<G>>>,
    Json(req): Json<PaintRequest>,
) -> impl IntoResponse {
    let color: Rgba = (req.color[0], req.color[1], req.color[2], req.color[3]);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    s.paint(req.x, req.y, color, ts).await;
    Json(serde_json::json!({ "ok": true }))
}

async fn node_info<G: GossipBackend + Clone>(
    State(s): State<Arc<AppState<G>>>,
) -> impl IntoResponse {
    Json(NodeInfo { id: s.node_id.to_string() })
}

async fn ws_handler<G: GossipBackend + Clone + 'static>(
    State(s): State<Arc<AppState<G>>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, s))
}

async fn handle_ws<G: GossipBackend + Clone>(
    mut socket: axum::extract::ws::WebSocket,
    state: Arc<AppState<G>>,
) {
    {
        let canvas = state.canvas.read().await;
        let msg = serde_json::to_string(&*canvas).unwrap();
        if socket.send(axum::extract::ws::Message::Text(msg.into())).await.is_err() {
            return;
        }
    }
    let mut rx = state.ws_tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(canvas) => {
                let msg = serde_json::to_string(&canvas).unwrap();
                if socket.send(axum::extract::ws::Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}
