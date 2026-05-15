use crate::canvas::{CanvasDeltaView, CanvasDocument, CanvasView, LeaderboardEntry, Rgba};
use crate::state::AppState;
use axum::{
    body::Body,
    extract::{ws::WebSocketUpgrade, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use crdt_core::DeltaCrdt;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct WsQuery {
    id: Option<String>,
}
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

/// Embeds the built Vue frontend (`frontend/dist/`) into the binary at
/// compile time. `static_handler` serves these assets from `/`, so a
/// single binary ships the app end-to-end. Run `npm run build --prefix
/// frontend` before `cargo build` to populate `dist/`.
#[derive(RustEmbed)]
#[folder = "../frontend/dist/"]
struct Frontend;

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

/// Body for `POST /api/canvas/paint`.
#[derive(Deserialize)]
pub struct PaintRequest {
    pub x: u8,
    pub y: u8,
    pub color: [u8; 4],
}

/// Response for `GET /api/node` — identifies this peer on the network.
#[derive(Serialize)]
pub struct NodeInfo {
    /// UUID of this node, assigned at startup.
    pub id: String,
    /// Socket address this node is listening on (e.g. `"127.0.0.1:3000"`).
    pub addr: String,
}

/// Body for `POST /api/palette` and `DELETE /api/palette`.
#[derive(Deserialize)]
pub struct PaletteRequest {
    pub color: [u8; 4],
}

/// Body for `POST /api/canvas/cursor`.
#[derive(Deserialize)]
pub struct CursorRequest {
    /// UUID of the user whose cursor is being updated.
    pub user_id: String,
    pub x: u8,
    pub y: u8,
}

/// Build the application router with all API routes, the WebSocket endpoint,
/// and the static file fallback for the embedded Vue frontend.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/canvas", get(get_canvas))
        .route("/api/canvas/paint", post(paint))
        .route("/api/canvas/cursor", post(cursor))
        .route("/api/node", get(node_info))
        .route(
            "/api/palette",
            get(get_palette).post(add_palette).delete(remove_palette),
        )
        .route("/api/leaderboard", get(get_leaderboard))
        .route("/ws", get(ws_handler))
        .fallback(static_handler)
        .with_state(state)
        .layer(CorsLayer::permissive())
}

/// `GET /api/canvas` — returns the full canvas as a [`CanvasView`] JSON snapshot.
async fn get_canvas(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    Json(CanvasView::from(&*s.canvas()))
}

/// `POST /api/canvas/paint` — paint a single pixel; always returns `{ ok: true }`.
async fn paint(State(s): State<Arc<AppState>>, Json(req): Json<PaintRequest>) -> impl IntoResponse {
    let color: Rgba = (req.color[0], req.color[1], req.color[2], req.color[3]);
    s.paint(req.x, req.y, color);
    Json(serde_json::json!({ "ok": true }))
}

/// `GET /api/node` — returns this node's UUID and listening address.
async fn node_info(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    Json(NodeInfo {
        id: s.node_id().to_string(),
        addr: s.addr().to_string(),
    })
}

/// `POST /api/canvas/cursor` — update the cursor position for a user.
///
/// `user_id` is taken from the request body without authentication; any client
/// can move any cursor. Acceptable for this project scope (no auth layer).
async fn cursor(
    State(s): State<Arc<AppState>>,
    Json(req): Json<CursorRequest>,
) -> impl IntoResponse {
    match Uuid::parse_str(&req.user_id) {
        Ok(user_id) => {
            s.update_cursor(user_id, req.x, req.y);
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

/// `GET /api/palette` — returns the current shared palette as a JSON array of RGBA arrays.
async fn get_palette(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    let colors: Vec<[u8; 4]> = s
        .canvas()
        .palette_colors()
        .into_iter()
        .map(|(r, g, b, a)| [r, g, b, a])
        .collect();
    Json(colors)
}

/// `POST /api/palette` — add a color to the shared palette; returns 201 Created.
async fn add_palette(
    State(s): State<Arc<AppState>>,
    Json(req): Json<PaletteRequest>,
) -> impl IntoResponse {
    s.add_palette_color((req.color[0], req.color[1], req.color[2], req.color[3]));
    StatusCode::CREATED
}

/// `DELETE /api/palette` — remove a color from the shared palette.
///
/// Returns 204 No Content on success, 404 Not Found if the color was not in the palette.
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

/// `GET /api/leaderboard` — returns pixel ownership counts sorted descending.
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

async fn static_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Frontend::get(path) {
        Some(content) => Response::builder()
            .header(header::CONTENT_TYPE, content.metadata.mimetype())
            .body(Body::from(content.data.into_owned()))
            .unwrap(),
        None => match Frontend::get("index.html") {
            Some(index) => Response::builder()
                .header(header::CONTENT_TYPE, "text/html")
                .body(Body::from(index.data.into_owned()))
                .unwrap(),
            None => Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(
                    "Frontend not embedded. Build with `npm run build --prefix frontend` before `cargo build`.",
                ))
                .unwrap(),
        },
    }
}

/// `GET /ws` — upgrade to a WebSocket connection and hand off to [`handle_ws`].
///
/// Accepts an optional `?id=<uuid>` query parameter. The frontend passes its
/// stable `sessionStorage` UUID so that cursor keys and `active_peers` UUIDs
/// share the same namespace. Falls back to a fresh UUID when absent or invalid.
async fn ws_handler(
    State(s): State<Arc<AppState>>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let user_id = q
        .id
        .as_deref()
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::new_v4);
    ws.on_upgrade(move |socket| handle_ws(socket, s, user_id))
}

async fn handle_ws(
    mut socket: axum::extract::ws::WebSocket,
    state: Arc<AppState>,
    user_id: Uuid,
) {
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
