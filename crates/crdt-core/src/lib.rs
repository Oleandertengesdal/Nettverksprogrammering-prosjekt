//! # CRDT Core
//!
//! Core CRDT data types and shared abstractions used by the rest of
//! the project. Currently provides:
//!
//! - [`counter::GCounter`] — a grow-only counter (state-based CRDT).
//! - [`clock::Dot`] and [`clock::ReplicaId`] — primitives for logical
//!   clocks.
//! - [`traits::CvRDT`] — the convergent (state-based) CRDT trait.

pub mod clock;
pub mod counter;
pub mod traits;

pub use counter::GCounter;
pub use traits::CvRDT;
