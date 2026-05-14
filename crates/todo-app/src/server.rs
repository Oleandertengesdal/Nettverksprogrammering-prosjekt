//! # HTTP/WebSocket server
//!
//! Builds the axum router, handles WebSocket upgrades, and bridges
//! client messages into the local CRDT state. Every applied operation:
//! 1) mutates the shared `TodoState`,
//! 2) wakes the sync layer so it can push the new state to peers,
//! 3) broadcasts a fresh JSON snapshot to every connected browser.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tower_http::services::ServeDir;

use crate::protocol::ClientMessage;
use crate::state::TodoState;

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<RwLock<TodoState>>,
    pub ws_tx: broadcast::Sender<String>,
    pub sync_notify: Option<broadcast::Sender<()>>,
}

impl AppState {
    pub fn new(state: TodoState) -> Self {
        let (ws_tx, _rx) = broadcast::channel::<String>(64);
        Self {
            inner: Arc::new(RwLock::new(state)),
            ws_tx,
            sync_notify: None,
        }
    }

    /// Attach a sync-layer notifier so local mutations propagate to peers.
    pub fn with_sync_notifier(mut self, sender: broadcast::Sender<()>) -> Self {
        self.sync_notify = Some(sender);
        self
    }
}

pub fn router(state: AppState, static_dir: &str) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .fallback_service(ServeDir::new(static_dir))
        .with_state(state)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.ws_tx.subscribe();

    if send_current_snapshot(&state, &mut sender).await.is_err() {
        return;
    }

    let state_for_recv = state.clone();
    let mut send_task = tokio::spawn(async move {
        while let Ok(json) = rx.recv().await {
            if sender.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(client_msg) => apply_and_broadcast(&state_for_recv, client_msg).await,
                    Err(e) => tracing::warn!("ignoring malformed message: {}", e),
                },
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

async fn send_current_snapshot(
    state: &AppState,
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
) -> Result<(), ()> {
    let snapshot = state.inner.read().await.snapshot();
    match serde_json::to_string(&snapshot) {
        Ok(json) => sender.send(Message::Text(json)).await.map_err(|_| ()),
        Err(e) => {
            tracing::error!("failed to serialize initial snapshot: {}", e);
            Err(())
        }
    }
}

async fn apply_and_broadcast(state: &AppState, msg: ClientMessage) {
    {
        let mut inner = state.inner.write().await;
        inner.apply(&msg);
    }
    if let Some(notify) = &state.sync_notify {
        let _ = notify.send(());
    }
    broadcast_snapshot(state).await;
}

pub async fn broadcast_snapshot(state: &AppState) {
    let snapshot = {
        let inner = state.inner.read().await;
        inner.snapshot()
    };

    match serde_json::to_string(&snapshot) {
        Ok(json) => {
            let _ = state.ws_tx.send(json);
        }
        Err(e) => tracing::error!("failed to serialize snapshot: {}", e),
    }
}
