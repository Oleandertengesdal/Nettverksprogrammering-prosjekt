//! # Todo CRDT state
//!
//! The replica-local state of the todo list, built from the primitives
//! in `crdt-core`:
//! - two [`GSet`]s for the "added" and "removed" id sets (2P-Set semantics),
//! - a [`VectorClock`] for tracking causality across replicas,
//! - plain `HashMap`s for the immutable text and the toggleable completed
//!   flag (the latter will be upgraded to an `LwwRegister` once it lands
//!   in `crdt-core`).

use crdt_core::{GSet, ReplicaId, VectorClock};
use std::collections::HashMap;

use crate::protocol::{ClientMessage, PeerSnapshot, ServerMessage, TodoItem};

pub struct TodoState {
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

impl TodoState {
    pub fn new(peer_id: String, replica_id: ReplicaId) -> Self {
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

    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    pub fn apply(&mut self, msg: &ClientMessage) {
        match msg {
            ClientMessage::Add { text } => self.add(text),
            ClientMessage::Toggle { id } => self.toggle(id),
            ClientMessage::Delete { id } => self.delete(id),
            ClientMessage::Resync => {}
        }
    }

    pub fn add(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        self.clock.increment(self.replica_id);
        let id = self.new_todo_id();
        self.added.insert(id.clone());
        self.texts.insert(id.clone(), trimmed.to_string());
        self.completed.insert(id.clone(), false);
        self.creators.insert(id, self.peer_id.clone());
    }

    pub fn toggle(&mut self, id: &str) {
        let key = id.to_string();
        if !self.added.contains(&key) || self.removed.contains(&key) {
            return;
        }
        self.clock.increment(self.replica_id);
        let current = self.completed.get(id).copied().unwrap_or(false);
        self.completed.insert(key, !current);
    }

    pub fn delete(&mut self, id: &str) {
        let key = id.to_string();
        if !self.added.contains(&key) {
            return;
        }
        self.clock.increment(self.replica_id);
        self.removed.insert(key);
    }

    pub fn snapshot(&self) -> ServerMessage {
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

    fn new_todo_id(&mut self) -> String {
        self.next_local_id += 1;
        format!("{}-{}", self.peer_id, self.next_local_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_creates_todo() {
        let mut s = TodoState::new("peer-1".into(), 1);
        s.add("Buy milk");
        if let ServerMessage::Snapshot { peers, .. } = s.snapshot() {
            assert_eq!(peers[0].todos.len(), 1);
            assert_eq!(peers[0].todos[0].text, "Buy milk");
            assert!(!peers[0].todos[0].completed);
        }
    }

    #[test]
    fn empty_add_is_ignored() {
        let mut s = TodoState::new("peer-1".into(), 1);
        s.add("   ");
        if let ServerMessage::Snapshot { peers, .. } = s.snapshot() {
            assert_eq!(peers[0].todos.len(), 0);
        }
    }

    #[test]
    fn toggle_flips_completion() {
        let mut s = TodoState::new("peer-1".into(), 1);
        s.add("Walk dog");
        let id = match s.snapshot() {
            ServerMessage::Snapshot { peers, .. } => peers[0].todos[0].id.clone(),
        };
        s.toggle(&id);
        if let ServerMessage::Snapshot { peers, .. } = s.snapshot() {
            assert!(peers[0].todos[0].completed);
        }
        s.toggle(&id);
        if let ServerMessage::Snapshot { peers, .. } = s.snapshot() {
            assert!(!peers[0].todos[0].completed);
        }
    }

    #[test]
    fn delete_removes_todo_from_snapshot() {
        let mut s = TodoState::new("peer-1".into(), 1);
        s.add("Temporary");
        let id = match s.snapshot() {
            ServerMessage::Snapshot { peers, .. } => peers[0].todos[0].id.clone(),
        };
        s.delete(&id);
        if let ServerMessage::Snapshot { peers, .. } = s.snapshot() {
            assert_eq!(peers[0].todos.len(), 0);
        }
    }
}
