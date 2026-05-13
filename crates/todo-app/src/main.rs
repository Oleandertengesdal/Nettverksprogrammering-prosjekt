//! # CRDT Todo Server — bootstrap
//!
//! Parses environment variables, builds the CRDT state, wires up the
//! axum router, and starts listening. All the real logic lives in the
//! sibling modules: `protocol`, `state`, and `server`.

mod protocol;
mod server;
mod state;

use std::net::SocketAddr;

use crdt_core::ReplicaId;

use crate::server::AppState;
use crate::state::TodoState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = Config::from_env();
    tracing::info!(
        "starting peer {} on http://127.0.0.1:{} (static dir: {})",
        config.peer_id,
        config.port,
        config.static_dir
    );

    let state = AppState::new(TodoState::new(config.peer_id.clone(), config.replica_id));
    let app = server::router(state, &config.static_dir);

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
}

impl Config {
    fn from_env() -> Self {
        let peer_id = std::env::var("PEER_ID").unwrap_or_else(|_| "peer-1".to_string());
        let port: u16 = std::env::var("PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8080);
        let static_dir =
            std::env::var("STATIC_DIR").unwrap_or_else(|_| "crates/todo-app/static".to_string());
        let replica_id: ReplicaId = peer_id
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(1);
        Self {
            peer_id,
            replica_id,
            port,
            static_dir,
        }
    }
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "todo_app=info,axum=info".to_string()),
        )
        .init();
}
