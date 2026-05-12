//! # G-Counter (Grow-only Counter)
//!
//! A grow-only counter (G-Counter) is a state-based (CvRDT) counter that
//! supports only increment operations. Each replica keeps its own local
//! count, and the global value is the sum of all per-replica counts.
//!
//! Because each replica only writes to its own slot, and merging takes the
//! element-wise maximum, concurrent increments from different replicas
//! always converge without conflicts.
//!
//! ## Example
//!
//! ```
//! use crdt_core::counter::GCounter;
//! use crdt_core::traits::CvRDT;
//!
//! let mut a = GCounter::new();
//! let mut b = GCounter::new();
//!
//! a.increment(1, 3); // replica 1 adds 3
//! b.increment(2, 4); // replica 2 adds 4
//!
//! a.merge(&b);
//! assert_eq!(a.value(), 7);
//! ```

#![allow(clippy::module_name_repetitions)]

use crate::clock::ReplicaId;
use crate::traits::CvRDT;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A grow-only counter CRDT.
///
/// Internally stores a map from each replica's id to the number of
/// increments that replica has observed locally. The visible counter
/// value is the sum of all entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GCounter {
    /// Per-replica increment counts. Only the owning replica ever writes
    /// to its own slot; other replicas learn about the value via `merge`.
    counts: HashMap<ReplicaId, u64>,
}

impl GCounter {
    /// Create a new, empty counter with value `0`.
    pub fn new() -> Self {
        Self {
            counts: HashMap::new(),
        }
    }

    /// Increment this replica's local count by `n`.
    ///
    /// Each replica should only ever call `increment` with its own
    /// `ReplicaId`. Calling with another replica's id is allowed by the
    /// type system but breaks the convergence guarantees of the CRDT.
    pub fn increment(&mut self, replica: ReplicaId, n: u64) {
        *self.counts.entry(replica).or_insert(0) += n;
    }

    /// Return the current observed value of the counter, computed as the
    /// sum of all per-replica counts.
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Number of replicas this counter has observed at least one
    /// increment from. Useful for debugging and tests.
    pub fn replica_count(&self) -> usize {
        self.counts.len()
    }
}

impl CvRDT for GCounter {
    /// Merge another G-Counter into this one by taking the element-wise
    /// maximum of each replica's count.
    ///
    /// This operation is commutative, associative, and idempotent, which
    /// is what makes the G-Counter a CRDT: replicas can merge in any
    /// order, any number of times, and still converge to the same state.
    fn merge(&mut self, other: &Self) {
        for (replica, count) in &other.counts {
            let entry = self.counts.entry(*replica).or_insert(0);
            *entry = (*entry).max(*count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn empty_counter_is_zero() {
        assert_eq!(GCounter::new().value(), 0);
    }

    #[test]
    fn single_replica_increments_accumulate() {
        let mut c = GCounter::new();
        c.increment(1, 5);
        c.increment(1, 7);
        assert_eq!(c.value(), 12);
    }

    #[test]
    fn merge_combines_replicas() {
        let mut a = GCounter::new();
        a.increment(1, 5);
        let mut b = GCounter::new();
        b.increment(2, 3);
        a.merge(&b);
        assert_eq!(a.value(), 8);
    }

    #[test]
    fn merge_is_idempotent() {
        let mut a = GCounter::new();
        a.increment(1, 5);
        let snapshot = a.clone();
        a.merge(&snapshot);
        assert_eq!(a, snapshot);
    }

    #[test]
    fn merge_is_commutative() {
        let mut a = GCounter::new();
        a.increment(1, 3);
        let mut b = GCounter::new();
        b.increment(2, 4);

        let mut ab = a.clone();
        ab.merge(&b);
        let mut ba = b.clone();
        ba.merge(&a);
        assert_eq!(ab, ba);
    }

    // Helper: build a GCounter from a list of (replica, amount) pairs.
    fn build(ops: &[(ReplicaId, u64)]) -> GCounter {
        let mut c = GCounter::new();
        for (r, n) in ops {
            c.increment(*r, *n);
        }
        c
    }

    proptest! {
        #[test]
        fn prop_merge_is_commutative(
            ops_a in proptest::collection::vec((0u64..5, 0u64..100), 0..20),
            ops_b in proptest::collection::vec((0u64..5, 0u64..100), 0..20),
        ) {
            let a = build(&ops_a);
            let b = build(&ops_b);
            let mut ab = a.clone();
            ab.merge(&b);
            let mut ba = b.clone();
            ba.merge(&a);
            prop_assert_eq!(ab, ba);
        }

        #[test]
        fn prop_merge_is_associative(
            ops_a in proptest::collection::vec((0u64..5, 0u64..100), 0..20),
            ops_b in proptest::collection::vec((0u64..5, 0u64..100), 0..20),
            ops_c in proptest::collection::vec((0u64..5, 0u64..100), 0..20),
        ) {
            let a = build(&ops_a);
            let b = build(&ops_b);
            let c = build(&ops_c);

            // (a ∪ b) ∪ c
            let mut ab_c = a.clone();
            ab_c.merge(&b);
            ab_c.merge(&c);

            // a ∪ (b ∪ c)
            let mut bc = b.clone();
            bc.merge(&c);
            let mut a_bc = a.clone();
            a_bc.merge(&bc);

            prop_assert_eq!(ab_c, a_bc);
        }

        #[test]
        fn prop_merge_is_idempotent(
            ops in proptest::collection::vec((0u64..5, 0u64..100), 0..20),
        ) {
            let a = build(&ops);
            let snapshot = a.clone();
            let mut twice = a.clone();
            twice.merge(&snapshot);
            prop_assert_eq!(twice, snapshot);
        }
    }
}
