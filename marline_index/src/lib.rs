//! Sketch-based key-value index with similarity search.
//!
//! Provides traits and types for indexes keyed by sketches that support
//! look-ups and nearest-neighbor queries.
//!
//! # Modules
//!
//! * [`index`] — Core index trait, search options, scoring types, and
//!   concrete index implementations.
//! * [`sketch`] — Sketch types used as index keys.

pub mod index;
pub mod sketch;
