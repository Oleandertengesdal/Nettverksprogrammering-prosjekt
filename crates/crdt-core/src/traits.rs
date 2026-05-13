//! # CRDT traits
//!
//! Shared abstractions over the different CRDT flavours implemented in
//! this crate.
pub trait CvRDT {
    /// Merge `other` into `self`, taking the least upper bound of the
    fn merge(&mut self, other: &Self);
}
