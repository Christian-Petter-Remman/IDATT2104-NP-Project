use crate::canvas::{CanvasDeltaView, CanvasDocument, CanvasView, LeaderboardEntry, Rgba};
use crate::state::AppState;
use axum::{
    extract::{ws::WebSocketUpgrade, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use crdt_core::DeltaCrdt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

/// Envelope for messages pushed to the browser over the WebSocket.
///
/// `Snapshot` carries the full [`CanvasView`] and is sent once when a
/// client connects (or after a server-side reset). `Delta` carries a
/// sparse [`CanvasDeltaView`] computed against that client's last-known
/// vector clock.
#[derive(Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
enum WsMessage {
    Snapshot(CanvasView),
    Delta(CanvasDeltaView),
}

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
        .route(
            "/api/palette",
            get(get_palette).post(add_palette).delete(remove_palette),
        )
        .route("/api/leaderboard", get(get_leaderboard))
        .route("/ws", get(ws_handler))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

async fn get_canvas(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    Json(CanvasView::from(&*s.canvas()))
}

async fn paint(State(s): State<Arc<AppState>>, Json(req): Json<PaintRequest>) -> impl IntoResponse {
    let color: Rgba = (req.color[0], req.color[1], req.color[2], req.color[3]);
    s.paint(req.x, req.y, color);
    Json(serde_json::json!({ "ok": true }))
}

async fn node_info(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    Json(NodeInfo {
        id: s.node_id().to_string(),
        addr: s.addr().to_string(),
    })
}

async fn get_palette(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let colors: Vec<[u8; 4]> = s
        .canvas()
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
    s.add_palette_color((req.color[0], req.color[1], req.color[2], req.color[3]));
    StatusCode::CREATED
}

async fn remove_palette(
    State(s): State<Arc<AppState>>,
    Json(req): Json<PaletteRequest>,
) -> impl IntoResponse {
    let removed = s.remove_palette_color((req.color[0], req.color[1], req.color[2], req.color[3]));
    if removed {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn get_leaderboard(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let board: Vec<LeaderboardEntry> = s
        .canvas()
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
    let user_id = Uuid::new_v4();
    state.add_user(user_id);

    // Send an initial full snapshot and remember the version it covers.
    // Subsequent pushes are deltas computed against this watermark.
    let mut last_seen = {
        let snapshot = state.snapshot();
        let version = snapshot.version();
        let envelope = WsMessage::Snapshot(CanvasView::from(&snapshot));
        let Ok(msg) = serde_json::to_string(&envelope) else {
            tracing::error!("failed to serialize canvas snapshot");
            state.remove_user(&user_id);
            return;
        };
        if socket
            .send(axum::extract::ws::Message::Text(msg))
            .await
            .is_err()
        {
            state.remove_user(&user_id);
            return;
        }
        version
    };

    let mut rx = state.subscribe();
    loop {
        if rx.changed().await.is_err() {
            break;
        }
        let snapshot = rx.borrow_and_update().clone();
        let delta = snapshot.delta_since(&last_seen);
        if CanvasDocument::is_empty_delta(&delta) {
            continue;
        }
        let view = CanvasDeltaView::project(&delta, &snapshot);
        let envelope = WsMessage::Delta(view);
        let Ok(msg) = serde_json::to_string(&envelope) else {
            tracing::error!("failed to serialize canvas delta");
            break;
        };
        if socket
            .send(axum::extract::ws::Message::Text(msg))
            .await
            .is_err()
        {
            break;
        }
        last_seen = snapshot.version();
    }

    state.remove_user(&user_id);
}
