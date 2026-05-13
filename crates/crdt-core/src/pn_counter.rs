use crate::clock::ReplicaId;
use crate::counter::GCounter;
use crate::traits::CvRDT;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PNCounter {
    increments: GCounter,
    decrements: GCounter,
}

impl PNCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn increment(&mut self, replica: ReplicaId, n: u64) {
        self.increments.increment(replica, n);
    }

    pub fn decrement(&mut self, replica: ReplicaId, n: u64) {
        self.decrements.increment(replica, n);
    }

    pub fn value(&self) -> i128 {
        i128::from(self.increments.value()) - i128::from(self.decrements.value())
    }
}

impl CvRDT for PNCounter {
    fn merge(&mut self, other: &Self) {
        self.increments.merge(&other.increments);
        self.decrements.merge(&other.decrements);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn empty_counter_is_zero() {
        assert_eq!(PNCounter::new().value(), 0);
    }

    #[test]
    fn increments_and_decrements_combine() {
        let mut c = PNCounter::new();
        c.increment(1, 10);
        c.decrement(1, 3);
        assert_eq!(c.value(), 7);
    }

    #[test]
    fn value_can_be_negative() {
        let mut c = PNCounter::new();
        c.decrement(1, 5);
        assert_eq!(c.value(), -5);
    }

    #[test]
    fn merge_combines_replicas() {
        let mut a = PNCounter::new();
        a.increment(1, 5);
        a.decrement(1, 1);
        let mut b = PNCounter::new();
        b.increment(2, 3);
        b.decrement(2, 2);
        a.merge(&b);
        assert_eq!(a.value(), 5);
    }

    #[test]
    fn merge_is_idempotent() {
        let mut a = PNCounter::new();
        a.increment(1, 5);
        a.decrement(1, 2);
        let snapshot = a.clone();
        a.merge(&snapshot);
        assert_eq!(a, snapshot);
    }

    fn build(ops: &[(ReplicaId, bool, u64)]) -> PNCounter {
        let mut c = PNCounter::new();
        for (r, is_inc, n) in ops {
            if *is_inc {
                c.increment(*r, *n);
            } else {
                c.decrement(*r, *n);
            }
        }
        c
    }

    proptest! {
        #[test]
        fn prop_merge_is_commutative(
            ops_a in proptest::collection::vec((0u64..5, any::<bool>(), 0u64..100), 0..20),
            ops_b in proptest::collection::vec((0u64..5, any::<bool>(), 0u64..100), 0..20),
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
            ops_a in proptest::collection::vec((0u64..5, any::<bool>(), 0u64..100), 0..20),
            ops_b in proptest::collection::vec((0u64..5, any::<bool>(), 0u64..100), 0..20),
            ops_c in proptest::collection::vec((0u64..5, any::<bool>(), 0u64..100), 0..20),
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
            ops in proptest::collection::vec((0u64..5, any::<bool>(), 0u64..100), 0..20),
        ) {
            let a = build(&ops);
            let snapshot = a.clone();
            let mut twice = a.clone();
            twice.merge(&snapshot);
            prop_assert_eq!(twice, snapshot);
        }
    }
}
