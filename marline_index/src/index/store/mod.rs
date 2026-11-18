//! Storage traits and in-memory backend for sketch indexes.

use crate::index::error::IndexError;
use crate::sketch::Sketch;
use std::collections::HashMap;
use std::hash::Hash;

pub mod index_storage;
pub use index_storage::IndexStorage;

/// Work performed while verifying existing candidates against one posting list.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateVerification {
    pub posting_visits: usize,
    pub candidate_probes: usize,
    pub matches: usize,
}

/// Storage for feature → posting-list records.
pub trait InvertedStorage<K, F>: Send + Sync
where
    K: Clone + Send + Sync,
    F: Copy + Send + Sync,
{
    /// Returns all keys that contain the feature.
    fn posting_list(&self, feature: F) -> Result<Vec<K>, IndexError>;

    /// Returns the posting-list length without cloning its keys when supported.
    fn posting_len(&self, feature: F) -> Result<usize, IndexError> {
        Ok(self.posting_list(feature)?.len())
    }

    /// Adds a feature's postings to candidate overlap counts.
    fn accumulate_posting(
        &self,
        feature: F,
        candidates: &mut HashMap<K, usize>,
    ) -> Result<usize, IndexError>
    where
        K: Eq + Hash,
    {
        let postings = self.posting_list(feature)?;
        let count = postings.len();
        for key in postings {
            *candidates.entry(key).or_default() += 1;
        }
        Ok(count)
    }

    /// Counts matches only for existing candidates while reporting the work done.
    fn verify_candidates(
        &self,
        feature: F,
        candidates: &mut HashMap<K, usize>,
    ) -> Result<CandidateVerification, IndexError>
    where
        K: Eq + Hash,
    {
        let postings = self.posting_list(feature)?;
        let mut work =
            CandidateVerification { posting_visits: postings.len(), ..Default::default() };
        for key in postings {
            if let Some(overlap) = candidates.get_mut(&key) {
                *overlap += 1;
                work.matches += 1;
            }
        }
        Ok(work)
    }

    /// Adds the key to the feature's posting list.
    fn insert_posting(&self, feature: F, key: K) -> Result<(), IndexError>;

    /// Removes the key from the feature's posting list.
    fn remove_posting(&self, feature: F, key: &K) -> Result<(), IndexError>;

    /// Returns the number of distinct indexed features.
    fn len_postings(&self) -> Result<usize, IndexError>;

    /// Removes all posting lists.
    fn clear_postings(&self) -> Result<(), IndexError>;
}

/// Complete storage backend required by [`crate::index::InvertedSketchIndex`].
pub trait Store<K, S>: InvertedStorage<K, S::Feature>
where
    K: Clone + Send + Sync,
    S: Sketch,
{
    /// Inserts an entry. Existing keys are not updated.
    fn insert_entry(&self, key: K, sketch: S) -> Result<(), IndexError>;

    /// Removes an entry from all posting lists using only its key.
    fn remove_entry(&self, key: &K) -> Result<(), IndexError>;

    /// Removes all entries from the storage.
    fn clear(&self) -> Result<(), IndexError> {
        self.clear_postings()
    }
}
