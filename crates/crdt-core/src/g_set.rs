//! # G-Set (Grow-only Set)
//!
//! A grow-only set CRDT: elements can be added but never removed.
//! Merge is set union, which is commutative, associative, and idempotent.

use crate::traits::CvRDT;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::Hash;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GSet<T: Eq + Hash> {
    elements: HashSet<T>,
}

impl<T: Eq + Hash> Default for GSet<T> {
    fn default() -> Self {
        Self {
            elements: HashSet::new(),
        }
    }
}

impl<T: Eq + Hash> GSet<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, value: T) -> bool {
        self.elements.insert(value)
    }

    pub fn contains(&self, value: &T) -> bool {
        self.elements.contains(value)
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.elements.iter()
    }
}

impl<T: Eq + Hash + Clone> CvRDT for GSet<T> {
    fn merge(&mut self, other: &Self) {
        self.elements.extend(other.elements.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn empty_set_is_empty() {
        let s: GSet<u32> = GSet::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn insert_adds_element() {
        let mut s = GSet::new();
        assert!(s.insert(1));
        assert!(s.contains(&1));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn reinsert_returns_false_and_does_not_grow() {
        let mut s = GSet::new();
        assert!(s.insert(1));
        assert!(!s.insert(1));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn merge_combines_elements() {
        let mut a = GSet::new();
        a.insert(1);
        a.insert(2);
        let mut b = GSet::new();
        b.insert(2);
        b.insert(3);
        a.merge(&b);
        assert_eq!(a.len(), 3);
        assert!(a.contains(&1));
        assert!(a.contains(&2));
        assert!(a.contains(&3));
    }

    #[test]
    fn merge_with_self_is_noop() {
        let mut a = GSet::new();
        a.insert(1);
        a.insert(2);
        let snapshot = a.clone();
        a.merge(&snapshot);
        assert_eq!(a, snapshot);
    }

    #[test]
    fn supports_string_elements() {
        let mut a: GSet<String> = GSet::new();
        a.insert("hello".to_string());
        let mut b: GSet<String> = GSet::new();
        b.insert("world".to_string());
        a.merge(&b);
        assert_eq!(a.len(), 2);
    }

    fn build(ops: &[u32]) -> GSet<u32> {
        let mut s = GSet::new();
        for v in ops {
            s.insert(*v);
        }
        s
    }

    proptest! {
        #[test]
        fn prop_merge_is_commutative(
            ops_a in proptest::collection::vec(0u32..50, 0..20),
            ops_b in proptest::collection::vec(0u32..50, 0..20),
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
            ops_a in proptest::collection::vec(0u32..50, 0..20),
            ops_b in proptest::collection::vec(0u32..50, 0..20),
            ops_c in proptest::collection::vec(0u32..50, 0..20),
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
            ops in proptest::collection::vec(0u32..50, 0..20),
        ) {
            let a = build(&ops);
            let snapshot = a.clone();
            let mut twice = a.clone();
            twice.merge(&snapshot);
            prop_assert_eq!(twice, snapshot);
        }
    }
}
