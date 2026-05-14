use crate::canvas::{CanvasView, Rgba};
use crate::gossip::GossipBackend;
use crate::state::AppState;
use axum::{
    extract::{ws::WebSocketUpgrade, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[derive(Deserialize)]
pub struct PaintRequest {
    pub x: u8,
    pub y: u8,
    pub color: [u8; 4],
}

#[derive(Serialize)]
pub struct NodeInfo {
    pub id: String,
    pub addr: String,
}

pub fn router<G: GossipBackend + Clone + 'static>(state: Arc<AppState<G>>) -> Router {
    Router::new()
        .route("/api/canvas", get(get_canvas::<G>))
        .route("/api/canvas/paint", post(paint::<G>))
        .route("/api/node", get(node_info::<G>))
        .route("/ws", get(ws_handler::<G>))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

async fn get_canvas<G: GossipBackend + Clone>(
    State(s): State<Arc<AppState<G>>>,
) -> impl IntoResponse {
    let canvas = s.canvas.read().await;
    Json(CanvasView::from(&*canvas))
}

async fn paint<G: GossipBackend + Clone>(
    State(s): State<Arc<AppState<G>>>,
    Json(req): Json<PaintRequest>,
) -> impl IntoResponse {
    let color: Rgba = (req.color[0], req.color[1], req.color[2], req.color[3]);
    s.paint(req.x, req.y, color).await;
    Json(serde_json::json!({ "ok": true }))
}

async fn node_info<G: GossipBackend + Clone>(
    State(s): State<Arc<AppState<G>>>,
) -> impl IntoResponse {
    Json(NodeInfo {
        id: s.node_id.to_string(),
        addr: s.addr.clone(),
    })
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
        let Ok(msg) = serde_json::to_string(&CanvasView::from(&*canvas)) else {
            tracing::error!("failed to serialize canvas state");
            return;
        };
        if socket
            .send(axum::extract::ws::Message::Text(msg))
            .await
            .is_err()
        {
            return;
        }
    }
    let mut rx = state.ws_tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(canvas) => {
                let Ok(msg) = serde_json::to_string(&CanvasView::from(&canvas)) else {
                    tracing::error!("failed to serialize canvas state");
                    break;
                };
                if socket
                    .send(axum::extract::ws::Message::Text(msg))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("ws client lagged, dropped {} messages", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
