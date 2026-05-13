//! # CRDT Core
//!
//! Core CRDT data types and shared abstractions used by the rest of
//! the project. Currently provides:
//!
//! - [`counter::GCounter`] — a grow-only counter (state-based CRDT).
//! - [`pn_counter::PNCounter`] — a positive-negative counter built from two G-Counters.
//! - [`g_set::GSet`] — a grow-only set (state-based CRDT).
//! - [`vector_clock::VectorClock`] — a logical clock for tracking causality.
//! - [`clock::ReplicaId`] — primitives for logical clocks.
//! - [`traits::CvRDT`] — the convergent (state-based) CRDT trait.

pub mod clock;
pub mod counter;
pub mod g_set;
pub mod pn_counter;
pub mod traits;
pub mod vector_clock;

pub use clock::ReplicaId;
pub use counter::GCounter;
pub use g_set::GSet;
pub use pn_counter::PNCounter;
pub use traits::CvRDT;
pub use vector_clock::VectorClock;
