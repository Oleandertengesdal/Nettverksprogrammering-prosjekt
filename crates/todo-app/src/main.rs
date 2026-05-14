//! # CRDT Todo Server — bootstrap
//!
//! Reads environment variables, builds the CRDT state, wires up the
//! axum router and the `crdt-sync` peer-to-peer layer, and starts
//! listening. The HTTP server and the sync node share the same
//! `Arc<RwLock<TodoState>>` so local edits and remote merges are both
//! visible to all connected browsers.

mod protocol;
mod server;
mod state;

use std::net::SocketAddr;

use crdt_core::ReplicaId;
use crdt_sync::SyncNode;

use crate::server::{broadcast_snapshot, AppState};
use crate::state::TodoState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = Config::from_env();
    tracing::info!(
        "starting peer {} on http://127.0.0.1:{} (sync listen: {}, peers: {:?})",
        config.peer_id,
        config.port,
        config.sync_listen,
        config.sync_peers
    );

    let app_state = AppState::new(TodoState::new(config.peer_id.clone(), config.replica_id));

    let sync_node = SyncNode::new(
        config.peer_id.clone(),
        app_state.inner.clone(),
        config.sync_listen,
        config.sync_peers.clone(),
    );

    let notifier = sync_node.change_notifier();
    let mut remote_updates = sync_node.subscribe_remote_updates();
    let app_state = app_state.with_sync_notifier(notifier);

    sync_node.run().await?;

    let state_for_remote = app_state.clone();
    tokio::spawn(async move {
        while remote_updates.recv().await.is_ok() {
            broadcast_snapshot(&state_for_remote).await;
        }
    });

    let app = server::router(app_state, &config.static_dir);
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

struct Config {
    peer_id: String,
    replica_id: ReplicaId,
    port: u16,
    static_dir: String,
    sync_listen: SocketAddr,
    sync_peers: Vec<SocketAddr>,
}

impl Config {
    fn from_env() -> Self {
        let peer_id = std::env::var("PEER_ID").unwrap_or_else(|_| "peer-1".to_string());
        let port: u16 = parse_env("PORT").unwrap_or(8080);
        let static_dir =
            std::env::var("STATIC_DIR").unwrap_or_else(|_| "crates/todo-app/static".to_string());
        let replica_id: ReplicaId = peer_id
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(1);

        let sync_port: u16 = parse_env("SYNC_PORT").unwrap_or(port + 1000);
        let sync_listen = SocketAddr::from(([127, 0, 0, 1], sync_port));
        let sync_peers = std::env::var("SYNC_PEERS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| s.trim().parse::<SocketAddr>().ok())
            .collect();

        Self {
            peer_id,
            replica_id,
            port,
            static_dir,
            sync_listen,
            sync_peers,
        }
    }
}

fn parse_env<T: std::str::FromStr>(key: &str) -> Option<T> {
    std::env::var(key).ok().and_then(|s| s.parse().ok())
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "todo_app=info,crdt_sync=info,axum=info".to_string()),
        )
        .init();
}
