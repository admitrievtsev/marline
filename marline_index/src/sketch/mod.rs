//! Sketch types used as keys in similarity indexing.
//!
//! A sketch compactly represents a data chunk while preserving similarity
//! information, which makes it suitable as an index key.

/// A placeholder sketch.
///
/// Will hold extracted features for use as an index key.
#[allow(dead_code)]
pub(crate) struct Sketch {}

pub mod feature;
pub mod similarity;
