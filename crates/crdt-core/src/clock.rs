//! # Clock primitives
//!
//! Building blocks for logical clocks used by the CRDTs in this crate.
//!
//! Currently exposes:
//! - [`ReplicaId`]: a stable identifier for a single replica.
//! - [`Dot`]: a `(replica, counter)` pair used to tag individual events.

use serde::{Deserialize, Serialize};

/// Identifier for a replica in the distributed system.
///
/// For this project a `u64` is sufficient; in a real deployment a
/// `Uuid` would be a safer choice to avoid collisions between nodes
/// that picked ids independently.
pub type ReplicaId = u64;

/// A single event in a replica's history, identified by the replica
/// that produced it and a monotonically increasing local counter.
///
/// Dots are the atomic unit of causality used by more advanced CRDTs
/// such as OR-Sets and dotted version vectors.
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
