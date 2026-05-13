//! # Wire protocol
//!
//! JSON messages exchanged between the JS frontend and the Rust server
//! over the WebSocket. All field names are serialized in camelCase to
//! match the conventions of the JS client.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerMessage {
    #[serde(rename = "snapshot")]
    Snapshot {
        local_peer_id: String,
        peers: Vec<PeerSnapshot>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerSnapshot {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub clock: HashMap<String, u64>,
    pub todos: Vec<TodoItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    pub completed: bool,
    pub created_by: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "todo.add")]
    Add { text: String },
    #[serde(rename = "todo.toggle")]
    Toggle { id: String },
    #[serde(rename = "todo.delete")]
    Delete { id: String },
    #[serde(rename = "resync")]
    Resync,
}
