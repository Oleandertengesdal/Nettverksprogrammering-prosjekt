//! # Todo CRDT state
//!
//! The replica-local state of the todo list, built from the primitives
//! in `crdt-core`:
//! - two [`GSet`]s for the "added" and "removed" id sets (2P-Set semantics),
//! - a [`VectorClock`] for tracking causality across replicas,
//! - plain `HashMap`s for the immutable text and creator labels,
//! - a `HashMap<id, bool>` for the toggleable completed flag, merged as
//!   monotonic OR. (Known limitation: a completed item can't be un-completed
//!   once another replica observes it as completed. Replacing this with an
//!   `LwwRegister` is on the roadmap.)
//!
//! Because every field is itself either a CRDT or a monotonic structure,
//! `TodoState` as a whole implements [`CvRDT`] and can be safely synced
//! between replicas by `crdt-sync`.

use crdt_core::{CvRDT, GSet, ReplicaId, VectorClock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::protocol::{ClientMessage, PeerSnapshot, ServerMessage, TodoItem};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct TodoState {
    #[serde(skip)]
    peer_id: String,
    #[serde(skip)]
    replica_id: ReplicaId,
    #[serde(skip)]
    next_local_id: u64,

    added: GSet<String>,
    removed: GSet<String>,
    texts: HashMap<String, String>,
    completed: HashMap<String, bool>,
    creators: HashMap<String, String>,
    clock: VectorClock,
}

impl TodoState {
    pub fn new(peer_id: String, replica_id: ReplicaId) -> Self {
        Self {
            peer_id,
            replica_id,
            next_local_id: 0,
            added: GSet::new(),
            removed: GSet::new(),
            texts: HashMap::new(),
            completed: HashMap::new(),
            creators: HashMap::new(),
            clock: VectorClock::new(),
        }
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

impl CvRDT for TodoState {
    fn merge(&mut self, other: &Self) {
        self.added.merge(&other.added);
        self.removed.merge(&other.removed);

        for (id, text) in &other.texts {
            self.texts.entry(id.clone()).or_insert_with(|| text.clone());
        }
        for (id, creator) in &other.creators {
            self.creators
                .entry(id.clone())
                .or_insert_with(|| creator.clone());
        }
        for (id, &remote_completed) in &other.completed {
            let entry = self.completed.entry(id.clone()).or_insert(false);
            *entry = *entry || remote_completed;
        }

        self.clock.merge(&other.clock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::PeerSnapshot;

    fn peers(state: &TodoState) -> Vec<PeerSnapshot> {
        let ServerMessage::Snapshot { peers, .. } = state.snapshot();
        peers
    }

    #[test]
    fn add_creates_todo() {
        let mut s = TodoState::new("peer-1".into(), 1);
        s.add("Buy milk");
        let p = peers(&s);
        assert_eq!(p[0].todos.len(), 1);
        assert_eq!(p[0].todos[0].text, "Buy milk");
        assert!(!p[0].todos[0].completed);
    }

    #[test]
    fn empty_add_is_ignored() {
        let mut s = TodoState::new("peer-1".into(), 1);
        s.add("   ");
        assert_eq!(peers(&s)[0].todos.len(), 0);
    }

    #[test]
    fn toggle_flips_completion() {
        let mut s = TodoState::new("peer-1".into(), 1);
        s.add("Walk dog");
        let id = peers(&s)[0].todos[0].id.clone();

        s.toggle(&id);
        assert!(peers(&s)[0].todos[0].completed);

        s.toggle(&id);
        assert!(!peers(&s)[0].todos[0].completed);
    }

    #[test]
    fn delete_removes_todo_from_snapshot() {
        let mut s = TodoState::new("peer-1".into(), 1);
        s.add("Temporary");
        let id = peers(&s)[0].todos[0].id.clone();
        s.delete(&id);
        assert_eq!(peers(&s)[0].todos.len(), 0);
    }

    #[test]
    fn merge_combines_additions_from_two_replicas() {
        let mut a = TodoState::new("peer-a".into(), 1);
        let mut b = TodoState::new("peer-b".into(), 2);
        a.add("From A");
        b.add("From B");
        a.merge(&b);
        b.merge(&a);
        assert_eq!(peers(&a)[0].todos.len(), 2);
        assert_eq!(peers(&b)[0].todos.len(), 2);
    }

    #[test]
    fn merge_propagates_deletion() {
        let mut a = TodoState::new("peer-a".into(), 1);
        let mut b = TodoState::new("peer-b".into(), 2);
        a.add("Doomed");
        let id = peers(&a)[0].todos[0].id.clone();
        b.merge(&a);
        assert_eq!(peers(&b)[0].todos.len(), 1);
        a.delete(&id);
        b.merge(&a);
        assert_eq!(peers(&b)[0].todos.len(), 0);
    }
}
