//! # Vector Clock
//!
//! A logical clock that tracks causality between events across replicas.
//! Each replica maintains a per-replica counter; comparing two clocks
//! reveals whether one event happened-before another, whether they are
//! equal, or whether they are concurrent. The clock is itself a CvRDT:
//! merging takes the element-wise maximum of the two clocks.

use crate::clock::ReplicaId;
use crate::traits::CvRDT;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    entries: HashMap<ReplicaId, u64>,
}

impl VectorClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn increment(&mut self, replica: ReplicaId) -> u64 {
        let entry = self.entries.entry(replica).or_insert(0);
        *entry += 1;
        *entry
    }

    pub fn get(&self, replica: ReplicaId) -> u64 {
        self.entries.get(&replica).copied().unwrap_or(0)
    }

    pub fn observe(&mut self, replica: ReplicaId, counter: u64) {
        if counter == 0 {
            return;
        }
        let entry = self.entries.entry(replica).or_insert(0);
        *entry = (*entry).max(counter);
    }

    pub fn happens_before(&self, other: &Self) -> bool {
        matches!(self.partial_cmp(other), Some(Ordering::Less))
    }

    pub fn concurrent_with(&self, other: &Self) -> bool {
        self.partial_cmp(other).is_none()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl PartialOrd for VectorClock {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let mut self_less = false;
        let mut self_greater = false;

        for replica in self.entries.keys().chain(other.entries.keys()) {
            let a = self.entries.get(replica).copied().unwrap_or(0);
            let b = other.entries.get(replica).copied().unwrap_or(0);
            match a.cmp(&b) {
                Ordering::Less => self_less = true,
                Ordering::Greater => self_greater = true,
                Ordering::Equal => {}
            }
            if self_less && self_greater {
                return None;
            }
        }

        match (self_less, self_greater) {
            (false, false) => Some(Ordering::Equal),
            (true, false) => Some(Ordering::Less),
            (false, true) => Some(Ordering::Greater),
            (true, true) => None,
        }
    }
}

impl CvRDT for VectorClock {
    fn merge(&mut self, other: &Self) {
        for (replica, count) in &other.entries {
            if *count == 0 {
                continue;
            }
            let entry = self.entries.entry(*replica).or_insert(0);
            *entry = (*entry).max(*count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn empty_clocks_are_equal() {
        let a = VectorClock::new();
        let b = VectorClock::new();
        assert_eq!(a, b);
        assert_eq!(a.partial_cmp(&b), Some(Ordering::Equal));
    }

    #[test]
    fn increment_advances_own_counter() {
        let mut a = VectorClock::new();
        assert_eq!(a.increment(1), 1);
        assert_eq!(a.increment(1), 2);
        assert_eq!(a.get(1), 2);
        assert_eq!(a.get(2), 0);
    }

    #[test]
    fn happens_before_detected() {
        let mut a = VectorClock::new();
        a.increment(1);
        let mut b = a.clone();
        b.increment(1);
        assert!(a.happens_before(&b));
        assert!(!b.happens_before(&a));
        assert_eq!(a.partial_cmp(&b), Some(Ordering::Less));
    }

    #[test]
    fn concurrent_events_detected() {
        let mut a = VectorClock::new();
        a.increment(1);
        let mut b = VectorClock::new();
        b.increment(2);
        assert!(a.concurrent_with(&b));
        assert!(b.concurrent_with(&a));
        assert_eq!(a.partial_cmp(&b), None);
    }

    #[test]
    fn equal_clocks_are_not_concurrent() {
        let mut a = VectorClock::new();
        a.increment(1);
        let b = a.clone();
        assert!(!a.concurrent_with(&b));
        assert!(!a.happens_before(&b));
    }

    #[test]
    fn merge_takes_elementwise_max() {
        let mut a = VectorClock::new();
        a.increment(1);
        a.increment(1);
        let mut b = VectorClock::new();
        b.increment(1);
        b.increment(2);
        a.merge(&b);
        assert_eq!(a.get(1), 2);
        assert_eq!(a.get(2), 1);
    }

    #[test]
    fn observe_advances_when_higher() {
        let mut a = VectorClock::new();
        a.observe(1, 5);
        assert_eq!(a.get(1), 5);
        a.observe(1, 3);
        assert_eq!(a.get(1), 5);
        a.observe(1, 7);
        assert_eq!(a.get(1), 7);
    }

    fn build(ops: &[(ReplicaId, u8)]) -> VectorClock {
        let mut v = VectorClock::new();
        for (r, n) in ops {
            for _ in 0..*n {
                v.increment(*r);
            }
        }
        v
    }

    proptest! {
        #[test]
        fn prop_merge_is_commutative(
            ops_a in proptest::collection::vec((0u64..5, 0u8..10), 0..20),
            ops_b in proptest::collection::vec((0u64..5, 0u8..10), 0..20),
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
            ops_a in proptest::collection::vec((0u64..5, 0u8..10), 0..20),
            ops_b in proptest::collection::vec((0u64..5, 0u8..10), 0..20),
            ops_c in proptest::collection::vec((0u64..5, 0u8..10), 0..20),
        ) {
            let a = build(&ops_a);
            let b = build(&ops_b);
            let c = build(&ops_c);

            let mut ab_c = a.clone();
            ab_c.merge(&b);
            ab_c.merge(&c);

            let mut bc = b.clone();
            bc.merge(&c);
            let mut a_bc = a.clone();
            a_bc.merge(&bc);

            prop_assert_eq!(ab_c, a_bc);
        }

        #[test]
        fn prop_merge_is_idempotent(
            ops in proptest::collection::vec((0u64..5, 0u8..10), 0..20),
        ) {
            let a = build(&ops);
            let snapshot = a.clone();
            let mut twice = a.clone();
            twice.merge(&snapshot);
            prop_assert_eq!(twice, snapshot);
        }

        #[test]
        fn prop_increment_makes_clock_greater(
            ops in proptest::collection::vec((0u64..5, 0u8..10), 0..20),
            replica in 0u64..5,
        ) {
            let before = build(&ops);
            let mut after = before.clone();
            after.increment(replica);
            prop_assert!(before.happens_before(&after));
            prop_assert!(!after.happens_before(&before));
        }

        #[test]
        fn prop_merged_clock_dominates_both(
            ops_a in proptest::collection::vec((0u64..5, 0u8..10), 0..20),
            ops_b in proptest::collection::vec((0u64..5, 0u8..10), 0..20),
        ) {
            let a = build(&ops_a);
            let b = build(&ops_b);
            let mut merged = a.clone();
            merged.merge(&b);
            prop_assert!(matches!(a.partial_cmp(&merged), Some(Ordering::Less) | Some(Ordering::Equal)));
            prop_assert!(matches!(b.partial_cmp(&merged), Some(Ordering::Less) | Some(Ordering::Equal)));
        }
    }
}
