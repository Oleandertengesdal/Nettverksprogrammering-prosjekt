//! # Clock primitives

use serde::{Deserialize, Serialize};

/// Identifier for a replica in the distributed system.

pub type ReplicaId = u64;

/// A single event in a replica's history, identified by the replica
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Dot {
    /// Replica-local sequence number for the event.
    pub counter: u64,
    /// The replica that produced the event.
    pub replica: ReplicaId,
}

impl Dot {
    /// Create a new dot for `replica` at sequence number `counter`.
    pub fn new(replica: ReplicaId, counter: u64) -> Self {
        Self { counter, replica }
    }
}
