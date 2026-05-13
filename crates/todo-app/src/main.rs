//! # CRDT Todo Server
//!
//! Single-node axum server that owns a CRDT todo state, serves the static
//! frontend, and pushes live state updates over a WebSocket. Every client
//! operation triggers a fresh snapshot broadcast to every connected client.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use crdt_core::{GSet, ReplicaId, VectorClock};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use tower_http::services::ServeDir;

#[derive(Clone)]
struct AppState {
    inner: Arc<RwLock<Inner>>,
    tx: broadcast::Sender<String>,
}

struct Inner {
    peer_id: String,
    replica_id: ReplicaId,
    added: GSet<String>,
    removed: GSet<String>,
    texts: HashMap<String, String>,
    completed: HashMap<String, bool>,
    creators: HashMap<String, String>,
    clock: VectorClock,
    next_local_id: u64,
}

impl Inner {
    fn new(peer_id: String, replica_id: ReplicaId) -> Self {
        Self {
            peer_id,
            replica_id,
            added: GSet::new(),
            removed: GSet::new(),
            texts: HashMap::new(),
            completed: HashMap::new(),
            creators: HashMap::new(),
            clock: VectorClock::new(),
            next_local_id: 0,
        }
    }

    fn new_todo_id(&mut self) -> String {
        self.next_local_id += 1;
        format!("{}-{}", self.peer_id, self.next_local_id)
    }

    fn add(&mut self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.clock.increment(self.replica_id);
        let id = self.new_todo_id();
        self.added.insert(id.clone());
        self.texts.insert(id.clone(), text);
        self.completed.insert(id.clone(), false);
        self.creators.insert(id, self.peer_id.clone());
    }

    fn toggle(&mut self, id: &str) {
        if !self.added.contains(&id.to_string()) || self.removed.contains(&id.to_string()) {
            return;
        }
        self.clock.increment(self.replica_id);
        let current = self.completed.get(id).copied().unwrap_or(false);
        self.completed.insert(id.to_string(), !current);
    }

    fn delete(&mut self, id: &str) {
        if !self.added.contains(&id.to_string()) {
            return;
        }
        self.clock.increment(self.replica_id);
        self.removed.insert(id.to_string());
    }

    fn snapshot(&self) -> ServerMessage {
        let mut todos: Vec<TodoItem> = self
            .added
            .iter()
            .filter(|id| !self.removed.contains(*id))
            .filter_map(|id| {
                Some(TodoItem {
                    id: id.clone(),
                    text: self.texts.get(id)?.clone(),
                    completed: self.completed.get(id).copied().unwrap_or(false),
                    created_by: self
                        .creators
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| self.peer_id.clone()),
                })
            })
            .collect();
        todos.sort_by(|a, b| a.id.cmp(&b.id));

        let mut clock = HashMap::new();
        clock.insert(self.peer_id.clone(), self.clock.get(self.replica_id));

        ServerMessage::Snapshot {
            local_peer_id: self.peer_id.clone(),
            peers: vec![PeerSnapshot {
                id: self.peer_id.clone(),
                name: format!("Peer {}", self.peer_id),
                online: true,
                clock,
                todos,
            }],
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ServerMessage {
    #[serde(rename = "snapshot")]
    Snapshot {
        local_peer_id: String,
        peers: Vec<PeerSnapshot>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PeerSnapshot {
    id: String,
    name: String,
    online: bool,
    clock: HashMap<String, u64>,
    todos: Vec<TodoItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TodoItem {
    id: String,
    text: String,
    completed: bool,
    created_by: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "todo.add")]
    Add { text: String },
    #[serde(rename = "todo.toggle")]
    Toggle { id: String },
    #[serde(rename = "todo.delete")]
    Delete { id: String },
    #[serde(rename = "resync")]
    Resync,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "todo_app=info,axum=info".to_string()),
        )
        .init();

    let peer_id = std::env::var("PEER_ID").unwrap_or_else(|_| "peer-1".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let static_dir = std::env::var("STATIC_DIR")
        .unwrap_or_else(|_| "crates/todo-app/static".to_string());

    let replica_id: ReplicaId = peer_id
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(1);

    let (tx, _rx) = broadcast::channel::<String>(64);
    let state = AppState {
        inner: Arc::new(RwLock::new(Inner::new(peer_id.clone(), replica_id))),
        tx,
    };

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .fallback_service(ServeDir::new(&static_dir))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("peer {} listening on http://{}", peer_id, addr);
    tracing::info!("serving static files from {}", static_dir);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    {
        let inner = state.inner.read().await;
        let snapshot = inner.snapshot();
        match serde_json::to_string(&snapshot) {
            Ok(json) => {
                if sender.send(Message::Text(json)).await.is_err() {
                    return;
                }
            }
            Err(e) => {
                tracing::error!("failed to serialize initial snapshot: {}", e);
                return;
            }
        }
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
                    Ok(client_msg) => apply_client_message(&state_for_recv, client_msg).await,
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

async fn apply_client_message(state: &AppState, msg: ClientMessage) {
    {
        let mut inner = state.inner.write().await;
        match msg {
            ClientMessage::Add { text } => inner.add(text),
            ClientMessage::Toggle { id } => inner.toggle(&id),
            ClientMessage::Delete { id } => inner.delete(&id),
            ClientMessage::Resync => {}
        }
    }

    broadcast_snapshot(state).await;
}

async fn broadcast_snapshot(state: &AppState) {
    let inner = state.inner.read().await;
    let snapshot = inner.snapshot();
    drop(inner);

    match serde_json::to_string(&snapshot) {
        Ok(json) => {
            let _ = state.tx.send(json);
        }
        Err(e) => tracing::error!("failed to serialize snapshot: {}", e),
    }
}

