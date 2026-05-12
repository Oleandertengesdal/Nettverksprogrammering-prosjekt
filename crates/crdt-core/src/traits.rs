//! # CRDT traits
//!
//! Shared abstractions over the different CRDT flavours implemented in
//! this crate.

/// A convergent (state-based) CRDT.
///
/// Implementors define a `merge` operation that combines another
/// replica's state into the local state. For a valid CvRDT, `merge`
/// must be:
///
/// - **commutative**: `a.merge(&b)` produces the same result as
///   `b.merge(&a)`,
/// - **associative**: the order in which replicas are merged does not
///   matter,
/// - **idempotent**: merging the same state twice is a no-op.
///
/// Together these laws guarantee that replicas converge to the same
/// state once they have all observed each other's updates.
pub trait CvRDT {
    /// Merge `other` into `self`, taking the least upper bound of the
    /// two states in the CRDT's join-semilattice.
    fn merge(&mut self, other: &Self);
}
