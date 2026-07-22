//! A sketch-based similarity search index.
//!
//! `marline_index` provides data structures and algorithms for building
//! key-value indexes that support nearest-neighbor search via sketches.
//! A sketch is a compact, fixed-size representation of a chunk's content
//! that preserves similarity information.
//!
//! # Overview
//!
//! - [`sketch`]: Defines the [`Sketch`] trait and [`FixedSketch<N>`]
//!   implementations for representing chunk fingerprints.
//! - [`index`]: Contains the [`SketchKVindex`] trait for sketch-based
//!   key-value indexes, the [`PalantirIndex`] concrete implementation, and
//!   the [`Store`] trait with its [`IndexStorage`] in-memory backend.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use marline_index::index::store::index_storage::IndexStorage;
//! use marline_index::index::PalantirIndex;
//! use marline_index::sketch::FixedSketch;
//!
//! let storage = IndexStorage::new();
//! let mut idx = PalantirIndex::new(storage);
//!
//! let sk = FixedSketch::<6>::new([1, 2, 3, 4, 5, 6]).unwrap();
//! idx.put(&42_u64, sk).unwrap();
//! ```

pub mod index;
pub mod sketch;
