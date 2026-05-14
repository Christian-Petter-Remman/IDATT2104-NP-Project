use crate::canvas::{CanvasView, LeaderboardEntry, Rgba};
use crate::state::AppState;
use axum::{
    extract::{ws::WebSocketUpgrade, State},
    http::StatusCode,
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

#[derive(Deserialize)]
pub struct PaletteRequest {
    pub color: [u8; 4],
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/canvas", get(get_canvas))
        .route("/api/canvas/paint", post(paint))
        .route("/api/node", get(node_info))
        .route("/api/palette", get(get_palette).post(add_palette).delete(remove_palette))
        .route("/api/leaderboard", get(get_leaderboard))
        .route("/ws", get(ws_handler))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

async fn get_canvas(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let canvas = s.canvas.read().await;
    Json(CanvasView::from(&*canvas))
}

async fn paint(
    State(s): State<Arc<AppState>>,
    Json(req): Json<PaintRequest>,
) -> impl IntoResponse {
    let color: Rgba = (req.color[0], req.color[1], req.color[2], req.color[3]);
    s.paint(req.x, req.y, color).await;
    Json(serde_json::json!({ "ok": true }))
}

async fn node_info(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    Json(NodeInfo {
        id: s.node_id.to_string(),
        addr: s.addr.clone(),
    })
}

async fn get_palette(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let canvas = s.canvas.read().await;
    let colors: Vec<[u8; 4]> = canvas
        .palette_colors()
        .into_iter()
        .map(|(r, g, b, a)| [r, g, b, a])
        .collect();
    Json(colors)
}

async fn add_palette(
    State(s): State<Arc<AppState>>,
    Json(req): Json<PaletteRequest>,
) -> impl IntoResponse {
    s.add_palette_color((req.color[0], req.color[1], req.color[2], req.color[3]))
        .await;
    StatusCode::CREATED
}

async fn remove_palette(
    State(s): State<Arc<AppState>>,
    Json(req): Json<PaletteRequest>,
) -> impl IntoResponse {
    s.remove_palette_color((req.color[0], req.color[1], req.color[2], req.color[3]))
        .await;
    StatusCode::NO_CONTENT
}

async fn get_leaderboard(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let canvas = s.canvas.read().await;
    let board: Vec<LeaderboardEntry> = canvas
        .ownership_leaderboard()
        .into_iter()
        .map(|(id, n)| LeaderboardEntry {
            peer_id: id.to_string(),
            pixels: n,
        })
        .collect();
    Json(board)
}

async fn ws_handler(State(s): State<Arc<AppState>>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, s))
}

async fn handle_ws(mut socket: axum::extract::ws::WebSocket, state: Arc<AppState>) {
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
